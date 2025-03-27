use std::{fs, fs::DirEntry, path::PathBuf, env, collections::HashMap, ffi::OsString, io, path::Path};
use dotenv::dotenv;
use alloy::{
    primitives::B256,
    signers::k256::sha2::{Digest, Sha256},
};
use blst::{min_pk::Signature, BLST_ERROR};
use clap::{Parser, ValueEnum};
use ethereum_consensus::{
    crypto::{PublicKey as BlsPublicKey, SecretKey as BlsSecretKey, Signature as BlsSignature},
    deneb::{compute_fork_data_root, compute_signing_root, Root},
};
use eyre::{bail, eyre, Context, ContextCompat, Result};
use lighthouse_eth2_keystore::Keystore;
use reqwest::{Certificate, Identity, StatusCode, Url};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};
use tracing_subscriber::fmt::Subscriber;

// Constants
pub const COMMIT_BOOST_DOMAIN_MASK: [u8; 4] = [109, 109, 111, 67];
/// The BLS Domain Separator used in Ethereum 2.0.
pub const BLS_DST_PREFIX: &[u8] = b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_POP_";

/// Default password used for keystores in the test vectors.
///
/// Reference: https://eips.ethereum.org/EIPS/eip-2335#test-cases
pub const DEFAULT_KEYSTORE_PASSWORD: &str = r#"𝔱𝔢𝔰𝔱𝔭𝔞𝔰𝔰𝔴𝔬𝔯𝔡🔑"#;

const PERMISSION_DELEGATE_PATH: &str = "/constraints/v1/builder/delegate";


/// CLI arguments
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    /// The action to perform (delegate or revoke)
    action: Action,
}


#[tokio::main]
async fn main() ->eyre::Result<()> {
    dotenv().ok();
    
    let subscriber = Subscriber::builder()
    .with_max_level(tracing::Level::DEBUG)
    .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let signer_type = env::var("SIGNER_TYPE").expect("please set a signer_type");
    let relay_url  = env::var("RELAY_URL").expect("couldn't find relay url in env file");
    let delegate_pubkey_str = env::var("DELEGATEE_PUBLICKEY").expect("couldn't find delegatee publickey in env file");
    let delegatee_pubkey:BlsPublicKey = parse_bls_public_key(delegate_pubkey_str.as_str()).expect("Invalid public key");
    let relay_endpoint = relay_url + PERMISSION_DELEGATE_PATH;  // Create the full URL once
    let out = env::var("OUT_FILE").expect("couldn't find out file in env file");


    if signer_type == "KEYSTORES" {
        let keys_path = env::var("KEYS_PATH").expect("couldn't find keys path in env file");
        let password_path = env::var("SECRETS_PATH").expect("couldn't find secrets path in env file");
        let keystore_secret = KeystoreSecret::from_directory(password_path.as_str()).unwrap();

        let signed_messages = generate_from_keystore(
            &keys_path,
            keystore_secret,
            delegatee_pubkey.clone(),
            Chain::Helder,
            Action::Delegate,
        ).expect("Invalid signed message request");

        debug!("Signed {} messages with keystore", signed_messages.len());

        // Verify signatures
        for message in &signed_messages {
            verify_message_signature(message, Chain::Helder).expect("invalid signature");
        }

        let client = reqwest::ClientBuilder::new().build().unwrap();

        let response = client
            .post(&relay_endpoint)
            .header("content-type", "application/json")
            .body(serde_json::to_string(&signed_messages)?)
            .send()
            .await?;

        let status = response.status();
        // Print response status
        info!("Response status: {}", status);

        // Print response body
        let body = response.text().await?;
        info!("Response body: {}", body);

        if status != StatusCode::OK {
            error!("failed to send  delegations to relay");
        } else {
            info!("submited  {} delegations to relay", signed_messages.len());
        }
    }

    if signer_type == "WEB3SIGNER" {
        let web3signer_url = env::var("WEB3SIGNER_URL").expect("couldn't find web3signer url in env file");

        let signed_messages_web3 = generate_from_web3signer(
            Web3SignerOpts{
                url:web3signer_url},
            delegatee_pubkey,
            Action::Delegate
            ).await?;

        debug!("Signed {} messages with web3signature", signed_messages_web3.len());

        let client = reqwest::ClientBuilder::new().build().unwrap();

        let response = client
            .post(&relay_endpoint)
            .header("content-type", "application/json")
            .body(serde_json::to_string(&signed_messages_web3)?)
            .send()
            .await?;

        let status = response.status();

        // Print response status
        info!("Response status: {}", status);

        // Print response body
        let body = response.text().await?;
        info!("Response body: {}", body);

        if status != StatusCode::OK {
            error!("failed to send  delegations to relay");
        } else {
            info!("submited  {} delegations to relay", signed_messages_web3.len());
        }
    }

    Ok(())
}


