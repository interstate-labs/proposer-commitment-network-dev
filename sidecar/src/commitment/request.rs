use std::{num::NonZeroUsize, sync::Arc};
use parking_lot::RwLock;
use alloy::{ primitives::{keccak256, Address}, eips::eip2718::{Decodable2718, Encodable2718} };
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use serde::{de, Deserialize, Deserializer, Serialize};
use reth_primitives::{PooledTransactionsElement};
use thiserror::Error;

use crate::constraints::{Constraint, deserialize_txs, serialize_txs};

#[derive(Debug)]
pub struct CommitmentRequestEvent {
  pub req: PreconfRequest,
  pub res: oneshot::Sender<PreconfResult>
}

#[derive(Debug, Clone)]
pub struct  CommitmentRequestHandler {
  cache: Arc<RwLock<lru::LruCache<u64, Vec<PreconfRequest>>>>,
  event_sender: mpsc::Sender<CommitmentRequestEvent>
}

impl CommitmentRequestHandler{
  pub fn new (event_sender: mpsc::Sender<CommitmentRequestEvent>) -> Arc<Self> {
    let cap = NonZeroUsize::new(100).unwrap();
    
    Arc::new(Self{
      cache: Arc::new(RwLock::new(lru::LruCache::new(cap))),
      event_sender,
    })
  }

  pub async fn handle_commitment_request( &self, request: &PreconfRequest) -> PreconfResult  {

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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreconfRequest {
  pub slot: u64,

  #[serde(deserialize_with = "deserialize_txs", serialize_with = "serialize_txs")]
  pub txs: Vec<Constraint>,

  pub(crate) sender: Address,
}

#[derive(Error, Debug)]
#[allow(missing_docs)]
#[non_exhaustive]
pub enum CommitmentRequestError {
  #[error("failed to parse JSON: {0}")]
  Parse(#[from] serde_json::Error),

  #[error("failed in handling commitment request: {0}")]
  Custom(String),
}

pub type PreconfResult  = Result<Value, CommitmentRequestError>;
