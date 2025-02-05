pub mod execution;
pub mod account_state;
pub mod fetcher;
pub mod execution_client;
pub mod pricing;
pub mod signature;

use std::{
    collections::HashMap, mem, num::NonZero, pin::Pin, task::{Context, Poll}, time::{Duration, Instant}
};

use alloy::rpc::types::beacon::events::HeadEvent;
use beacon_api_client::Topic;
use beacon_api_client::{mainnet::Client, BlockId, ProposerDuty};
use execution::ExecutionState;
use fetcher::ClientState;
use futures::StreamExt;
use futures::{future::poll_fn, Future, FutureExt};
use reth_primitives::{PooledTransactionsElement, TransactionSigned};
use reth_primitives_v115::PooledTransaction;
use signature::AlloySignatureWrapper;
use tokio::time::Sleep;
use tokio::{sync::broadcast, task::AbortHandle};
use reth_primitives::PooledTransactionsElement::{
    BlobTransaction, Eip1559, Eip2930, Eip7702, Legacy,
};
use alloy_v092::consensus::{Signed, TxEip1559, TxEip2930, TxEip4844, TxEip7702, TxLegacy};
use ethereum_consensus::{
    crypto::PublicKey as ECBlsPublicKey,
    crypto::{KzgCommitment, KzgProof},
    deneb::{
        mainnet::{Blob, BlobsBundle},
        BeaconBlockHeader,
    },
    phase0::mainnet::SLOTS_PER_EPOCH,
};

use tokio::time::error::Elapsed;
use crate::{constraints::{SignedConstraints, TransactionExt}, metrics::ApiMetrics};

use crate::{commitment::{inclusion::InclusionRequest, request::PreconfRequest}, utils::transactions::FullTransaction};
use crate::config::ValidatorIndexes;
use crate::config::ChainConfig;

#[derive(Debug, thiserror::Error)]
pub enum StateError {

  #[error("invalid slot: {0}")]
  InvalidSlot(u64),
  #[error("deadline expired")]
  DeadlineExpired,
  #[error("no validator in slot")]
  NoValidatorInSlot,
  #[error("failed in fetching proposer duties from beacon")]
  FailedFetcingProposerDuties,
  #[error("Beacon API error: {0}")]
  BeaconApiError(#[from] beacon_api_client::Error),
  #[error("{0}")]
  Custom(String),
  #[error("Maximum retries exceeded for get_beacon_header")]
  MaxRetriesExceeded,
  #[error("Timeout error: {0}")]
  Timeout(Elapsed),

}

#[derive(Debug, Default)]
#[allow(missing_docs)]
pub struct Epoch {
    pub value: u64,
    pub start_slot: u64,
    pub proposer_duties: Vec<ProposerDuty>,
}

pub struct ConstraintState {
    pub blocks: HashMap<u64, Block>,
    pub commitment_deadline: CommitmentDeadline,
    pub deadline_duration: Duration,
    pub latest_slot: u64,
    pub latest_slot_timestamp: Instant,
    pub current_epoch: Epoch,
    pub header: BeaconBlockHeader,
    pub validator_indexes: ValidatorIndexes,
    pub max_commitments_in_block: usize,
    pub max_commitment_gas: NonZero<u64>,
    pub min_priority_fee: u128,
    pub block_gas_limit: u64,
    pub max_tx_input_bytes: usize,
    pub max_init_code_byte_size: usize,
    pub config: ChainConfig,
    pub beacon_client: Client,
    pub execution: ExecutionState<ClientState>,
}

use tokio::time::timeout;

const TIMEOUT_SECS: u64 = 10;
const MAX_RETRIES: u8 = 5;
const RETRY_BACKOFF_MILLIS: u64 = 100;

impl ConstraintState {
    pub fn new(
        beacon_client: Client,
        validator_indexes: ValidatorIndexes,
        commitment_deadline_duration: Duration,
        execution: ExecutionState<ClientState>
    ) -> Self {
        Self {
            blocks: HashMap::new(),
            commitment_deadline: CommitmentDeadline::new(0, Duration::from_millis(100)),
            deadline_duration: commitment_deadline_duration,
            latest_slot: Default::default(),
            latest_slot_timestamp: Instant::now(),
            current_epoch: Default::default(),
            validator_indexes,
            beacon_client,
            execution,
            header: BeaconBlockHeader::default(),
            max_commitments_in_block: 128,
            max_commitment_gas: NonZero::new(10_000_000).unwrap(),
            min_priority_fee: 1_000_000_000,
            block_gas_limit: 30_000_000,
            max_tx_input_bytes: 4 * 32 * 1024,
            max_init_code_byte_size: 2 * 24576,
            config: Default::default(),
        }
    }

