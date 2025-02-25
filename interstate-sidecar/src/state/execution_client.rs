use std::ops::{Deref, DerefMut};

use alloy_v092::{
    eips::BlockNumberOrTag,
    primitives::{Address, Bytes, TxHash, B256, U256, U64},
    providers::{ProviderBuilder, RootProvider},
    rpc::{
        client::{BatchRequest, ClientBuilder, RpcClient},
        types::{FeeHistory, TransactionReceipt},
    },
    transports::{http::Http, TransportErrorKind, TransportResult},
};
use futures::{stream::FuturesUnordered, StreamExt};
use reqwest::{Client, Url};

use super::account_state::AccountState;

#[derive(Clone, Debug)]
pub struct ExecutionClient {
    rpc: RpcClient<Http<Client>>,
    inner: RootProvider<Http<Client>>,
}

impl Deref for ExecutionClient {
    type Target = RootProvider<Http<Client>>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for ExecutionClient {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl ExecutionClient {
    pub fn new<U: Into<Url>>(url: U) -> Self {
        let url = url.into();
        let rpc = ClientBuilder::default().http(url.clone());
        let inner = ProviderBuilder::new().on_http(url);

        Self { rpc, inner }
    }

    pub fn new_batch(&self) -> BatchRequest<'_, Http<Client>> {
        self.rpc.new_batch()
    }

    pub async fn get_chain_id(&self) -> TransportResult<u64> {
        let chain_id: String = self.rpc.request("eth_chainId", ()).await?;
        let chain_id = chain_id
            .get(2..)
            .ok_or(TransportErrorKind::Custom("not hex prefixed result".into()))?;

        let decoded = u64::from_str_radix(chain_id, 16).map_err(|e| {
            TransportErrorKind::Custom(
                format!("could not decode {} into u64: {}", chain_id, e).into(),
            )
        })?;
        Ok(decoded)
    }

    pub async fn get_basefee(&self, block_number: Option<u64>) -> TransportResult<u128> {
        let tag = block_number.map_or(BlockNumberOrTag::Latest, BlockNumberOrTag::Number);

        let fee_history: FeeHistory = self
            .rpc
            .request("eth_feeHistory", (U64::from(1), tag, &[] as &[f64]))
            .await?;

        let Some(base_fee) = fee_history.latest_block_base_fee() else {
            return Err(TransportErrorKind::Custom("Base fee not found".into()).into());
        };

        Ok(base_fee)
    }

    pub async fn get_blob_basefee(&self, block_number: Option<u64>) -> TransportResult<u128> {
        let block_count = U64::from(1);
        let tag = block_number.map_or(BlockNumberOrTag::Latest, BlockNumberOrTag::Number);
        let reward_percentiles: Vec<f64> = vec![];
        let fee_history: FeeHistory = self
            .rpc
            .request("eth_feeHistory", (block_count, tag, &reward_percentiles))
            .await?;

        Ok(fee_history.latest_block_blob_base_fee().unwrap_or(0))
    }

    pub async fn get_head(&self) -> TransportResult<u64> {
        let result: U64 = self.rpc.request("eth_blockNumber", ()).await?;

        Ok(result.to())
    }

    pub async fn get_account_state(
        &self,
        address: &Address,
        block_number: Option<u64>,
    ) -> TransportResult<AccountState> {
        let mut batch = self.rpc.new_batch();

        let tag = block_number.map_or(BlockNumberOrTag::Latest, BlockNumberOrTag::Number);

        let balance = batch
            .add_call("eth_getBalance", &(address, tag))
            .expect("Correct parameters");

        let tx_count = batch
            .add_call("eth_getTransactionCount", &(address, tag))
            .expect("Correct parameters");

        let code = batch
            .add_call("eth_getCode", &(address, tag))
            .expect("Correct parameters");

        batch.send().await?;

        let tx_count: U64 = tx_count.await?;
        let balance: U256 = balance.await?;
        let code: Bytes = code.await?;

        Ok(AccountState {
            balance,
            transaction_count: tx_count.to(),
            has_code: !code.is_empty(),
        })
    }

    #[allow(unused)]
    pub async fn send_raw_transaction(&self, raw: Bytes) -> TransportResult<B256> {
        self.rpc.request("eth_sendRawTransaction", [raw]).await
    }

    pub async fn get_receipts(
        &self,
        hashes: &[TxHash],
    ) -> TransportResult<Vec<Option<TransactionReceipt>>> {
        let mut batch = self.rpc.new_batch();

        let futs = FuturesUnordered::new();

        for hash in hashes {
            futs.push(
                batch
                    .add_call("eth_getTransactionReceipt", &(&[hash]))
                    .expect("Correct parameters"),
            );
        }

        batch.send().await?;

        Ok(futs
            .collect::<Vec<TransportResult<TransactionReceipt>>>()
            .await
            .into_iter()
            .map(|r| r.ok())
            .collect())
    }
}
