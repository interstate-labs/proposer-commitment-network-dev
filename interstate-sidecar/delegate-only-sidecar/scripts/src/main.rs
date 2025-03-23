use clap::Parser;
use tracing::error;

/// CLI command definitions and options.
mod cli;
mod delegate;
mod revoke;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let _ = dotenvy::dotenv();
    let _ = tracing_subscriber::fmt().with_target(false).try_init();

    if let Err(err) = rustls::crypto::ring::default_provider().install_default() {
        error!("Failed to install default TLS provider: {:?}", err);
    }

    cli::Opts::parse().command.run().await
}
