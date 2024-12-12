use reqwest::{ Url, Client, ClientBuilder, StatusCode };

use ethereum_consensus::{
  builder::SignedValidatorRegistration, crypto::{KzgCommitment, PublicKey as BlsPublicKey, Signature as BlsSignature}, deneb::{self, mainnet::{BlobsBundle, SignedBlindedBeaconBlock, MAX_BLOB_COMMITMENTS_PER_BLOCK}, presets::mainnet::ExecutionPayloadHeader, Hash32}, serde::as_str, ssz::prelude::*, types::mainnet::ExecutionPayload, Fork
};
use alloy::hex;
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

use crate::error::{BuilderApiError, ErrorResponse};

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

#[derive(Clone)]
pub struct BuilderClient {
  pub url: Url,
  client: Client,
}

impl BuilderClient {
    pub fn new<U: Into<Url>>(url: U) -> Self {
        Self {
            url: url.into(),
            client: ClientBuilder::new()
                .user_agent("builder-api-extender")
                .build()
                .unwrap(),
        }
    }

    fn endpoint(&self, path: &str) -> Url {
        self.url.join(path).unwrap_or_else(|e| {
            tracing::error!(err = ?e, "Failed to join path: {} with url: {}", path, self.url);
            self.url.clone()
        })
    }

    /// Implements: <https://ethereum.github.io/builder-specs/#/Builder/status>
    pub async fn status(&self) -> Result<StatusCode, BuilderApiError> {
    Ok(self
        .client
        .get(self.endpoint(STATUS_PATH))
        .header("content-type", "application/json")
        .send()
        .await?
        .status())
}

    /// Implements: <https://ethereum.github.io/builder-specs/#/Builder/registerValidator>
    pub async fn register_validators(
        &self,
        registrations: Vec<SignedValidatorRegistration>,
    ) -> Result<(), BuilderApiError> {
        let response = self
            .client
            .post(self.endpoint(REGISTER_VALIDATORS_PATH))
            .header("content-type", "application/json")
            .body(serde_json::to_vec(&registrations)?)
            .send()
            .await?;

        if response.status() != StatusCode::OK {
            let error = response.json::<ErrorResponse>().await?;
            return Err(BuilderApiError::FailedRegisteringValidators(error));
        }

        Ok(())
    }

    /// Implements: <https://ethereum.github.io/builder-specs/#/Builder/getHeader>
    pub async fn get_header(
        &self,
        params: GetHeaderParams,
    ) -> Result<VersionedValue<SignedBuilderBid>, BuilderApiError> {
    let parent_hash = format!("0x{}", hex::encode(params.parent_hash.as_ref()));
    let public_key = format!("0x{}", hex::encode(params.public_key.as_ref()));
    
    let response = self
        .client
        .get(self.endpoint(&format!(
            "/eth/v1/builder/header/{}/{}/{}",
            params.slot, parent_hash, public_key
        )))
        .header("content-type", "application/json")
        .send()
        .await?;

    if response.status() != StatusCode::OK {
        let error = response.json::<ErrorResponse>().await?;
        return Err(BuilderApiError::FailedGettingHeader(error));
    }

    let header = response.json::<VersionedValue<SignedBuilderBid>>().await?;

    Ok(header)
}

    /// Implements: <https://ethereum.github.io/builder-specs/#/Builder/submitBlindedBlock>
    pub async fn get_payload(
        &self,
        signed_block: SignedBlindedBeaconBlock,
    ) -> Result<GetPayloadResponse, BuilderApiError> {
      let response = self
          .client
          .post(self.endpoint(GET_PAYLOAD_PATH))
          .header("content-type", "application/json")
          .body(serde_json::to_vec(&signed_block)?)
          .send()
          .await?;

      if response.status() != StatusCode::OK {
          let error = response.json::<ErrorResponse>().await?;
          return Err(BuilderApiError::FailedGettingPayload(error));
      }

      let payload = response.json().await?;

      Ok(payload)
  }

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

#[derive(Debug, Deserialize, Clone)]
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(bound = "T: serde::Serialize + serde::de::DeserializeOwned")]
pub struct VersionedValue<T> {
    pub version: Fork,
    pub data: T,
    #[serde(flatten)]
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub meta: HashMap<String, serde_json::Value>,
}