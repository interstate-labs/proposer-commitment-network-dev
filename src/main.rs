use alloy::primitives::FixedBytes;
use rand::RngCore;
use state::ConstraintState;
use tokio::sync::mpsc;
use commitment::request::CommitmentRequestEvent;
use tracing_subscriber::fmt::Subscriber;
use blst::min_pk::SecretKey;

use constraints::{run_commit_booster, ConstraintsMessage, SignedConstraints };
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

    let config = Config::parse_from_cli().unwrap();

    let ( sender, mut receiver ) = mpsc::channel(1024);

    run_commitment_rpc_server(sender, &config).await;
    run_commit_booster(&config).await;

    let mut constraint_state = ConstraintState::new();

    let mut rng = rand::thread_rng();
    let mut ikm = [0u8; 32];
    rng.fill_bytes(&mut ikm);
    let signer_key = SecretKey::key_gen(&ikm, &[]).unwrap();

    loop {
        tokio::select! {
            Some( CommitmentRequestEvent{req, res} ) = receiver.recv() => {
                tracing::info!("Received preconfirmation request");
                let slot = req.slot;
                
                let message = ConstraintsMessage::build(10, req);
                let signature =  BLSBytes::from(signer_key.sign(&message.digest(), BLS_DST_PREFIX, &[]).to_bytes()).to_string();
                let signed_constraints = SignedConstraints { message, signature };
                constraint_state.add_constraint(slot, signed_constraints);

                tracing::info!("{:#?}", constraint_state.blocks);

                let response = serde_json::to_value( PreconfResponse { ok: true}).map_err(Into::into);
                let _ = res.send(response).ok();
            },
            Some(slot) = constraint_state.commitment_deadline.wait() => {
                tracing::info!("The commitment deadline is reached in slot {}", slot);

                let Some(block) = constraint_state.remove_constraints_at_slot(slot) else {
                    tracing::debug!("Couldn't find a block at slot {slot}");
                    continue;
                };
                tracing::debug!("revmoed constraints at slot {slot}");




            }
        }
    }

}
