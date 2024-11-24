use std::{
  collections::HashMap,
  pin::Pin,
  task::{Context, Poll},
  time::{Duration, Instant}
};

use beacon_api_client::{mainnet::Client, BlockId, ProposerDuty};
use alloy::rpc::types::beacon::events::HeadEvent;
use beacon_api_client::Topic;
use futures::StreamExt;
use tokio::{sync::broadcast, task::AbortHandle};

use futures::{future::poll_fn, Future, FutureExt};
use tokio::time::Sleep;

use ethereum_consensus::{deneb::BeaconBlockHeader, phase0::mainnet::SLOTS_PER_EPOCH};

use crate::constraints::SignedConstraints;
use crate::commitment::request::PreconfRequest;
use crate::config::ValidatorIndexes;

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

  pub beacon_client: Client
}

impl ConstraintState {
  pub fn new (beacon_client: Client, validator_indexes: ValidatorIndexes, commitment_deadline_duration: Duration) -> Self {
    Self {
      blocks: HashMap::new(),
      commitment_deadline: CommitmentDeadline::new(0, Duration::from_millis(100)),
      deadline_duration: commitment_deadline_duration,
      latest_slot: Default::default(),
      latest_slot_timestamp: Instant::now(),
      current_epoch: Default::default(),
      validator_indexes,
      beacon_client,
      header: BeaconBlockHeader::default(),
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

  pub fn remove_constraints_at_slot( &mut self, slot: u64) -> Option<Block> {
    self.blocks.remove(&slot)
  }

  pub fn validate_preconf_request(&self, request: &PreconfRequest) -> Result<u64, StateError> {
  
    // Check if the slot is in the current epoch
    if request.slot < self.current_epoch.start_slot || request.slot >= self.current_epoch.start_slot + SLOTS_PER_EPOCH {
        return Err(StateError::InvalidSlot(request.slot));
    }

    // If the request is for the next slot, check if it's within the commitment deadline
    if request.slot == self.latest_slot + 1
        && self.latest_slot_timestamp + self.deadline_duration < Instant::now()
    {
        return Err(StateError::DeadlineExpired);
    }

    // Find the validator index for the given slot
    let validator_index = self.find_validator_index_for_slot(request.slot)?;

    Ok(validator_index)
  }

  fn find_validator_index_for_slot(&self, slot: u64) -> Result<u64, StateError> {
    self.current_epoch
        .proposer_duties
        .iter()
        .find(|&duty| {
            duty.slot == slot && self.validator_indexes.contains(duty.validator_index as u64)
        })
        .map(|duty| duty.validator_index as u64)
        .ok_or(StateError::NoValidatorInSlot)
  }

  async fn fetch_proposer_duties(&mut self, epoch: u64) -> Result<(), StateError> {
    match self.beacon_client.get_proposer_duties(epoch).await.map_err(|_| StateError::FailedFetcingProposerDuties){
      Ok(duties) =>  self.current_epoch.proposer_duties = duties.1,
      Err(err) => return Err(err)
    };
    Ok(())
  }

  pub async fn update_head(&mut self, head: u64) -> Result<(), StateError> {
    self.commitment_deadline =
        CommitmentDeadline::new(head + 1, self.deadline_duration);

    let update = self
        .beacon_client
        .get_beacon_header(BlockId::Slot(head))
        .await?;

    self.header = update.header.message;

    self.latest_slot_timestamp = Instant::now();
    self.latest_slot = head;

    let slot = self.header.slot;
    let epoch = slot / SLOTS_PER_EPOCH;

    self.blocks.remove(&slot);

    if epoch != self.current_epoch.value {
        self.current_epoch.value = epoch;
        self.current_epoch.start_slot = epoch * SLOTS_PER_EPOCH;

        self.fetch_proposer_duties(epoch).await?;
    }

    Ok(())
}
}

#[derive(Debug, Default)]
pub struct  Block {
  pub signed_constraints_list: Vec<SignedConstraints>
}

impl Block {
  pub fn add_constraints ( &mut self, constraints: SignedConstraints) {
    self.signed_constraints_list.push(constraints);
  }
  pub fn remove_constraints( &mut self, slot: u64){
    self.signed_constraints_list.remove(slot.try_into().unwrap());
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