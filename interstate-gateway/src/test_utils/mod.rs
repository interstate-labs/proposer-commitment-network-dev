use std::collections::HashMap;

use alloy::{
    network::TransactionBuilder,
    primitives::{Address, U256},
    rpc::types::TransactionRequest,
};

use crate::config::Config;

/// Return a mock configuration for testing purposes.
pub(crate) fn get_test_config() -> Config {
    let mut envs = HashMap::new();

    envs.insert("COMMITMENT_PORT".to_string(), "9063".to_string());
    envs.insert("METRICS_PORT".to_string(), "8018".to_string());
    envs.insert("CHAIN".to_string(), "kurtosis".to_string());
    envs.insert("VALIDATOR_INDEXES".to_string(), "0..64".to_string());
    envs.insert(
        "BEACON_API_URL".to_string(),
        "http://127.0.0.1:36477".to_string(),
    );
    envs.insert(
        "EXECUTION_API_URL".to_string(),
        "http://127.0.0.1:36468".to_string(),
    );
    envs.insert(
        "ENGINE_API_URL".to_string(),
        "http://127.0.0.1:36469".to_string(),
    );
    envs.insert(
        "CB_URL".to_string(),
        "http://127.0.0.1:4000".to_string(),
    );
    envs.insert("BUILDER_PORT".to_string(), "9064".to_string());
    envs.insert(
        "JWT".to_string(),
        "dc49981516e8e72b401a63e6405495a32dafc3939b5d6d83cc319ac0388bca1b".to_string(),
    );
    envs.insert("SLOT_TIME".to_string(), "2".to_string());
    envs.insert("COMMITMENT_DEADLINE".to_string(), "100".to_string());
    envs.insert(
        "FEE_RECIPIENT".to_string(),
        "0x8aC112a5540f441cC9beBcC647041A6E0D595B94".to_string(),
    );
    envs.insert(
        "DELEGATIONS_PATH".to_string(),
        "/work/proposer-commitment-network/sidecar/delegations/delegations.json".to_string(),
    );
    envs.insert(
        "GATEWAY_CONTRACT".to_string(),
        "0x6db20C530b3F96CD5ef64Da2b1b931Cb8f264009".to_string(),
    );

    Config::new(envs)
}

/// Create a default transaction template to use for tests
pub(crate) fn default_test_transaction(sender: Address, nonce: Option<u64>) -> TransactionRequest {
    TransactionRequest::default()
        .with_from(sender)
        // Burn it
        .with_to(Address::ZERO)
        .with_chain_id(1337)
        .with_nonce(nonce.unwrap_or(0))
        .with_value(U256::from(100))
        .with_gas_limit(21_000)
        .with_max_priority_fee_per_gas(1_000_000_000) // 1 gwei
        .with_max_fee_per_gas(20_000_000_000)
}
