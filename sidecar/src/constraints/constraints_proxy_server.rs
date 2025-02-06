use axum::{
    body::{self, Body},
    extract::{ConnectInfo, Path, Request, State},
    middleware::Next,
    response::Html,
    routing::{get, post},
    Json, Router,
};
use axum_client_ip::{InsecureClientIp, SecureClientIp};
use parking_lot::Mutex;
use reqwest::{StatusCode, Url};
use std::{net::{IpAddr, SocketAddr}, sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot};

use ethereum_consensus::{
    builder::SignedValidatorRegistration, deneb::mainnet::SignedBlindedBeaconBlock, Fork,
};
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;

use crate::{
    config::Config,
    constraints::{
        CommitBoostApi, GET_HEADER_PATH, GET_PAYLOAD_PATH, REGISTER_VALIDATORS_PATH, STATUS_PATH,
    },
    delegation::load_signed_delegations,
    errors::CommitBoostError,
};

use super::{
    builder::{GetHeaderParams, GetPayloadResponse, PayloadAndBid, SignedBuilderBid},
    VersionedValue,
};

const MAX_BLINDED_BLOCK_LENGTH: usize = 1024 * 1024;

const GET_HEADER_WITH_PROOFS_TIMEOUT: Duration = Duration::from_millis(500);

pub async fn run_constraints_proxy_server<P>(
    config: &Config,
    fallback_payload_fetcher: P,
) -> eyre::Result<CommitBoostApi>
where
    P: PayloadFetcher + Send + Sync + 'static,
{
    let mut delegations = Vec::new();
    if let Some(delegations_path) = &config.delegations_path {
        match load_signed_delegations(delegations_path) {
            Ok(contents) => {
                tracing::info!("Loaded {} delegations", contents.len());
                delegations.extend(contents);
            }
            Err(e) => {
                tracing::error!(%e, "Failed to load delegations");
            }
        }
    }

    let commit_boost_api: CommitBoostApi =
        CommitBoostApi::new(config.collector_url.clone(), &delegations);
    let proxy_server = Arc::new(ConstraintsAPIProxyServer::new(
        commit_boost_api.clone(),
        fallback_payload_fetcher,
        config.beacon_api_url.clone()
    ));

    let router = Router::new()
        .route("/", get(description))
        .route(STATUS_PATH, get(ConstraintsAPIProxyServer::status))
        .route(
            REGISTER_VALIDATORS_PATH,
            post(ConstraintsAPIProxyServer::register_validators),
        )
        .route(GET_HEADER_PATH, get(ConstraintsAPIProxyServer::get_header))
        .route(
            GET_PAYLOAD_PATH,
            post(ConstraintsAPIProxyServer::get_payload),
        )
        .layer(ServiceBuilder::new().layer(TraceLayer::new_for_http()))
        .with_state(proxy_server);

    let addr: SocketAddr = SocketAddr::from(([0, 0, 0, 0], config.builder_port));

    //TODO: replace a listening port as a builder
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    tokio::spawn(async {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await.unwrap();
    });

    tracing::info!("commit boost server is listening on .. {}", addr);

    Ok(commit_boost_api)
}
pub struct ConstraintsAPIProxyServer<P> {
    proxier: CommitBoostApi,
    fallback_payload: Mutex<Option<GetPayloadResponse>>,
    fallback_bid: Mutex<Option<SignedBuilderBid>>,
    payload_fetcher: P,
    beacon_api_url: Url,
}

