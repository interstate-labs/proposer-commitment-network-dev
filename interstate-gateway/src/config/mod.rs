use group_config::{HOLEKSY_CHAIN_ID, KURTOSIS_CHAIN_ID, MAINNET_CHAIN_ID, HELDER_CHAIN_ID};
use reqwest::Url;

use rand::RngCore;
use std::{collections::HashMap, path::PathBuf, str::FromStr};

use alloy::primitives::Address;
use blst::min_pk::SecretKey as BLSSecretKey;

pub mod group_config;
pub mod limits;
pub use group_config::{Chain, ChainConfig, ValidatorIndexes};

/// Default port for the commitment server exposed by the sidecar.
pub const DEFAULT_COMMITMENT_PORT: u16 = 8000;

/// Default port for the MEV-Boost proxy server.
pub const DEFAULT_MEV_BOOST_PROXY_PORT: u16 = 18551;

pub const DEFAULT_METRICS_PORT: u16 = 8018;

/// Configuration of the sidecar.
#[derive(Debug, Clone)]
pub struct Config {
    /// Port to listen on for incoming commitment requests
    pub commitment_port: u16,
    /// Port to listen on for incoming commitment requests
    pub metrics_port: u16,
    /// The builder server port to listen on (handling constraints apis)
    pub builder_port: u16,
    /// The constraints collector url
    pub cb_url: Url,
    /// relay url
    pub relay_url: Url,
    /// The router url
    pub sidecar_info_sender_url: Url,
    /// URL for the beacon client API URL
    pub beacon_api_url: Url,
    /// The execution API url
    pub execution_api_url: Url,
    /// The engine API url
    pub engine_api_url: Url,
    /// The chain on which the sidecar is running
    pub chain: ChainConfig,
    /// The jwt.hex secret to authenticate calls to the engine API
    pub jwt_hex: String,
    /// The fee recipient address for fallback blocks
    pub fee_recipient: Address,
    /// Local builder bls private key for signing fallback payloads.
    pub builder_bls_private_key: BLSSecretKey,
    pub keystore_secrets_path: PathBuf,
    /// Path to the keystores folder.
    pub keystore_pubkeys_path: PathBuf,
    /// Path to the delegations file.
    /// Gateway contract address
    pub gateway_contract: Address,
    /// Web3Signer settings
    pub web3signer_url: String,
    pub ca_cert_path: String,
    pub combined_pem_path: String,
    pub commit_boost_signer_url: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            commitment_port: DEFAULT_COMMITMENT_PORT,
            builder_port: DEFAULT_MEV_BOOST_PROXY_PORT,
            metrics_port: DEFAULT_METRICS_PORT,
            cb_url: "http://localhost:3030".parse().expect("Valid URL"),
            relay_url: "http://localhost:3040".parse().expect("Valid URL"),
            sidecar_info_sender_url: "http://localhost:8000".parse().expect("Valid URL"),
            beacon_api_url: "http://localhost:5052".parse().expect("Valid URL"),
            execution_api_url: "http://localhost:8545".parse().expect("Valid URL"),
            engine_api_url: "http://localhost:8551".parse().expect("Valid URL"),
            chain: ChainConfig::default(),
            jwt_hex: String::new(),
            fee_recipient: Address::ZERO,
            builder_bls_private_key: random_bls_secret(),
            gateway_contract: Address::from_str("0x8aC112a5540f441cC9beBcC647041A6E0D595B94")
                .unwrap(),
            web3signer_url: String::new(),
            ca_cert_path: String::new(),
            combined_pem_path: String::new(),
            commit_boost_signer_url: String::new(),
            keystore_secrets_path: PathBuf::from(
                "/root/assigned_data/secrets",
            ),
            keystore_pubkeys_path: PathBuf::from(
                "/root/assigned_data/keys",
            ),
        }
    }
}

impl Config {
    pub fn new(envs: HashMap<String, String>) -> Self {
        let chain = ChainConfig {
            chain: match envs["CHAIN"].clone().as_str() {
                "kurtosis" => Chain::Kurtosis,
                "mainnet" => Chain::Mainnet,
                "holesky" => Chain::Holesky,
                "helder" => Chain::Helder,
                _ => Chain::Holesky,
            },
            commitment_deadline: envs["COMMITMENT_DEADLINE"].parse().unwrap(),
            slot_time: envs["SLOT_TIME"].parse().unwrap(),
            id: match envs["CHAIN"].clone().as_str() {
                "kurtosis" => KURTOSIS_CHAIN_ID,
                "mainnet" => HOLEKSY_CHAIN_ID,
                "holesky" => HOLEKSY_CHAIN_ID,
                "helder" => KURTOSIS_CHAIN_ID,
                _ => HOLEKSY_CHAIN_ID,
            },
        };

        Self {
            commitment_port: envs["COMMITMENT_PORT"].parse().unwrap(),
            metrics_port: envs["METRICS_PORT"].parse().unwrap(),
            builder_port: envs["BUILDER_PORT"].parse().unwrap(),
            cb_url: "http://localhost:3030".parse().expect("Valid URL"),
            relay_url: envs["RELAY_URL"].parse().expect("Valid URL"),
            sidecar_info_sender_url: "http://localhost:8000".parse().expect("Valid URL"),
            beacon_api_url: envs["BEACON_API_URL"].parse().expect("Valid URL"),
            execution_api_url: envs["EXECUTION_API_URL"].parse().expect("Valid URL"),
            engine_api_url: envs["ENGINE_API_URL"].parse().expect("Valid URL"),
            chain: chain,
            jwt_hex: envs["JWT"].clone(),
            fee_recipient: Address::parse_checksummed(&envs["FEE_RECIPIENT"], None).unwrap(),
            builder_bls_private_key: random_bls_secret(),
            gateway_contract: Address::from_str("0x8aC112a5540f441cC9beBcC647041A6E0D595B94")
            .unwrap(),
            web3signer_url: "http://localhost:3030".parse().expect("Valid URL"),
            ca_cert_path: String::new(),
            combined_pem_path: String::new(),
            commit_boost_signer_url: "http://localhost:3030".parse().expect("Valid URL"),
            keystore_secrets_path: PathBuf::from(envs["KEYSTORE_SECRETS_PATH"].as_str()),
            keystore_pubkeys_path: PathBuf::from(envs["KEYSTORE_PUBKEYS_PATH"].as_str()),
        }
    }
}

