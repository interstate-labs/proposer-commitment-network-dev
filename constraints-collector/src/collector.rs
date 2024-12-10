use alloy::hex;
use parking_lot::RwLock;

use ethereum_consensus::{
    builder::SignedValidatorRegistration, crypto::{KzgCommitment, PublicKey as BlsPublicKey, Signature as BlsSignature}, deneb::{self, mainnet::{BlobsBundle, SignedBlindedBeaconBlock, MAX_BLOB_COMMITMENTS_PER_BLOCK}, presets::mainnet::ExecutionPayloadHeader, Hash32}, serde::as_str, ssz::prelude::*, types::mainnet::ExecutionPayload, Fork
  };
  
use reqwest::{ StatusCode, Client as ReqwestClient, Url, ClientBuilder };
use std::{collections::HashMap, sync::Arc};
use tracing::error;

use serde::{Deserialize, Serialize};

use crate::{
  SignedConstraints,
  STATUS_PATH, REGISTER_VALIDATORS_PATH, GET_HEADER_PATH, GET_PAYLOAD_PATH, CONSTRAINTS_PATH,
  CollectorError,
  ErrorResponse
};

/// A thread-safe collector for storing constraints.
#[derive(Clone, Debug)]
pub struct ConstraintsCollector {
    pub collected_constraints: Arc<RwLock<HashMap<u64, Vec<SignedConstraints>>>>,
    cb_client: ReqwestClient,
    cb_url: Url
}



impl ConstraintsCollector {
    pub fn new(url: Url) -> Self {
        Self { 
          collected_constraints: Default::default(), 
          cb_client: ClientBuilder::new().user_agent("constraints-collector").build().unwrap(), 
          cb_url: url 
        }
    }

    /// Removes all constraints before the given slot.
    pub fn remove_before_constraints(&self, slot: u64) {
        self.collected_constraints.write().retain(|k, _| *k >= slot);
    }

    /// Gets and removes the constraints for the given slot.
    pub fn remove_constraints(&self, slot: u64) -> Option<Vec<SignedConstraints>> {
        self.collected_constraints.write().remove(&slot)
    }

    //// Implements build apis
    /// Implements: <https://ethereum.github.io/builder-specs/#/Builder/status>
    pub async fn status(&self) -> Result<StatusCode, CollectorError> {
          Ok(self
              .cb_client
              .get(self.cb_url.join(STATUS_PATH).unwrap())
              .header("content-type", "application/json")
              .send()
              .await?
              .status())
      }
  
    /// Implements: <https://ethereum.github.io/builder-specs/#/Builder/registerValidator>
    pub async fn register_validators(
        &self,
        registrations: Vec<SignedValidatorRegistration>,
    ) -> Result<(), CollectorError> {
        let response = self
            .cb_client
            .post(self.cb_url.join(REGISTER_VALIDATORS_PATH).unwrap())
            .header("content-type", "application/json")
            .body(serde_json::to_vec(&registrations)?)
            .send()
            .await?;

        if response.status() != StatusCode::OK {
            let error = response.json::<ErrorResponse>().await?;
            return Err(CollectorError::FailedRegisteringValidators(error));
        }

        Ok(())
    }

    /// Implements: <https://ethereum.github.io/builder-specs/#/Builder/submitBlindedBlock>
    pub async fn get_payload(
        &self,
        signed_block: SignedBlindedBeaconBlock,
    ) -> Result<GetPayloadResponse, CollectorError> {
        let response = self
            .cb_client
            .post(self.cb_url.join(GET_PAYLOAD_PATH).unwrap())
            .header("content-type", "application/json")
            .body(serde_json::to_vec(&signed_block)?)
            .send()
            .await?;

        if response.status() != StatusCode::OK {
            let error = response.json::<ErrorResponse>().await?;
            return Err(CollectorError::FailedGettingPayload(error));
        }

        let payload = response.json().await?;

        Ok(payload)
    }

    pub async fn send_constraints(
        &self,
        slot: u64,
    ) -> Result<(), CollectorError> {

        let constraints_store = self.collected_constraints.read().clone();

        let Some(collected_constraints) = constraints_store.get(&slot) else {
          return Err(
            CollectorError::FailedSubmittingConstraints(ErrorResponse::new(500, "Failed in getting collected constraints".to_string()))
          );
        };

        tracing::debug!("collected constraints to be sent: {:#?}", collected_constraints);

        let response = self
        .cb_client
        .post(self.cb_url.join(CONSTRAINTS_PATH).unwrap())
        .header("content-type", "application/json")
        .body(serde_json::to_vec(collected_constraints)?)
        .send()
        .await?;

        if response.status() != StatusCode::OK {
            let error = response.json::<ErrorResponse>().await?;
            return Err(CollectorError::FailedSubmittingConstraints(error));
        }


        Ok(())
    }

    pub async fn get_header_with_proofs(
        &self,
        params: GetHeaderParams,
    ) -> Result<VersionedValue<SignedBuilderBid>, CollectorError> {
          let parent_hash = format!("0x{}", hex::encode(params.parent_hash.as_ref()));
          let public_key = format!("0x{}", hex::encode(params.public_key.as_ref()));
  
          let response = self
              .cb_client
              .get(self.cb_url.join(&format!(
                  "/eth/v1/builder/header_with_proofs/{}/{}/{}",
                  params.slot, parent_hash, public_key,
              )).unwrap())
              .header("content-type", "application/json")
              .send()
              .await?;
  
          if response.status() != StatusCode::OK {
              let error = response.json::<ErrorResponse>().await?;
              return Err(CollectorError::FailedGettingHeader(error));
          }
  
          let header = response.json::<VersionedValue<SignedBuilderBid>>().await?;
  
          if !matches!(header.version, Fork::Deneb) {
              return Err(CollectorError::InvalidFork(header.version.to_string()));
          };
  
          // TODO: verify proofs here?
  
          Ok(header)
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