/// Generate signed delegations/revocations using a keystore file
///
/// - Read the keystore file
/// - Decrypt the keypairs using the password
/// - Create messages
/// - Compute the signing roots and sign the message
/// - Return the signed message
pub fn generate_from_keystore(
    keys_path: &str,
    keystore_secret: KeystoreSecret,
    delegatee_pubkey: BlsPublicKey,
    chain: Chain,
    action: Action,
) -> Result<Vec<SignedMessage>> {
    let keystores_paths = keystore_paths(keys_path)?;
    let mut signed_messages = Vec::with_capacity(keystores_paths.len());
    debug!("Found {} keys in the keystore", keystores_paths.len());

    for path in keystores_paths {
        let ks = Keystore::from_json_file(path).map_err(KeystoreError::Eth2Keystore)?;
        let password = keystore_secret.get(ks.pubkey()).ok_or(KeystoreError::MissingPassword)?;
        let kp = ks.decrypt_keypair(password.as_bytes()).map_err(KeystoreError::Eth2Keystore)?;
        let validator_pubkey = BlsPublicKey::try_from(kp.pk.serialize().to_vec().as_ref())?;
        let validator_private_key = kp.sk;

        match action {
            Action::Delegate => {
                let message = DelegationMessage::new(validator_pubkey, delegatee_pubkey.clone());
                let signing_root = compute_commit_boost_signing_root(message.digest(), &chain)?;
                let signature = validator_private_key.sign(signing_root.0.into());
                let signature = BlsSignature::try_from(signature.serialize().as_ref())?;
                let signed = SignedDelegation { message, signature };
                signed_messages.push(SignedMessage::Delegation(signed));
            }
            Action::Revoke => {
                let message = RevocationMessage::new(validator_pubkey, delegatee_pubkey.clone());
                let signing_root = compute_commit_boost_signing_root(message.digest(), &chain)?;
                let signature = validator_private_key.sign(signing_root.0.into());
                let signature = BlsSignature::try_from(signature.serialize().as_ref())?;
                let signed = SignedRevocation { message, signature };
                signed_messages.push(SignedMessage::Revocation(signed));
            }
        }
    }

    Ok(signed_messages)
}

/// Verify the signature of a signed message
pub fn verify_message_signature(message: &SignedMessage, chain: Chain) -> Result<()> {
    match message {
        SignedMessage::Delegation(signed_delegation) => {
            let signer_pubkey = signed_delegation.message.validator_pubkey.clone();
            let digest = signed_delegation.message.digest();
  
            let blst_sig =
                blst::min_pk::Signature::from_bytes(signed_delegation.signature.as_ref())
                    .map_err(|e| eyre::eyre!("Failed to parse signature: {:?}", e))?;
  
            // Verify the signature
            verify_commit_boost_root(signer_pubkey, digest, &blst_sig, &chain)
        }
        SignedMessage::Revocation(signed_revocation) => {
            let signer_pubkey = signed_revocation.message.validator_pubkey.clone();
            let digest = signed_revocation.message.digest();
  
            let blst_sig =
                blst::min_pk::Signature::from_bytes(signed_revocation.signature.as_ref())
                    .map_err(|e| eyre::eyre!("Failed to parse signature: {:?}", e))?;
  
            // Verify the signature
            verify_commit_boost_root(signer_pubkey, digest, &blst_sig, &chain)
        }
    }
  }
  


