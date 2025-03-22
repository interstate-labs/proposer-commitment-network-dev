use std::{fs, path::PathBuf, env};
use dotenv::dotenv;
use alloy::{
    primitives::B256,
    signers::k256::sha2::{Digest, Sha256},
};
use blst::{min_pk::Signature, BLST_ERROR};
use clap::ValueEnum;
use ethereum_consensus::{
    crypto::{
    PublicKey as BlsPublicKey, SecretKey as BlsSecretKey, Signature as BlsSignature,
    },
    deneb::{compute_fork_data_root, compute_signing_root, Root}
};
use eyre::{eyre, Context, Result};
use keystore::generate_from_keystore;
use reqwest::StatusCode;
use web3signer::{generate_from_web3signer, Web3SignerOpts};
use types::{Action, Chain};
use utils::{keystore_paths, write_to_file, KeystoreError, KeystoreSecret};
use lighthouse_eth2_keystore::Keystore;
use serde::Serialize;
use signer::{compute_commit_boost_signing_root, verify_message_signature};
use tracing::{debug, error, info, warn};
use tracing_subscriber::fmt::Subscriber;
mod keystore;
mod signer;
mod utils;
mod web3signer;
mod types;

pub use utils::parse_bls_public_key;
const PERMISSION_DELEGATE_PATH: &str = "/constraints/v1/builder/delegate";
#[tokio::main]
async fn main() ->eyre::Result<()> {
    dotenv().ok();
    
    let subscriber = Subscriber::builder()
    .with_max_level(tracing::Level::DEBUG)
    .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let keys_path = env::var("KEYS_PATH").expect("couldn't find keys path in env file");
    let password_path = env::var("SECRETS_PATH").expect("couldn't find secrets path in env file");
    let out = env::var("OUT_FILE").expect("couldn't find out file in env file");
    let out_web3 = env::var("OUT_FILE_WEB3").expect("couldn't find out file in env file");
    let relay_url  = env::var("RELAY_URL").expect("couldn't find relay url in env file");
    let web3signer_url = env::var("WEB3SIGNER_URL").expect("couldn't find web3signer url in env file");
    let delegate_pbukey_str = env::var("DELEGATEE_PUBLICKEY").expect("couldn't find delegatee publickey in env file");
    let delegatee_pubkey:BlsPublicKey = parse_bls_public_key(delegate_pbukey_str.as_str()).expect("Invalid public key");
    let keystore_secret = KeystoreSecret::from_directory(password_path.as_str()).unwrap();
    
    let signed_messages = generate_from_keystore(
        &keys_path,
        keystore_secret,
        delegatee_pubkey.clone(),
        Chain::Kurtosis,
        Action::Delegate,
    ).expect("Invalid signed message request");

    debug!("Signed {} messages with keystore", signed_messages.len());
    
    // let signed_messages_web3 = generate_from_web3signer(Web3SignerOpts{ url:web3signer_url}, delegatee_pubkey, Action::Delegate).await?;
    // debug!("Signed {} messages with web3signature", signed_messages_web3.len());


    // Verify signatures
    for message in &signed_messages {
        verify_message_signature(message, Chain::Kurtosis).expect("invalid signature");
    }

    // write_to_file(out.as_str(), &signed_messages).expect("invalid file");

    // write_to_file(out_web3.as_str(), &signed_messages_web3).expect("invalid file");

    let client = reqwest::ClientBuilder::new().build().unwrap();

    let response = client
    .post(relay_url + PERMISSION_DELEGATE_PATH)
    .header("content-type", "application/json")
    .body(serde_json::to_string(&signed_messages)?)
    .send()
    .await?;

    if response.status() != StatusCode::OK {
        error!("failed to send  delegations to relay");
    } else {
        info!("submited  {} delegations to relay", signed_messages.len());
    }

    Ok(())
}

