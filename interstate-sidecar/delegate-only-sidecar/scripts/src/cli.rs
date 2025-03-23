use clap::{Parser, Subcommand, Args};
use eyre::Result;
use delegate::delegate;
use revoke::revoke;

use crate::delegate::delegate;
use crate::revoke::revoke;

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
    pub delegatee_pubkey: String,

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
    pub delegatee_pubkey: String,

    /// Output file path for the signed messages
    #[arg(long, env = "OUTPUT_FILE_PATH", default_value = "revocations.json")]
    pub output_path: String,
}

impl Cli {
    pub async fn run(self) -> Result<()> {
        match self.command {
            Commands::Delegate(cmd) => {
                // Implement delegate command
                println!("Delegating to {}", cmd.delegatee_pubkey);
                delegate(&cmd.signer_type, &cmd.delegatee_pubkey, &cmd.relay_url).await?;
                Ok(())
            }

            Commands::Revoke(cmd) => {
                // Implement revoke command
                println!("Revoking delegation for {}", cmd.delegatee_pubkey);
                revoke(&cmd.delegatee_pubkey, cmd.output_path).await?;
                Ok(())
            }
        }
    }
}