impl<P> ConstraintsAPIProxyServer<P>
where
    P: PayloadFetcher + Send + Sync,
{
    pub fn new(proxier: CommitBoostApi, payload_fetcher: P, beacon_api_url: Url) -> Self {
        Self {
            proxier,
            fallback_payload: Mutex::new(None),
            fallback_bid: Mutex::new(None),
            payload_fetcher,
            beacon_api_url,
        }
    }

    fn is_ip_in_url(beacon_api_url: &Url, my_socket_addr: SocketAddr) -> bool {
        let my_ip = my_socket_addr.ip(); // Extract only the IP from SocketAddr
    
        match beacon_api_url.host() {
            Some(url::Host::Ipv4(ip)) => ip == my_ip,
            Some(url::Host::Ipv6(ip)) => ip == my_ip,
            _ => false, // URL has a domain name, not an IP
        }
    }
    
    async fn status(
        ConnectInfo(addr): ConnectInfo<SocketAddr>,
        State(server): State<Arc<ConstraintsAPIProxyServer<P>>>,
    ) -> StatusCode {
        tracing::debug!(?addr, "handling STATUS request");
        if Self::is_ip_in_url(&server.beacon_api_url, addr) == false {
            return StatusCode::UNAUTHORIZED
        }
        let status = match server.proxier.status().await {
            Ok(status) => status,
            Err(err) => {
                tracing::error!(%err, "Failed in getting status from commit-boost");
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        status
    }

    async fn get_header(
        ConnectInfo(addr): ConnectInfo<SocketAddr>,
        State(server): State<Arc<ConstraintsAPIProxyServer<P>>>,
        Path(params): Path<GetHeaderParams>,
    ) -> Result<Json<VersionedValue<SignedBuilderBid>>, CommitBoostError> {
        tracing::debug!("handling GET_HEADER request");
        if Self::is_ip_in_url(&server.beacon_api_url, addr) == false {
            return Err(CommitBoostError::Unauthorized("".to_string()));
        }
        let slot = params.slot;
        match tokio::time::timeout(
            GET_HEADER_WITH_PROOFS_TIMEOUT,
            server.proxier.get_header_with_proofs(params),
        )
        .await
        {
            Ok(header) => {
                let mut fallback_payload = server.fallback_payload.lock();
                *fallback_payload = None;

                tracing::debug!(?header, "got valid proofs of header");
                return Ok(Json(header?));
            }
            Err(err) => {
                tracing::error!(
                    ?err,
                    "Failed in getting header with proof from commit-boost"
                );
            }
        };

        // let Some(payload_and_bid) = server.payload_fetcher.fetch_payload(slot).await else {
        //   tracing::debug!("No fallback payload for slot {slot}");
        //   return Err(CommitBoostError::FailedToFetchLocalPayload(slot));
        // };

        let payload_and_bid = server.payload_fetcher.fetch_payload(slot).await.unwrap();

        {
            // Cache both the payload and the bid
            let mut local_payload = server.fallback_payload.lock();
            *local_payload = Some(payload_and_bid.payload.clone());

            let mut local_bid = server.fallback_bid.lock();
            *local_bid = Some(payload_and_bid.bid.clone());
        }

        let hash = payload_and_bid.bid.message.header.block_hash.clone();
        let number = payload_and_bid.bid.message.header.block_number;
        tracing::debug!( %hash, "Fetched local payload for slot {slot}");

        {
            // Since we've signed a local header, set the payload for
            // the following `get_payload` request.
            let mut local_payload = server.fallback_payload.lock();
            *local_payload = Some(payload_and_bid.payload);
        }

        let versioned_bid = VersionedValue::<SignedBuilderBid> {
            version: Fork::Deneb,
            data: payload_and_bid.bid,
            meta: Default::default(),
        };

        tracing::info!(%hash, number, ?versioned_bid, "Returned a fallback payload header");
        Ok(Json(versioned_bid))
    }

    async fn get_payload(
        ConnectInfo(addr): ConnectInfo<SocketAddr>,
        State(server): State<Arc<ConstraintsAPIProxyServer<P>>>,
        req: Request<Body>,
    ) -> Result<Json<GetPayloadResponse>, CommitBoostError> {
        tracing::debug!("handling GET_PAYLOAD request");
        if Self::is_ip_in_url(&server.beacon_api_url, addr) == false {
            return Err(CommitBoostError::Unauthorized("".to_string()));
        }
        let body_bytes = body::to_bytes(req.into_body(), MAX_BLINDED_BLOCK_LENGTH)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to read request body");
                e
            })?;

        // Convert to signed blinded beacon block
        let signed_blinded_block = serde_json::from_slice::<SignedBlindedBeaconBlock>(&body_bytes)
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to parse signed blinded block");
                e
            })?;
        // If we have a locally built payload, it means we signed a local header.
        // Return it and clear the cache.
        // if let Some(local_payload) = server.fallback_payload.lock().take() {
        //     check_locally_built_payload_integrity(&signed_blinded_block, &local_payload)?;

        //     tracing::debug!("Valid local block found, returning: {local_payload:?}");
        //     return Ok(Json(local_payload));
        // }

        if let (Some(local_payload), Some(local_bid)) = (
            server.fallback_payload.lock().as_ref().cloned(),
            server.fallback_bid.lock().as_ref().cloned(),
        ) {
            check_locally_built_payload_integrity(&signed_blinded_block, &local_payload)?;

            tracing::debug!("Valid local block found, returning: {local_payload:?}");
            return Ok(Json(local_payload));
        }

        match server
            .proxier
            .get_payload(signed_blinded_block)
            .await
            .map(Json)
            .map_err(|e| {
                tracing::error!(%e, "Failed to get payload from mev-boost");
                e
            }) {
            Ok(payload) => return Ok(payload),
            Err(err) => {
                tracing::error!("Failed in getting payload from commit-boost");
                return Err(err);
            }
        };
    }

    async fn register_validators(
        ConnectInfo(addr): ConnectInfo<SocketAddr>,
        State(server): State<Arc<ConstraintsAPIProxyServer<P>>>,
        Json(registers): Json<Vec<SignedValidatorRegistration>>,
    ) -> Result<StatusCode, CommitBoostError> {
        tracing::debug!("handling REGISTER_VALIDATORS_REQUEST");
        if Self::is_ip_in_url(&server.beacon_api_url, addr) == false {
            return Err(CommitBoostError::Unauthorized("".to_string()));
        }
        server
            .proxier
            .register_validators(registers)
            .await
            .map(|_| StatusCode::OK)
    }
}

