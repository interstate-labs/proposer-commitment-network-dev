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
use web3signer::{generate_from_web3signer, Web3SignerOpts};
use types::{Action, Chain};
use utils::{keystore_paths, write_to_file, KeystoreError, KeystoreSecret};
use lighthouse_eth2_keystore::Keystore;
use serde::Serialize;
use signer::{compute_commit_boost_signing_root, verify_message_signature};
use tracing::{debug, warn};

mod keystore;
mod signer;
mod utils;
mod web3signer;
mod types;

pub use utils::parse_bls_public_key;

#[tokio::main]
async fn main() ->eyre::Result<()> {
    dotenv().ok();
    let keys_path = env::var("KEYS_PATH").expect("couldn't find keys path in env file");
    let password_path = env::var("SECRETS_PATH").expect("couldn't find secrets path in env file");
    let out = env::var("OUT_FILE").expect("couldn't find out file in env file");
    let out_web3 = env::var("OUT_FILE_WEB3").expect("couldn't find out file in env file");
    let delegate_pbukey_str = env::var("DELEGATEE_PUBLICKEY").expect("couldn't find delegatee publickey in env file");
    let delegatee_pubkey:BlsPublicKey = parse_bls_public_key(delegate_pbukey_str.as_str()).expect("Invalid public key");
    let _ = tracing_subscriber::fmt().with_target(false).try_init();
    let keystore_secret = KeystoreSecret::from_directory(password_path.as_str()).unwrap();
    
    let signed_messages = generate_from_keystore(
        &keys_path,
        keystore_secret,
        delegatee_pubkey.clone(),
        Chain::Kurtosis,
        Action::Delegate,
    ).expect("Invalid signed message request");

    let signed_messages_web3 = generate_from_web3signer(Web3SignerOpts{ url:"http://127.0.0.1:32785".to_string()}, delegatee_pubkey, Action::Delegate).await?;

    debug!("Signed {} messages with keystore", signed_messages.len());
    debug!("Signed {} messages with web3signature", signed_messages_web3.len());


    // Verify signatures
    for message in &signed_messages {
        verify_message_signature(message, Chain::Kurtosis).expect("invalid signature");
    }

    write_to_file(out.as_str(), &signed_messages).expect("invalid file");

    write_to_file(out_web3.as_str(), &signed_messages_web3).expect("invalid file");

    Ok(())
}

