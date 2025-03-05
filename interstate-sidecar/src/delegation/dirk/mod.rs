use std::fs;

use alloy::primitives::B256;
use clap::Parser;
use distributed::DistributedDirkAccount;
use ethereum_consensus::crypto::PublicKey as BlsPublicKey;
use eyre::{bail, Context, Result};
use pb::eth2_signer_api::{AccountManagerClient, ListAccountsRequest, ListAccountsResponse, ListerClient, LockAccountRequest, ResponseState, SignRequest, SignRequestId, SignerClient, UnlockAccountRequest};
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity};
use tracing::{debug, warn};
use ethereum_consensus::crypto::bls::Signature as BlsSignature;
pub mod pb;
pub mod distributed;
pub mod recover_signature;

use crate::delegation::signing::compute_domain_from_mask;

use super::{types::{
    Chain, DelegationMessage, RevocationMessage, SignedDelegation, SignedMessage, SignedRevocation
}, web3signer::Action};

#[derive(Debug, Clone)]
pub struct Dirk {
    lister: ListerClient<Channel>,
    signer: SignerClient<Channel>,
    account_mng: AccountManagerClient<Channel>,
}

impl Dirk {
    /// Connect to the DIRK server with the given address and TLS credentials.
    pub async fn connect(addr: String, credentials: DirkTlsCredentials) -> Result<Self> {
        let addr = addr.parse()?;
        let tls_config = compose_credentials(credentials)?;
        let conn = Channel::builder(addr).tls_config(tls_config)?.connect().await?;

        let lister = ListerClient::new(conn.clone());
        let signer = SignerClient::new(conn.clone());
        let account_mng = AccountManagerClient::new(conn);

        Ok(Self { lister, signer, account_mng })
    }

    /// List all accounts in the keystore.
    pub async fn list_accounts(&mut self, wallet_path: String) -> Result<ListAccountsResponse> {
        // Request all accounts in the given path. Only one path at a time
        // as done in https://github.com/wealdtech/go-eth2-wallet-dirk/blob/182f99b22b64d01e0d4ae67bf47bb055763465d7/grpc.go#L121
        let req = ListAccountsRequest { paths: vec![wallet_path] };
        let res = self.lister.list_accounts(req).await?.into_inner();

        if !matches!(res.state(), ResponseState::Succeeded) {
            bail!("Failed to list accounts: {:?}", res);
        }

        debug!(
            accounts = %res.accounts.len(),
            distributed_accounts = %res.distributed_accounts.len(),
            "List accounts request succeeded"
        );

        Ok(res)
    }

    /// Try to unlock an account using the provided passphrases
    /// If the account is unlocked, return Ok(()), otherwise return an error
    pub async fn try_unlock_account_with_passphrases(
        &mut self,
        account_name: String,
        passphrases: &[String],
    ) -> Result<()> {
        let mut unlocked = false;
        for passphrase in passphrases {
            if self.unlock_account(account_name.clone(), passphrase.clone()).await? {
                unlocked = true;
                break;
            }
        }

        if !unlocked {
            bail!("Failed to unlock account {}", account_name);
        }

        Ok(())
    }

    /// Unlock an account in the keystore with the given passphrase.
    pub async fn unlock_account(
        &mut self,
        account_name: String,
        passphrase: String,
    ) -> Result<bool> {
        let pf_bytes = passphrase.as_bytes().to_vec();
        let req = UnlockAccountRequest { account: account_name.clone(), passphrase: pf_bytes };
        let res = self.account_mng.unlock(req).await?.into_inner();

        match res.state() {
            ResponseState::Succeeded => {
                debug!("Unlock request succeeded for account {}", account_name);
                Ok(true)
            }
            ResponseState::Denied => {
                debug!("Unlock request denied for account {}", account_name);
                Ok(false)
            }
            ResponseState::Unknown => bail!("Unknown response from unlock account: {:?}", res),
            ResponseState::Failed => bail!("Failed to unlock account: {:?}", res),
        }
    }

    /// Lock an account in the keystore.
    pub async fn lock_account(&mut self, account_name: String) -> Result<bool> {
        let req = LockAccountRequest { account: account_name.clone() };
        let res = self.account_mng.lock(req).await?.into_inner();

        match res.state() {
            ResponseState::Succeeded => {
                debug!("Lock request succeeded for account {}", account_name);
                Ok(true)
            }
            ResponseState::Denied => {
                debug!("Lock request denied for account {}", account_name);
                Ok(false)
            }
            ResponseState::Unknown => bail!("Unknown response from lock account: {:?}", res),
            ResponseState::Failed => bail!("Failed to lock account: {:?}", res),
        }
    }

    /// Request a signature from the remote signer.
    pub async fn request_signature(
        &mut self,
        account_name: String,
        hash: B256,
        domain: B256,
    ) -> Result<BlsSignature> {
        let req = SignRequest {
            data: hash.to_vec(),
            domain: domain.to_vec(),
            id: Some(SignRequestId::Account(account_name)),
        };

        let res = self.signer.sign(req).await?.into_inner();

        if !matches!(res.state(), ResponseState::Succeeded) {
            bail!("Failed to sign data: {:?}", res);
        }
        if res.signature.is_empty() {
            bail!("Empty signature returned");
        }

        let sig = BlsSignature::try_from(res.signature.as_slice())
            .wrap_err("Failed to parse signature")?;

        debug!("Dirk Signature request succeeded");
        Ok(sig)
    }
}