    pub fn add_constraint(&mut self, slot: u64, signed_constraints: SignedConstraints) {
        if let Some(block) = self.blocks.get_mut(&slot) {
            block.add_constraints(signed_constraints);
        } else {
            let mut block = Block::default();
            block.add_constraints(signed_constraints);
            self.blocks.insert(slot, block);
        }
    }

    pub fn replace_constraints(&mut self, slot: u64, signed_constraints: &Vec<SignedConstraints>) {
        tracing::debug!("here is replace constraints function");
        if let Some(block) = self.blocks.get_mut(&slot) {
            tracing::debug!(
                "current constraints {}",
                block.signed_constraints_list.len()
            );
            block.replace_constraints(signed_constraints);
            tracing::debug!(
                "replaced constraints {}",
                block.signed_constraints_list.len()
            );
        } else {
            let mut block = Block::default();
            block.replace_constraints(signed_constraints);
            self.blocks.insert(slot, block.clone());
            tracing::debug!(
                "replaced constraints {}",
                block.signed_constraints_list.len()
            );
        }
    }

    pub fn remove_constraints_at_slot(&mut self, slot: u64) -> Option<Block> {
        self.blocks.remove(&slot)
    }

    pub async fn validate_preconf_request(
        &mut self,
        request: &PreconfRequest,
    ) -> Result<ECBlsPublicKey, StateError> {
        // Check if the chain is eth mainnet
        if request.chain_id != self.config.id {
            return Err(StateError::Custom(format!(
                "Invalid chain ID: expected {}, got {:?}",
                self.config.id,
                request.chain_id
            )));
        }

        // Check if the slot is in the current epoch
        if request.slot < self.current_epoch.start_slot
            || request.slot >= self.current_epoch.start_slot + SLOTS_PER_EPOCH
        {
            return Err(StateError::InvalidSlot(request.slot));
        }

        // If the request is for the next slot, check if it's within the commitment deadline
        if request.slot == self.latest_slot + 1
            && self.latest_slot_timestamp + self.deadline_duration < Instant::now()
        {
            return Err(StateError::DeadlineExpired);
        }

        // Find the validator publickey for the given slot
        let public_key = self.find_validator_pubkey_for_slot(request.slot)?;

        if request.txs.len() >= self.max_commitments_in_block {
            return Err(StateError::Custom(
                "Overflow commitments amount".to_string(),
            ));
        }

        // Check if there is room for more commitments
        if let Some(block) = self.blocks.get(&request.slot) {
            if block.transactions_count() + request.txs.len() >= self.max_commitments_in_block {
                return Err(StateError::Custom(
                    "Overflow commitments amount".to_string(),
                ));
            }
        }

        // Check if the committed gas exceeds the maximum
        let template_committed_gas = self
            .blocks
            .get(&request.slot)
            .map(|t| t.committed_gas())
            .unwrap_or(0);

        // Check total committed gas stays below max_commitment_gas
        if template_committed_gas + request.gas_limit() >= self.max_commitment_gas.into() {
            return Err(StateError::Custom("Overflow gas limit".to_string()));
        }

        // Get parent block gas limit by fetching the parent block header
        let parent_block_id = BlockId::Root(self.header.parent_root);
        let parent_header = self.beacon_client.get_beacon_header(parent_block_id);
        let parent_gas_limit = match parent_header.message.execution_payload {
            Some(payload) => payload.gas_limit,
            None => return Err(StateError::Custom("Parent block missing execution payload".to_string())),
        };

        // Calculate allowed gas limit range based on EIP-1559 constraints
        let min_gas_limit = parent_gas_limit - parent_gas_limit / 1024;
        let max_gas_limit = parent_gas_limit + parent_gas_limit / 1024;

        // Check request gas limit is within allowed EIP-1559 range 
        if request.gas_limit() < min_gas_limit || request.gas_limit() > max_gas_limit {
            return Err(StateError::Custom(format!(
                "Invalid gas limit. Must be within [{}, {}]", 
                min_gas_limit, max_gas_limit
            )));
        }

        // Check if the transaction size exceeds the maximum
        if !request.validate_tx_size_limit(self.max_tx_input_bytes) {
            return Err(StateError::Custom(
                "Overflow transaction size in input bytes".to_string(),
            ));
        }

        // Check if the transaction is a contract creation and the init code size exceeds the
        // maximum
        if !request.validate_init_code_limit(self.max_init_code_byte_size) {
            return Err(StateError::Custom(
                "Overflow transaction size in code bytes".to_string(),
            ));
        }

        // Ensure max_priority_fee_per_gas is less than max_fee_per_gas
        if !request.validate_max_priority_fee() {
            return Err(StateError::Custom(
                "Overflow transaction priority fee".to_string(),
            ));
        }

        // Check if the max_fee_per_gas would cover the maximum possible basefee.
        let _slot_diff = request.slot.saturating_sub(self.latest_slot);

        // TODO: Calculate the max possible basefee given the slot diff.
        if request.slot <= self.latest_slot {
            return Err(StateError::Custom(
                "Target slot is passed already".to_string(),
            ));
        }

        // // Execution Layer Validation
        let mut txs: Vec<FullTransaction> = vec![];
        for tx in request.txs.clone() {
            let pooled_tx = tx.tx;
            let full_tx: FullTransaction;
            match pooled_tx {
                Legacy {
                    transaction,
                    signature,
                    hash,
                } => {
                    full_tx = FullTransaction::from(PooledTransaction::Legacy(Signed::new_unchecked(
                        TxLegacy {
                            chain_id: transaction.chain_id,
                            nonce: transaction.nonce,
                            gas_price: transaction.gas_price,
                            gas_limit: transaction.gas_limit,
                            to: transaction.to,
                            value: transaction.value,
                            input: transaction.input,
                        },
                        signature,
                        hash,
                    )))
                }
                Eip2930 {
                    transaction,
                    signature,
                    hash,
                } => {
                    full_tx = FullTransaction::from(PooledTransaction::Eip2930(Signed::new_unchecked(
                        TxEip2930 {
                            chain_id: transaction.chain_id,
                            nonce: transaction.nonce,
                            gas_price: transaction.gas_price,
                            gas_limit: transaction.gas_limit,
                            to: transaction.to,
                            value: transaction.value,
                            input: transaction.input,
                            access_list: transaction.access_list,
                        },
                        signature,
                        hash,
                    )))
                }
                Eip1559 {
                    transaction,
                    signature,
                    hash,
                } => {
                    full_tx = FullTransaction::from(PooledTransaction::Eip1559(Signed::new_unchecked(
                        TxEip1559 {
                            chain_id: transaction.chain_id,
                            nonce: transaction.nonce,
                            gas_limit: transaction.gas_limit,
                            to: transaction.to,
                            value: transaction.value,
                            input: transaction.input,
                            access_list: transaction.access_list,
                            max_fee_per_gas: transaction.max_fee_per_gas,
                            max_priority_fee_per_gas: transaction.max_priority_fee_per_gas,
                        },
                        signature,
                        hash,
                    )))
                }
                Eip7702 {
                    transaction,
                    signature,
                    hash,
                } => {
                    let authorization_list = unsafe { mem::transmute(transaction.authorization_list) };
                    full_tx = FullTransaction::from(PooledTransaction::Eip7702(Signed::new_unchecked(
                        TxEip7702 {
                            chain_id: transaction.chain_id,
                            nonce: transaction.nonce,
                            gas_limit: transaction.gas_limit,
                            to: transaction.to,
                            value: transaction.value,
                            input: transaction.input,
                            access_list: transaction.access_list,
                            max_fee_per_gas: transaction.max_fee_per_gas,
                            max_priority_fee_per_gas: transaction.max_priority_fee_per_gas,
                            authorization_list: authorization_list,
                        },
                        signature,
                        hash,
                    )))
                }
                BlobTransaction(blob_transaction) => {
                    full_tx = FullTransaction::from(PooledTransaction::Eip4844(Signed::new_unchecked(
                        alloy_v092::consensus::TxEip4844WithSidecar {
                            tx: TxEip4844 {
                                chain_id: blob_transaction.transaction.tx.chain_id,
                                nonce: blob_transaction.transaction.tx.nonce,
                                gas_limit: blob_transaction.transaction.tx.gas_limit,
                                max_fee_per_gas: blob_transaction.transaction.tx.max_fee_per_gas,
                                max_priority_fee_per_gas: blob_transaction
                                    .transaction
                                    .tx
                                    .max_priority_fee_per_gas,
                                to: blob_transaction.transaction.tx.to,
                                value: blob_transaction.transaction.tx.value,
                                access_list: blob_transaction.transaction.tx.access_list,
                                blob_versioned_hashes: blob_transaction
                                    .transaction
                                    .tx
                                    .blob_versioned_hashes,
                                max_fee_per_blob_gas: blob_transaction.transaction.tx.max_fee_per_blob_gas,
                                input: blob_transaction.transaction.tx.input,
                            },
                            sidecar: alloy_v092::consensus::BlobTransactionSidecar {
                                blobs: blob_transaction.transaction.sidecar.blobs,
                                commitments: blob_transaction.transaction.sidecar.commitments,
                                proofs: blob_transaction.transaction.sidecar.proofs,
                            },
                        },
                        blob_transaction.signature,
                        blob_transaction.hash,
                    )))
                }
            }
            txs.push(full_tx);
        }

        let mut ir: InclusionRequest = InclusionRequest {
            slot: request.slot.clone(),
            txs,
            signature: Some(AlloySignatureWrapper::try_from(request.signature.as_bytes().as_ref()).unwrap()),
            signer: Some(request.sender),
        };
        
        let result = self.execution.verify_el_tx(&mut ir).await;
        match result {
            Ok(_) => {
                Ok(public_key)
            }
            Err(err) => {
                return Err(StateError::Custom("Execution Layer Validation Failed!".to_string()))
            }
        }
    }

