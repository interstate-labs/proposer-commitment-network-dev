use std::fs;

use alloy::primitives::B256;
use ethereum_consensus::crypto::bls::Signature as BlsSignature;
use eyre::{bail, Context, Result};
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity};
use tracing::debug;

use super::{
    pb::eth2_signer_api::{
        AccountManagerClient, ListAccountsRequest, ListAccountsResponse, ListerClient,
        LockAccountRequest, ResponseState, SignRequest, SignRequestId, SignerClient,
        UnlockAccountRequest,
    },
    DirkTlsCredentials,
};

/// Utility to recover a threshold signature from partial signature shards.
mod recover_signature;

/// Module to work with Dirk distributed accounts.
pub mod distributed;

/// Test utilities for the DIRK client.
#[cfg(test)]
pub mod test_util;

/// A Dirk remote signer.
///
/// Available services:
/// - `Lister`: List accounts in the keystore.
/// - `Signer`: Request a signature from the remote signer.
/// - `AccountManager`: Manage accounts in the keystore (lock and unlock accounts).
///
/// Reference: https://github.com/attestantio/dirk
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
        let conn = Channel::builder(addr)
            .tls_config(tls_config)?
            .connect()
            .await?;

        let lister = ListerClient::new(conn.clone());
        let signer = SignerClient::new(conn.clone());
        let account_mng = AccountManagerClient::new(conn);

        Ok(Self {
            lister,
            signer,
            account_mng,
        })
    }

    /// List all accounts in the keystore.
    pub async fn list_accounts(&mut self, wallet_path: String) -> Result<ListAccountsResponse> {
        // Request all accounts in the given path. Only one path at a time
        // as done in https://github.com/wealdtech/go-eth2-wallet-dirk/blob/182f99b22b64d01e0d4ae67bf47bb055763465d7/grpc.go#L121
        let req = ListAccountsRequest {
            paths: vec![wallet_path],
        };
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
            if self
                .unlock_account(account_name.clone(), passphrase.clone())
                .await?
            {
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
        let req = UnlockAccountRequest {
            account: account_name.clone(),
            passphrase: pf_bytes,
        };
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
        let req = LockAccountRequest {
            account: account_name.clone(),
        };
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Test connecting to a DIRK server and listing available accounts.
    ///
    /// ```shell
    /// cargo test --package bolt --bin bolt -- common::dirk::tests::test_dirk_single_connection_e2e
    /// --exact --show-output --ignored
    /// ```
    #[tokio::test]
    #[ignore = "Requires Dirk to be installed on the system"]
    async fn test_dirk_single_connection_e2e() -> eyre::Result<()> {
        let (url, cred, mut dirk_proc) = test_util::start_single_dirk_test_server().await?;
        let mut dirk = Dirk::connect(url, cred).await?;

        let accounts = dirk.list_accounts("wallet1".to_string()).await?;
        println!("Dirk Accounts: {:?}", accounts);

        // make sure to stop the dirk server
        dirk_proc.kill()?;

        Ok(())
    }

    /// Test unlocking an account in the DIRK server.
    /// This test requires a running DIRK server.
    ///
    /// ```shell
    /// cargo test --package bolt --bin bolt -- common::dirk::tests::test_unlock_account_e2e
    /// --exact --show-output --ignored
    /// ```
    #[tokio::test]
    #[ignore = "Requires Dirk to be installed on the system"]
    async fn test_unlock_account_e2e() -> eyre::Result<()> {
        let (url, cred, mut dirk_proc) = test_util::start_single_dirk_test_server().await?;
        let mut dirk = Dirk::connect(url, cred).await?;

        let account_name = "account1".to_string();
        let passphrase = "secret".to_string();

        let unlocked = dirk.unlock_account(account_name, passphrase).await?;
        println!("Account unlocked: {}", unlocked);

        // make sure to stop the dirk server
        dirk_proc.kill()?;

        Ok(())
    }

    /// Test locking an account in the DIRK server.
    ///
    /// ```shell
    /// cargo test --package bolt --bin bolt -- common::dirk::tests::test_dirk_multi_connection_e2e
    /// --exact --show-output --ignored
    /// ```
    ///
    /// NOTE: in order for the example certificates to work on your machine, you need to
    /// modify the /etc/hosts file to include the following entry:
    ///
    /// ```text
    /// 127.0.0.1       localhost localhost-1 localhost-2 localhost-3
    /// ```
    ///
    /// This is because we map 3 different server certificates to localhost in order
    /// to test the multi-node functionality of DIRK.
    #[tokio::test]
    #[ignore = "Requires Dirk to be installed on the system"]
    async fn test_dirk_multi_connection_e2e() -> eyre::Result<()> {
        let (url, cred, mut dirk_procs) = test_util::start_multi_dirk_test_server().await?;
        let mut dirk = Dirk::connect(url, cred).await?;

        let accounts = dirk
            .list_accounts("DistributedWallet1/1".to_string())
            .await?;
        println!("Dirk Accounts: {:?}", accounts);

        // make sure to stop the dirk servers
        for proc in &mut dirk_procs {
            proc.kill()?;
        }

        Ok(())
    }
}
