use std::{borrow::Cow, collections::HashMap, net::SocketAddr, str::FromStr, sync::Arc};

use alloy::{
    hex, 
    primitives::{Bytes, TxKind, U256, Address, FixedBytes}, 
    eips::eip2718::{Decodable2718, Encodable2718},
    signers::k256::sha2::{Digest, Sha256},
    consensus::BlobTransactionSidecar,
};
use axum::{Router, extract::{Path, State}, response::Html, routing::{ get, post }, Json};
use builder::{GetHeaderParams, GetPayloadResponse, SignedBuilderBid};

use reth_primitives::{ PooledTransactionsElement, TxType};

use ethereum_consensus::{crypto::PublicKey as ECBlsPublicKey, builder::SignedValidatorRegistration, deneb::mainnet::SignedBlindedBeaconBlock, Fork, deneb::compute_signing_root};
use serde::{Deserialize, Serialize, de, ser::SerializeSeq };

use reqwest::{ Client, ClientBuilder, StatusCode, Url };

use crate::{
    commitment::request::PreconfRequest, 
    errors::{CommitBoostError, ErrorResponse},
    delegation::{SignedDelegationMessage, SignedRevocationMessage},
};
use crate::config::Config;

mod builder;
mod block_builder;
pub(crate) mod signature;
mod constraints_proxy_server;

pub use builder::FallbackBuilder;
pub use constraints_proxy_server::{run_constraints_proxy_server, FallbackPayloadFetcher, FetchPayloadRequest, LocalPayloadIntegrityError};

/// The path to the builder API status endpoint.
pub const STATUS_PATH: &str = "/eth/v1/builder/status";
/// The path to the builder API register validators endpoint.
pub const REGISTER_VALIDATORS_PATH: &str = "/eth/v1/builder/validators";
/// The path to the builder API get header endpoint.
pub const GET_HEADER_PATH: &str = "/eth/v1/builder/header/:slot/:parent_hash/:pubkey";
/// The path to the builder API get payload endpoint.
pub const GET_PAYLOAD_PATH: &str = "/eth/v1/builder/blinded_blocks";
/// The path to the constraints API submit constraints endpoint.
pub const CONSTRAINTS_PATH: &str = "/constraints/v1/builder/constraints";
/// The path to the constraints API submit constraints endpoint.
pub const PERMISSION_DELEGATE_PATH: &str = "/constraints/v1/builder/delegate";
/// The path to the constraints API submit constraints endpoint.
pub const PERMISSION_REVOKE_PATH: &str = "/constraints/v1/builder/revoke";
/// The path to the constraints API collect constraints endpoint.
pub const CONSTRAINTS_COLLECT_PATH: &str = "/constraints/v1/builder/constraints_collect";


pub trait TransactionExt {
    /// Returns the gas limit of the transaction.
    fn gas_limit(&self) -> u64;

    /// Returns the value of the transaction.
    fn value(&self) -> U256;

    /// Returns the type of the transaction.
    fn tx_type(&self) -> TxType;

    /// Returns the kind of the transaction.
    fn tx_kind(&self) -> TxKind;

    /// Returns the input data of the transaction.
    fn input(&self) -> &Bytes;

    /// Returns the chain ID of the transaction.
    fn chain_id(&self) -> Option<u64>;

    /// Returns the blob sidecar of the transaction, if any.
    fn blob_sidecar(&self) -> Option<&BlobTransactionSidecar>;

    /// Returns the size of the transaction in bytes.
    fn size(&self) -> usize;
}

impl TransactionExt for PooledTransactionsElement {
    fn gas_limit(&self) -> u64 {
        match self {
            PooledTransactionsElement::Legacy { transaction, .. } => transaction.gas_limit,
            PooledTransactionsElement::Eip2930 { transaction, .. } => transaction.gas_limit,
            PooledTransactionsElement::Eip1559 { transaction, .. } => transaction.gas_limit,
            PooledTransactionsElement::BlobTransaction(blob_tx) => blob_tx.transaction.tx.gas_limit,
            _ => unimplemented!(),
        }
    }