/// Verify the signature with the public key of the signer using the Commit Boost domain.
#[allow(dead_code)]
pub fn verify_commit_boost_root(
    pubkey: BlsPublicKey,
    root: [u8; 32],
    signature: &Signature,
    chain: &Chain,
) -> Result<()> {
    verify_root(pubkey, root, signature, compute_domain_from_mask(chain.fork_version()))
}
  
/// Verify the signature of the object with the given public key.
pub fn verify_root(
    pubkey: BlsPublicKey,
    root: [u8; 32],
    signature: &Signature,
    domain: [u8; 32],
) -> Result<()> {
    let signing_root = compute_signing_root(&root, domain)?;
    let pk = blst::min_pk::PublicKey::from_bytes(pubkey.as_ref()).unwrap();
    let res = signature.verify(true, signing_root.as_ref(), BLS_DST_PREFIX, &[], &pk, true);
    if res == BLST_ERROR::BLST_SUCCESS {
        Ok(())
    } else {
        Err(eyre!("bls verification failed"))
    }
}
  
  
/// Helper function to compute the signing root for a message
pub fn compute_commit_boost_signing_root(message: [u8; 32], chain: &Chain) -> Result<B256> {
    compute_signing_root(&message, compute_domain_from_mask(chain.fork_version()))
        // Ethereum-consensus uses a different version of alloy so we need to do this cast
        .map(|r| B256::from_slice(r.to_vec().as_slice()))
        .map_err(|e| eyre!("Failed to compute signing root: {}", e))
}
  
/// Compute the commit boost domain from the fork version
pub fn compute_domain_from_mask(fork_version: [u8; 4]) -> [u8; 32] {
    let mut domain = [0; 32];

    // Note: the application builder domain specs require the genesis_validators_root
    // to be 0x00 for any out-of-protocol message. The commit-boost domain follows the
    // same rule.
    let root = Root::default();
    let fork_data_root = compute_fork_data_root(fork_version, root).expect("valid fork data");

    domain[..4].copy_from_slice(&COMMIT_BOOST_DOMAIN_MASK);
    domain[4..].copy_from_slice(&fork_data_root[..28]);
    domain
}

#[derive(Debug, thiserror::Error)]
pub enum KeystoreError {
    #[error("failed to read keystore directory: {0}")]
    ReadFromDirectory(#[from] std::io::Error),
    #[error("Failed to read or decrypt keystore: {0:?}")]
    Eth2Keystore(lighthouse_eth2_keystore::Error),
    #[error("Missing password for keypair")]
    MissingPassword,
}

impl KeystoreSecret {
    /// Load the keystore passwords from a directory containing individual password files.
    pub fn from_directory(root_dir: &str) -> Result<Self> {
        let mut secrets = HashMap::new();
        for entry in fs::read_dir(root_dir)
            .wrap_err(format!("failed to read secrets directory. path: {}", &root_dir))?
        {
            let entry = entry.wrap_err("Failed to read secrets directory entry")?;
            let path = entry.path();

            let filename = path.file_name().wrap_err("Secret file name")?.to_string_lossy();
            let secret = fs::read_to_string(&path).wrap_err("Failed to read secret file")?;
            secrets.insert(filename.trim_start_matches("0x").to_string(), secret);
        }
        Ok(Self::Directory(secrets))
    }

    /// Set a unique password for all validators in the keystore.
    pub fn from_unique_password(password: String) -> Self {
        Self::Unique(password)
    }

