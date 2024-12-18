use alloy::hex::hex;
use ethereum_consensus::{
  crypto::{KzgCommitment, PublicKey as BlsPublicKey, Signature as BlsSignature},
  deneb::{
      self,
      mainnet::{BlobsBundle, MAX_BLOB_COMMITMENTS_PER_BLOCK},
      presets::mainnet::ExecutionPayloadHeader,
      Hash32,
  },
  serde::as_str,
  ssz::prelude::*,
  types::mainnet::ExecutionPayload,
  Fork,
};
use blst::min_pk::SecretKey as BLSSecretKey;

use crate::config::{ChainConfig, Config};
use crate::state::Block;

use super::{
    block_builder:: { 
        create_consensus_execution_payload, 
        create_execution_payload_header, 
        BlockBuilder
    },
    signature::sign_builder_message};

#[derive(Debug, serde::Deserialize)]
pub struct GetHeaderParams {
    pub slot: u64,
    pub parent_hash: Hash32,
    #[serde(rename = "pubkey")]
    pub public_key: BlsPublicKey,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PayloadAndBlobs {
    pub execution_payload: ExecutionPayload,
    pub blobs_bundle: BlobsBundle,
}

impl Default for PayloadAndBlobs {
    fn default() -> Self {
        Self {
            execution_payload: ExecutionPayload::Deneb(deneb::ExecutionPayload::default()),
            blobs_bundle: BlobsBundle::default(),
        }
    }
}

#[derive(Debug)]
pub struct PayloadAndBid {
    pub bid: SignedBuilderBid,
    pub payload: GetPayloadResponse,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "version", content = "data")]
pub enum GetPayloadResponse {
    #[serde(rename = "bellatrix")]
    Bellatrix(ExecutionPayload),
    #[serde(rename = "capella")]
    Capella(ExecutionPayload),
    #[serde(rename = "deneb")]
    Deneb(PayloadAndBlobs),
    #[serde(rename = "electra")]
    Electra(PayloadAndBlobs),
}

impl GetPayloadResponse {
    pub fn block_hash(&self) -> &Hash32 {
        match self {
            GetPayloadResponse::Capella(payload) => payload.block_hash(),
            GetPayloadResponse::Bellatrix(payload) => payload.block_hash(),
            GetPayloadResponse::Deneb(payload) => payload.execution_payload.block_hash(),
            GetPayloadResponse::Electra(payload) => payload.execution_payload.block_hash(),
        }
    }

    pub fn execution_payload(&self) -> &ExecutionPayload {
        match self {
            GetPayloadResponse::Capella(payload) => payload,
            GetPayloadResponse::Bellatrix(payload) => payload,
            GetPayloadResponse::Deneb(payload) => &payload.execution_payload,
            GetPayloadResponse::Electra(payload) => &payload.execution_payload,
        }
    }
}

impl From<PayloadAndBlobs> for GetPayloadResponse {
    fn from(payload_and_blobs: PayloadAndBlobs) -> Self {
        match payload_and_blobs.execution_payload.version() {
            Fork::Phase0 => GetPayloadResponse::Capella(payload_and_blobs.execution_payload),
            Fork::Altair => GetPayloadResponse::Capella(payload_and_blobs.execution_payload),
            Fork::Capella => GetPayloadResponse::Capella(payload_and_blobs.execution_payload),
            Fork::Bellatrix => GetPayloadResponse::Bellatrix(payload_and_blobs.execution_payload),
            Fork::Deneb => GetPayloadResponse::Deneb(payload_and_blobs),
            Fork::Electra => GetPayloadResponse::Electra(payload_and_blobs),
        }
    }
}


#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct SignedBuilderBid {
    pub message: BuilderBid,
    pub signature: BlsSignature,
}

#[derive(Debug, Default, Clone, SimpleSerialize, serde::Serialize, serde::Deserialize)]
pub struct BuilderBid {
    pub header: ExecutionPayloadHeader,
    pub blob_kzg_commitments: List<KzgCommitment, MAX_BLOB_COMMITMENTS_PER_BLOCK>,
    #[serde(with = "as_str")]
    pub value: U256,
    #[serde(rename = "pubkey")]
    pub public_key: BlsPublicKey,
}

