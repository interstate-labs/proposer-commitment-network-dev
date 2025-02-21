pub mod score_cache;
pub mod transactions;

use std::collections::HashSet;

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

pub async fn send_sidecar_info(pubkeys: HashSet<PublicKey>, server_url: Url, sidecar_port: u16) -> eyre::Result<()> {
    let mut sidecar_url ="http://".to_string();
    sidecar_url.push_str(local_ip().expect("Failed to get the local ip address").to_string().as_str());
    sidecar_url.push_str(":");
    sidecar_url.push_str(sidecar_port.to_string().as_str());
    
    let client = reqwest::ClientBuilder::new().user_agent("interstate-boost").build().unwrap();

    let data = SidecarInfo {
        pubkeys: pubkeys.into_iter().collect(),
        url: sidecar_url
    };
    
    let response = client.
    post(server_url.clone())
    .json(&data)
    .send()
    .await?;

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