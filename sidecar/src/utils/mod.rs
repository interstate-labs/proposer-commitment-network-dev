pub mod score_cache;
pub mod transactions;

use std::collections::HashSet;

use alloy::hex;
use blst::min_pk::SecretKey;
use rand::RngCore;
use local_ip_address::local_ip;
use ethereum_consensus::crypto::PublicKey;
use reqwest::{StatusCode, Url};

use crate::errors::ErrorResponse;

pub fn create_random_bls_secretkey() -> SecretKey {
    let mut rng = rand::thread_rng();
    let mut ikm = [0u8; 32];
    rng.fill_bytes(&mut ikm);
    SecretKey::key_gen(&ikm, &[]).unwrap()
}

pub async fn send_sidecar_info(pubkeys: Vec<String>, server_url: Url, sidecar_port: u16) -> eyre::Result<()> {
    let ip = reqwest::get("http://checkip.amazonaws.com")
        .await?
        .text()
        .await?;

    let mut sidecar_url ="http://".to_string();
    sidecar_url.push_str(ip.as_str());
    sidecar_url.push_str(":");
    sidecar_url.push_str(sidecar_port.to_string().as_str());
    
    let client = reqwest::ClientBuilder::new().user_agent("interstate-boost").build().unwrap();
    let mut pubkey_array: Vec<PublicKey> = vec![];
    for pk in pubkeys {
        let w3s_pubkey = PublicKey::try_from(hex::decode(pk).unwrap_or_default().as_slice()).unwrap_or_default();
        pubkey_array.push(w3s_pubkey);
    }

    let data = SidecarInfo {
        pubkeys: pubkey_array,
        url: sidecar_url
    };
    let response = client.
    post(server_url.clone())
    .json(&data)
    .send()
    .await?;
    tracing::debug!("sent sidecar data");

    if response.status() != StatusCode::OK {
        let error = response.json::<ErrorResponse>().await?;
        tracing::error!(?error, "Failed to send sidecar data to server {}", server_url.as_str());
    }
    tracing::info!("Sent successfully sidecar data to the server {}", server_url.as_str());
    Ok(())
}

#[derive(Debug, serde::Serialize)]
struct SidecarInfo {
    pubkeys: Vec<PublicKey>,
    url: String
}