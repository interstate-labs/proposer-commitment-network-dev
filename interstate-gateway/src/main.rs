use crate::commitment::request::{PreconfRequest, PreconfResult};
use alloy::hex::{self, decode};
use alloy::rpc::types::beacon::{BlsPublicKey, BlsSignature};
use alloy::{primitives::FixedBytes, rpc::types::beacon::events::HeadEvent};
pub use beacon_api_client::mainnet::Client;
use commitment::request::{CommitmentRequestError, CommitmentRequestEvent};
use delegation::cb_signer::{trim_hex_prefix, CBSigner};
use delegation::types::SignedDelegation;
use ethereum_consensus::crypto::PublicKey as ECBlsPublicKey;

use delegation::web3signer::{Web3Signer, Web3SignerTlsCredentials};
use ethereum_consensus::crypto::PublicKey;
use keystores::Keystores;
use metrics::{run_metrics_server, ApiMetrics};
use serde::{Deserialize, Serialize};
use state::{execution::ExecutionState, fetcher::ClientState, ConstraintState, HeadEventListener};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tracing_subscriber::fmt::Subscriber;
use utils::send_sidecar_info;

use commitment::{run_commitment_rpc_server, PreconfResponse};
use config::{
    limits::{LimitOptions, DEFAULT_GAS_LIMIT},
    Config,
};
use constraints::builder::PayloadAndBid;
use constraints::CommitBoostApi;
use constraints::{
    run_constraints_proxy_server, ConstraintsMessage, FallbackBuilder, FallbackPayloadFetcher,
    FetchPayloadRequest, SignedConstraints, TransactionExt,
};
use env_file_reader::read_file;

use tokio::sync::oneshot::Sender;
mod builder;
mod commitment;
mod config;
mod constraints;
mod crypto;
mod delegation;
mod errors;
mod metrics;
mod onchain;
mod state;
mod test_utils;
mod utils;
mod keystores;

pub type BLSBytes = FixedBytes<96>;
pub const BLS_DST_PREFIX: &[u8] = b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_POP_";

async fn handle_preconfirmation_request(
    req: PreconfRequest,
    res: Sender<PreconfResult>,
    constraint_state: Arc<Mutex<ConstraintState>>,
    keystores: Keystores,
    relay_client: reqwest::Client,
    relay_url:reqwest::Url
) {
    let mut constraint_state = constraint_state.lock().await;

    tracing::info!("Received preconfirmation request");
    ApiMetrics::increment_received_commitments_count();

    let slot = req.slot;
    let pubkeys = keystores.get_pubkeys();

    match constraint_state.validate_preconf_request(req.clone()).await {
        Ok(pubkey) => {

            let response = relay_client.
            get(relay_url.join(&format!("/relay/v1/builder/delegations?slot={}", slot).as_str()).expect("invalid delegation url")).send()
            .await.expect("failed to get delegations");

            let delegations: Vec<SignedDelegation> = response.json().await.expect("failed to deserialize delgations");
            let mut signed_contraints_list: Vec<SignedConstraints> = vec![];

           

            for delegation in delegations {
                if (delegation.message.validator_pubkey == pubkey) && (pubkeys.contains(&delegation.message.delegatee_pubkey)) {

                    for tx in req.clone().txs.iter() {
                        let message = ConstraintsMessage::from_tx(delegation.message.delegatee_pubkey.clone(), slot, tx.clone());
                        let digest = message.digest();
        
                        let signature = keystores.sign_commit_boost_root(digest, &delegation.message.delegatee_pubkey);
        
                        let signed_constraints = match signature {
                            Ok(signature) => SignedConstraints { message, signature },
                            Err(e) => {
                                tracing::error!(?e, "Failed to sign constraints");
                                return;
                            }
                        };
        
                        ApiMetrics::increment_preconfirmed_transactions_count(tx.tx.tx_type());
        
                        constraint_state.add_constraint(slot, signed_constraints.clone());
                        signed_contraints_list.push(signed_constraints.clone());
                    }
                   
                } else{}
            }

            let response = serde_json::to_value(PreconfResponse {
                ok: true,
                signed_contraints_list,
            })
            .map_err(Into::into);
            let _ = res.send(response).ok();
           
        }
        Err(err) => {
            ApiMetrics::increment_validation_errors_count("validation error".to_string());
            tracing::error!(?err, "validation error");
            res.send(Err(CommitmentRequestError::Custom(err.to_string())))
                .err();
        }
    };
}

async fn handle_commitment_deadline(
    slot: u64,
    constraint_state: Arc<Mutex<ConstraintState>>,
    commit_boost_api: Arc<Mutex<CommitBoostApi>>,
    fallback_builder: Arc<Mutex<FallbackBuilder>>,
) {
    let mut constraint_state = constraint_state.lock().await;
    let commit_boost_api = commit_boost_api.lock().await;
    let mut fallback_builder = fallback_builder.lock().await;

    tracing::info!("The commitment deadline is reached in slot {}", slot);

    let Some(block) = constraint_state.blocks.remove(&slot) else {
        tracing::debug!("Couldn't find a block at slot {slot}");
        return;
    };

    tracing::debug!("removed constraints at slot {slot}");

    match commit_boost_api
        .send_constraints(&block.signed_constraints_list)
        .await
    {
        Ok(_) => tracing::info!("Sent constratins successfully."),
        Err(err) => tracing::error!(err = ?err, "Error sending constraints"),
    };

    if let Err(e) = fallback_builder.build_fallback_payload(&block, slot).await {
        tracing::error!(err = ?e, "Failed in building fallback payload at slot {slot}");
    };
}

