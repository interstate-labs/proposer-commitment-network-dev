use clap::{Parser, Subcommand, Args};
use eyre::Result;

use crate::delegate;  // Import the module itself
use crate::revoke;
use ethereum_consensus::crypto::PublicKey as BlsPublicKey;


#[derive(Debug, Parser)]
#[command(author, version, about, arg_required_else_help(true))]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Parser)]
pub struct Opts {
    #[command(subcommand)]
    pub command: Commands
}

#[async_trait::async_trait]
pub trait Command {
    async fn run(&self) -> eyre::Result<()>;
}


#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Delegate validator keys
    Delegate(DelegateCommand),
    /// Revoke delegations
    Revoke(RevokeCommand),
}

#[derive(Debug, Args)]
pub struct DelegateCommand {
    /// The delegatee public key
    #[arg(long, env = "DELEGATEE_PUBKEY")]
    pub delegatee_pubkey: BlsPublicKey,

    #[arg(long, env = "RELAY_URL")]
    pub relay_url: String,

    #[arg(long, env = "SIGNER_TYPE")]
    pub signer_type: String,
    /// Output file path for the signed messages
    #[arg(long, env = "OUTPUT_FILE_PATH", default_value = "delegations.json")]
    pub output_path: String,

    /// Chain to use (mainnet, goerli, etc)
    #[arg(long, env = "CHAIN")]
    pub chain: String,
}

#[derive(Debug, Args)]
pub struct RevokeCommand {
    /// The delegatee public key to revoke
    #[arg(long, env = "DELEGATEE_PUBKEY")]
    pub delegatee_pubkey: BlsPublicKey,

    /// Output file path for the signed messages
    #[arg(long, env = "OUTPUT_FILE_PATH", default_value = "revocations.json")]
    pub output_path: String,
}

// Implement the trait for your Commands enum
#[async_trait::async_trait]
impl Command for Commands {
    async fn run(&self) -> eyre::Result<()> {
        match self {
            Commands::Delegate(cmd) => cmd.run().await,
            Commands::Revoke(cmd) => cmd.run().await,
            // Add other command variants as needed
        }
    }
}