pub struct FallbackBuilder {
    // be used to sign the block bid
    bls_secret_key: BLSSecretKey,
    // chain config
    chain: ChainConfig,
    // block generator
    block_builder: BlockBuilder,
    // the last built block with bid
    payload_and_bid: Option<PayloadAndBid>
}

impl FallbackBuilder {
    pub fn new(config: &Config) -> Self {
        Self {
            bls_secret_key: config.builder_bls_private_key.clone(),
            chain: config.chain.clone(),
            block_builder: BlockBuilder::new(config),
            payload_and_bid: None
        }  
    }

    pub async fn build_fallback_payload( &mut self, block: &Block) -> Result<(), BuilderError> {
        let transactions = block.convert_constraints_to_transactions();
        let blobs_bundle = block.parse_to_blobs_bundle();
        let kzg_commitments = blobs_bundle.commitments.clone();

        // 1. build a fallback payload with the given transactions, on top of
        // the current head of the chain
        let sealed_block = self
            .block_builder
            .build_sealed_block(&transactions)
            .await?;

        // NOTE: we use a big value for the bid to ensure it gets chosen by mev-boost.
        // the client has no way to actually verify this, and we don't need to trust
        // an external relay as this block is self-built, so the fake bid value is fine.
        //
        // NOTE: we don't strictly need this. The validator & beacon nodes have options
        // to ALWAYS prefer PBS blocks. This is a safety measure that doesn't hurt to keep.
        let value = U256::from(1_000_000_000_000_000_000u128);

        let eth_payload = create_consensus_execution_payload(&sealed_block);
        let payload_and_blobs = PayloadAndBlobs {
            execution_payload: eth_payload,
            blobs_bundle,
        };

        // 2. create a signed builder bid with the sealed block header we just created
        let eth_header = create_execution_payload_header(&sealed_block, transactions);

        // 3. sign the bid with the local builder's BLS key
        let signed_bid = self.create_signed_builder_bid(value, eth_header, kzg_commitments)?;

        // 4. prepare a get_payload response for when the beacon node will ask for it
        let get_payload_response = GetPayloadResponse::from(payload_and_blobs);

        self.payload_and_bid = Some(PayloadAndBid {
            bid: signed_bid,
            payload: get_payload_response,
        });

        Ok(())
    }
    
    /// Get the cached payload and bid from the local builder, consuming the value.
    #[inline]
    pub fn get_cached_payload(&mut self) -> Option<PayloadAndBid> {
        self.payload_and_bid.take()
    }
    
    /// transform a sealed header into a signed builder bid using
    /// the local builder's BLS key.
    fn create_signed_builder_bid(
        &self,
        value: U256,
        header: ExecutionPayloadHeader,
        blob_kzg_commitments: Vec<KzgCommitment>,
    ) -> Result<SignedBuilderBid, BuilderError> {
        // compat: convert from blst to ethereum consensus types
        let pubkey = self.bls_secret_key.sk_to_pk().to_bytes();
        let consensus_pubkey = BlsPublicKey::try_from(pubkey.as_slice()).expect("valid pubkey bytes");
        let blob_kzg_commitments = List::try_from(blob_kzg_commitments).expect("valid list");

        let message = BuilderBid {
            header,
            blob_kzg_commitments,
            public_key: consensus_pubkey,
            value,
        };

        let signature = sign_builder_message(&self.chain, &self.bls_secret_key, &message)?;

        Ok(SignedBuilderBid { message, signature })
    }

}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
#[allow(missing_docs)]
pub enum BuilderError {
    #[error("Failed to parse from integer: {0}")]
    Parse(#[from] std::num::ParseIntError),
    #[error("Failed to de/serialize JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Failed to decode hex: {0}")]
    Hex(#[from] hex::FromHexError),
    #[error("Invalid JWT: {0}")]
    Jwt(#[from] reth_rpc_layer::JwtError),
    #[error("Failed HTTP request: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("Failed while fetching from RPC: {0}")]
    Transport(#[from] alloy::transports::TransportError),
    #[error("Failed in SSZ merkleization: {0}")]
    Merkleization(#[from] MerkleizationError),
    #[error("Failed while interacting with beacon client: {0}")]
    BeaconApi(#[from] beacon_api_client::Error),
    #[error("Failed to parse hint from engine response: {0}")]
    InvalidEngineHint(String),
    #[error("Failed to build payload: {0}")]
    Custom(String),
}

