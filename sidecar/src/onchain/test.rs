// src/eigenlayer/test.rs
#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::config::eigenlayer::EigenLayerConfig;
    use ethers::prelude::*;
    use std::sync::Arc;

    const TEST_RPC: &str = "https://ethereum-holesky.publicnode.com";
    const CONTRACT_ADDRESS: &str = "0xE24a2A782766c3160361C59Ad59b3Adc7DfB3630";
    const TEST_OPERATOR: &str = "0x1234567890123456789012345678901234567890";
    const TEST_STRATEGY: &str = "0x2234567890123456789012345678901234567890";

    async fn setup_client() -> Arc<EigenLayerClient> {
        let config = EigenLayerConfig {
            contract_address: CONTRACT_ADDRESS.to_string(),
            rpc_url: TEST_RPC.to_string(),
        };
        config.create_client().await.unwrap()
    }

    #[tokio::test]
    async fn test_contract_connection() {
        let client = setup_client().await;
        assert_eq!(client.address, CONTRACT_ADDRESS.parse::<Address>().unwrap());
    }

    #[tokio::test]
    async fn test_strategy_enabled() {
        let client = setup_client().await;
        let strategy_address = TEST_STRATEGY.parse::<Address>().unwrap();
        
        let result = client.is_strategy_enabled(strategy_address).await;
        assert!(result.is_ok(), "Strategy enabled check failed: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_get_operator_stake() {
        let client = setup_client().await;
        let operator = TEST_OPERATOR.parse::<Address>().unwrap();
        let collateral = TEST_STRATEGY.parse::<Address>().unwrap();
        let timestamp = 0u64;

        let result = client.get_operator_stake_at(operator, collateral, timestamp).await;
        assert!(result.is_ok(), "Get operator stake failed: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_get_restaked_strategies() {
        let client = setup_client().await;
        let operator = TEST_OPERATOR.parse::<Address>().unwrap();

        let result = client.get_operator_restaked_strategies(operator).await;
        assert!(result.is_ok(), "Get restaked strategies failed: {:?}", result.err());
    }
}