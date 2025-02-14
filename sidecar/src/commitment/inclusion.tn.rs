use super::super::state::pricing::{InclusionPricer, PricingError};
use super::{
    super::crypto::SignerECDSA,
    misc::{IntoSigned, Signed},
};
use crate::utils::transactions::{deserialize_txs, serialize_txs, FullTransaction};
use crate::{
    state::signature::{AlloySignatureWrapper, SignatureError},
    utils::transactions::TransactionExtForPooledTransaction,
};
use alloy_v092::{
    consensus::Transaction,
    primitives::{keccak256, Address, PrimitiveSignature, B256},
};
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CommitmentRequest {
    Inclusion(InclusionRequest),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SignedCommitment {
    Inclusion(InclusionCommitment),
}

pub type InclusionCommitment = Signed<InclusionRequest, AlloySignatureWrapper>;

impl From<SignedCommitment> for InclusionCommitment {
    fn from(commitment: SignedCommitment) -> Self {
        match commitment {
            SignedCommitment::Inclusion(inclusion) => inclusion,
        }
    }
}

impl SignedCommitment {
    pub fn into_inclusion_commitment(self) -> Option<InclusionCommitment> {
        match self {
            Self::Inclusion(inclusion) => Some(inclusion),
        }
    }
}

impl CommitmentRequest {
    pub fn as_inclusion_request(&self) -> Option<&InclusionRequest> {
        match self {
            Self::Inclusion(req) => Some(req),
        }
    }

    pub async fn commit_and_sign<S: SignerECDSA>(
        self,
        signer: &S,
    ) -> eyre::Result<SignedCommitment> {
        match self {
            Self::Inclusion(req) => req
                .commit_and_sign(signer)
                .await
                .map(SignedCommitment::Inclusion),
        }
    }

    pub fn signature(&self) -> Option<&AlloySignatureWrapper> {
        match self {
            Self::Inclusion(req) => req.signature.as_ref(),
        }
    }
}

#[cfg_attr(test, derive(Default))]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InclusionRequest {
    pub slot: u64,

    #[serde(deserialize_with = "deserialize_txs", serialize_with = "serialize_txs")]
    pub txs: Vec<FullTransaction>,

    #[serde(skip)]
    pub signature: Option<AlloySignatureWrapper>,

    #[serde(skip)]
    pub signer: Option<Address>,
}

impl InclusionRequest {
    pub async fn commit_and_sign<S: SignerECDSA>(
        self,
        signer: &S,
    ) -> eyre::Result<InclusionCommitment> {
        let digest = self.digest();
        let signature = signer.sign_hash(&digest).await?;
        let signature = PrimitiveSignature::try_from(signature.as_bytes().as_ref())?;
        let ic = self.into_signed(signature.into());
        Ok(ic)
    }

    pub fn validate_basefee(&self, min: u128) -> bool {
        for tx in &self.txs {
            if tx.max_fee_per_gas() < min {
                return false;
            }
        }

        true
    }

    pub fn validate_chain_id(&self, chain_id: u64) -> bool {
        for tx in &self.txs {
            info!("tx chain id = {:?}", tx.chain_id());
            if let Some(id) = tx.chain_id() {
                if id != chain_id {
                    return false;
                }
            }
        }

        true
    }

    pub fn validate_tx_size_limit(&self, limit: usize) -> bool {
        for tx in &self.txs {
            if tx.size() > limit {
                return false;
            }
        }

        true
    }

    pub fn validate_init_code_limit(&self, limit: usize) -> bool {
        for tx in &self.txs {
            if tx.kind().is_create() && tx.input().len() > limit {
                return false;
            }
        }

        true
    }

    pub fn validate_max_priority_fee(&self) -> bool {
        for tx in &self.txs {
            if tx.max_priority_fee_per_gas() > Some(tx.max_fee_per_gas()) {
                return false;
            }
        }

        true
    }

    pub fn validate_min_priority_fee(
        &self,
        pricing: &InclusionPricer,
        preconfirmed_gas: u64,
        min_inclusion_profit: u64,
        max_base_fee: u128,
    ) -> Result<bool, PricingError> {
        let mut local_preconfirmed_gas = preconfirmed_gas;
        for tx in &self.txs {
            let min_priority_fee = pricing
                .calculate_min_priority_fee(tx.gas_limit(), preconfirmed_gas)?
                + min_inclusion_profit;

            let tip = tx.effective_tip_per_gas(max_base_fee).unwrap_or_default();
            if tip < min_priority_fee as u128 {
                return Err(PricingError::TipTooLow {
                    tip,
                    min_priority_fee: min_priority_fee as u128,
                });
            }

            local_preconfirmed_gas = local_preconfirmed_gas.saturating_add(tx.gas_limit());
        }
        Ok(true)
    }

    pub fn gas_limit(&self) -> u64 {
        self.txs.iter().map(|tx| tx.gas_limit()).sum()
    }

    pub fn signer(&self) -> Option<Address> {
        self.signer
    }

    pub fn set_signature(&mut self, signature: AlloySignatureWrapper) {
        self.signature = Some(signature);
    }

    pub fn set_signer(&mut self, signer: Address) {
        self.signer = Some(signer);
    }

    pub fn recover_signers(&mut self) -> Result<(), SignatureError> {
        for tx in &mut self.txs {
            let signer = tx.recover_signer().map_err(|_| SignatureError)?;
            tx.sender = Some(signer);
        }

        Ok(())
    }
}

impl InclusionRequest {
    pub fn digest(&self) -> B256 {
        let mut data = Vec::new();

        data.extend_from_slice(
            &self
                .txs
                .iter()
                .map(|tx| tx.hash().as_slice())
                .collect::<Vec<_>>()
                .concat(),
        );

        data.extend_from_slice(&self.slot.to_le_bytes());

        keccak256(&data)
    }
}

impl From<InclusionRequest> for CommitmentRequest {
    fn from(req: InclusionRequest) -> Self {
        Self::Inclusion(req)
    }
}
