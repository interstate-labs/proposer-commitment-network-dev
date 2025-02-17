use alloy::{
    primitives::{Address, Bytes, U256},
    providers::{ProviderBuilder, RootProvider},
    sol,
    transports::http::Http,
};
use eyre::bail;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use reth_primitives::revm_primitives::bitvec::view::BitViewSized;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureWithSaltAndExpiry {
    pub signature: Bytes,
    pub salt: [u8; 32],
    pub expiry: U256,
}

sol! {
    #[allow(missing_docs)]
    #[sol(rpc)]
    interface EigenLayerContract {
        #[derive(Debug, Default, Serialize)]
        struct OperatorSignature {
            bytes signature;
            bytes32 salt;
            uint256 expiry;
        }

        // Core functions
        function registerOperator(
            string calldata rpc,
            string calldata rpc1,
            string calldata rpc2,
            OperatorSignature calldata operatorSignature
        ) external;

        function registerOperatorToAVS(
            address operator,
            OperatorSignature calldata operatorSignature
        ) external;

        // Strategy management
        function deregisterStrategy(address strategy) external;
        function registerStrategy(address strategy) external;
        function pauseStrategy() external;
        function unpauseStrategy() external;
        
        // View functions
        function getOperatorRestakedStrategies(address operator) 
            external view returns (address[] memory strategies);
        
        function getOperatorStakeAt(
            address operator,
            address collateral,
            uint48 timestamp
        ) external view returns (uint256 amount);

        function getCurrentPeriod() external view returns (uint48 periodIndex);
        function getProviderCollateral(address operator, address collateral) 
            external view returns (uint256 amount);
        function getProviderCollateralTokens(address operator) 
            external view returns (address[] memory tokens, uint256[] memory amounts);
        function getRestakeableStrategies() external view returns (address[] memory strategies);
        function getWhitelistedStrategies() external view returns (address[] memory strategies);
        function isStrategyEnabled(address strategy) external view returns (bool);
        
        // Ownership and administration
        function owner() external view returns (address);
        function renounceOwnership() external;
        function transferOwnership(address newOwner) external;
        
        // Metadata management
        function updateAVSMetadataURI(string calldata metadataURI) external;
        
        // Upgrade functionality
        function UPGRADE_INTERFACE_VERSION() external view returns (string);
        function proxiableUUID() external view returns (bytes32);
        function upgradeToAndCall(address newImplementation, bytes calldata data) external payable;
        
        // Helper views
        function avsDirectory() external view returns (address);
        function restakingHelper() external view returns (address);
    }
}

use EigenLayerContract::EigenLayerContractInstance;

#[derive(Debug, Clone)]
pub struct EigenLayerController(EigenLayerContractInstance<Http<Client>, RootProvider<Http<Client>>>);

impl EigenLayerController {
    pub fn from_address<U: Into<Url>>(execution_client_url: U, contract_address: Address) -> Self {
        let provider = ProviderBuilder::new().on_http(execution_client_url.into());
        let contract = EigenLayerContract::new(contract_address, provider);
        Self(contract)
    }

    // Operator Registration
    pub async fn register_operator(
        &self,
        rpc: String,
        rpc1: String,
        rpc2: String,
        signature: SignatureWithSaltAndExpiry,
    ) -> eyre::Result<()> {
        let sig = EigenLayerContract::OperatorSignature {
            signature: signature.signature,
            salt: signature.salt.into(),
            expiry: signature.expiry,
        };

        self.0.registerOperator(rpc, rpc1, rpc2, sig)
            .send()
            .await
            .map_err(|e| eyre::eyre!("Register operator failed: {:?}", e))?;
        Ok(())
    }

    pub async fn register_operator_to_avs(
        &self,
        operator: Address,
        signature: SignatureWithSaltAndExpiry,
    ) -> eyre::Result<()> {
        let sig = EigenLayerContract::OperatorSignature {
            signature: signature.signature,
            salt: signature.salt.into(),
            expiry: signature.expiry,
        };

        self.0.registerOperatorToAVS(operator, sig)
            .send()
            .await
            .map_err(|e| eyre::eyre!("Register operator to AVS failed: {:?}", e))?;
        Ok(())
    }

    // Strategy Management
    pub async fn deregister_strategy(&self, strategy: Address) -> eyre::Result<()> {
        self.0.deregisterStrategy(strategy)
            .send()
            .await
            .map_err(|e| eyre::eyre!("Deregister strategy failed: {:?}", e))?;
        Ok(())
    }

    pub async fn register_strategy(&self, strategy: Address) -> eyre::Result<()> {
        self.0.registerStrategy(strategy)
            .send()
            .await
            .map_err(|e| eyre::eyre!("Register strategy failed: {:?}", e))?;
        Ok(())
    }

    pub async fn pause_strategy(&self) -> eyre::Result<()> {
        self.0.pauseStrategy()
            .send()
            .await
            .map_err(|e| eyre::eyre!("Pause strategy failed: {:?}", e))?;
        Ok(())
    }

    pub async fn unpause_strategy(&self) -> eyre::Result<()> {
        self.0.unpauseStrategy()
            .send()
            .await
            .map_err(|e| eyre::eyre!("Unpause strategy failed: {:?}", e))?;
        Ok(())
    }