    /// Get the password for the given validator public key.
    pub fn get(&self, validator_pubkey: &str) -> Option<&str> {
        match self {
            Self::Unique(password) => Some(password.as_str()),
            Self::Directory(secrets) => secrets.get(validator_pubkey).map(|s| s.as_str()),
        }
    }
}

/// Manual drop implementation to clear the password from memory
/// when the KeystoreSecret is dropped.
impl Drop for KeystoreSecret {
    fn drop(&mut self) {
        match self {
            Self::Unique(password) => {
                let bytes = unsafe { password.as_bytes_mut() };
                for b in bytes.iter_mut() {
                    *b = 0;
                }
            }
            Self::Directory(secrets) => {
                for secret in secrets.values_mut() {
                    let bytes = unsafe { secret.as_bytes_mut() };
                    for b in bytes.iter_mut() {
                        *b = 0;
                    }
                }
            }
        }
    }
}

/// Returns the paths of all the keystore files provided in `keys_path`.
///
/// We're expecting a directory structure like:
/// ${keys_path}/
/// -- 0x1234.../validator.json
/// -- 0x5678.../validator.json
/// -- ...
/// Reference: https://github.com/chainbound/bolt/blob/4634ff905561009e4e74f9921dfdabf43717010f/bolt-sidecar/src/signer/keystore.rs#L109
pub fn keystore_paths(keys_path: &str) -> Result<Vec<PathBuf>> {
    let keys_path_buf = Path::new(keys_path).to_path_buf();
    let json_extension = OsString::from("json");

    let mut keystores_paths = vec![];
    // Iter over the `keys` directory
    for entry in read_dir(keys_path_buf)
        .wrap_err(format!("failed to read keys directory. path: {keys_path}"))?
    {
        let path = read_path(entry)?;
        if path.is_dir() {
            for entry in read_dir(path)? {
                let path = read_path(entry)?;
                if path.is_file() && path.extension() == Some(&json_extension) {
                    keystores_paths.push(path);
                }
            }
        }
    }

    Ok(keystores_paths)
}

fn read_path(entry: io::Result<DirEntry>) -> Result<PathBuf> {
    Ok(entry.map_err(KeystoreError::ReadFromDirectory)?.path())
}

fn read_dir(path: PathBuf) -> Result<fs::ReadDir> {
    fs::read_dir(path).wrap_err("Failed to read directory")
}

/// Parse a BLS public key from a string
pub fn parse_bls_public_key(delegatee_pubkey: &str) -> Result<BlsPublicKey> {
    let hex_pk = delegatee_pubkey.strip_prefix("0x").unwrap_or(delegatee_pubkey);
    BlsPublicKey::try_from(
        hex::decode(hex_pk).wrap_err("Failed to hex-decode delegatee pubkey")?.as_slice(),
    )
    .map_err(|e| eyre::eyre!("Failed to parse delegatee public key '{}': {}", hex_pk, e))
}

/// Write some serializable data to an output json file
pub fn write_to_file<T: Serialize>(out: &str, data: &T) -> Result<()> {
    let out_path = PathBuf::from(out);
    let out_file = fs::File::create(out_path)?;
    serde_json::to_writer_pretty(out_file, data)?;
    Ok(())
}

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
                // let signature = BlsSignature::try_from(trimmed_signature.as_bytes())?;
                let signature = BlsSignature::try_from(hex::decode(trimmed_signature)?.as_slice())?;
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

#[derive(Serialize, Deserialize)]
pub struct Keys {
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
pub struct CommitBoostKeys {
    pub keys: Vec<Keys>,
}

/// Request signature from the Web3Signer.
#[derive(Serialize, Deserialize)]
pub struct CommitBoostSignatureRequest {
    #[serde(rename = "type")]
    pub type_: String,
    pub pubkey: String,
    pub object_root: String,
}

/// Supported chains for the CLI
#[derive(Debug, Clone, Copy, ValueEnum, Hash, PartialEq, Eq)]
#[clap(rename_all = "kebab_case")]
pub enum Chain {
    Mainnet,
    Holesky,
    Helder,
    Kurtosis,
}

impl Chain {
    /// Get the fork version for the given chain.
    pub fn fork_version(&self) -> [u8; 4] {
        match self {
            Chain::Mainnet => [0, 0, 0, 0],
            Chain::Holesky => [1, 1, 112, 0],
            Chain::Helder => [16, 0, 0, 0],
            Chain::Kurtosis => [16, 0, 0, 56],
        }
    }