    fn value(&self) -> U256 {
        match self {
            PooledTransactionsElement::Legacy { transaction, .. } => transaction.value,
            PooledTransactionsElement::Eip2930 { transaction, .. } => transaction.value,
            PooledTransactionsElement::Eip1559 { transaction, .. } => transaction.value,
            PooledTransactionsElement::BlobTransaction(blob_tx) => blob_tx.transaction.tx.value,
            _ => unimplemented!(),
        }
    }

    fn tx_type(&self) -> TxType {
        match self {
            PooledTransactionsElement::Legacy { .. } => TxType::Legacy,
            PooledTransactionsElement::Eip2930 { .. } => TxType::Eip2930,
            PooledTransactionsElement::Eip1559 { .. } => TxType::Eip1559,
            PooledTransactionsElement::BlobTransaction(_) => TxType::Eip4844,
            _ => unimplemented!(),
        }
    }

    fn tx_kind(&self) -> TxKind {
        match self {
            PooledTransactionsElement::Legacy { transaction, .. } => transaction.to,
            PooledTransactionsElement::Eip2930 { transaction, .. } => transaction.to,
            PooledTransactionsElement::Eip1559 { transaction, .. } => transaction.to,
            PooledTransactionsElement::BlobTransaction(blob_tx) => {
                TxKind::Call(blob_tx.transaction.tx.to)
            }
            _ => unimplemented!(),
        }
    }

    fn input(&self) -> &Bytes {
        match self {
            PooledTransactionsElement::Legacy { transaction, .. } => &transaction.input,
            PooledTransactionsElement::Eip2930 { transaction, .. } => &transaction.input,
            PooledTransactionsElement::Eip1559 { transaction, .. } => &transaction.input,
            PooledTransactionsElement::BlobTransaction(blob_tx) => &blob_tx.transaction.tx.input,
            _ => unimplemented!(),
        }
    }

    fn chain_id(&self) -> Option<u64> {
        match self {
            PooledTransactionsElement::Legacy { transaction, .. } => transaction.chain_id,
            PooledTransactionsElement::Eip2930 { transaction, .. } => Some(transaction.chain_id),
            PooledTransactionsElement::Eip1559 { transaction, .. } => Some(transaction.chain_id),
            PooledTransactionsElement::BlobTransaction(blob_tx) => {
                Some(blob_tx.transaction.tx.chain_id)
            }
            _ => unimplemented!(),
        }
    }

    fn blob_sidecar(&self) -> Option<&BlobTransactionSidecar> {
        match self {
            PooledTransactionsElement::BlobTransaction(blob_tx) => {
                Some(&blob_tx.transaction.sidecar)
            }
            _ => None,
        }
    }

