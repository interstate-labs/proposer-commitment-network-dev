use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use eyre::Result;
#[derive(Serialize, Deserialize)]
struct Keys {
    /// The consensus keys stored in the Web3Signer.
    pub consensus: String,
    /// The two below proxy fields are here for deserialisation purposes.
    /// They are not used as signing is only over the consensus type.
    #[allow(unused)]
    pub proxy_bls: Vec<String>,
    #[allow(unused)]
    pub proxy_ecdsa: Vec<String>,
}

/// Outer container for response.
#[derive(Serialize, Deserialize)]
struct CommitBoostKeys {
    keys: Vec<Keys>,
}

/// Request signature from the Web3Signer.
#[derive(Serialize, Deserialize)]
struct CommitBoostSignatureRequest {
    #[serde(rename = "type")]
    pub type_: String,
    pub pubkey: String,
    pub object_root: String,
}

#[derive(Clone)]
pub struct CBSigner {
    client: Client,
    base_url: String,
    jwt_token: Arc<Mutex<Option<String>>>,
}

impl CBSigner {
    // Constructor to create a new API Client
    pub fn new(base_url: &str, jwt: &str) -> Self {
        CBSigner {
            client: Client::new(),
            base_url: base_url.to_string(),
            jwt_token: Arc::new(Mutex::new(Some(jwt.to_string()))),
        }
    }
    // Helper function to construct full URL
    fn full_url(&self, endpoint: &str) -> String {
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            endpoint.trim_start_matches('/')
        )
    }
    // Generic function to send GET requests with authentication
    pub async fn get_list_accounts(&self) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let url = self.full_url("signer/v1/get_pubkeys");
        let jwt = self.jwt_token.lock().await;
        let mut headers = HeaderMap::new();

        if let Some(token) = jwt.as_ref() {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", token))?,
            );
        }

        let response = self
            .client
            .get(url)
            .headers(headers)
            .send()
            .await?
            .json::<CommitBoostKeys>()
            .await?;

        let consensus_keys: Vec<String> = response
            .keys
            .into_iter()
            .map(|key_set| key_set.consensus)
            .collect();
        Ok(consensus_keys)
    }

    // Generic function to send POST requests with authentication
    pub async fn request_signature(
        &self,
        pub_key: &str,
        object_root: &str,
    ) -> Result<String> {
        let url = self.full_url("/signer/v1/request_signature");
        let jwt = self.jwt_token.lock().await;
        let mut headers = HeaderMap::new();

        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Some(token) = jwt.as_ref() {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", token))?,
            );
        }

        let body = CommitBoostSignatureRequest {
            type_: "consensus".to_string(),
            pubkey: pub_key.to_string(),
            object_root: object_root.to_string(),
        };

        let response = self
            .client
            .post(url)
            .headers(headers)
            .json(&body)
            .send()
            .await?
            .text()
            .await?;

        Ok(response)
    }
}

/// A utility function to trim the pre-pended 0x prefix for hex strings.
pub fn trim_hex_prefix(hex: &str) -> Result<String> {
    let trimmed = hex
        .get(2..)
        .ok_or_else(|| eyre::eyre!("Invalid hex string: {hex}"))?;
    Ok(trimmed.to_string())
}