    // View Functions
    pub async fn get_operator_restaked_strategies(&self, operator: Address) -> eyre::Result<Vec<Address>> {
        self.0.getOperatorRestakedStrategies(operator)
            .call()
            .await
            .map(|result| result.strategies)
            .map_err(|e| eyre::eyre!("Get restaked strategies failed: {:?}", e))
    }

    pub async fn get_operator_stake_at(
        &self,
        operator: Address,
        collateral: Address,
        timestamp: u64,
    ) -> eyre::Result<U256> {
        use alloy::primitives::Uint;
        let timestamp_uint = Uint::<48, 1>::from(timestamp);
        self.0.getOperatorStakeAt(operator, collateral, timestamp_uint)
            .call()
            .await
            .map(|result| result.amount)
            .map_err(|e| eyre::eyre!("Get operator stake failed: {:?}", e))
    }

    pub async fn get_current_period(&self) -> eyre::Result<u64> {
        self.0.getCurrentPeriod()
            .call()
            .await
            .map(|result| result.periodIndex.to()) // Fixed conversion using to()
            .map_err(|e| eyre::eyre!("Get current period failed: {:?}", e))
    }
    pub async fn get_provider_collateral(
        &self,
        operator: Address,
        collateral: Address,
    ) -> eyre::Result<U256> {
        self.0.getProviderCollateral(operator, collateral)
            .call()
            .await
            .map(|result| result.amount)
            .map_err(|e| eyre::eyre!("Get provider collateral failed: {:?}", e))
    }

    pub async fn get_provider_collateral_tokens(
        &self,
        operator: Address,
    ) -> eyre::Result<(Vec<Address>, Vec<U256>)> {
        self.0.getProviderCollateralTokens(operator)
            .call()
            .await
            .map(|result| (result.tokens, result.amounts))
            .map_err(|e| eyre::eyre!("Get collateral tokens failed: {:?}", e))
    }

    pub async fn get_restakeable_strategies(&self) -> eyre::Result<Vec<Address>> {
        self.0.getRestakeableStrategies()
            .call()
            .await
            .map(|result| result.strategies)
            .map_err(|e| eyre::eyre!("Get restakeable strategies failed: {:?}", e))
    }

    pub async fn get_whitelisted_strategies(&self) -> eyre::Result<Vec<Address>> {
        self.0.getWhitelistedStrategies()
            .call()
            .await
            .map(|result| result.strategies)
            .map_err(|e| eyre::eyre!("Get whitelisted strategies failed: {:?}", e))
    }

    pub async fn is_strategy_enabled(&self, strategy: Address) -> eyre::Result<bool> {
        self.0.isStrategyEnabled(strategy)
            .call()
            .await
            .map(|result| result._0)
            .map_err(|e| eyre::eyre!("Check strategy enabled failed: {:?}", e))
    }

    // Ownership Management
    pub async fn owner(&self) -> eyre::Result<Address> {
        self.0.owner()
            .call()
            .await
            .map(|result| result._0)
            .map_err(|e| eyre::eyre!("Get owner failed: {:?}", e))
    }

    pub async fn renounce_ownership(&self) -> eyre::Result<()> {
        self.0.renounceOwnership()
            .send()
            .await
            .map_err(|e| eyre::eyre!("Renounce ownership failed: {:?}", e))?;
        Ok(())
    }

    pub async fn transfer_ownership(&self, new_owner: Address) -> eyre::Result<()> {
        self.0.transferOwnership(new_owner)
            .send()
            .await
            .map_err(|e| eyre::eyre!("Transfer ownership failed: {:?}", e))?;
        Ok(())
    }

    // Metadata Management
    pub async fn update_avs_metadata_uri(&self, metadata_uri: String) -> eyre::Result<()> {
        self.0.updateAVSMetadataURI(metadata_uri)
            .send()
            .await
            .map_err(|e| eyre::eyre!("Update metadata URI failed: {:?}", e))?;
        Ok(())
    }

    // Upgrade Functionality
    pub async fn upgrade_interface_version(&self) -> eyre::Result<String> {
        self.0.UPGRADE_INTERFACE_VERSION()
            .call()
            .await
            .map(|result| result._0)
            .map_err(|e| eyre::eyre!("Get upgrade interface version failed: {:?}", e))
    }

    pub async fn proxiable_uuid(&self) -> eyre::Result<[u8; 32]> {
        self.0.proxiableUUID()
            .call()
            .await
            .map(|result| result._0.to_vec().try_into().unwrap()) // Convert to Vec<u8> first, then to [u8; 32]
            .map_err(|e| eyre::eyre!("Get proxiable UUID failed: {:?}", e))
    }
    pub async fn upgrade_to_and_call(
        &self,
        new_implementation: Address,
        data: Bytes,
    ) -> eyre::Result<()> {
        self.0.upgradeToAndCall(new_implementation, data)
            .send()
            .await
            .map_err(|e| eyre::eyre!("Upgrade failed: {:?}", e))?;
        Ok(())
    }

    // Helper Views
    pub async fn avs_directory(&self) -> eyre::Result<Address> {
        self.0.avsDirectory()
            .call()
            .await
            .map(|result| result._0)
            .map_err(|e| eyre::eyre!("Get AVS directory failed: {:?}", e))
    }

    pub async fn restaking_helper(&self) -> eyre::Result<Address> {
        self.0.restakingHelper()
            .call()
            .await
            .map(|result| result._0)
            .map_err(|e| eyre::eyre!("Get restaking helper failed: {:?}", e))
    }
}
