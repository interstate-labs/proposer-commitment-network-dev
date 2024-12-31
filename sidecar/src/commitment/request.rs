use std::{num::NonZeroUsize, str::FromStr, sync::Arc};
use parking_lot::RwLock;
use alloy::{ eips::eip2718::{Decodable2718, Encodable2718}, hex, primitives::{keccak256, Address, PrimitiveSignature, B256}};
use reqwest::Url;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use serde::{de, Deserialize, Deserializer, Serialize};
use reth_primitives::{PooledTransactionsElement};
use thiserror::Error;

use crate::constraints::{deserialize_txs, serialize_txs, Constraint, TransactionExt};
use crate::onchain::gateway::GatewayController;

#[derive(Debug)]
pub struct CommitmentRequestEvent {
  pub req: PreconfRequest,
  pub res: oneshot::Sender<PreconfResult>
}

#[derive(Debug, Clone)]
pub struct  CommitmentRequestHandler {
  cache: Arc<RwLock<lru::LruCache<u64, Vec<PreconfRequest>>>>,
  event_sender: mpsc::Sender<CommitmentRequestEvent>,
  gateway_controller: GatewayController,
}

impl CommitmentRequestHandler{
  pub fn new <U: Into<Url>> (event_sender: mpsc::Sender<CommitmentRequestEvent>, rpc_url: U, contract_address: Address) -> Arc<Self> {
    let cap = NonZeroUsize::new(100).unwrap();
    
    Arc::new(Self{
      cache: Arc::new(RwLock::new(lru::LruCache::new(cap))),
      event_sender,
      gateway_controller: GatewayController::from_address(rpc_url, contract_address)
    })
  }

  pub async fn handle_commitment_request( &self, request: &PreconfRequest) -> PreconfResult  {
    let digest = request.digest();
    let recovered_signer = request.signature.recover_address_from_prehash(&digest).map_err(|e|{
      CommitmentRequestError::Custom("Failed to recover signer from request signature".to_string())
    })?;

    if recovered_signer != request.sender {
      tracing::error!("Signer is a not a sender");
      return Err(CommitmentRequestError::Custom("Invalid signature".to_string()));
    }

    let (response_tx, response_rx) = oneshot::channel();

    let event = CommitmentRequestEvent {
      req: request.clone(),
      res: response_tx
    };
    let _ = self.event_sender.send(event).await.map_err(|e|{
      tracing::error!(err = ?e, "Failed in handling commitment request");
      CommitmentRequestError::Custom("Failed in handling commitment request".to_owned())
    });

    tracing::debug!("sent request to event loop");

    match response_rx.await {
      // TODO: format the user response to be more clear. Right now it's just the raw
      // signed constraints object.
      // Docs: https://chainbound.github.io/bolt-docs/api/commitments-api#bolt_inclusionpreconfirmation
      Ok(event_response) => event_response,
      Err(e) => {
          tracing::error!(err = ?e, "Failed in receiving commitment request event response from event loop");
          Err(CommitmentRequestError::Custom("Failed in receiving commitment request event response from event loop".to_owned()))
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
}

impl PreconfRequest {
  pub fn digest(&self) -> B256 {
    let mut data = Vec::new();
    // First field is the concatenation of all the transaction hashes
    data.extend_from_slice(
        &self.txs
          .iter()
          .map(|tx| tx.tx.hash().as_slice())
          .collect::<Vec<_>>()
          .concat(),
    );
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

pub type PreconfResult  = Result<Value, CommitmentRequestError>;


fn deserialize_sig<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: std::fmt::Display,
{
    let s = String::deserialize(deserializer)?;
    T::from_str(s.trim_start_matches("0x")).map_err(de::Error::custom)
}

fn serialize_sig<S: serde::Serializer>(sig: &PrimitiveSignature, serializer: S) -> Result<S::Ok, S::Error> {
    let parity = sig.v();
    // As bytes encodes the parity as 27/28, need to change that.
    let mut bytes = sig.as_bytes();
    bytes[bytes.len() - 1] = if parity { 1 } else { 0 };
    serializer.serialize_str(&format!("0x{}", hex::encode(bytes)))
}
