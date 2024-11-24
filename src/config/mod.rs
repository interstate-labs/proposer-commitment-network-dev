use reqwest::Url;

use std::str::FromStr;

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
    /// Validator indexes of connected validators that the sidecar should accept commitments on behalf of
    pub validator_indexes: ValidatorIndexes,
    /// The chain on which the sidecar is running
    pub chain: ChainConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            commitment_port: DEFAULT_COMMITMENT_PORT,
            builder_port: DEFAULT_MEV_BOOST_PROXY_PORT,
            commit_boost_url: "http://localhost:3030".parse().expect("Valid URL"),
            beacon_api_url: "http://localhost:5052".parse().expect("Valid URL"),
            validator_indexes: ValidatorIndexes::default(),
            chain: ChainConfig::default(),
        }
    }
}

impl Config {
    pub fn new(commitment_port:&String, builder_port:&String, commit_boost_url:&String, beacon_api_url:&String, private_key_str:&String, jwt_hex:&String, validator_indexes:&String, chain:&String, commitment_deadline: &String, slot_time: &String) -> Self {

        let validators = ValidatorIndexes::from_str(&validator_indexes.as_str()).unwrap();

        let chain = ChainConfig {
            chain: match chain.clone().as_str() {
                "kurtosis" => Chain::Kurtosis,
                "holesky" => Chain::Holesky,
                _ => Chain::Holesky
            },
            commitment_deadline: commitment_deadline.parse().unwrap(),
            slot_time: slot_time.parse().unwrap()
        };

        Self {
            commitment_port: commitment_port.parse().unwrap(),
            builder_port: builder_port.parse().unwrap(),
            commit_boost_url: commit_boost_url.parse().expect("Valid URL"),
            beacon_api_url: beacon_api_url.parse().expect("Valid URL"),
            validator_indexes: validators,
            chain: chain,
        }
    }
}
