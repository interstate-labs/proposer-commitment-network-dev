use group_config::{HOLEKSY_CHAIN_ID, KURTOSIS_CHAIN_ID};
use reqwest::Url;

use std::{collections::HashMap, path::PathBuf, str::FromStr};
use rand::RngCore;

use blst::min_pk::SecretKey as BLSSecretKey;
use alloy::primitives::Address;

pub mod group_config;
pub use group_config::{ChainConfig, ValidatorIndexes, Chain};

/// Default port for the commitment server exposed by the sidecar.
pub const DEFAULT_COMMITMENT_PORT: u16 = 8000;

/// Default port for the MEV-Boost proxy server.
pub const DEFAULT_MEV_BOOST_PROXY_PORT: u16 = 18551;

/// Configuration of the sidecar.
#[derive(Debug, Clone)]
pub struct Config {
    /// Port to listen on for incoming commitment requests
    pub commitment_port: u16,
    /// The builder server port to listen on (handling constraints apis)
    pub builder_port: u16,
    /// The constraints collector url
    pub collector_url: Url,
    /// The constraints collector websocket url
    pub collector_ws: String,
    /// URL for the beacon client API URL
    pub beacon_api_url: Url,
    /// The execution API url
    pub execution_api_url: Url,
    /// The engine API url
    pub engine_api_url: Url,
    /// Validator indexes of connected validators that the sidecar should accept commitments on behalf of
    pub validator_indexes: ValidatorIndexes,
    /// The chain on which the sidecar is running
    pub chain: ChainConfig,
    /// The jwt.hex secret to authenticate calls to the engine API
    pub jwt_hex: String,
    /// The fee recipient address for fallback blocks
    pub fee_recipient: Address,
    /// Local bulider bls private key for signing fallback payloads.
    pub builder_bls_private_key: BLSSecretKey,
    /// The path to the ERC-2335 keystore secret passwords.
    pub keystore_secrets_path: PathBuf,
    /// Path to the keystores folder.
    pub keystore_pubkeys_path: PathBuf,
   /// Path to the delegations file.
   pub delegations_path: Option<PathBuf>,

}

impl Default for Config {
    fn default() -> Self {
        Self {
            commitment_port: DEFAULT_COMMITMENT_PORT,
            builder_port: DEFAULT_MEV_BOOST_PROXY_PORT,
            collector_url: "http://localhost:3030".parse().expect("Valid URL"),
            beacon_api_url: "http://localhost:5052".parse().expect("Valid URL"),
            execution_api_url: "http://localhost:8545".parse().expect("Valid URL"),
            engine_api_url: "http://localhost:8551".parse().expect("Valid URL"),
            validator_indexes: ValidatorIndexes::default(),
            chain: ChainConfig::default(),
            jwt_hex: String::new(),
            fee_recipient: Address::ZERO,
            builder_bls_private_key: random_bls_secret(),
            collector_ws: String::new(),
            keystore_secrets_path: PathBuf::from("/work/proposer-commitment-network/sidecar/keystores/secrets"),
            keystore_pubkeys_path: PathBuf::from("/work/proposer-commitment-network/sidecar/keystores/keys"),
            delegations_path: None
        }
    }
}

