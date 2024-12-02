use group_config::{HOLEKSY_CHAIN_ID, KURTOSIS_CHAIN_ID};
use reqwest::Url;
use reth_primitives::Address;

use std::{collections::HashMap, str::FromStr};
use rand::RngCore;

use blst::min_pk::SecretKey as BLSSecretKey;

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
    /// URL for the MEV-Boost sidecar client to use
    pub commit_boost_url: Url,
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            commitment_port: DEFAULT_COMMITMENT_PORT,
            builder_port: DEFAULT_MEV_BOOST_PROXY_PORT,
            commit_boost_url: "http://localhost:3030".parse().expect("Valid URL"),
            beacon_api_url: "http://localhost:5052".parse().expect("Valid URL"),
            execution_api_url: "http://localhost:8545".parse().expect("Valid URL"),
            engine_api_url: "http://localhost:8551".parse().expect("Valid URL"),
            validator_indexes: ValidatorIndexes::default(),
            chain: ChainConfig::default(),
            jwt_hex: String::new(),
            fee_recipient: Address::ZERO,
            builder_bls_private_key: random_bls_secret(),
        }
    }
}

impl Config {
    pub fn new(envs: HashMap<String, String>) -> Self {
        // ,&envs["BUILDER_PORT"],&envs["COMMIT_BOOST_URL"],&envs["BEACON_API_URL"], &envs["PRIVATE_KEY"], &envs["JWT_HEX"], &envs["VALIDATOR_INDEXES"], , &envs["COMMITMENT_DEADLINE"], &envs["SLOT_TIME"]
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
            commit_boost_url: envs["COMMIT_BOOST_URL"].parse().expect("Valid URL"),
            beacon_api_url: envs["BEACON_API_URL"].parse().expect("Valid URL"),
            execution_api_url: envs["EXECUTION_API_URL"].parse().expect("Valid URL"),
            engine_api_url: envs["ENGINE_API_URL"].parse().expect("Valid URL"),
            validator_indexes: validators,
            chain: chain,
            jwt_hex: envs["JWT"].clone(),
            fee_recipient: Address::parse_checksummed(&envs["FEE_RECIPIENT"], None).unwrap() ,
            builder_bls_private_key: random_bls_secret(),
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