  fn find_validator_pubkey_for_slot(&self, slot: u64) -> Result<ECBlsPublicKey, StateError> {
    self.current_epoch
        .proposer_duties
        .iter()
        .find(|&duty| 
            duty.slot == slot
        )
        .map(|duty| duty.public_key.clone())
        .ok_or(StateError::NoValidatorInSlot)
  }

  async fn get_beacon_header_with_retry(&self, head: u64) -> Result<BeaconBlockHeader, StateError> {
    let mut retries_remaining = MAX_RETRIES;
    let mut backoff_millis = RETRY_BACKOFF_MILLIS;

    loop {
        let result = timeout(
            Duration::from_secs(TIMEOUT_SECS),
            self.beacon_client.get_beacon_header(BlockId::Slot(head)),
        )
        .await
        .map_err(StateError::Timeout)?;

        if let Ok(update) = result {
            return Ok(update.header.message);
        }

        if retries_remaining == 0 {
            return Err(StateError::MaxRetriesExceeded);
        }

        retries_remaining -= 1;
        tokio::time::sleep(Duration::from_millis(backoff_millis)).await;
        backoff_millis *= 2;
    }
}

  pub async fn update_head(&mut self, head: u64) -> Result<(), StateError> {
    self.commitment_deadline =
        CommitmentDeadline::new(head + 1, self.deadline_duration);

    self.header = self.get_beacon_header_with_retry(head).await?;

    self.latest_slot_timestamp = Instant::now();
    self.latest_slot = head;

    let slot = self.header.slot;
    ApiMetrics::set_latest_head(slot as u32);
    let epoch = slot / SLOTS_PER_EPOCH;

    self.blocks.remove(&slot);

    if epoch != self.current_epoch.value {
        self.current_epoch.value = epoch;
        self.current_epoch.start_slot = epoch * SLOTS_PER_EPOCH;

        self.fetch_proposer_duties(epoch).await?;

    }

    Ok(())
  }