    fn size(&self) -> usize {
        match self {
            PooledTransactionsElement::Legacy { transaction, .. } => transaction.size(),
            PooledTransactionsElement::Eip2930 { transaction, .. } => transaction.size(),
            PooledTransactionsElement::Eip1559 { transaction, .. } => transaction.size(),
            PooledTransactionsElement::BlobTransaction(blob_tx) => blob_tx.transaction.tx.size(),
            _ => unimplemented!(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct SignedConstraints {
    /// The constraints that need to be signed.
    pub message: ConstraintsMessage,
    /// The signature of the proposer sidecar.
    pub signature: FixedBytes<96>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct ConstraintsMessage {
    /// The validator publickeyt of the proposer sidecar.
    pub pubkey: ECBlsPublicKey,

    /// The consensus slot at which the constraints are valid
    pub slot: u64,

    /// Indicates whether these constraints are only valid on the top of the block.
    /// NOTE: Per slot, only 1 top-of-block bundle is valid.
    pub top: bool,

    /// The constraints that need to be signed.
    #[serde(deserialize_with = "deserialize_txs", serialize_with = "serialize_txs")]
    pub transactions: Vec<Constraint>,
}

impl ConstraintsMessage {
  pub fn build(validator_pubkey: ECBlsPublicKey, request: PreconfRequest) -> Self {
    let constraints = request.txs;
    Self {
        pubkey:validator_pubkey,
        slot: request.slot,
        transactions:constraints,
        top: false
    }
  }
  
  pub fn from_tx(validator_pubkey: ECBlsPublicKey, slot: u64, constraint: Constraint) -> Self {
    Self { pubkey:validator_pubkey, slot, top: false, transactions: vec![constraint] }
  }

  pub fn digest(&self) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(self.pubkey.to_vec());
    hasher.update(self.slot.to_le_bytes());
    hasher.update((self.top as u8).to_le_bytes());

    for constraint in &self.transactions {
        hasher.update(constraint.tx.hash());
    }

    hasher.finalize().into()
}

}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Constraint {
    pub(crate) tx: PooledTransactionsElement,
    pub(crate) sender: Option<Address>,
}

impl From<PooledTransactionsElement> for Constraint {
    fn from(tx: PooledTransactionsElement) -> Self {
        Self { tx, sender: None }
    }
}

impl Constraint {
    pub fn decode_enveloped(data: impl AsRef<[u8]>) -> eyre::Result<Self> {
        let tx = PooledTransactionsElement::decode_2718(&mut data.as_ref())?;
        Ok(Self { tx, sender: None })
    }

}

#[derive(Debug, Clone)]
pub struct CommitBoostApi {
    url: Url,
    client: Client,
    delegations: Vec<SignedDelegationMessage>
}

impl CommitBoostApi {
    pub fn new (url: Url, delegations_messages: &Vec<SignedDelegationMessage>) -> Self {
        Self {
            url,
            client: ClientBuilder::new().user_agent("interstate-boost").build().unwrap(),
            delegations: delegations_messages.clone()
        }
    }

    pub fn get_constraints_signer(&self, validator_pubkey: ECBlsPublicKey) -> Option<ECBlsPublicKey> {
        None
    }    
    
    /// Builder API
    /// Implements: <https://ethereum.github.io/builder-specs/#/Builder/status>
    async fn status(&self) -> Result<StatusCode, CommitBoostError> {
        Ok(self
            .client
            .get(self.url.join(STATUS_PATH).unwrap())
            .header("content-type", "application/json")
            .send()
            .await?
            .status())
    }

    /// Implements: <https://ethereum.github.io/builder-specs/#/Builder/registerValidator>
    async fn register_validators(
        &self,
        registrations: Vec<SignedValidatorRegistration>,
    ) -> Result<(), CommitBoostError> {
        let response = self
            .client
            .post(self.url.join(REGISTER_VALIDATORS_PATH).unwrap())
            .header("content-type", "application/json")
            .body(serde_json::to_vec(&registrations)?)
            .send()
            .await?;

        if response.status() != StatusCode::OK {
            let error = response.json::<ErrorResponse>().await?;
            return Err(CommitBoostError::FailedRegisteringValidators(error));
        }

        Ok(())
    }

    /// Implements: <https://ethereum.github.io/builder-specs/#/Builder/getHeader>
    async fn get_header(
        &self,
        params: GetHeaderParams,
    ) -> Result<SignedBuilderBid, CommitBoostError> {
        let parent_hash = format!("0x{}", hex::encode(params.parent_hash.as_ref()));
        let public_key = format!("0x{}", hex::encode(params.public_key.as_ref()));

        let response = self
            .client
            .get(self.url.join(&format!(
                "/eth/v1/builder/header/{}/{}/{}",
                params.slot, parent_hash, public_key
            )).unwrap())
            .header("content-type", "application/json")
            .send()
            .await?;

        if response.status() != StatusCode::OK {
            let error = response.json::<ErrorResponse>().await?;
            return Err(CommitBoostError::FailedGettingHeader(error));
        }

        let header = response.json::<SignedBuilderBid>().await?;

        Ok(header)
    }

    /// Implements: <https://ethereum.github.io/builder-specs/#/Builder/submitBlindedBlock>
    async fn get_payload(
        &self,
        signed_block: SignedBlindedBeaconBlock,
    ) -> Result<GetPayloadResponse, CommitBoostError> {
        let response = self
            .client
            .post(self.url.join(GET_PAYLOAD_PATH).unwrap())
            .header("content-type", "application/json")
            .body(serde_json::to_vec(&signed_block)?)
            .send()
            .await?;

        if response.status() != StatusCode::OK {
            let error = response.json::<ErrorResponse>().await?;
            return Err(CommitBoostError::FailedGettingPayload(error));
        }

        let payload = response.json().await?;

        Ok(payload)
    }


    // Constraints API
    pub async fn send_constraints(
        &self,
        constraints: &Vec<SignedConstraints>,
    ) -> Result<(), CommitBoostError> {

        tracing::debug!("constraints to be sent: {:#?}", constraints);

        let response = self
            .client
            .post(self.url.join(CONSTRAINTS_PATH).unwrap())
            .header("content-type", "application/json")
            .body(serde_json::to_vec(&constraints)?)
            .send()
            .await?;

        // let response_text = response.clone().text().await?;
        tracing::info!("response status: {}", response.status());

        // if response.status() != StatusCode::OK {
        //     let error = response.json::<ErrorResponse>().await?;
        //     return Err(CommitBoostError::FailedSubmittingConstraints(error));
        // }

        Ok(())
    }

    pub async fn send_constraints_to_be_collected(
        &self,
        constraints: &Vec<SignedConstraints>,
    ) -> Result<(), CommitBoostError> {

        let response = self
            .client
            .post(self.url.join(CONSTRAINTS_COLLECT_PATH).unwrap())
            .header("content-type", "application/json")
            .json(constraints)
            .send()
            .await?;

        // tracing::info!("response status: {}", response.status());

        if response.status() != StatusCode::OK {
            let error = response.json::<ErrorResponse>().await?;
            return Err(CommitBoostError::FailedSubmittingConstraints(error));
        }

        Ok(())
    }

    async fn get_header_with_proofs(
        &self,
        params: GetHeaderParams,
    ) -> Result<VersionedValue<SignedBuilderBid>, CommitBoostError> {
        let parent_hash = format!("0x{}", hex::encode(params.parent_hash.as_ref()));
        let public_key = format!("0x{}", hex::encode(params.public_key.as_ref()));

        let response = self
            .client
            .get(self.url.join(&format!(
                "/eth/v1/builder/header_with_proofs/{}/{}/{}",
                params.slot, parent_hash, public_key,
            )).unwrap())
            .header("content-type", "application/json")
            .send()
            .await?;

        if response.status() != StatusCode::OK {
            let error = response.json::<ErrorResponse>().await?;
            return Err(CommitBoostError::FailedGettingHeader(error));
        }

        let header = response.json::<VersionedValue<SignedBuilderBid>>().await?;

        if !matches!(header.version, Fork::Deneb) {
            return Err(CommitBoostError::InvalidFork(header.version.to_string()));
        };

        // TODO: verify proofs here?

        Ok(header)
    }

    async fn delegate(&self, signed_data: &[SignedDelegationMessage]) -> Result<(), CommitBoostError> {
        let response = self
            .client
            .post(self.url.join(PERMISSION_DELEGATE_PATH).unwrap())
            .header("content-type", "application/json")
            .body(serde_json::to_string(signed_data)?)
            .send()
            .await?;

        if response.status() != StatusCode::OK {
            let error = response.json::<ErrorResponse>().await?;
            return Err(CommitBoostError::FailedDelegating(error));
        }

        Ok(())
    }

    async fn revoke(&self, signed_data: &[SignedRevocationMessage]) -> Result<(), CommitBoostError> {
        let response = self
            .client
            .post(self.url.join(PERMISSION_REVOKE_PATH).unwrap())
            .header("content-type", "application/json")
            .body(serde_json::to_string(signed_data)?)
            .send()
            .await?;

        if response.status() != StatusCode::OK {
            let error = response.json::<ErrorResponse>().await?;
            return Err(CommitBoostError::FailedRevoking(error));
        }

        Ok(())
    }


}

#[derive(Debug, Serialize, Deserialize)]
#[serde(bound = "T: serde::Serialize + serde::de::DeserializeOwned")]
pub struct VersionedValue<T> {
    pub version: Fork,
    pub data: T,
    #[serde(flatten)]
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub meta: HashMap<String, serde_json::Value>,
}

/// Serialize a list of transactions into a sequence of hex-encoded strings.
pub fn serialize_txs<S: serde::Serializer>(
    txs: &[Constraint],
    serializer: S,
  ) -> Result<S::Ok, S::Error> {
    let mut seq = serializer.serialize_seq(Some(txs.len()))?;
    for tx in txs {
        let encoded = tx.tx.encoded_2718();
        seq.serialize_element(&hex::encode_prefixed(encoded))?;
    }
    seq.end()
  }
  
  /// Deserialize a list of transactions from a sequence of hex-encoded strings.
  pub fn deserialize_txs<'de, D>(deserializer: D) -> Result<Vec<Constraint>, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    let hex_strings = <Vec<Cow<'_, str>> as de::Deserialize>::deserialize(deserializer)?;
    let mut txs = Vec::with_capacity(hex_strings.len());
  
    for s in hex_strings {
        let data = hex::decode(s.trim_start_matches("0x")).map_err(de::Error::custom)?;
        let transaction = PooledTransactionsElement::decode_2718(&mut data.as_slice())
            .map_err(de::Error::custom)
            .map(|tx| Constraint { tx, sender: None})?;
        txs.push(transaction);
    }
  
    Ok(txs)
  }

#[cfg(test)]
mod tests {
    use super::*;
    use blst::min_pk::Signature as BlsSignature;
    use crate::{config::{Chain, ChainConfig}, keystores::BLSSig, test_utils::{default_test_transaction, get_test_config}, utils::create_random_bls_secretkey, BLS_DST_PREFIX};
    use alloy::{
        eips::eip2718::Encodable2718,
        network::{EthereumWallet, TransactionBuilder},
        primitives::{bytes, hex, keccak256, Address},
        signers::{k256::ecdsa::SigningKey, local::PrivateKeySigner},
    };
    use ethereum_consensus::crypto::PublicKey as ECECBlsPublicKey;
    use reth_primitives::TransactionSigned;
    #[derive(Debug, Clone, PartialEq)]
    struct MockTransaction {
        data: Vec<u8>,
    }

    #[test]
    fn test_constraints_message_build() {
        let signer = create_random_bls_secretkey();

        let tx_bytes = bytes!("f8678085019dc6838082520894deaddeaddeaddeaddeaddeaddeaddeaddeaddead38808360306ca06664c078fa60bd3ece050903dd295949908dd9686ec8871fa558f868e031cd39a00ed4f0b122b32b73f19230fabe6a726e2d07f84eda5beaa42a1ae1271bdee39f").to_vec();
        let tx = Constraint::decode_enveloped(tx_bytes.as_slice()).unwrap();

        let constraint = ConstraintsMessage::from_tx( ECBlsPublicKey::try_from(signer.sk_to_pk().to_bytes().as_ref()).unwrap(), 165, tx);
        let chain = ChainConfig::default();
        let digest = constraint.digest();

        let signing_root = compute_signing_root(&digest, chain.commit_boost_domain()).unwrap();
        let sig = signer.sign(signing_root.as_slice(), BLS_DST_PREFIX, &[]);
        let signature = BLSSig::from_slice(&sig.to_bytes());

        let signed_constraints = SignedConstraints { message: constraint, signature };

        // verify the signature
        let blst_sig = BlsSignature::from_bytes(signed_constraints.signature.as_ref()).unwrap();

        let signing_root_verify = compute_signing_root(&digest, chain.commit_boost_domain()).unwrap();
        let pk = blst::min_pk::PublicKey::from_bytes(signer.sk_to_pk().to_bytes().as_ref()).unwrap();

        let res = blst_sig.verify(true, signing_root_verify.as_ref(), BLS_DST_PREFIX, &[], &pk, true);

        assert!(res==blst::BLST_ERROR::BLST_SUCCESS);
    }
}