/// Compose the TLS credentials for Dirk from the given paths.
fn compose_credentials(creds: DirkTlsCredentials) -> Result<ClientTlsConfig> {
    let client_cert = fs::read(creds.client_cert_path).wrap_err("Failed to read client cert")?;
    let client_key = fs::read(creds.client_key_path).wrap_err("Failed to read client key")?;

    // Create client identity (certificate + key)
    let identity = Identity::from_pem(&client_cert, &client_key);

    // Configure the TLS client
    let mut tls_config = ClientTlsConfig::new().identity(identity);

    // Add CA certificate if provided
    if let Some(ca_path) = creds.ca_cert_path {
        let ca_cert = fs::read(ca_path).wrap_err("Failed to read CA certificate")?;
        tls_config = tls_config.ca_certificate(Certificate::from_pem(&ca_cert));
    }

    Ok(tls_config)
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
    #[clap(long, env = "DIRK_PASSPHRASES", value_delimiter = ',', hide_env_values = true)]
    pub passphrases: Option<Vec<String>>,

    /// The TLS credentials for connecting to the DIRK keystore.
    #[clap(flatten)]
    pub tls_credentials: DirkTlsCredentials,
}

/// Generate signed delegations/revocations using remote Dirk signers
pub async fn generate_from_dirk(
    opts: DirkOpts,
    delegatee_pubkey: BlsPublicKey,
    chain: Chain,
    action: Action,
) -> Result<Vec<SignedMessage>> {
    // read the accounts from the remote Dirk signer at the provided URL
    let mut dirk = Dirk::connect(opts.url.clone(), opts.tls_credentials.clone()).await?;
    let accounts = dirk.list_accounts(opts.wallet_path).await?;
    debug!(
        regular = %accounts.accounts.len(),
        distributed = %accounts.distributed_accounts.len(),
        "Found remote accounts"
    );

    // specify the signing domain (it needs to be included in the signing requests)
    let domain = B256::from(compute_domain_from_mask(chain.fork_version()));

    let total_accounts = accounts.accounts.len() + accounts.distributed_accounts.len();
    let mut signed_messages = Vec::with_capacity(total_accounts);

    // regular and distributed account work differently.
    // - For regular accounts, we can sign the message directly
    // - For distributed accounts, we need to:
    //    - Look into the account's participants and threshold configuration
    //    - Connect to at least `threshold` nodes individually
    //    - Sign the message on each node
    //    - Aggregate the signatures

    for account in accounts.accounts {
        let name = account.name.clone();
        let validator_pubkey = BlsPublicKey::try_from(account.public_key.as_slice())?;

        if let Some(passphrases) = &opts.passphrases {
            dirk.try_unlock_account_with_passphrases(name.clone(), passphrases).await?;
        } else {
            bail!("A passphrase is required in order to sign messages remotely with Dirk");
        }

        // Sign the message with the connected Dirk instance
        let signed_message = match action {
            Action::Delegate => {
                let message = DelegationMessage::new(validator_pubkey, delegatee_pubkey.clone());
                let root = message.digest().into(); // Dirk does the hash tree root internally
                let signature = dirk.request_signature(name.clone(), root, domain).await?;
                SignedMessage::Delegation(SignedDelegation { message, signature })
            }
            Action::Revoke => {
                let message = RevocationMessage::new(validator_pubkey, delegatee_pubkey.clone());
                let root = message.digest().into(); // Dirk does the hash tree root internally
                let signature = dirk.request_signature(name.clone(), root, domain).await?;
                SignedMessage::Revocation(SignedRevocation { message, signature })
            }
        };

        // Try to lock the account back after signing
        if let Err(err) = dirk.lock_account(name.clone()).await {
            warn!("Failed to lock account after signing {}: {:?}", name, err);
        }

        signed_messages.push(signed_message);
    }

    for account in accounts.distributed_accounts {
        let name = account.name.clone();
        let distributed_dirk = DistributedDirkAccount::new(account, opts.tls_credentials.clone())?;
        let validator_pubkey = distributed_dirk.composite_public_key().clone();

        // Sign the message with the distributed Dirk account (threshold signature of the quorum)
        let signed_message = match action {
            Action::Delegate => {
                let message = DelegationMessage::new(validator_pubkey, delegatee_pubkey.clone());
                let root = message.digest().into(); // Dirk does the hash tree root internally
                let signature = distributed_dirk.threshold_sign(name.clone(), root, domain).await?;
                SignedMessage::Delegation(SignedDelegation { message, signature })
            }
            Action::Revoke => {
                let message = RevocationMessage::new(validator_pubkey, delegatee_pubkey.clone());
                let root = message.digest().into(); // Dirk does the hash tree root internally
                let signature = distributed_dirk.threshold_sign(name.clone(), root, domain).await?;
                SignedMessage::Revocation(SignedRevocation { message, signature })
            }
        };

        // Sanity check: verify the recovered signature early to debug aggregate signature issues
        // Note: this is done twice (here and in the main loop) to help debug sharded signatures
        if let Err(err) = signed_message.verify_signature(chain) {
            bail!(
                "Failed to verify recovered signature for distributed account '{}': {:?}",
                name,
                err
            );
        }

        // Add the final message to the list of signed messages
        signed_messages.push(signed_message);
    }

    // Sanity check: count the total number of signed messages
    if signed_messages.len() != total_accounts {
        bail!(
            "Failed to sign messages for all accounts. Expected {}, got {}",
            total_accounts,
            signed_messages.len()
        );
    }

    Ok(signed_messages)
}