use clap::Parser;
use tracing::error;

/// CLI command definitions and options.
mod cli;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let _ = dotenvy::dotenv();
    let _ = tracing_subscriber::fmt().with_target(false).try_init();

    cli::Opts::parse().command.run().await
}