    pub fn from_id(id: u64) -> Option<Self> {
        match id {
            1 => Some(Self::Mainnet),
            17000 => Some(Self::Holesky),
            3151908 => Some(Self::Kurtosis),
            7014190335 => Some(Self::Helder),
            _ => None,
        }
    }
}

/// The action to perform.
#[derive(Debug, Clone, ValueEnum, PartialEq)]
#[clap(rename_all = "kebab_case")]
pub enum Action {
    /// Create a delegation message.
    Delegate,
    /// Create a revocation message.
    Revoke,
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
enum SignedMessageAction {
    /// Signal delegation of a validator pubkey to a delegatee pubkey.
    Delegation,
    /// Signal revocation of a previously delegated pubkey.
    Revocation,
}

/// Transparent serialization of signed messages.
/// This is used to serialize and deserialize signed messages
///
/// e.g. serde_json::to_string(&signed_message):
/// ```
/// {
///    "message": {
///       "action": 0,
///       "validator_pubkey": "0x...",
///       "delegatee_pubkey": "0x..."
///    },
///   "signature": "0x..."
/// },
/// ```
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum SignedMessage {
    Delegation(SignedDelegation),
    Revocation(SignedRevocation),
}

impl SignedMessage {
    /// Verify the signature of a signed message
    pub fn verify_signature(&self, chain: Chain) -> eyre::Result<()> {
        match self {
            Self::Delegation(signed_delegation) => {
                let signer_pubkey = signed_delegation.message.validator_pubkey.clone();
                let digest = signed_delegation.message.digest();

                let blst_sig =
                    blst::min_pk::Signature::from_bytes(signed_delegation.signature.as_ref())
                        .map_err(|e| eyre::eyre!("Failed to parse signature: {:?}", e))?;

                // Verify the signature
                verify_commit_boost_root(signer_pubkey, digest, &blst_sig, &chain)
            }
            Self::Revocation(signed_revocation) => {
                let signer_pubkey = signed_revocation.message.validator_pubkey.clone();
                let digest = signed_revocation.message.digest();

                let blst_sig =
                    blst::min_pk::Signature::from_bytes(signed_revocation.signature.as_ref())
                        .map_err(|e| eyre::eyre!("Failed to parse signature: {:?}", e))?;

                // Verify the signature
                verify_commit_boost_root(signer_pubkey, digest, &blst_sig, &chain)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SignedDelegation {
    pub message: DelegationMessage,
    pub signature: BlsSignature,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DelegationMessage {
    action: u8,
    pub validator_pubkey: BlsPublicKey,
    pub delegatee_pubkey: BlsPublicKey,
}

impl DelegationMessage {
    /// Create a new delegation message.
    pub fn new(validator_pubkey: BlsPublicKey, delegatee_pubkey: BlsPublicKey) -> Self {
        Self { action: SignedMessageAction::Delegation as u8, validator_pubkey, delegatee_pubkey }
    }

    /// Compute the digest of the delegation message.
    pub fn digest(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update([self.action]);
        hasher.update(self.validator_pubkey.to_vec());
        hasher.update(self.delegatee_pubkey.to_vec());

        hasher.finalize().into()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SignedRevocation {
    pub message: RevocationMessage,
    pub signature: BlsSignature,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RevocationMessage {
    action: u8,
    pub validator_pubkey: BlsPublicKey,
    pub delegatee_pubkey: BlsPublicKey,
}

impl RevocationMessage {
    /// Create a new revocation message.
    pub fn new(validator_pubkey: BlsPublicKey, delegatee_pubkey: BlsPublicKey) -> Self {
        Self { action: SignedMessageAction::Revocation as u8, validator_pubkey, delegatee_pubkey }
    }

    /// Compute the digest of the revocation message.
    pub fn digest(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update([self.action]);
        hasher.update(self.validator_pubkey.to_vec());
        hasher.update(self.delegatee_pubkey.to_vec());

        hasher.finalize().into()
    }
}

/// EIP-2335 keystore secret kind.
pub enum KeystoreSecret {
    /// When using a unique password for all validators in the keystore
    /// (e.g. for Prysm keystore)
    Unique(String),
    /// When using a directory to hold individual passwords for each validator
    /// according to the format: secrets/0x{validator_pubkey} = {password}
    Directory(HashMap<String, String>),
}