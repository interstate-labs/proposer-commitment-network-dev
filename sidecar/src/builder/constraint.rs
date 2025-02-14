use alloy_v092::signers::k256::sha2::{Digest, Sha256};
use ethereum_consensus::crypto::PublicKey as BlsPublicKey;
use serde::{Deserialize, Serialize};

use crate::{
    commitment::request::PreconfRequest, constraints::Constraint, crypto::{bls::BLSSig, SignableBLS}, utils::transactions::{deserialize_txs, serialize_txs, FullTransaction}
};

pub type BatchedSignedConstraints = Vec<SignedConstraints>;

#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct SignedConstraints {
    pub message: ConstraintsMessage,
    pub signature: BLSSig,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Default, Eq)]
pub struct ConstraintsMessage {
    pub pubkey: BlsPublicKey,
    pub slot: u64,
    pub top: bool,
    #[serde(serialize_with = "serialize_txs")]
    pub transactions: Vec<Constraint>,
}

impl ConstraintsMessage {
    pub fn build(pubkey: BlsPublicKey, request: PreconfRequest) -> Self {
        let transactions = request.txs;

        Self {
            pubkey,
            slot: request.slot,
            top: false,
            transactions,
        }
    }

    pub fn from_tx(pubkey: BlsPublicKey, slot: u64, tx: Constraint) -> Self {
        Self {
            pubkey,
            slot,
            top: false,
            transactions: vec![tx],
        }
    }
}

impl SignableBLS for ConstraintsMessage {
    fn digest(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.pubkey.to_vec());
        hasher.update(self.slot.to_le_bytes());
        hasher.update((self.top as u8).to_le_bytes());

        for tx in &self.transactions {
            hasher.update(tx.tx.hash());
        }

        hasher.finalize().into()
    }
}