  async fn fetch_proposer_duties(&mut self, epoch: u64) -> Result<(), StateError> {
      // Retry settings
      let retry_delay = Duration::from_secs(2);
      let max_retries = 5;

      let mut retries = 0;

      loop {
          match self
              .beacon_client
              .get_proposer_duties(epoch)
              .await
              .map_err(|_| StateError::FailedFetcingProposerDuties)
          {
              Ok(duties) => {
                  self.current_epoch.proposer_duties = duties.1;
                  break;
              }
              Err(_) if retries < max_retries => {
                  retries += 1;
                  tokio::time::sleep(retry_delay).await;
              }
              Err(err) => return Err(err),
          };
      }
      Ok(())
  }
}

#[derive(Debug, Default, Clone)]
pub struct Block {
    pub signed_constraints_list: Vec<SignedConstraints>,
}

impl Block {
    pub fn add_constraints(&mut self, constraints: SignedConstraints) {
        self.signed_constraints_list.push(constraints);
    }

    pub fn replace_constraints(&mut self, constraints: &Vec<SignedConstraints>) {
        self.signed_constraints_list = constraints.clone();
    }

    pub fn remove_constraints(&mut self, slot: u64) {
        self.signed_constraints_list
            .remove(slot.try_into().unwrap());
    }

    pub fn get_transactions(&self) -> Vec<PooledTransactionsElement> {
        self.signed_constraints_list
            .iter()
            .flat_map(|sc| sc.message.transactions.iter().map(|c| c.tx.clone()))
            .collect()
    }

    pub fn convert_constraints_to_transactions(&self) -> Vec<TransactionSigned> {
        self.signed_constraints_list
            .iter()
            .flat_map(|sc| {
                sc.message
                    .transactions
                    .iter()
                    .map(|c| c.tx.clone().into_transaction())
            })
            .collect()
    }

