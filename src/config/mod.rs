use alloy::{hex, primitives::Address};
use blst::min_pk::SecretKey;
use clap::Parser;
use reqwest::Url;

use std::{fs::read_to_string, path::Path, str::FromStr, num::NonZero};


use crate::utils::create_bls_secret;

pub mod group_config;
pub use group_config::{ChainConfig, ValidatorIndexes};

/// Default port for the commitment server exposed by the sidecar.
pub const DEFAULT_COMMITMENT_PORT: u16 = 8000;

/// Default port for the MEV-Boost proxy server.
pub const DEFAULT_MEV_BOOST_PROXY_PORT: u16 = 18551;

/// Command-line options for the sidecar
#[derive(Parser, Debug)]
pub struct Opts {
    /// Port to listen on for incoming commitment requests
    #[clap(short = 'p', long)]
    pub(super) port: Option<u16>,
    /// URL for the beacon client
    #[clap(short = 'c', long)]
    pub(super) beacon_api_url: String,
    /// URL for the MEV-Boost sidecar client to use
    #[clap(short = 'b', long)]
    pub(super) commit_boost_url: String,
    /// Execution client API URL
    #[clap(short = 'x', long)]
    pub(super) execution_api_url: String,
    /// Execution client Engine API URL
    #[clap(short = 'e', long)]
    pub(super) engine_api_url: String,
    /// MEV-Boost proxy server port to use
    #[clap(short = 'y', long)]
    pub(super) builder_port: u16,
    /// Max number of commitments to accept per block
    #[clap(short = 'm', long)]
    pub(super) max_commitments: Option<NonZero<usize>>,
    /// Max committed gas per slot
    #[clap(short = 'g', long)]
    pub(super) max_committed_gas: Option<NonZero<u64>>,
    /// Validator indexes of connected validators that the sidecar
    /// should accept commitments on behalf of. Accepted values:
    /// - a comma-separated list of indexes (e.g. "1,2,3,4")
    /// - a contiguous range of indexes (e.g. "1..4")
    /// - a mix of the above (e.g. "1,2..4,6..8")
    #[clap(short = 'v', long, value_parser = ValidatorIndexes::from_str)]
    pub(super) validator_indexes: ValidatorIndexes,
    /// The JWT secret token to authenticate calls to the engine API.
    ///
    /// It can either be a hex-encoded string or a file path to a file
    /// containing the hex-encoded secret.
    #[clap(short = 'j', long)]
    pub(super) jwt_hex: String,
    /// The fee recipient address for fallback blocks
    #[clap(short = 'f', long)]
    pub(super) fee_recipient: Address,
    /// Secret BLS key to sign fallback payloads with
    /// (If not provided, a random key will be used)
    #[clap(short = 'K', long)]
    pub(super) builder_private_key: Option<String>,
    /// Chain config for the chain on which the sidecar is running
    #[clap(flatten)]
    pub(super) chain: ChainConfig,
    /// Private key to use for signing preconfirmation requests
    #[clap(short = 'k', long)]
    pub(super) private_key: Option<String>,
}

/// Configuration options for the sidecar. These are parsed from
/// command-line options in the form of [`Opts`].
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
    /// Private key to use for signing preconfirmation requests
    pub private_key: Option<SecretKey>,
    /// The jwt.hex secret to authenticate calls to the engine API
    pub jwt_hex: String,
    /// Validator indexes of connected validators that the
    /// sidecar should accept commitments on behalf of
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
            execution_api_url: "http://localhost:8545".parse().expect("Valid URL"),
            engine_api_url: "http://localhost:8551".parse().expect("Valid URL"),
            private_key: Some(create_bls_secret()),
            jwt_hex: String::new(),
            validator_indexes: ValidatorIndexes::default(),
            chain: ChainConfig::default(),
        }
    }
}

/// Limits for the sidecar.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Maximum number of commitments to accept per block
    pub max_commitments_per_slot: NonZero<usize>,
    pub max_committed_gas_per_slot: NonZero<u64>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_commitments_per_slot: NonZero::new(128).expect("Valid non-zero"),
            max_committed_gas_per_slot: NonZero::new(10_000_000).expect("Valid non-zero"),
        }
    }
}

impl Config {
    /// Parse the command-line options and return a new [`Config`] instance
    pub fn parse_from_cli() -> eyre::Result<Self> {
        let opts = Opts::parse();
        Self::try_from(opts)
    }
}

impl TryFrom<Opts> for Config {
    type Error = eyre::Report;

    fn try_from(opts: Opts) -> Result<Self, Self::Error> {
        let mut config = Config::default();

        if let Some(port) = opts.port {
            config.commitment_port = port;
        }

        config.private_key = if let Some(sk) = opts.private_key {
            // Check if the string starts with "0x" and remove it
            let hex_sk = sk.strip_prefix("0x").unwrap_or(&sk);

            let sk = SecretKey::from_bytes(&hex::decode(hex_sk)?)
                .map_err(|e| eyre::eyre!("Failed decoding BLS secret key: {:?}", e))?;
            Some(sk)
        } else {
            None
        };

        config.jwt_hex = if opts.jwt_hex.starts_with("0x") {
            opts.jwt_hex.trim_start_matches("0x").to_string()
        } else if Path::new(&opts.jwt_hex).exists() {
            read_to_string(opts.jwt_hex)
                .map_err(|e| eyre::eyre!("Failed reading JWT secret file: {:?}", e))?
                .trim_start_matches("0x")
                .to_string()
        } else {
            opts.jwt_hex
        };

        // Validate the JWT secret
        if config.jwt_hex.len() != 64 {
            eyre::bail!("Engine JWT secret must be a 32 byte hex string");
        } else {
            tracing::info!("Engine JWT secret loaded successfully");
        }

        config.builder_port = opts.builder_port;
        config.engine_api_url = opts.engine_api_url.parse()?;
        config.execution_api_url = opts.execution_api_url.parse()?;
        config.beacon_api_url = opts.beacon_api_url.parse()?;
        config.commit_boost_url = opts.commit_boost_url.parse()?;

        config.validator_indexes = opts.validator_indexes;

        config.chain = opts.chain;

        Ok(config)
    }
}