async fn handle_local_payload_request(
    slot: u64,
    fallback_builder: Arc<Mutex<FallbackBuilder>>,
    response_tx: Sender<Option<PayloadAndBid>>,
) {
    let mut fallback_builder = fallback_builder.lock().await;

    tracing::info!(slot, "Received local payload request");

    let Some(payload_and_bid) = fallback_builder.get_cached_payload() else {
        tracing::warn!("No local payload found for {slot}");
        let _ = response_tx.send(None);
        return;
    };

    if let Err(e) = response_tx.send(Some(payload_and_bid)) {
        tracing::error!(err = ?e, "Failed to send payload and bid in response channel");
    } else {
        tracing::debug!("Sent payload and bid to response channel");
    }
}

async fn handle_head_event(slot: u64, constraint_state: Arc<Mutex<ConstraintState>>) {
    let mut constraint_state = constraint_state.lock().await;

    tracing::info!(slot, "Got received a new head event");

    // We use None to signal that we want to fetch the latest EL head
    if let Err(e) = constraint_state.update_head(slot).await {
        tracing::error!(err = ?e, "Occurred errors in updating the constraint state head");
    }

    // We use None to signal that we want to fetch the latest EL head
    if let Err(e) = constraint_state.execution.update_head(None, slot).await {
        tracing::error!(err = ?e, "Failed to update execution state head");
    }
}

#[tokio::main]
async fn main() {
    let subscriber = Subscriber::builder()
        .with_max_level(tracing::Level::DEBUG)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    // let config = Config::parse_from_cli().unwrap();
    tracing::info!("path: {}", env!["CARGO_MANIFEST_DIR"]);
    let mut env_path = env!["CARGO_MANIFEST_DIR"].to_string();
    env_path.push_str("/.env");
    let envs = read_file(env_path).unwrap();

    let (sender, mut receiver) = mpsc::channel(1024);
    let config = Config::new(envs);
    let keystores = Keystores::new(
        &config.keystore_pubkeys_path,
        &config.keystore_secrets_path,
        &config.chain,
    );

    let commit_boost_signer_url = &config.commit_boost_signer_url;
    let jwt = &config.jwt_hex;
    tracing::info!(?commit_boost_signer_url);

    let web3signer_enabled = !config.ca_cert_path.is_empty() && !config.combined_pem_path.is_empty();
    tracing::info!(?web3signer_enabled);
    let _ = run_metrics_server(config.metrics_port);

    run_commitment_rpc_server(sender, &config).await;

    let (payload_tx, mut payload_rx) = mpsc::channel(16);
    let payload_fetcher = FallbackPayloadFetcher::new(payload_tx);

    let commit_boost_api = run_constraints_proxy_server(&config, payload_fetcher)
        .await
        .unwrap();

    let beacon_client = Client::new(config.beacon_api_url.clone());

    let relay_client = reqwest::Client::builder().build().expect("failed to create relay client");

    let client_state = ClientState::new(config.execution_api_url.clone());
    // let mut constraint_state = Arc::new(RwLock::new(ConstraintState::new( beacon_client.clone(), config.validator_indexes.clone(), config.chain.get_commitment_deadline_duration()))) ;
    let constraint_state = ConstraintState::new(
        beacon_client.clone(),
        config.chain.get_commitment_deadline_duration(),
        ExecutionState::new(client_state, LimitOptions::default(), DEFAULT_GAS_LIMIT)
            .await
            .expect("Failed to create Execution State"),
        &config.chain,
    );

    let mut head_event_listener = HeadEventListener::run(beacon_client);

    let fallback_builder = FallbackBuilder::new(&config);

    tracing::debug!("Connected to the server!");

    let constraint_state_arc = Arc::new(Mutex::new(constraint_state));
    let commit_boost_api = Arc::new(Mutex::new(commit_boost_api));
    let fallback_builder = Arc::new(Mutex::new(fallback_builder));

    loop {
        let constraint_stat_inner_clone = Arc::clone(&constraint_state_arc);
        let mut constraint_state_inner = constraint_stat_inner_clone.lock().await;
        // this will be unlocked after the second tokio::select slot is finished.
        tokio::select! {
            Some( CommitmentRequestEvent{req, res} ) = receiver.recv() => {
                tracing::info!("received preconf request");
                let constraint_state_clone = Arc::clone(&constraint_state_arc);
                tokio::spawn(
                    handle_preconfirmation_request(req, res, constraint_state_clone, keystores.clone(), relay_client.clone(), config.relay_url.clone())
                );
            },
            Some(slot) = constraint_state_inner.commitment_deadline.wait() => {
                let constraint_state_clone = Arc::clone(&constraint_state_arc);
                tokio::spawn(
                    handle_commitment_deadline(slot+1, constraint_state_clone, commit_boost_api.clone(), fallback_builder.clone())
                );
            },
            Some(FetchPayloadRequest { slot, response_tx }) = payload_rx.recv() => {
                handle_local_payload_request(slot, fallback_builder.clone(), response_tx).await;
            },
            // Some(Ok(msg)) = read.next() => {
            //     if let tokio_tungstenite::tungstenite::protocol::Message::Text(text) = msg {
            //         let merged_constraints: Vec<SignedConstraints> = serde_json::from_str(text.as_str()).unwrap();

            //         tracing::debug!("Received {} merged constraints", merged_constraints.len());
            //         constraint_state.replace_constraints(merged_constraints[0].message.slot, &merged_constraints);
            //     }
            // },
            Ok(HeadEvent { slot, .. }) = head_event_listener.next_head() => {
                let constraint_state_clone = Arc::clone(&constraint_state_arc);
                tokio::spawn(
                    handle_head_event(slot, constraint_state_clone)
                );
            },
        }
    }
}
