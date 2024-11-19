use alloy::primitives::FixedBytes;
use controller::{run_commitment_rpc_server, PreconfResponse};
use rand::RngCore;
use state::ConstraintState;
use tokio::sync::mpsc;
use commitment::request::CommitmentRequestEvent;
use tracing_subscriber::fmt::Subscriber;
use constraints::{ ConstraintsMessage, SignedConstraints };
use blst::min_pk::SecretKey;

mod commitment;
mod controller;
mod state;
mod constraints;

pub type BLSBytes = FixedBytes<96>;

#[tokio::main]
async fn main() {
    pub const BLS_DST_PREFIX: &[u8] = b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_POP_";

    let subscriber = Subscriber::builder()
    .with_max_level(tracing::Level::DEBUG)
    .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    let ( sender, mut receiver ) = mpsc::channel(1024);
    run_commitment_rpc_server(sender).await;
    let mut constraint_state = ConstraintState::new();
    tracing::debug!("started rpc server");

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
            }
        }
    }

}