async fn description() -> Html<&'static str> {
    Html("This is an endpoint to interact with commit-boost")
}

#[derive(Debug)]
pub struct FetchPayloadRequest {
    pub slot: u64,
    pub response_tx: oneshot::Sender<Option<PayloadAndBid>>,
}

#[derive(Debug, Clone)]
pub struct FallbackPayloadFetcher {
    tx: mpsc::Sender<FetchPayloadRequest>,
}

impl FallbackPayloadFetcher {
    pub fn new(tx: mpsc::Sender<FetchPayloadRequest>) -> Self {
        Self { tx }
    }
}

#[async_trait::async_trait]
impl PayloadFetcher for FallbackPayloadFetcher {
    async fn fetch_payload(&self, slot: u64) -> Option<PayloadAndBid> {
        let (response_tx, response_rx) = oneshot::channel();

        let fetch_params = FetchPayloadRequest { response_tx, slot };
        self.tx.send(fetch_params).await.ok()?;

        match response_rx.await {
            Ok(res) => res,
            Err(e) => {
                tracing::error!(err = ?e, "Failed to fetch payload");
                None
            }
        }
    }
}

#[async_trait::async_trait]
pub trait PayloadFetcher {
    async fn fetch_payload(&self, slot: u64) -> Option<PayloadAndBid>;
}

#[derive(Debug)]
pub struct NoopPayloadFetcher;

