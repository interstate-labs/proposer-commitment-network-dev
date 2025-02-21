use alloy::signers::k256::sha2::{Digest, Sha256};
use alloy_v092::{providers::Provider, transports::Transport};
use clap::ValueEnum;
use ethereum_consensus::crypto::{PublicKey as BlsPublicKey, Signature as BlsSignature};
use eyre::Result;
use serde::Serialize;

use super::signing::verify_commit_boost_root;

/// Supported chains for the CLI
#[derive(Debug, Clone, Copy, ValueEnum, Hash, PartialEq, Eq)]
#[clap(rename_all = "kebab_case")]
pub enum Chain {
    Mainnet,
    Holesky,
    Helder,
    Kurtosis,
}

impl Chain {
    /// Get the fork version for the given chain.
    pub fn fork_version(&self) -> [u8; 4] {
        match self {
            Self::Mainnet => [0, 0, 0, 0],
            Self::Holesky => [1, 1, 112, 0],
            Self::Helder => [16, 0, 0, 0],
            Self::Kurtosis => [16, 0, 0, 56],
        }
    }

    /// Get the chain ID for the given chain. Returns `None` if the chain ID is not supported.
    pub fn from_id(id: u64) -> Option<Self> {
        match id {
            1 => Some(Self::Mainnet),
            17000 => Some(Self::Holesky),
            3151908 => Some(Self::Kurtosis),
            7014190335 => Some(Self::Helder),
            _ => None,
        }
    }

    /// Get the chain ID for the given chain. Returns an error if the chain ID is not supported.
    pub fn try_from_id(id: u64) -> eyre::Result<Self> {
        Self::from_id(id).ok_or_else(|| eyre::eyre!("chain id {} not supported", id))
    }

    /// Tries to get the chain ID from an online provider.
    pub async fn try_from_provider<T, P>(provider: &P) -> eyre::Result<Self>
    where
        T: Transport + Clone,
        P: Provider<T>,
    {
        let chain_id = provider.get_chain_id().await?;
        Self::try_from_id(chain_id)
    }
}

/// Event types that can be emitted by the validator pubkey to
/// signal some action on the Bolt protocol.
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
enum SignedMessageAction {
    /// Signal delegation of a validator pubkey to a delegatee pubkey.
    Delegation,
    /// Signal revocation of a previously delegated pubkey.
    Revocation,
}

/// Transparent serialization of signed messages.
/// This is used to serialize and deserialize signed messages
///
/// e.g. serde_json::to_string(&signed_message):
/// ```
/// {
///    "message": {
///       "action": 0,
///       "validator_pubkey": "0x...",
///       "delegatee_pubkey": "0x..."
///    },
///   "signature": "0x..."
/// },
/// ```
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum SignedMessage {
    Delegation(SignedDelegation),
    Revocation(SignedRevocation),
}

impl SignedMessage {
    /// Verify the signature of a signed message
    pub fn verify_signature(&self, chain: Chain) -> Result<()> {
        match self {
            Self::Delegation(signed_delegation) => {
                let signer_pubkey = signed_delegation.message.validator_pubkey.clone();
                let digest = signed_delegation.message.digest();

                let blst_sig =
                    blst::min_pk::Signature::from_bytes(signed_delegation.signature.as_ref())
                        .map_err(|e| eyre::eyre!("Failed to parse signature: {:?}", e))?;

                // Verify the signature
                verify_commit_boost_root(signer_pubkey, digest, &blst_sig, &chain)
            }
            Self::Revocation(signed_revocation) => {
                let signer_pubkey = signed_revocation.message.validator_pubkey.clone();
                let digest = signed_revocation.message.digest();

                let blst_sig =
                    blst::min_pk::Signature::from_bytes(signed_revocation.signature.as_ref())
                        .map_err(|e| eyre::eyre!("Failed to parse signature: {:?}", e))?;

                // Verify the signature
                verify_commit_boost_root(signer_pubkey, digest, &blst_sig, &chain)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SignedDelegation {
    pub message: DelegationMessage,
    pub signature: BlsSignature,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DelegationMessage {
    action: u8,
    pub validator_pubkey: BlsPublicKey,
    pub delegatee_pubkey: BlsPublicKey,
}

impl DelegationMessage {
    /// Create a new delegation message.
    pub fn new(validator_pubkey: BlsPublicKey, delegatee_pubkey: BlsPublicKey) -> Self {
        Self { action: SignedMessageAction::Delegation as u8, validator_pubkey, delegatee_pubkey }
    }

    /// Compute the digest of the delegation message.
    pub fn digest(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update([self.action]);
        hasher.update(self.validator_pubkey.to_vec());
        hasher.update(self.delegatee_pubkey.to_vec());

        hasher.finalize().into()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SignedRevocation {
    pub message: RevocationMessage,
    pub signature: BlsSignature,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RevocationMessage {
    action: u8,
    pub validator_pubkey: BlsPublicKey,
    pub delegatee_pubkey: BlsPublicKey,
}

impl RevocationMessage {
    /// Create a new revocation message.
    pub fn new(validator_pubkey: BlsPublicKey, delegatee_pubkey: BlsPublicKey) -> Self {
        Self { action: SignedMessageAction::Revocation as u8, validator_pubkey, delegatee_pubkey }
    }

    /// Compute the digest of the revocation message.
    pub fn digest(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update([self.action]);
        hasher.update(self.validator_pubkey.to_vec());
        hasher.update(self.delegatee_pubkey.to_vec());

        hasher.finalize().into()
    }
}
