use crate::commitment::request::{PreconfRequest, PreconfResult};
use alloy::hex::{self, decode};
use alloy::{primitives::FixedBytes, rpc::types::beacon::events::HeadEvent};
pub use beacon_api_client::mainnet::Client;
use commitment::request::{CommitmentRequestError, CommitmentRequestEvent};
use delegation::web3signer::{Web3Signer, Web3SignerTlsCredentials, trim_hex_prefix};

use ethereum_consensus::crypto::PublicKey;
use metrics::{run_metrics_server, ApiMetrics};
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

pub type BLSBytes = FixedBytes<96>;
pub const BLS_DST_PREFIX: &[u8] = b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_POP_";

async fn handle_preconfirmation_request(
    req: PreconfRequest,
    res: Sender<PreconfResult>,
    constraint_state: Arc<Mutex<ConstraintState>>,
    mut web3signer: Web3Signer,
) {
    let mut constraint_state = constraint_state.lock().await;

    tracing::info!("Received preconfirmation request");
    ApiMetrics::increment_received_commitments_count();

    let slot = req.slot;
    let pubkeys = web3signer.list_accounts().await.expect("Failed to load accounts");

    match constraint_state.validate_preconf_request(req.clone()).await {
        Ok(pubkey) => {
            let pubkey_str = format!("{:#?}", pubkey);
            if !pubkeys.contains(&pubkey_str) {
                tracing::error!(
                    "Not available validator in slot {} to sign in sidecar",
                    slot
                );
                return;
            }
            // TODO::Validate preconfirmation request
            let mut signed_contraints_list: Vec<SignedConstraints> = vec![];

            for tx in req.clone().txs.iter() {
                // web3signer
                let accounts = web3signer
                    .list_accounts()
                    .await
                    .expect("Web3signer fetching failed!");
                let trimmed_account = trim_hex_prefix(&accounts[0]).unwrap_or_default();
                let w3s_pubkey = PublicKey::try_from(hex::decode(trimmed_account).unwrap_or_default().as_slice()).unwrap_or_default();
                let w3s_message = ConstraintsMessage::from_tx(w3s_pubkey, slot, tx.clone());
                let w3s_digest = format!("0x{}", &hex::encode(w3s_message.digest()));
                let w3s_signature = web3signer
                    .request_signature(&accounts[0], &w3s_digest)
                    .await
                    .expect("Web3signer signature failed!");
                let mut bytes_array = [0u8; 96];
                let bytes = hex::decode(w3s_signature.trim_start_matches("0x")).unwrap_or_default();
                bytes_array[..bytes.len()].copy_from_slice(&bytes);

                let signed_constraints= SignedConstraints { message: w3s_message, signature: FixedBytes(bytes_array)} ;

                ApiMetrics::increment_preconfirmed_transactions_count(tx.tx.tx_type());

                constraint_state.add_constraint(slot, signed_constraints.clone());
                signed_contraints_list.push(signed_constraints.clone());

                // match commit_boost_api.send_constraints_to_be_collected(&vec![signed_constraints.clone()]).await {
                //     Ok(_) => tracing::info!(?signed_constraints,"Sent constratins successfully to be collected."),
                //     Err(err) => tracing::error!(err = ?err, "Error sending constraints to be collected")
                // };
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

    let _ = run_metrics_server(config.metrics_port);

    run_commitment_rpc_server(sender, &config).await;

    let (payload_tx, mut payload_rx) = mpsc::channel(16);
    let payload_fetcher = FallbackPayloadFetcher::new(payload_tx);

    let commit_boost_api = run_constraints_proxy_server(&config, payload_fetcher)
        .await
        .unwrap();

    let beacon_client = Client::new(config.beacon_api_url.clone());

    let client_state = ClientState::new(config.execution_api_url.clone());
    // let mut constraint_state = Arc::new(RwLock::new(ConstraintState::new( beacon_client.clone(), config.validator_indexes.clone(), config.chain.get_commitment_deadline_duration()))) ;
    let constraint_state = ConstraintState::new(
        beacon_client.clone(),
        config.validator_indexes.clone(),
        config.chain.get_commitment_deadline_duration(),
        ExecutionState::new(client_state, LimitOptions::default(), DEFAULT_GAS_LIMIT)
            .await
            .expect("Failed to create Execution State"),
        &config.chain,
    );

    let mut head_event_listener = HeadEventListener::run(beacon_client);

    let fallback_builder = FallbackBuilder::new(&config);

    //  let ws_stream = match connect_async(config.collector_ws.clone()).await {
    //     Ok((stream, response)) => {
    //         println!("Handshake for client has been completed");
    //         // This will be the HTTP response, same as with server this is the last moment we
    //         // can still access HTTP stuff.
    //         println!("Server response was {response:?}");
    //         stream
    //     }
    //     Err(e) => {
    //         println!("WebSocket handshake for client  failed with {e}!");
    //         return;
    //     }
    // };

    tracing::debug!("Connected to the server!");
    
    let web3signer_url = config.web3signer_url.clone();
    let creds = Web3SignerTlsCredentials { ca_cert_path: config.ca_cert_path.clone(), combined_pem_path: config.combined_pem_path.clone() };
    let mut web3signer = Web3Signer::connect(web3signer_url, creds)
        .await
        .expect("Web3signer connection failed!");

    let accounts = web3signer
        .list_accounts()
        .await
        .expect("Web3signer fetching failed!");

    tracing::info!(?accounts);
    let _ = send_sidecar_info(
        accounts,
        config.sidecar_info_sender_url,
        config.commitment_port,
    )
    .await;

    let constraint_state_arc = Arc::new(Mutex::new(constraint_state));

    let commit_boost_api = Arc::new(Mutex::new(commit_boost_api));
    let fallback_builder = Arc::new(Mutex::new(fallback_builder));

    // let (mut write, mut read) = ws_stream.split();
    // let constraint_state_store = constraint_state.write();
    loop {
        let constraint_stat_inner_clone = Arc::clone(&constraint_state_arc);
        let mut constraint_state_inner = constraint_stat_inner_clone.lock().await;
        // this will be unlocked after the second tokio::select slot is finished.
        tokio::select! {
            Some( CommitmentRequestEvent{req, res} ) = receiver.recv() => {
                tracing::info!("received preconf request");
                let constraint_state_clone = Arc::clone(&constraint_state_arc);
                tokio::spawn(
                    handle_preconfirmation_request(req, res, constraint_state_clone, web3signer.clone())
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
