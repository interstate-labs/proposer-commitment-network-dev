use alloy::{
    eips::merge::EPOCH_SLOTS,
    primitives::{utils::format_ether, B256, U256},
    rpc::types::beacon::{relay::ValidatorRegistration, BlsPublicKey},
};
use cb_pbs::{register_validator, BuilderApi, BuilderApiState, PbsState};
use reqwest::Url;
use serde_json::Value;

use async_trait::async_trait;
use axum::{
    extract::{Path, State},
    http::{header::USER_AGENT, HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use cb_common::{
    pbs::{
        error::{PbsError, ValidationError},
        GetHeaderResponse, RelayClient, SignedExecutionPayloadHeader, EMPTY_TX_ROOT_HASH,
        HEADER_START_TIME_UNIX_MS,
    },
    types::Chain,
    utils::get_user_agent_with_version,
};
use eyre::{ContextCompat, Result};
use futures::{future::join_all, stream::FuturesUnordered, StreamExt};
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio::time::sleep;
use tracing::{debug, error, info, warn, Instrument};

use crate::{
    metrics::{
        ERROR_CODE_TIMEOUT_STR, INVALID_BIDS_COUNT, LATENCY_BY_RELAY, RELAY_HTTP_STATUS,
        TAG_GET_HEADER_WITH_PROOFS,
    },
    types::ValidationContext,
};

use super::{
    constraints::ConstraintStore,
    error::PbsClientError,
    proofs::validate_multiproofs,
    types::{
        Config, FetchHeaderParams, GetHeaderWithProofsResponse, RequestConfig, SignedDelegation,
        SignedRevocation, VerifiedConstraints,
    },
};

const SUBMIT_CONSTRAINTS_ROUTE: &str = "/constraints/v1/builder/constraints";
const DELEGATE_ROUTE: &str = "/constraints/v1/builder/delegate";
const REVOKE_ROUTE: &str = "/constraints/v1/builder/revoke";
const HEADER_WITH_PROOFS_ROUTE: &str =
    "/eth/v1/builder/header_with_proofs/:slot/:parent_hash/:pubkey";

const ERROR_CODE_TIMEOUT: u16 = 555;

const SECONDS_PER_SLOT: u64 = 12;
const MILLIS_PER_SECOND: u64 = 1_000;

// State containing runtime-specific information
#[derive(Clone)]
pub struct BuilderRuntimeState {
    #[allow(unused)]
    config: Config,
    constraints: ConstraintStore,
    client: reqwest::Client,
}

impl BuilderApiState for BuilderRuntimeState {}

impl BuilderRuntimeState {
    pub fn new(settings: Config) -> Self {
        Self {
            config: settings,
            constraints: ConstraintStore::new(),
            client: reqwest::Client::new(),
        }
    }
}

/// Additional endpoints are defined in [extra_routes](ConstraintsApi::extra_routes).
pub struct ConstraintsApi;

#[async_trait]
impl BuilderApi<BuilderRuntimeState> for ConstraintsApi {
    /// Handles the registration of a validator with the builder.
    ///
    /// This function is used to periodically clean up outdated constraints.
    async fn register_validator(
        validator_registrations: Vec<ValidatorRegistration>,
        request_headers: HeaderMap,
        runtime_state: PbsState<BuilderRuntimeState>,
    ) -> eyre::Result<()> {
        let slot = fetch_current_slot_number(&runtime_state.data.config.beacon_rpc).await?;

        info!("Clearing constraints before slot {slot}");
        runtime_state.data.constraints.remove_before_constraints(slot);

        register_validator(validator_registrations, request_headers, runtime_state).await
    }

    /// Fetches the extra routes necessary for supporting the constraints API as per
    fn extra_routes() -> Option<Router<PbsState<BuilderRuntimeState>>> {
        let mut router = Router::new();
        router = router.route(SUBMIT_CONSTRAINTS_ROUTE, post(submit_constraints));
        router = router.route(DELEGATE_ROUTE, post(delegate));
        router = router.route(REVOKE_ROUTE, post(revoke));
        router = router.route(HEADER_WITH_PROOFS_ROUTE, get(get_header_with_proofs));
        Some(router)
    }
}

#[tracing::instrument(skip_all)]
async fn submit_constraints(
    State(mut state): State<PbsState<BuilderRuntimeState>>,
    Json(constraints): Json<Vec<VerifiedConstraints>>,
) -> Result<impl IntoResponse, PbsClientError> {
    info!("Sending {} constraints to the relays for processing", constraints.len());

    let current_slot = fetch_current_slot_number(&state.data.config.beacon_rpc)
        .await
        .map_err(|e| PbsClientError::BadRequest)?;

    // Save constraints for the slot to verify proofs against later.
    for signed_constraints in &constraints {
        let slot = signed_constraints.message.slot;
        info!("current_slot: {}", current_slot);
        info!("epoch_slots: {}", EPOCH_SLOTS);
        info!("received_target_ slot: {}", slot);

        // Only accept constraints for the current or next epoch.
        if slot > current_slot + EPOCH_SLOTS * 2 {
            warn!(slot, current_slot, "The constraints are scheduled for a time that is too far in the future to be processed at this moment.");
            return Err(PbsClientError::BadRequest);
        }
        info!("starting to add constraints");
        if let Err(e) =
            state.data.constraints.add_constraints(slot, signed_constraints.message.clone())
        {
            error!(slot, error = %e, "Failed to save constraints");
            return Err(PbsClientError::BadRequest);
        }
    }
    info!("starting to post to relay");
    relay_post_request(state, SUBMIT_CONSTRAINTS_ROUTE, &constraints).await?;
    Ok(StatusCode::OK)
}

/// Transfers the right to submit constraints to another BLS key.
#[tracing::instrument(skip_all)]
async fn delegate(
    State(state): State<PbsState<BuilderRuntimeState>>,
    Json(delegations): Json<Vec<SignedDelegation>>,
) -> Result<impl IntoResponse, PbsClientError> {
    info!("Received delegation request");
    relay_post_request(state, DELEGATE_ROUTE, &delegations).await?;
    Ok(StatusCode::OK)
}

/// Revokes constraint submission rights from a BLS key.
#[tracing::instrument(skip_all)]
async fn revoke(
    State(state): State<PbsState<BuilderRuntimeState>>,
    Json(revocations): Json<Vec<SignedRevocation>>,
) -> Result<impl IntoResponse, PbsClientError> {
    info!("received revoke request");
    relay_post_request(state, REVOKE_ROUTE, &revocations).await?;
    Ok(StatusCode::OK)
}

/// Fetches a header along with its proofs for a given slot and parent hash.
#[tracing::instrument(skip_all, fields(slot = params.slot))]
async fn get_header_with_proofs(
    State(mut state): State<PbsState<BuilderRuntimeState>>,
    Path(params): Path<FetchHeaderParams>,
    req_headers: HeaderMap,
) -> Result<impl IntoResponse, PbsClientError> {
    let ms_into_slot = ms_into_slot(params.slot, state.data.config.genesis_time_sec);

    let max_timeout_ms = state
        .pbs_config()
        .timeout_get_header_ms
        .min(state.pbs_config().late_in_slot_time_ms.saturating_sub(ms_into_slot));

    if max_timeout_ms == 0 {
        warn!(
            ms_into_slot,
            threshold = state.pbs_config().late_in_slot_time_ms,
            "Since the slot has progressed beyond the expected time frame, we are bypassing the relay requests at this point."
        );

        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // prepare headers, except for start time which is set in `send_one_get_header`
    let mut send_headers = HeaderMap::new();

    send_headers.insert(USER_AGENT, get_user_agent_with_version(&req_headers).unwrap());

    let relays = state.config.relays.clone();

    let mut handles = Vec::with_capacity(relays.len());
    for relay in relays.iter() {
        handles.push(send_timed_get_header(
            params,
            relay.clone(),
            state.config.chain,
            send_headers.clone(),
            ms_into_slot,
            max_timeout_ms,
            ValidationContext {
                skip_sigverify: state.pbs_config().skip_sigverify,
                min_bid_wei: state.pbs_config().min_bid_wei,
            },
        ));
    }

    let results = join_all(handles).await;
    let mut relay_bids = Vec::with_capacity(relays.len());
    let mut hash_to_proofs = HashMap::new();

    // Get and remove the constraints for this slot
    let maybe_constraints = state.data.constraints.remove_constraints(params.slot);

    for (i, res) in results.into_iter().enumerate() {
        let relay_id = relays[i].id.as_ref();

        match res {
            Ok(Some(res)) => {
                let root = res.data.header.message.header.transactions_root;

                let start = Instant::now();

                // If we have constraints to verify, do that here in order to validate the bid
                if let Some(ref constraints) = maybe_constraints {
                    // Verify the multiproofs and continue if not valid
                    if let Err(e) = validate_multiproofs(constraints, &res.data.proofs, root) {
                        error!(?e, relay_id, "Verification of the multiproof was unsuccessful, so we are opting to skip processing the bid.");
                        INVALID_BIDS_COUNT.with_label_values(&[relay_id]).inc();
                        continue;
                    }
                    let elapsed = start.elapsed();
                    tracing::info!(
                        "The multiproof has been successfully verified in {:?}",
                        elapsed
                    );
                    let message = format!("message: INTERSTATE-COMMIT-BOOST: [INTERSTATE]: verified merkle proof for slot {} in {:?}", params.slot, elapsed);
                    let url = "http://162.55.190.235:3001/events";
                    let message_json = HashMap::from([("message", message)]);
                    match state.data.client.post(url).json(&message_json).send().await {
                        Ok(_) => info!("Sent an event to the listener"),
                        Err(err) => error!("Failed to send an event to the listener: {:?}", err),
                    };

                    // Save the proofs per block hash
                    hash_to_proofs
                        .insert(res.data.header.message.header.block_hash, res.data.proofs);
                }

                let vanilla_response =
                    GetHeaderResponse { version: res.version, data: res.data.header };

                relay_bids.push(vanilla_response)
            }
            Ok(_) => {}
            Err(err) if err.is_timeout() => error!(err = "Timed Out", relay_id),
            Err(err) => error!(?err, relay_id),
        }
    }

    if let Some(header) = relay_bids.iter().max_by_key(|v| v.value()) {
        Ok((StatusCode::OK, axum::Json(header)).into_response())
    } else {
        Ok(StatusCode::NO_CONTENT.into_response())
    }

    // if let Some(winning_bid) = state.add_bids(params.slot, relay_bids) {
    //     let header = winning_bid.clone();
    //     Ok((StatusCode::OK, axum::Json(header)).into_response())
    // } else {
    //     Ok(StatusCode::NO_CONTENT.into_response())
    // }
}

#[tracing::instrument(skip_all, name = "handler", fields(relay_id = relay.id.as_ref()))]
async fn send_timed_get_header(
    params: FetchHeaderParams,
    relay: RelayClient,
    chain: Chain,
    headers: HeaderMap,
    ms_into_slot: u64,
    mut timeout_left_ms: u64,
    validation: ValidationContext,
) -> Result<Option<GetHeaderWithProofsResponse>, PbsError> {
    let url = relay.get_url(&format!(
        "/eth/v1/builder/header_with_proofs/{}/{}/{}",
        params.slot, params.parent_hash, params.pubkey
    ))?;

    if relay.config.enable_timing_games {
        if let Some(target_ms) = relay.config.target_first_request_ms {
            // sleep until target time in slot

            let delay = target_ms.saturating_sub(ms_into_slot);
            if delay > 0 {
                debug!(target_ms, ms_into_slot, "TG: waiting to send first header request");
                timeout_left_ms = timeout_left_ms.saturating_sub(delay);
                sleep(Duration::from_millis(delay)).await;
            } else {
                debug!(target_ms, ms_into_slot, "TG: request already late enough in slot");
            }
        }

        if let Some(send_freq_ms) = relay.config.frequency_get_header_ms {
            let mut handles = Vec::new();

            debug!(send_freq_ms, timeout_left_ms, "TG: sending multiple header requests");

            loop {
                handles.push(tokio::spawn(
                    send_one_get_header(
                        params,
                        relay.clone(),
                        chain,
                        RequestConfig {
                            timeout_ms: timeout_left_ms,
                            url: url.clone(),
                            headers: headers.clone(),
                        },
                        validation.clone(),
                    )
                    .in_current_span(),
                ));

                if timeout_left_ms > send_freq_ms {
                    // enough time for one more
                    timeout_left_ms = timeout_left_ms.saturating_sub(send_freq_ms);
                    sleep(Duration::from_millis(send_freq_ms)).await;
                } else {
                    break;
                }
            }

            let results = join_all(handles).await;
            let mut n_headers = 0;

            if let Some((_, maybe_header)) = results
                .into_iter()
                .filter_map(|res| {
                    // ignore join error and timeouts, log other errors
                    res.ok().and_then(|inner_res| match inner_res {
                        Ok(maybe_header) => {
                            n_headers += 1;
                            Some(maybe_header)
                        }
                        Err(err) if err.is_timeout() => None,
                        Err(err) => {
                            error!(?err, "TG: error sending header request");
                            None
                        }
                    })
                })
                .max_by_key(|(start_time, _)| *start_time)
            {
                debug!(n_headers, "TG: received headers from relay");
                return Ok(maybe_header);
            } else {
                // all requests failed
                warn!("TG: no headers received");

                return Err(PbsError::RelayResponse {
                    error_msg: "no headers received".to_string(),
                    code: ERROR_CODE_TIMEOUT,
                });
            }
        }
    }

    // if no timing games or no repeated send, just send one request
    send_one_get_header(
        params,
        relay,
        chain,
        RequestConfig { timeout_ms: timeout_left_ms, url, headers },
        validation,
    )
    .await
    .map(|(_, maybe_header)| maybe_header)
}

async fn send_one_get_header(
    params: FetchHeaderParams,
    relay: RelayClient,
    chain: Chain,
    mut req_config: RequestConfig,
    validation: ValidationContext,
) -> Result<(u64, Option<GetHeaderWithProofsResponse>), PbsError> {
    // the timestamp in the header is the consensus block time which is fixed,
    // use the beginning of the request as proxy to make sure we use only the
    // last one received
    let start_request_time = utcnow_ms();
    req_config.headers.insert(HEADER_START_TIME_UNIX_MS, HeaderValue::from(start_request_time));
    let url = req_config.url.clone();
    info!("url: {url}");
    let start_request = Instant::now();
    let res = match relay
        .client
        .get(req_config.url)
        .timeout(Duration::from_millis(req_config.timeout_ms))
        .headers(req_config.headers)
        .send()
        .await
    {
        Ok(res) => res,
        Err(err) => {
            RELAY_HTTP_STATUS
                .with_label_values(&[ERROR_CODE_TIMEOUT_STR, TAG_GET_HEADER_WITH_PROOFS, &relay.id])
                .inc();
            return Err(err.into());
        }
    };

    let request_latency = start_request.elapsed();
    LATENCY_BY_RELAY
        .with_label_values(&[TAG_GET_HEADER_WITH_PROOFS, &relay.id])
        .observe(request_latency.as_secs_f64());

    let code = res.status();
    RELAY_HTTP_STATUS
        .with_label_values(&[code.as_str(), TAG_GET_HEADER_WITH_PROOFS, &relay.id])
        .inc();

    let response_bytes = res.bytes().await?;
    if !code.is_success() {
        return Err(PbsError::RelayResponse {
            error_msg: String::from_utf8_lossy(&response_bytes).into_owned(),
            code: code.as_u16(),
        });
    };

    if code == StatusCode::NO_CONTENT {
        debug!(
            ?code,
            latency = ?request_latency,
            response = ?response_bytes,
            "no header from relay"
        );
        return Ok((start_request_time, None));
    }

    let get_header_response: GetHeaderWithProofsResponse = serde_json::from_slice(&response_bytes)
        .map_err(|e| PbsError::JsonDecode {
            err: e,
            raw: String::from_utf8(response_bytes.to_vec()).unwrap_or("Invalid UTF-8".to_string()),
        })?;
    debug!(
        latency = ?request_latency,
        block_hash = %get_header_response.data.message.header.block_hash,
        value_eth = format_ether(get_header_response.data.message.value),
        "received new header"
    );

    validate_header(
        &get_header_response.data,
        chain,
        relay.pubkey(),
        params.parent_hash,
        validation.skip_sigverify,
        validation.min_bid_wei,
    )?;

    Ok((start_request_time, Some(get_header_response)))
}

fn validate_header(
    signed_header: &SignedExecutionPayloadHeader,
    chain: Chain,
    expected_relay_pubkey: BlsPublicKey,
    parent_hash: B256,
    skip_sig_verify: bool,
    minimum_bid_wei: U256,
) -> Result<(), ValidationError> {
    let block_hash = signed_header.message.header.block_hash;
    let received_relay_pubkey = signed_header.message.pubkey;
    let tx_root = signed_header.message.header.transactions_root;
    let value = signed_header.message.value;

    if block_hash == B256::ZERO {
        return Err(ValidationError::EmptyBlockhash);
    }

    if parent_hash != signed_header.message.header.parent_hash {
        return Err(ValidationError::ParentHashMismatch {
            expected: parent_hash,
            got: signed_header.message.header.parent_hash,
        });
    }

    if tx_root == EMPTY_TX_ROOT_HASH {
        return Err(ValidationError::EmptyTxRoot);
    }

    if value <= minimum_bid_wei {
        return Err(ValidationError::BidTooLow { min: minimum_bid_wei, got: value });
    }

    if expected_relay_pubkey != received_relay_pubkey {
        return Err(ValidationError::PubkeyMismatch {
            expected: expected_relay_pubkey,
            got: received_relay_pubkey,
        });
    }

    // if !skip_sig_verify {
    //     // Verify the signature against the builder domain.
    //     verify_signed_message(
    //         chain,
    //         &received_relay_pubkey,
    //         &signed_header.message,
    //         &signed_header.signature,
    //         APPLICATION_BUILDER_DOMAIN,
    //     )
    //     .map_err(ValidationError::Sigverify)?;
    // }

    Ok(())
}

/// Send a POST request to all relays. Only returns an error if all of the requests fail.
async fn relay_post_request<T>(
    state: PbsState<BuilderRuntimeState>,
    path: &str,
    body: &T,
) -> Result<(), PbsClientError>
where
    T: Serialize,
{
    debug!("Sending POST request to {} relays", state.config.relays.len());
    // Forward constraints to all relays.
    let mut responses = FuturesUnordered::new();

    for relay in state.config.relays {
        let url = relay.get_url(path).map_err(|_| PbsClientError::BadRequest)?;
        responses.push(relay.client.post(url).json(&body).send());
    }

    let mut success = false;
    while let Some(res) = responses.next().await {
        match res {
            Ok(response) => {
                let url = response.url().clone();
                let status = response.status();
                let body = response.text().await.ok();
                if status != StatusCode::OK {
                    error!(
                        %status,
                        %url,
                        "Failed to POST to relay: {body:?}"
                    )
                } else {
                    debug!(%url, "Successfully sent POST request to relay");
                    success = true;
                }
            }
            Err(e) => error!(error = ?e, "Failed to POST to relay"),
        }
    }

    if success {
        Ok(())
    } else {
        Err(PbsClientError::NoResponse)
    }
}

fn timestamp_of_slot_start_millis(slot: u64, genesis: u64) -> u64 {
    let seconds_since_genesis = genesis + slot * SECONDS_PER_SLOT;
    seconds_since_genesis * MILLIS_PER_SECOND
}
fn ms_into_slot(slot: u64, genesis: u64) -> u64 {
    let slot_start_ms = timestamp_of_slot_start_millis(slot, genesis);
    utcnow_ms().saturating_sub(slot_start_ms)
}

/// Millis
fn utcnow_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
}

async fn fetch_current_slot_number(beacon_url: &Url) -> eyre::Result<u64> {
    let res = reqwest::get(beacon_url.join("eth/v1/beacon/headers/head")?).await?;
    let res = res.json::<Value>().await?;
    let slot = res.pointer("/data/header/message/slot").wrap_err("missing slot")?;
    Ok(slot.as_u64().unwrap_or(slot.as_str().wrap_err("invalid slot type")?.parse()?))
}