#[async_trait::async_trait]
impl PayloadFetcher for NoopPayloadFetcher {
    async fn fetch_payload(&self, slot: u64) -> Option<PayloadAndBid> {
        tracing::info!(slot, "Fetch payload called");
        None
    }
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum LocalPayloadIntegrityError {
    #[error(
        "Locally built payload does not match signed header. 
        {field_name} mismatch: expected {expected}, have {have}"
    )]
    FieldMismatch {
        field_name: String,
        expected: String,
        have: String,
    },
}

/// Helper macro to compare fields of the signed header and the local block.
macro_rules! assert_payload_fields_eq {
    ($expected:expr, $have:expr, $field_name:ident) => {
        if $expected != $have {
            tracing::error!(
                field_name = stringify!($field_name),
                expected = %$expected,
                have = %$have,
                "Local block does not match signed header"
            );
            return Err(LocalPayloadIntegrityError::FieldMismatch {
                field_name: stringify!($field_name).to_string(),
                expected: $expected.to_string(),
                have: $have.to_string(),
            });
        }
    };
}

fn check_locally_built_payload_integrity(
    signed_blinded_block: &SignedBlindedBeaconBlock,
    local_payload: &GetPayloadResponse,
) -> Result<(), LocalPayloadIntegrityError> {
    let header_signed_by_cl = &signed_blinded_block.message.body.execution_payload_header;
    let local_execution_payload = local_payload.execution_payload();

    assert_payload_fields_eq!(
        &header_signed_by_cl.block_hash,
        local_execution_payload.block_hash(),
        BlockHash
    );

    assert_payload_fields_eq!(
        header_signed_by_cl.block_number,
        local_execution_payload.block_number(),
        BlockNumber
    );

    assert_payload_fields_eq!(
        &header_signed_by_cl.state_root,
        local_execution_payload.state_root(),
        StateRoot
    );

    assert_payload_fields_eq!(
        &header_signed_by_cl.receipts_root,
        local_execution_payload.receipts_root(),
        ReceiptsRoot
    );

    assert_payload_fields_eq!(
        &header_signed_by_cl.prev_randao,
        local_execution_payload.prev_randao(),
        PrevRandao
    );

    assert_payload_fields_eq!(
        &header_signed_by_cl.gas_limit,
        &local_execution_payload.gas_limit(),
        GasLimit
    );

    assert_payload_fields_eq!(
        &header_signed_by_cl.gas_used,
        &local_execution_payload.gas_used(),
        GasUsed
    );

    assert_payload_fields_eq!(
        &header_signed_by_cl.timestamp,
        &local_execution_payload.timestamp(),
        Timestamp
    );

    assert_payload_fields_eq!(
        &header_signed_by_cl.extra_data,
        local_execution_payload.extra_data(),
        ExtraData
    );

    assert_payload_fields_eq!(
        &header_signed_by_cl.base_fee_per_gas,
        local_execution_payload.base_fee_per_gas(),
        BaseFeePerGas
    );

    assert_payload_fields_eq!(
        &header_signed_by_cl.parent_hash,
        local_execution_payload.parent_hash(),
        ParentHash
    );

    assert_payload_fields_eq!(
        &header_signed_by_cl.fee_recipient,
        local_execution_payload.fee_recipient(),
        FeeRecipient
    );

    assert_payload_fields_eq!(
        &header_signed_by_cl.logs_bloom,
        local_execution_payload.logs_bloom(),
        LogsBloom
    );

    assert_payload_fields_eq!(
        &header_signed_by_cl.blob_gas_used,
        &local_execution_payload.blob_gas_used().unwrap_or_default(),
        BlobGasUsed
    );

    assert_payload_fields_eq!(
        &header_signed_by_cl.excess_blob_gas,
        &local_execution_payload
            .excess_blob_gas()
            .unwrap_or_default(),
        ExcessBlobGas
    );

    // TODO: Sanity check: recalculate transactions and withdrawals roots
    // and assert them against the header

    // TODO: Sanity check: verify the validator signature
    // signed_blinded_block.verify_signature()?;

    Ok(())
}
