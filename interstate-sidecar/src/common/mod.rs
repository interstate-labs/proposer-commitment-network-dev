use alloy::hex;
use clap::Parser;
use ethereum_consensus::crypto::bls::{
    PublicKey as BlsPublicKey, SecretKey as BlsSecretKey, Signature as BlsSignature,
};
use eyre::{bail, Ok, Result};
use web3signer::trim_hex_prefix;

use crate::constraints::ConstraintDigest;

use {dirk::Dirk, pb::eth2_signer_api::ListAccountsResponse, web3signer::Web3Signer};

pub mod dirk;
mod pb;
pub mod web3signer;

#[derive(Clone)]
pub enum Signer {
    Web3Signer {
        signer: Web3Signer,
        opts: Web3SignerOpts,
    },
    Dirk {
        signer: Dirk,
        opts: DirkOpts,
    },
}

/// Options for connecting to a DIRK keystore.
#[derive(Debug, Clone, Parser)]
pub struct DirkOpts {
    /// The URL of the DIRK keystore.
    #[clap(long, env = "DIRK_URL")]
    pub url: String,

    /// The path of the wallets in the DIRK keystore.
    #[clap(long, env = "DIRK_WALLET_PATH")]
    pub wallet_path: String,

    /// The passphrases to unlock the wallet in the DIRK keystore.
    /// If multiple are provided, they are tried in order until one works.
    #[clap(
        long,
        env = "DIRK_PASSPHRASES",
        value_delimiter = ',',
        hide_env_values = true
    )]
    pub passphrases: Option<Vec<String>>,

    /// The TLS credentials for connecting to the DIRK keystore.
    #[clap(flatten)]
    pub tls_credentials: DirkTlsCredentials,
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

/// TLS credentials for connecting to a remote Dirk server.
#[derive(Debug, Clone, PartialEq, Eq, Parser)]
pub struct DirkTlsCredentials {
    /// Path to the client certificate file. (.crt)
    #[clap(long, env = "CLIENT_CERT_PATH")]
    pub client_cert_path: String,
    /// Path to the client key file. (.key)
    #[clap(long, env = "CLIENT_KEY_PATH")]
    pub client_key_path: String,
    /// Path to the CA certificate file. (.crt)
    #[clap(long, env = "CA_CERT_PATH")]
    pub ca_cert_path: Option<String>,
}

pub async fn list_accounts(signer: &mut Signer) -> Result<Vec<BlsPublicKey>> {
    match signer {
        Signer::Web3Signer { signer, .. } => {
            let accounts = signer.list_accounts().await?;
            list_from_web3signer_accounts(&accounts)
        }
        Signer::Dirk { signer, opts } => {
            let accounts = signer.list_accounts(opts.wallet_path.clone()).await?;
            list_from_dirk_accounts(accounts)
        }
    }
}

pub async fn request_signature<Message>(signer: Signer, message: Message) -> Result<BlsSignature>
where
    Message: ConstraintDigest,
{
    match signer {
        Signer::Web3Signer { mut signer, .. } => {
            let accounts = signer.list_accounts().await?;
            let signing_root = format!("0x{}", &hex::encode(message.digest()));
            let returned_signature = signer
                .request_signature(&accounts[0], &signing_root)
                .await?;
            let trimmed_signature = trim_hex_prefix(&returned_signature)?;
            let signature = BlsSignature::try_from(hex::decode(trimmed_signature)?.as_slice())?;
            Ok(signature)
        }
        _ => bail!("Dirk!"),
    }
}

/// Derive public keys from the provided web3signer accounts.
pub fn list_from_web3signer_accounts(accounts: &[String]) -> Result<Vec<BlsPublicKey>> {
    let mut pubkeys = Vec::with_capacity(accounts.len());

    for acc in accounts {
        let trimmed_account = &acc.clone()[2..];
        let pubkey = BlsPublicKey::try_from(hex::decode(trimmed_account)?.as_slice())?;
        pubkeys.push(pubkey);
    }

    Ok(pubkeys)
}

/// Derive public keys from the provided dirk accounts.
pub fn list_from_dirk_accounts(accounts: ListAccountsResponse) -> Result<Vec<BlsPublicKey>> {
    let count = accounts.accounts.len() + accounts.distributed_accounts.len();
    let mut pubkeys = Vec::with_capacity(count);

    for acc in accounts.accounts {
        let pubkey = BlsPublicKey::try_from(acc.public_key.as_slice())?;
        pubkeys.push(pubkey);
    }
    for acc in accounts.distributed_accounts {
        let pubkey = BlsPublicKey::try_from(acc.composite_public_key.as_slice())?;
        pubkeys.push(pubkey);
    }

    Ok(pubkeys)
}
