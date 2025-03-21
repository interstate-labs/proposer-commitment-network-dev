use super::types::{
    DelegationMessage, RevocationMessage, SignedDelegation, SignedMessage, SignedRevocation,
};
use alloy::hex;
use clap::{Parser, ValueEnum};
use ethereum_consensus::crypto::{PublicKey as BlsPublicKey, Signature as BlsSignature};
use eyre::{bail, Context, Result};
use reqwest::{Certificate, Identity, Url};
use serde::{Deserialize, Serialize};
use std::{fs, process::{Child, Command}, time::Duration};
use tracing::debug;

/// Web3Signer remote server.
///
///  Functionality:
/// - List consensus accounts in the keystore.
/// - Sign roots over the consensus type.
///
/// Reference: https://docs.web3signer.consensys.io/reference
#[derive(Clone)]
pub struct Web3Signer {
    base_url: Url,
    client: reqwest::Client,
}

impl Web3Signer {
    /// Establish connection to a remote Web3Signer instance with TLS credentials.
    pub async fn connect(addr: String, credentials: Web3SignerTlsCredentials) -> Result<Self> {
        let base_url = addr.parse()?;
        let (cert, identity) = compose_credentials(credentials)?;
        
        let client = reqwest::Client::builder()
            .add_root_certificate(cert)
            .identity(identity)
            .use_rustls_tls()
            .build()?;

        Ok(Self { base_url, client })
    }

    /// List the consensus accounts of the keystore.
    ///
    /// Only the consensus keys are returned.
    /// This is due to signing only being over the consensus type.
    ///
    /// Reference: https://commit-boost.github.io/commit-boost-client/api/
    pub async fn w3_list_accounts(&self) -> Result<Vec<String>> {
        let path = self.base_url.join("/signer/v1/get_pubkeys")?;
        tracing::info!(?path);
        let resp = self
            .client
            .get(path)
            .send()
            .await?
            .json::<CommitBoostKeys>()
            .await?;

        let consensus_keys: Vec<String> = resp
            .keys
            .into_iter()
            .map(|key_set| key_set.consensus)
            .collect();

        Ok(consensus_keys)
    }

    /// Request a signature from the remote signer.
    ///
    /// This will sign an arbituary root over the consensus type.
    ///
    /// Reference: https://commit-boost.github.io/commit-boost-client/api/
    pub async fn w3_request_signature(&self, pub_key: &str, object_root: &str) -> Result<String> {
        let path = self.base_url.join("/signer/v1/request_signature")?;
        let body = CommitBoostSignatureRequest {
            type_: "consensus".to_string(),
            pubkey: pub_key.to_string(),
            object_root: object_root.to_string(),
        };

        let resp = self
            .client
            .post(path)
            .json(&body)
            .send()
            .await?
            .json::<String>()
            .await?;

        Ok(resp)
    }
}

/// Compose the TLS credentials for the Web3Signer.
///
/// Returns the CA certificate and the identity (combined PEM).
fn compose_credentials(credentials: Web3SignerTlsCredentials) -> Result<(Certificate, Identity)> {
    let ca_cert = fs::read(credentials.ca_cert_path).wrap_err("Failed to read CA cert")?;
    let ca_cert = Certificate::from_pem(&ca_cert)?;

    let identity = fs::read(credentials.combined_pem_path).wrap_err("Failed to read PEM")?;
    let identity = Identity::from_pem(&identity)?;

    Ok((ca_cert, identity))
}

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

/// The action to perform.
#[derive(Debug, Clone, Copy, ValueEnum)]
#[clap(rename_all = "kebab_case")]
pub enum Action {
    /// Create a delegation message.
    Delegate,
    /// Create a revocation message.
    Revoke,
}

/// Options for connecting to a Web3Signer keystore.
#[derive(Debug, Clone, Parser)]
pub struct Web3SignerOpts {
    /// The URL of the Web3Signer keystore.
    #[clap(long, env = "WEB3SIGNER_URL")]
    pub url: String,

    /// The TLS credentials for connecting to the Web3Signer keystore.
    #[clap(flatten)]
    pub tls_credentials: Web3SignerTlsCredentials,
}

/// TLS credentials for connecting to a remote Web3Signer server.
#[derive(Debug, Clone, PartialEq, Eq, Parser)]
pub struct Web3SignerTlsCredentials {
    /// Path to the CA certificate file. (.crt)
    #[clap(long, env = "CA_CERT_PATH")]
    pub ca_cert_path: String,
    /// Path to the PEM encoded private key and certificate file. (.pem)
    #[clap(long, env = "CLIENT_COMBINED_PEM_PATH")]
    pub combined_pem_path: String,
}

/// Generate signed delegations/recovations using a remote Web3Signer.
pub async fn generate_from_web3signer(
    opts: Web3SignerOpts,
    delegatee_pubkey: BlsPublicKey,
    action: Action,
) -> Result<Vec<SignedMessage>> {
    // Connect to web3signer.
    let mut web3signer = Web3Signer::connect(opts.url, opts.tls_credentials).await?;

    // Read in the accounts from the remote keystore.
    let accounts = web3signer.w3_list_accounts().await?;
    debug!("Found {} remote accounts to sign with", accounts.len());

    let mut signed_messages = Vec::with_capacity(accounts.len());

    for account in accounts {
        // Parse the BLS key of the account.
        // Trim the pre-pended 0x.
        let trimmed_account = trim_hex_prefix(&account)?;
        let pubkey = BlsPublicKey::try_from(hex::decode(trimmed_account)?.as_slice())?;

        match action {
            Action::Delegate => {
                let message = DelegationMessage::new(pubkey.clone(), delegatee_pubkey.clone());
                // Web3Signer expects the pre-pended 0x.
                let signing_root = format!("0x{}", &hex::encode(message.digest()));
                let returned_signature = web3signer
                    .w3_request_signature(&account, &signing_root)
                    .await?;
                // Trim the 0x.
                let trimmed_signature = trim_hex_prefix(&returned_signature)?;
                let signature = BlsSignature::try_from(hex::decode(trimmed_signature)?.as_slice())?;
                let signed = SignedDelegation { message, signature };
                signed_messages.push(SignedMessage::Delegation(signed));
            }
            Action::Revoke => {
                let message = RevocationMessage::new(pubkey.clone(), delegatee_pubkey.clone());
                // Web3Signer expects the pre-pended 0x.
                let signing_root = format!("0x{}", &hex::encode(message.digest()));
                let returned_signature = web3signer
                    .w3_request_signature(&account, &signing_root)
                    .await?;
                // Trim the 0x.
                let trimmed_signature = trim_hex_prefix(&returned_signature)?;
                let signature = BlsSignature::try_from(trimmed_signature.as_bytes())?;
                let signed = SignedRevocation { message, signature };
                signed_messages.push(SignedMessage::Revocation(signed));
            }
        }
    }

    Ok(signed_messages)
}

/// A utility function to trim the pre-pended 0x prefix for hex strings.
pub fn trim_hex_prefix(hex: &str) -> Result<String> {
    let trimmed = hex
        .get(2..)
        .ok_or_else(|| eyre::eyre!("Invalid hex string: {hex}"))?;
    Ok(trimmed.to_string())
}