    pub fn parse_to_blobs_bundle(&self) -> BlobsBundle {
        let (commitments, proofs, blobs) =
            self.signed_constraints_list
                .iter()
                .flat_map(|sc| sc.message.transactions.iter())
                .filter_map(|c| c.tx.blob_sidecar())
                .fold(
                    (Vec::new(), Vec::new(), Vec::new()),
                    |(mut commitments, mut proofs, mut blobs), bs| {
                        commitments.extend(bs.commitments.iter().map(|c| {
                            KzgCommitment::try_from(c.as_slice()).expect("both are 48 bytes")
                        }));
                        proofs.extend(
                            bs.proofs.iter().map(|p| {
                                KzgProof::try_from(p.as_slice()).expect("both are 48 bytes")
                            }),
                        );
                        blobs.extend(bs.blobs.iter().map(|b| {
                            Blob::try_from(b.as_slice()).expect("both are 131_072 bytes")
                        }));
                        (commitments, proofs, blobs)
                    },
                );

        BlobsBundle {
            commitments,
            proofs,
            blobs,
        }
    }

    pub fn transactions_count(&self) -> usize {
        self.signed_constraints_list.len()
    }

    pub fn committed_gas(&self) -> u64 {
        self.signed_constraints_list.iter().fold(0, |acc, sc| {
            acc + sc
                .message
                .transactions
                .iter()
                .fold(0, |acc, c| acc + c.tx.gas_limit())
        })
    }
}

/// The deadline for a which a commitment is considered valid.
#[derive(Debug)]
pub struct CommitmentDeadline {
    slot: u64,
    sleep: Option<Pin<Box<Sleep>>>,
}

impl CommitmentDeadline {
    /// Create a new deadline for a given slot and duration.
    pub fn new(slot: u64, duration: Duration) -> Self {
        let sleep = Some(Box::pin(tokio::time::sleep(duration)));
        Self { slot, sleep }
    }

    /// Poll the deadline until it is reached.
    pub async fn wait(&mut self) -> Option<u64> {
        let slot = poll_fn(|cx| self.poll_unpin(cx)).await;
        self.sleep = None;
        slot
    }
}

/// Poll the deadline until it is reached.
///
/// - If already reached, the future will return `None` immediately.
/// - If not reached, the future will return `Some(slot)` when the deadline is reached.
impl Future for CommitmentDeadline {
    type Output = Option<u64>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Some(ref mut sleep) = self.sleep else {
            return Poll::Ready(None);
        };

        match sleep.as_mut().poll(cx) {
            Poll::Ready(_) => Poll::Ready(Some(self.slot)),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[derive(Debug)]
pub struct HeadEventListener {
    /// Channel to receive updates of the "Head" beacon topic
    new_heads_rx: broadcast::Receiver<HeadEvent>,
    /// Handle to the background task that listens for new head events.
    /// Kept to allow for graceful shutdown.
    quit: AbortHandle,
}

/// A topic for subscribing to new head events
#[derive(Debug)]
pub struct NewHeadsTopic;

impl Topic for NewHeadsTopic {
    const NAME: &'static str = "head";

    type Data = HeadEvent;
}

impl HeadEventListener {
    /// start listening for new head events
    pub fn run(beacon_client: Client) -> Self {
        let (new_heads_tx, new_heads_rx) = broadcast::channel(32);

        let task = tokio::spawn(async move {
            loop {
                let mut event_stream = match beacon_client.get_events::<NewHeadsTopic>().await {
                    Ok(events) => events,
                    Err(err) => {
                        tracing::warn!(?err, "failed to subscribe to new heads topic, retrying...");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                };

                let event = match event_stream.next().await {
                    Some(Ok(event)) => event,
                    Some(Err(err)) => {
                        tracing::warn!(?err, "error reading new head event stream, retrying...");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                    None => {
                        tracing::warn!("new head event stream ended, retrying...");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                };

                if let Err(err) = new_heads_tx.send(event) {
                    tracing::warn!(?err, "failed to broadcast new head event to subscribers");
                }
            }
        });

        Self {
            new_heads_rx,
            quit: task.abort_handle(),
        }
    }

    pub fn stop(self) {
        self.quit.abort();
    }

    pub async fn next_head(&mut self) -> Result<HeadEvent, broadcast::error::RecvError> {
        self.new_heads_rx.recv().await
    }

    pub fn subscribe_new_heads(&self) -> broadcast::Receiver<HeadEvent> {
        self.new_heads_rx.resubscribe()
    }
}
