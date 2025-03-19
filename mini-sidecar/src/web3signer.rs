use std::fs;

use crate::types::{
        Action, CommitBoostKeys, DelegationMessage, RevocationMessage, SignedDelegation, SignedRevocation
    };
use clap::Parser;
use ethereum_consensus::crypto::{PublicKey as BlsPublicKey, Signature as BlsSignature};
use eyre::{Context, Result};
use reqwest::{Certificate, Identity, Url};
use tracing::debug;

use super::types::{SignedMessage, CommitBoostSignatureRequest};

/// Generate signed delegations/recovations using a remote Web3Signer.
pub async fn generate_from_web3signer(
    opts: Web3SignerOpts,
    delegatee_pubkey: BlsPublicKey,
    action: Action,
) -> Result<Vec<SignedMessage>> {
    // Connect to web3signer.
    let mut web3signer = Web3Signer::connect(opts.url).await?;

    // Read in the accounts from the remote keystore.
    let accounts = web3signer.list_accounts().await?;
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
                let returned_signature =
                    web3signer.request_signature(&account, &signing_root).await?;
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
                let returned_signature =
                    web3signer.request_signature(&account, &signing_root).await?;
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
fn trim_hex_prefix(hex: &str) -> Result<String> {
    let trimmed = hex.get(2..).ok_or_else(|| eyre::eyre!("Invalid hex string: {hex}"))?;
    Ok(trimmed.to_string())
}


#[derive(Clone)]
pub struct Web3Signer {
    base_url: Url,
    client: reqwest::Client,
}

impl Web3Signer {
    /// Establish connection to a remote Web3Signer instance with TLS credentials.
    pub async fn connect(addr: String) -> Result<Self> {
        let base_url = addr.parse()?;

        let client = reqwest::Client::builder().build()?;

        Ok(Self { base_url, client })
    }

    /// List the consensus accounts of the keystore.
    ///
    /// Only the consensus keys are returned.
    /// This is due to signing only being over the consensus type.
    ///
    /// Reference: https://commit-boost.github.io/commit-boost-client/api/
    pub async fn list_accounts(&mut self) -> Result<Vec<String>> {
        let path = self.base_url.join("/signer/v1/get_pubkeys")?;
        let resp = self.client.get(path).send().await?.json::<CommitBoostKeys>().await?;

        let consensus_keys: Vec<String> =
            resp.keys.into_iter().map(|key_set| key_set.consensus).collect();

        Ok(consensus_keys)
    }

    /// Request a signature from the remote signer.
    ///
    /// This will sign an arbituary root over the consensus type.
    ///
    /// Reference: https://commit-boost.github.io/commit-boost-client/api/
    pub async fn request_signature(&mut self, pub_key: &str, object_root: &str) -> Result<String> {
        let path = self.base_url.join("/signer/v1/request_signature")?;
        let body = CommitBoostSignatureRequest {
            type_: "consensus".to_string(),
            pubkey: pub_key.to_string(),
            object_root: object_root.to_string(),
        };

        let resp = self.client.post(path).json(&body).send().await?.json::<String>().await?;

        Ok(resp)
    }
}

/// Options for connecting to a Web3Signer keystore.
#[derive(Debug, Clone, Parser)]
pub struct Web3SignerOpts {
    /// The URL of the Web3Signer keystore.
    pub url: String,
}



#[cfg(test)]
mod tests {
    use crate::{
        types::{Action, Chain},
        utils::parse_bls_public_key, web3signer::generate_from_web3signer,
    };

    use super::Web3SignerOpts;

    /// Test generating signed delegations using a remote Web3Signer signer.
    ///
    /// ```shell
    /// cargo test --package bolt --bin bolt -- commands::delegate::web3signer::tests::test_delegation_web3signer
    /// --exact --show-output  --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore = "Requires Web3Signer to be installed on the system"]
    async fn test_delegation_web3signer() -> eyre::Result<()> {
        let _ = tracing_subscriber::fmt::try_init();
        let url = "http://web3server_url.com".to_string();
        let delegatee_pubkey = "0x83eeddfac5e60f8fe607ee8713efb8877c295ad9f8ca075f4d8f6f2ae241a30dd57f78f6f3863a9fe0d5b5db9d550b93";
        let delegatee_pubkey = parse_bls_public_key(delegatee_pubkey)?;
        let chain = Chain::Mainnet;

        let opts = Web3SignerOpts { url};

        let signed_delegations =
            generate_from_web3signer(opts, delegatee_pubkey, Action::Delegate).await?;

        let signed_message = signed_delegations.first().expect("to get signed delegation");

        signed_message.verify_signature(chain)?;

        Ok(())
    }
}