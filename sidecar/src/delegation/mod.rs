use std::{fs::read_to_string, ops::Deref, path::PathBuf};

use alloy::signers::k256::sha2::{ Sha256, Digest };
use ethereum_consensus::crypto::{ Signature as BlsSignature, PublicKey as BlsPublicKey};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedDelegationMessage {
  pub message: DelegationMessage,
  pub signature: BlsSignature,
}

impl Deref for SignedDelegationMessage {
  type Target = DelegationMessage;

  fn deref(&self) -> &Self::Target {
      &self.message
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedRevocationMessage {
  pub message: DelegationMessage,
  pub signature: BlsSignature,
}

impl Deref for SignedRevocationMessage {
  type Target = DelegationMessage;

  fn deref(&self) -> &Self::Target {
      &self.message
  }
}

pub enum DelegationMessageType {
  Delegation,
  Revocation
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DelegationMessage {
  pub message_type: u8,
  pub validator_pubkey: BlsPublicKey,
  pub target_pubkey: BlsPublicKey
}

impl DelegationMessage {
  pub fn new(validator_pubkey: BlsPublicKey, target_pubkey: BlsPublicKey) -> Self {
    DelegationMessage {
      message_type: DelegationMessageType::Delegation as u8,
      validator_pubkey,
      target_pubkey
    }
  }

  pub fn digest(&self) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update([self.message_type]);
    hasher.update(self.validator_pubkey.to_vec());
    hasher.update(self.target_pubkey.to_vec());
    let result = hasher.finalize().into();
    result
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RevocationMessage {
  pub message_type: u8,
  pub validator_pubkey: BlsPublicKey,
  pub target_pubkey: BlsPublicKey
}

impl RevocationMessage {
  pub fn new(validator_pubkey: BlsPublicKey, target_pubkey: BlsPublicKey) -> Self {
    RevocationMessage {
      message_type: DelegationMessageType::Revocation as u8,
      validator_pubkey,
      target_pubkey
    }
  }

  pub fn digest(&self) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update([self.message_type]);
    hasher.update(self.validator_pubkey.to_vec());
    hasher.update(self.target_pubkey.to_vec());
    let result = hasher.finalize().into();
    result
  }
}


pub fn load_signed_delegations(path: &PathBuf) -> eyre::Result<Vec<SignedDelegationMessage>> {
  match read_to_string(path) {
    Ok(content) => {
      let delegations: Vec<SignedDelegationMessage> = serde_json::from_str(&content)?;
      Ok(delegations)
    },
    Err(e) => {
      Err(eyre::eyre!("Failed to read delegations file: {}", e))
    }
  }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    #[test]
    fn test_read_signed_delegations_from_file() {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("mock_data/delegations.json");

        let delegations = super::load_signed_delegations(&path)
            .expect("Failed to read delegations from file");

        assert_eq!(delegations.len(), 1);
        assert_eq!(
            format!("{:?}", delegations[0].message.validator_pubkey), 
            "0x83b85769a8f2a1a6bd3a609e51b460f6fb897daff1157991479421493926faeffa6670152524403929a8a7e551d345f3"
        );
    }
}