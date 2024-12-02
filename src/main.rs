use alloy::{primitives::FixedBytes, rpc::types::beacon::events::HeadEvent};
use rand::RngCore;
use state::{ ConstraintState, HeadEventListener };
use tokio::sync::mpsc;
use commitment::request::CommitmentRequestEvent;
use tracing_subscriber::fmt::Subscriber;
use blst::min_pk::SecretKey;

pub use beacon_api_client::mainnet::Client;

use env_file_reader::read_file;

use constraints::{run_constraints_proxy_server, FallbackPayloadFetcher, FetchPayloadRequest, ConstraintsMessage, FallbackBuilder, SignedConstraints };
use commitment::{run_commitment_rpc_server, PreconfResponse};
use config::Config;

mod commitment;
mod state;
mod constraints;
mod errors;
mod config;
mod utils;

pub type BLSBytes = FixedBytes<96>;

#[tokio::main]
async fn main() {
    pub const BLS_DST_PREFIX: &[u8] = b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_POP_";

    let subscriber = Subscriber::builder()
    .with_max_level(tracing::Level::DEBUG)
    .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    // let config = Config::parse_from_cli().unwrap();
    let envs = read_file("/work/proposer-commitment-network/.env").unwrap();

    let ( sender, mut receiver ) = mpsc::channel(1024);
    let config = Config::new(envs);

    run_commitment_rpc_server(sender, &config).await;

    let (payload_tx, mut payload_rx) = mpsc::channel(16);
    let payload_fetcher = FallbackPayloadFetcher::new(payload_tx);

    let commit_boost_api = run_constraints_proxy_server(&config, payload_fetcher).await.unwrap();

    let beacon_client = Client::new(config.beacon_api_url.clone());

    let mut constraint_state = ConstraintState::new( beacon_client.clone(), config.validator_indexes.clone(), config.chain.get_commitment_deadline_duration());

    let mut rng = rand::thread_rng();
    let mut ikm = [0u8; 32];
    rng.fill_bytes(&mut ikm);
    let signer_key = SecretKey::key_gen(&ikm, &[]).unwrap();

    let mut head_event_listener = HeadEventListener::run(beacon_client);

    let mut fallback_builder = FallbackBuilder::new(&config);

    loop {
        tokio::select! {
            Some( CommitmentRequestEvent{req, res} ) = receiver.recv() => {
                tracing::info!("Received preconfirmation request");
                let slot = req.slot;
                
                let message = ConstraintsMessage::build(10, req);
                let signature =  BLSBytes::from(signer_key.sign(&message.digest(), BLS_DST_PREFIX, &[]).to_bytes());
                let signed_constraints = SignedConstraints { message, signature };
                constraint_state.add_constraint(slot, signed_constraints);

                let response = serde_json::to_value( PreconfResponse { ok: true}).map_err(Into::into);
                let _ = res.send(response).ok();
            },
            Some(slot) = constraint_state.commitment_deadline.wait() => {
                tracing::info!("The commitment deadline is reached in slot {}", slot);

                let Some(block) = constraint_state.remove_constraints_at_slot(slot) else {
                    tracing::debug!("Couldn't find a block at slot {slot}");
                    continue;
                };
                tracing::debug!("removed constraints at slot {slot}");

                match commit_boost_api.send_constraints(&block.signed_constraints_list).await {
                    Ok(_) => tracing::info!("Sent constratins successfully."),
                    Err(err) => tracing::error!(err = ?err, "Error sending constraints, retrying...")
                };

                if let Err(e) = fallback_builder.build_fallback_payload(&block).await {
                    tracing::error!(err = ?e, "Failed in building fallback payload at slot {slot}");
                };

            },
            Some(FetchPayloadRequest { slot, response_tx }) = payload_rx.recv() => {
                tracing::info!(slot, "Received local payload request");

                let Some(payload_and_bid) = fallback_builder.get_cached_payload() else  {
                        tracing::warn!("No local payload found for {slot}");
                        let _ = response_tx.send(None);
                        continue;
                };

                if let Err(e) = response_tx.send(Some(payload_and_bid)) {
                    tracing::error!(err = ?e, "Failed to send payload and bid in response channel");
                } else {
                    tracing::debug!("Sent payload and bid to response channel");
                }
            },
            Ok(HeadEvent { slot, .. }) = head_event_listener.next_head() => {
                tracing::info!(slot, "Got received a new head event");

                // We use None to signal that we want to fetch the latest EL head
                if let Err(e) = constraint_state.update_head(slot).await {
                    tracing::error!(err = ?e, "Occurred errors in updating the constraint state head");
                }
            },
        }
    }

}
