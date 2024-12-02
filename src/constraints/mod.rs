use std::{collections::HashMap, net::SocketAddr, str::FromStr, sync::Arc};

use alloy::{hex, primitives::{keccak256, Address, FixedBytes}};
use axum::{Router, extract::{Path, State}, response::Html, routing::{ get, post }, Json};
use builder::{GetHeaderParams, GetPayloadResponse, SignedBuilderBid};

use reth_primitives::PooledTransactionsElement;

use ethereum_consensus::{ builder::SignedValidatorRegistration, deneb::mainnet::SignedBlindedBeaconBlock, Fork,};

use serde::{Deserialize, Serialize};

use reqwest::{ Client, ClientBuilder, StatusCode, Url };

use crate::{commitment::{request::{serialize_tx, PreconfRequest}}, errors::{CommitBoostError, ErrorResponse}};
use crate::config::Config;

mod builder;
mod block_builder;
mod signature;
mod constraints_proxy_server;

pub use builder::FallbackBuilder;
pub use constraints_proxy_server::{run_constraints_proxy_server, FallbackPayloadFetcher, FetchPayloadRequest};

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

#[derive(Serialize, Debug, Clone, PartialEq, Default)]
pub struct SignedConstraints {
    /// The constraints that need to be signed.
    pub message: ConstraintsMessage,
    /// The signature of the proposer sidecar.
    pub signature: FixedBytes<96>,
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
    #[serde(rename(serialize = "tx"), serialize_with = "serialize_tx")]
    pub(crate) transaction: PooledTransactionsElement,
    pub(crate) sender: Address,
}

impl Constraint {
   pub fn from_transaction(
      tx: PooledTransactionsElement,
      index: Option<u64>,
      sender: Address,
    ) -> Self {
        Self {
            transaction:tx,
            index,
            sender,
        }
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        let mut data = Vec::new();
        self.transaction.encode_enveloped(&mut data);
        data.extend_from_slice(&self.index.unwrap_or(0).to_le_bytes());
        data
    }
}

#[derive(Debug, Clone)]
pub struct CommitBoostApi {
    url: Url,
    client: Client
}

impl CommitBoostApi {
    pub fn new (url: Url) -> Self {
        Self {
            url,
            client: ClientBuilder::new().user_agent("interstate-boost").build().unwrap()
        }
    }

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

        // let response_text = response.text().await?;
        tracing::info!("response status: {}", response.status());

        // if response.status() != StatusCode::OK {
        //     let error = response.json::<ErrorResponse>().await?;
        //     return Err(CommitBoostError::FailedSubmittingConstraints(error));
        // }

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

pub async fn run_commit_booster (config:&Config) -> CommitBoostApi {

    //TODO: replace commit-boost url
    let commit_booster: Arc<CommitBoostApi> = Arc::new(CommitBoostApi::new(config.commit_boost_url.clone()));

    let router = Router::new()
    .route("/", get(description))
    .route(STATUS_PATH, get(status))
    .route(
        REGISTER_VALIDATORS_PATH,
        post(register_validators),
    )
    .route(GET_HEADER_PATH, get(get_header))
    .route(GET_PAYLOAD_PATH, post(get_payload))
    .with_state(commit_booster);

    let addr: SocketAddr = SocketAddr::from(([0,0,0,0], config.builder_port));

    //TODO: replace a listening port as a builder
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    tokio::spawn( async {
        axum::serve(listener, router).await.unwrap();
    });

    tracing::info!("commit boost server is listening on .. {}", addr);

    CommitBoostApi::new(config.commit_boost_url.clone())
}

async fn description() -> Html<& 'static str> {
    Html("This is an endpoint to interact with commit-boost")
}

async fn status(State(api):State<Arc<CommitBoostApi>>) -> StatusCode {
    tracing::debug!("handling STATUS request");

    let status = match api.status().await {
        Ok(status) => status,
        Err(err) => {
            tracing::error!(%err, "Failed in getting status from commit-boost");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };

    status
}

async fn get_header( State(api):State<Arc<CommitBoostApi>>, Path(params): Path<GetHeaderParams>) -> Result<Json<VersionedValue<SignedBuilderBid>>, CommitBoostError> {
    tracing::debug!("handling GET_HEADER request");
    match api.get_header_with_proofs(params).await {
        Ok(header) => {
            return Ok(Json(header));
        },
        Err(err) => {
            tracing::error!("Failed in getting header with proof from commit-boost");
            return Err(err);
        }
    }
}

async fn get_payload( State(api): State<Arc<CommitBoostApi>>, Json(signed_blinded_block):Json<SignedBlindedBeaconBlock>) -> Result<Json<GetPayloadResponse>, CommitBoostError> {
    tracing::debug!("handling GET_PAYLOAD request");

    match api
            .get_payload(signed_blinded_block)
            .await
            .map(Json)
            .map_err(|e| {
                tracing::error!(%e, "Failed to get payload from mev-boost");
                e
            })
    {
        Ok(payload) => return Ok(payload),
        Err(err) => {
            tracing::error!("Failed in getting payload from commit-boost");
            return Err(err);
        }
    };
}

async fn register_validators( State(api):State<Arc<CommitBoostApi>>, Json(registors):Json<Vec<SignedValidatorRegistration>>) -> Result<StatusCode, CommitBoostError> {
    tracing::debug!("handling REGISTER_VALIDATORS_REQUEST");
    api.register_validators(registors).await.map(|_| StatusCode::OK)
}