use std::{collections::HashMap, time::Duration};

use alloy_v092::{
    eips::BlockNumberOrTag,
    primitives::{Address, Bytes, TxHash, U256, U64},
    rpc::types::TransactionReceipt,
    transports::TransportError,
};
use futures::{stream::FuturesOrdered, StreamExt};
use reqwest::Url;
use tracing::error;

use super::{account_state::AccountState, execution::StateUpdate, execution_client::ExecutionClient};

const MAX_RETRIES: u32 = 8;

const RETRY_BACKOFF_MS: u64 = 200;

#[async_trait::async_trait]
pub trait StateFetcher {
    async fn get_state_update(
        &self,
        addresses: Vec<&Address>,
        head: Option<u64>,
    ) -> Result<StateUpdate, TransportError>;

    async fn get_head(&self) -> Result<u64, TransportError>;

    async fn get_basefee(&self, block_number: Option<u64>) -> Result<u128, TransportError>;

    async fn get_blob_basefee(&self, block_number: Option<u64>) -> Result<u128, TransportError>;

    async fn get_account_state(
        &self,
        address: &Address,
        block_number: Option<u64>,
    ) -> Result<AccountState, TransportError>;

    async fn get_chain_id(&self) -> Result<u64, TransportError>;

    async fn get_receipts_unordered(
        &self,
        hashes: &[TxHash],
    ) -> Result<Vec<Option<TransactionReceipt>>, TransportError>;
}

#[derive(Clone, Debug)]
pub struct ClientState {
    client: ExecutionClient,
    retry_backoff: Duration,
}

impl ClientState {
    pub fn new<U: Into<Url>>(url: U) -> Self {
        Self {
            client: ExecutionClient::new(url),
            retry_backoff: Duration::from_millis(RETRY_BACKOFF_MS),
        }
    }
}

#[async_trait::async_trait]
impl StateFetcher for ClientState {
    async fn get_state_update(
        &self,
        addresses: Vec<&Address>,
        block_number: Option<u64>,
    ) -> Result<StateUpdate, TransportError> {
        let mut batch = self.client.new_batch();

        let tag = block_number.map_or(BlockNumberOrTag::Latest, BlockNumberOrTag::Number);
        let mut account_states = HashMap::with_capacity(addresses.len());

        let mut nonce_futs = FuturesOrdered::new();
        let mut balance_futs = FuturesOrdered::new();
        let mut code_futs = FuturesOrdered::new();

        let block_number = if let Some(block_number) = block_number {
            block_number
        } else {
            self.client.get_head().await?
        };

        for addr in &addresses {
            let nonce = batch
                .add_call("eth_getTransactionCount", &(addr, tag))
                .expect("Invalid parameters");
            let balance =
                batch.add_call("eth_getBalance", &(addr, tag)).expect("Invalid parameters");
            let code = batch.add_call("eth_getCode", &(addr, tag)).expect("Invalid parameters");

            nonce_futs.push_back(nonce);
            balance_futs.push_back(balance);
            code_futs.push_back(code);
        }

        batch.send().await?;

        let basefee = self.client.get_basefee(None);
        let blob_basefee = self.client.get_blob_basefee(None);

        let (nonce_vec, balance_vec, code_vec, basefee, blob_basefee) = tokio::join!(
            nonce_futs.collect::<Vec<_>>(),
            balance_futs.collect::<Vec<_>>(),
            code_futs.collect::<Vec<_>>(),
            basefee,
            blob_basefee,
        );

        for (addr, nonce) in addresses.iter().zip(nonce_vec) {
            let nonce: U64 = nonce?;

            account_states
                .entry(**addr)
                .and_modify(|s: &mut AccountState| {
                    s.transaction_count = nonce.to();
                })
                .or_insert(AccountState {
                    transaction_count: nonce.to(),
                    balance: U256::ZERO,
                    has_code: false,
                });
        }

        for (addr, balance) in addresses.iter().zip(balance_vec) {
            let balance = balance?;

            account_states
                .entry(**addr)
                .and_modify(|s: &mut AccountState| {
                    s.balance = balance;
                })
                .or_insert(AccountState { transaction_count: 0, balance, has_code: false });
        }

        for (addr, code) in addresses.iter().zip(code_vec) {
            let code: Bytes = code?;

            account_states
                .entry(**addr)
                .and_modify(|s: &mut AccountState| {
                    s.has_code = !code.is_empty();
                })
                .or_insert(AccountState {
                    transaction_count: 0,
                    balance: U256::ZERO,
                    has_code: !code.is_empty(),
                });
        }

        Ok(StateUpdate {
            account_states,
            min_basefee: basefee?,
            min_blob_basefee: blob_basefee?,
            block_number,
        })
    }

    async fn get_head(&self) -> Result<u64, TransportError> {
        self.client.get_head().await
    }

    async fn get_basefee(&self, block_number: Option<u64>) -> Result<u128, TransportError> {
        self.client.get_basefee(block_number).await
    }

    async fn get_blob_basefee(&self, block_number: Option<u64>) -> Result<u128, TransportError> {
        self.client.get_blob_basefee(block_number).await
    }

    async fn get_account_state(
        &self,
        address: &Address,
        block_number: Option<u64>,
    ) -> Result<AccountState, TransportError> {
        let mut retries = 0;

        loop {
            match self.client.get_account_state(address, block_number).await {
                Ok(state) => return Ok(state),
                Err(e) => {
                    retries += 1;
                    if retries >= MAX_RETRIES {
                        return Err(e);
                    }

                    error!(error = ?e, "Error getting account state, retrying...");
                    tokio::time::sleep(self.retry_backoff).await;
                }
            }
        }
    }

    async fn get_chain_id(&self) -> Result<u64, TransportError> {
        self.client.get_chain_id().await
    }

    async fn get_receipts_unordered(
        &self,
        hashes: &[TxHash],
    ) -> Result<Vec<Option<TransactionReceipt>>, TransportError> {
        self.client.get_receipts(hashes).await
    }
}