/// Generate a random BLS secret key.
pub fn random_bls_secret() -> BLSSecretKey {
    let mut rng = rand::thread_rng();
    let mut ikm = [0u8; 32];
    rng.fill_bytes(&mut ikm);
    BLSSecretKey::key_gen(&ikm, &[]).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    #[test]
    fn test_config_default() {
        let default_config = Config::default();

        assert_eq!(default_config.commitment_port, DEFAULT_COMMITMENT_PORT);
        assert_eq!(default_config.builder_port, DEFAULT_MEV_BOOST_PROXY_PORT);
        assert_eq!(
            default_config.cb_url.as_str(),
            "http://localhost:3030/"
        );
        assert_eq!(
            default_config.beacon_api_url.as_str(),
            "http://localhost:5052/"
        );
        assert_eq!(
            default_config.execution_api_url.as_str(),
            "http://localhost:8545/"
        );
        assert_eq!(
            default_config.engine_api_url.as_str(),
            "http://localhost:8551/"
        );
        assert!(default_config.jwt_hex.is_empty());
        assert_eq!(default_config.fee_recipient, Address::ZERO);
    }

    #[test]
    fn test_config_new() {
        let mut envs = HashMap::new();
        envs.insert("COMMITMENT_PORT".to_string(), "8001".to_string());
        envs.insert("BUILDER_PORT".to_string(), "18552".to_string());
        envs.insert("METRICS_PORT".to_string(), "8018".to_string());
        envs.insert(
            "CB_URL".to_string(),
            "http://localhost:4000".to_string(),
        );
        envs.insert(
            "COLLECTOR_SOCKET".to_string(),
            "ws://localhost:4001".to_string(),
        );
        envs.insert(
            "BEACON_API_URL".to_string(),
            "http://localhost:6000".to_string(),
        );
        envs.insert(
            "EXECUTION_API_URL".to_string(),
            "http://localhost:7000".to_string(),
        );
        envs.insert(
            "ENGINE_API_URL".to_string(),
            "http://localhost:8000".to_string(),
        );
        envs.insert("CHAIN".to_string(), "kurtosis".to_string());
        envs.insert("COMMITMENT_DEADLINE".to_string(), "12".to_string());
        envs.insert("SLOT_TIME".to_string(), "10".to_string());
        envs.insert("JWT".to_string(), "test-jwt".to_string());
        envs.insert(
            "FEE_RECIPIENT".to_string(),
            "0x0000000000000000000000000000000000000001".to_string(),
        );
        envs.insert(
            "GATEWAY_CONTRACT".to_string(),
            "0x6db20C530b3F96CD5ef64Da2b1b931Cb8f264009".to_string(),
        );

        let config = Config::new(envs);

        assert_eq!(config.commitment_port, 8001);
        assert_eq!(config.builder_port, 18552);
        assert_eq!(config.metrics_port, 8018);
        assert_eq!(config.cb_url.as_str(), "http://localhost:4000/");
        assert_eq!(config.beacon_api_url.as_str(), "http://localhost:6000/");
        assert_eq!(config.execution_api_url.as_str(), "http://localhost:7000/");
        assert_eq!(config.engine_api_url.as_str(), "http://localhost:8000/");
        assert_eq!(config.jwt_hex, "test-jwt");
        assert_eq!(
            config.fee_recipient,
            Address::parse_checksummed("0x0000000000000000000000000000000000000001", None).unwrap()
        );

        assert_eq!(config.chain.id, KURTOSIS_CHAIN_ID);
        assert_eq!(config.chain.commitment_deadline, 12);
        assert_eq!(config.chain.slot_time, 10);
    }

    #[test]
    fn test_random_bls_secret() {
        let key1 = random_bls_secret();
        let key2 = random_bls_secret();

        assert_ne!(
            key1.to_bytes(),
            key2.to_bytes(),
            "Keys should be random and unique"
        );
    }
}