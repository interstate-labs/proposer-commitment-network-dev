use clap::{Args, ValueEnum, ArgGroup};
use alloy::primitives::b256;
use std::{time::Duration, str::FromStr};

/// Holesky builder domain for signing messages.
const BUILDER_DOMAIN_HOLESKY: [u8; 32] =
    b256!("000000015b83a23759c560b2d0c64576e1dcfc34ea94c4988f3e0d9f77f05387").0;

/// Devnet builder domain for signing messages.
const BUILDER_DOMAIN_KURTOSIS: [u8; 32] =
    b256!("000000010b41be4cdb34d183dddca5398337626dcdcfaf1720c1202d3b95f84e").0;
   
/// Default slot time duration in seconds.
pub const DEFAULT_SLOT_TIME_SECONDS: u64 = 12;

/// Default commitment deadline duration.
pub const DEFAULT_COMMITMENT_DEADLINE_MILLIS: u64 = 8_000;

/// Chain configration
#[derive(Debug, Clone, Args)]
pub struct ChainConfig {
    /// chain name
    #[clap(short = 'C', long, default_value = "holesky")]
    chain: Chain,
    /// commitment deadline
    #[clap(short = 'd', long, default_value_t = DEFAULT_COMMITMENT_DEADLINE_MILLIS)]
    commitment_deadline: u64,
    /// customized slot time
    #[clap(short = 's', long, default_value_t = DEFAULT_SLOT_TIME_SECONDS)]
    pub slot_time: u64,
}

impl Default for ChainConfig {
    fn default() -> Self {
        Self {
            chain: Chain::Holesky,
            commitment_deadline: DEFAULT_COMMITMENT_DEADLINE_MILLIS,
            slot_time: DEFAULT_SLOT_TIME_SECONDS,
        }
    }
}

/// Available chains for the interstate sidecar
#[derive(Debug, Clone, ValueEnum)]
#[clap(rename_all = "kebab_case")]
#[allow(missing_docs)]
pub enum Chain {
    Holesky,
    Kurtosis,
}

impl ChainConfig {
    
    /// get chain name.
    pub fn get_name(&self) -> &'static str {
        match self.chain {
            Chain::Holesky => "holesky",
            Chain::Kurtosis => "kurtosis",
        }
    }

    /// get chain id.
    pub fn get_chain_id(&self) -> u64 {
        match self.chain {
            Chain::Holesky => 17000,
            Chain::Kurtosis => 3151908,
        }
    }
    /// get builder domain.
    pub fn builder_domain(&self) -> [u8; 32] {
        match self.chain {
            Chain::Holesky => BUILDER_DOMAIN_HOLESKY,
            Chain::Kurtosis => BUILDER_DOMAIN_KURTOSIS,
        }
    }

    /// get fork version.
    pub fn fork_version(&self) -> [u8; 4] {
        match self.chain {
            Chain::Holesky => [1, 1, 112, 0],
            Chain::Kurtosis => [16, 0, 0, 56],
        }
    }

    /// get duration of commitment deadline.
    pub fn get_commitment_deadline_duration(&self) -> Duration {
        Duration::from_millis(self.commitment_deadline)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ValidatorIndexes(Vec<u64>);

impl ValidatorIndexes {
    pub fn contains(&self, index: u64) -> bool {
        self.0.contains(&index)
    }
}

impl FromStr for ValidatorIndexes {
    type Err = eyre::Report;

    /// Parse an array of validator indexes. Accepted values:
    /// - a single index (e.g. "1")
    /// - a comma-separated list of indexes (e.g. "1,2,3,4")
    /// - a contiguous range of indexes (e.g. "1..4")
    /// - a mix of the above (e.g. "1,2..4,6..8")
    ///
    /// TODO: add parsing from a directory path, using the format of
    /// validator definitions
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let mut vec = Vec::new();

        for comma_separated_part in s.split(',') {
            if comma_separated_part.contains("..") {
                let mut parts = comma_separated_part.split("..");

                let start = parts.next().ok_or_else(|| eyre::eyre!("Invalid range"))?;
                let start = start.parse::<u64>()?;

                let end = parts.next().ok_or_else(|| eyre::eyre!("Invalid range"))?;
                let end = end.parse::<u64>()?;

                vec.extend(start..=end);
            } else {
                let index = comma_separated_part.parse::<u64>()?;
                vec.push(index);
            }
        }

        Ok(Self(vec))
    }
}

impl From<Vec<u64>> for ValidatorIndexes {
    fn from(vec: Vec<u64>) -> Self {
        Self(vec)
    }
}
