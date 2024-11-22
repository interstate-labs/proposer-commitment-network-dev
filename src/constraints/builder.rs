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
use ::serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct GetHeaderParams {
    pub slot: u64,
    pub parent_hash: Hash32,
    #[serde(rename = "pubkey")]
    pub public_key: BlsPublicKey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Serialize, Deserialize)]
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


#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SignedBuilderBid {
    pub message: BuilderBid,
    pub signature: BlsSignature,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct BuilderBid {
    pub header: ExecutionPayloadHeader,
    pub blob_kzg_commitments: List<KzgCommitment, MAX_BLOB_COMMITMENTS_PER_BLOCK>,
    #[serde(with = "as_str")]
    pub value: U256,
    #[serde(rename = "pubkey")]
    pub public_key: BlsPublicKey,
}