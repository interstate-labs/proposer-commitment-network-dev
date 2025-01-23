use cb_common::config::load_pbs_custom_config;
use cb_pbs::{ PbsState, PbsService };
use eyre::Result;

mod constraints;
mod error;
mod metrics;
mod proofs;
mod server;
mod types;

#[cfg(test)]
mod testutil;

use server::{BuilderRuntimeState, ConstraintsApi};
use tracing_subscriber::EnvFilter;

use types::Config;

#[tokio::main]
async fn main() -> Result<()> {
    let (pbs_config, extra) = load_pbs_custom_config::<Config>().await?;
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();

    let chain = pbs_config.chain;
    tracing::info!(?chain, "Starting interstate-boost with relays below:");

    for relay in &pbs_config.relays {
        tracing::info!("ID: {} - URI: {}", relay.id, relay.config.entry.url);
    }
    let genesis = extra.genesis_time_sec;
    tracing::info!(?genesis, "genesis time is below:");
    let custom_state = BuilderRuntimeState::new(extra);
    let state = PbsState::new(pbs_config).with_data(custom_state);

    // let (current_slot, _) = state.get_slot_and_uuid();

    // tracing::info!("current_slot {}", current_slot);

    metrics::initialize_metrics()?;

    PbsService::run::<BuilderRuntimeState, ConstraintsApi>(state).await
}
