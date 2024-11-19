use alloy::primitives::{Address, keccak256};
use reth_primitives::PooledTransactionsElement;
use serde::Serialize;
use crate::commitment::{self, request::{serialize_tx, PreconfRequest}};
use secp256k1::Message;

#[derive(Serialize, Debug, Clone, PartialEq, Default)]
pub struct SignedConstraints {
    /// The constraints that need to be signed.
    pub message: ConstraintsMessage,
    /// The signature of the proposer sidecar.
    pub signature: String,
}


#[derive(Serialize, Debug, Clone, PartialEq, Default)]
pub struct ConstraintsMessage {
    /// The validator index of the proposer sidecar.
    pub validator_index: u64,
    /// The consensus slot at which the constraints are valid
    pub slot: u64,
    /// The constraints that need to be signed.
    pub constraints: Vec<Constraint>,
}

impl ConstraintsMessage {
  pub fn build(validator_index: u64, request: PreconfRequest) -> Self {
    let constraints = vec![Constraint::from_transaction(
        request.tx,
        None,
        request.sender,
    )];
    Self {
        validator_index,
        slot: request.slot,
        constraints,
    }
  }

  pub fn digest(&self) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&self.validator_index.to_le_bytes());
    data.extend_from_slice(&self.slot.to_le_bytes());

    let mut constraint_bytes = Vec::new();
    for constraint in &self.constraints {
        constraint_bytes.extend_from_slice(&constraint.as_bytes());
    }
    data.extend_from_slice(&constraint_bytes);

    keccak256(data).0.to_vec()
  }

}
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Constraint {
    pub index: Option<u64>,
    #[serde(serialize_with = "serialize_tx")]
    pub(crate) tx: PooledTransactionsElement,
    #[serde(skip)]
    pub(crate) sender: Address,
}

impl Constraint {
   pub fn from_transaction(
      tx: PooledTransactionsElement,
      index: Option<u64>,
      sender: Address,
    ) -> Self {
        Self {
            tx,
            index,
            sender,
        }
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        let mut data = Vec::new();
        self.tx.encode_enveloped(&mut data);
        data.extend_from_slice(&self.index.unwrap_or(0).to_le_bytes());
        data
    }
}