impl Config {
    pub fn new(envs: HashMap<String, String>) -> Self {
        // ,&envs["BUILDER_PORT"],&envs["collector_url"],&envs["BEACON_API_URL"], &envs["PRIVATE_KEY"], &envs["JWT_HEX"], &envs["VALIDATOR_INDEXES"], , &envs["COMMITMENT_DEADLINE"], &envs["SLOT_TIME"]
        let validators = ValidatorIndexes::from_str(&envs["VALIDATOR_INDEXES"].as_str()).unwrap();

        let chain = ChainConfig {
            chain: match envs["CHAIN"].clone().as_str() {
                "kurtosis" => Chain::Kurtosis,
                "holesky" => Chain::Holesky,
                _ => Chain::Holesky
            },
            commitment_deadline: envs["COMMITMENT_DEADLINE"].parse().unwrap(),
            slot_time: envs["SLOT_TIME"].parse().unwrap(),
            id: match envs["CHAIN"].clone().as_str() {
                "kurtosis" => KURTOSIS_CHAIN_ID,
                "holesky" => HOLEKSY_CHAIN_ID,
                _ => HOLEKSY_CHAIN_ID
            }
        };

        Self {
            commitment_port: envs["COMMITMENT_PORT"].parse().unwrap(),
            builder_port: envs["BUILDER_PORT"].parse().unwrap(),
            collector_url: envs["COLLECTOR_URL"].parse().expect("Valid URL"),
            collector_ws:envs["COLLECTOR_SOCKET"].parse().expect("Valid URL"),
            beacon_api_url: envs["BEACON_API_URL"].parse().expect("Valid URL"),
            execution_api_url: envs["EXECUTION_API_URL"].parse().expect("Valid URL"),
            engine_api_url: envs["ENGINE_API_URL"].parse().expect("Valid URL"),
            validator_indexes: validators,
            chain: chain,
            jwt_hex: envs["JWT"].clone(),
            fee_recipient: Address::parse_checksummed(&envs["FEE_RECIPIENT"], None).unwrap() ,
            builder_bls_private_key: random_bls_secret(),
            keystore_secrets_path: PathBuf::from(envs["KEYSTORE_SECRETS_PATH"].as_str()),
            keystore_pubkeys_path: PathBuf::from(envs["KEYSTORE_PUBKEYS_PATH"].as_str()),
            delegations_path: { if envs["DELEGATIONS_PATH"].len() > 0 {Some(PathBuf::from(envs["DELEGATIONS_PATH"].as_str()))} else {None} } 
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
            default_config.collector_url.as_str(),
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
        assert!(default_config.collector_ws.is_empty());
    }

    #[test]
    fn test_config_new() {
        let mut envs = HashMap::new();
        envs.insert("COMMITMENT_PORT".to_string(), "8001".to_string());
        envs.insert("BUILDER_PORT".to_string(), "18552".to_string());
        envs.insert("COLLECTOR_URL".to_string(), "http://localhost:4000".to_string());
        envs.insert("COLLECTOR_SOCKET".to_string(), "ws://localhost:4001".to_string());
        envs.insert("BEACON_API_URL".to_string(), "http://localhost:6000".to_string());
        envs.insert("EXECUTION_API_URL".to_string(), "http://localhost:7000".to_string());
        envs.insert("ENGINE_API_URL".to_string(), "http://localhost:8000".to_string());
        envs.insert("VALIDATOR_INDEXES".to_string(), "0,1,2".to_string());
        envs.insert("CHAIN".to_string(), "kurtosis".to_string());
        envs.insert("COMMITMENT_DEADLINE".to_string(), "12".to_string());
        envs.insert("SLOT_TIME".to_string(), "10".to_string());
        envs.insert("JWT".to_string(), "test-jwt".to_string());
        envs.insert("FEE_RECIPIENT".to_string(), "0x0000000000000000000000000000000000000001".to_string());
        envs.insert("KEYSTORE_SECRETS_PATH".to_string(), "/work/proposer-commitment-network/sidecar/keystores/secrets".to_string());
        envs.insert("KEYSTORE_PUBKEYS_PATH".to_string(), "/work/proposer-commitment-network/sidecar/keystores/keys".to_string());

        let config = Config::new(envs);

        assert_eq!(config.commitment_port, 8001);
        assert_eq!(config.builder_port, 18552);
        assert_eq!(config.collector_url.as_str(), "http://localhost:4000/");
        assert_eq!(config.collector_ws, "ws://localhost:4001");
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

        assert_ne!(key1.to_bytes(), key2.to_bytes(), "Keys should be random and unique");
    }
}