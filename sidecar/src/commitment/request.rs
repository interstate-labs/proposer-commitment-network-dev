use alloy::{
    hex,
    primitives::{keccak256, Address, PrimitiveSignature, SignatureError, B256},
};

use parking_lot::RwLock;
use reqwest::Url;
use serde::{de, Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::{num::NonZeroUsize, str::FromStr, sync::Arc};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use crate::{constraints::{deserialize_txs, serialize_txs, Constraint, TransactionExt}, state::pricing::{PreconfPricer, PricingError}};
use crate::onchain::gateway::GatewayController;

#[derive(Debug)]
pub struct CommitmentRequestEvent {
    pub req: PreconfRequest,
    pub res: oneshot::Sender<PreconfResult>,
}

#[derive(Debug, Clone)]
pub struct CommitmentRequestHandler {
    cache: Arc<RwLock<lru::LruCache<u64, Vec<PreconfRequest>>>>,
    event_sender: mpsc::Sender<CommitmentRequestEvent>,
    gateway_controller: GatewayController,
}

impl CommitmentRequestHandler {
    pub fn new<U: Into<Url>>(
        event_sender: mpsc::Sender<CommitmentRequestEvent>,
        rpc_url: U,
        contract_address: Address,
    ) -> Arc<Self> {
        let cap = NonZeroUsize::new(100).unwrap();

        Arc::new(Self {
            cache: Arc::new(RwLock::new(lru::LruCache::new(cap))),
            event_sender,
            gateway_controller: GatewayController::from_address(rpc_url, contract_address),
        })
    }

    pub async fn handle_commitment_request(&self, request: &PreconfRequest) -> PreconfResult {
        let digest = request.digest();
        let recovered_signer = request
            .signature
            .recover_address_from_prehash(&digest)
            .map_err(|_e| {
                CommitmentRequestError::Custom(
                    "Failed to recover signer from request signature".to_string(),
                )
            })?;

        if recovered_signer != request.sender {
            tracing::error!("Signer is a not a sender");
            return Err(CommitmentRequestError::Custom(
                "Invalid signature".to_string(),
            ));
        }

        for tx in request.txs.iter() {
            if !tx.validate() {
                tracing::error!("Sender of the transaction is not a signer");
                return Err(CommitmentRequestError::Custom(
                    "Sender of the transaction is invalid".to_owned(),
                ));
            }
        }

        let (response_tx, response_rx) = oneshot::channel();

        let event = CommitmentRequestEvent {
            req: request.clone(),
            res: response_tx,
        };

        if self.event_sender.try_send(event).is_err() {
            tracing::error!("Channel full - cannot process new commitment request");
            return Err(CommitmentRequestError::Custom(
                "System overloaded - please try again later".to_owned(),
            ));
        }

        tracing::debug!("sent request to event loop");
        match response_rx.await {
            Ok(event_response) => event_response,
            Err(e) => {
                tracing::error!(err = ?e, "Failed in receiving commitment request event response from event loop");
                Err(CommitmentRequestError::Custom(
                    "Failed in receiving commitment request event response from event loop"
                        .to_owned(),
                ))
            }
        }
    }

    pub async fn verify_ip(&self, ip: String) -> eyre::Result<bool> {
        self.gateway_controller.check_ip(ip).await
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreconfRequest {
    pub slot: u64,

    #[serde(deserialize_with = "deserialize_txs", serialize_with = "serialize_txs")]
    pub txs: Vec<Constraint>,

    #[serde(deserialize_with = "deserialize_sig", serialize_with = "serialize_sig")]
    pub signature: PrimitiveSignature,

    pub(crate) sender: Address,

    pub chain_id: u64,
}

impl PreconfRequest {
    pub fn digest(&self) -> B256 {
        let mut data = Vec::new();
        // Include the slot field
        data.extend_from_slice(&self.slot.to_be_bytes());
        // Concatenation of all the transaction hashes
        for tx in &self.txs {
            data.extend_from_slice(tx.tx.hash().as_slice());
        }
        keccak256(data)
    }

    pub fn gas_limit(&self) -> u64 {
        self.txs.iter().map(|c| c.tx.gas_limit()).sum()
    }
    /// Validates the tx size limit.
    pub fn validate_tx_size_limit(&self, limit: usize) -> bool {
        for c in &self.txs {
            if c.tx.size() > limit {
                return false;
            }
        }

        true
    }

    /// Validates the init code limit.
    pub fn validate_init_code_limit(&self, limit: usize) -> bool {
        for c in &self.txs {
            if c.tx.tx_kind().is_create() && c.tx.input().len() > limit {
                return false;
            }
        }

        true
    }

    /// Validates the priority fee against the max fee per gas.
    /// Returns true if the fee is less than or equal to the max fee per gas, false otherwise.
    /// Ref: https://github.com/paradigmxyz/reth/blob/2d592125128c3742ff97b321884f93f9063abcb2/crates/transaction-pool/src/validate/eth.rs#L242
    pub fn validate_max_priority_fee(&self) -> bool {
        for c in &self.txs {
            if c.tx.max_priority_fee_per_gas() > Some(c.tx.max_fee_per_gas()) {
                return false;
            }
        }

        true
    }

    /// Validates the transaction fees against a minimum basefee.
    /// Returns true if the fee is greater than or equal to the min, false otherwise.
    pub fn validate_basefee(&self, min: u128) -> bool {
        for tx in &self.txs {
            if tx.tx.max_fee_per_gas() < min {
                return false;
            }
        }

        true
    }

    pub fn recover_signers(&mut self) -> Result<(), SignatureError> {
        for tx in &mut self.txs {
            let signer = tx.tx.recover_signer().unwrap_or_default();
            tx.sender = Some(signer);
        }

        Ok(())
    }

    pub fn validate_min_priority_fee(
        &self,
        pricing: &PreconfPricer,
        preconfirmed_gas: u64,
        min_inclusion_profit: u64,
        max_base_fee: u128,
    ) -> Result<bool, PricingError> {
        // Each included tx will move the price up
        // So we need to calculate the minimum priority fee for each tx
        let mut local_preconfirmed_gas = preconfirmed_gas;
        for tx in &self.txs {
            // Calculate minimum required priority fee for this transaction
            let min_priority_fee = pricing
                .calculate_min_priority_fee(tx.tx.gas_limit(), preconfirmed_gas)? +
                min_inclusion_profit;

            let tip = tx.effective_tip_per_gas(max_base_fee).unwrap_or_default();
            if tip < min_priority_fee as u128 {
                return Err(PricingError::TipTooLow {
                    tip,
                    min_priority_fee: min_priority_fee as u128,
                });
            }
            // Increment the preconfirmed gas for the next transaction in the bundle
            local_preconfirmed_gas = local_preconfirmed_gas.saturating_add(tx.tx.gas_limit());
        }
        Ok(true)
    }

    /// Validates the transaction chain id against the provided chain id.
    /// Returns true if the chain id matches, false otherwise. Will always return true
    /// for pre-EIP155 transactions.
    pub fn validate_chain_id(&self, chain_id: u64) -> bool {
        for tx in &self.txs {
            // Check if pre-EIP155 transaction
            if let Some(id) = tx.tx.chain_id() {
                if id != chain_id {
                    return false;
                }
            }
        }

        true
    }
}

#[derive(Error, Debug)]
#[allow(missing_docs)]
#[non_exhaustive]
pub enum CommitmentRequestError {
    #[error("failed to parse JSON: {0}")]
    Parse(#[from] serde_json::Error),

    #[error("failed in handling commitment request: {0}")]
    Custom(String),

    #[error("Not allowed ip: {0}")]
    NotAllowedIP(String),
}

pub type PreconfResult = Result<Value, CommitmentRequestError>;

fn deserialize_sig<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: std::fmt::Display,
{
    let s = String::deserialize(deserializer)?;
    T::from_str(s.trim_start_matches("0x")).map_err(de::Error::custom)
}

fn serialize_sig<S: serde::Serializer>(
    sig: &PrimitiveSignature,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let parity = sig.v();
    // As bytes encodes the parity as 27/28, need to change that.
    let mut bytes = sig.as_bytes();
    bytes[bytes.len() - 1] = if parity { 1 } else { 0 };
    serializer.serialize_str(&format!("0x{}", hex::encode(bytes)))
}
