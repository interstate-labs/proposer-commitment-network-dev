use alloy::{
    consensus::{Header, EMPTY_OMMER_ROOT_HASH},
    eips::{
        calc_excess_blob_gas, calc_next_block_base_fee, eip1559::BaseFeeParams,
        eip2718::Encodable2718, eip4895::Withdrawal, BlockNumberOrTag,
    },
    hex::FromHex,
    primitives::{Address, Bloom, Bytes, B256, B64, U256},
    rpc::{
        client::{ClientBuilder, RpcClient},
        types::{
            engine::{
                ExecutionPayload as AlloyExecutionPayload, ExecutionPayloadV1, ExecutionPayloadV2,
                ExecutionPayloadV3,
            },
            Block, Withdrawals,
        },
    },
    transports::{http::Http, TransportResult},
};

use reth_primitives::{proofs, BlockBody, SealedBlock, SealedHeader, TransactionSigned};

use ethereum_consensus::{
    bellatrix::mainnet::Transaction,
    capella::spec,
    deneb::{
        mainnet::{
            ExecutionPayloadHeader as ConsensusExecutionPayloadHeader,
            Withdrawal as ConsensusWithdrawal, MAX_TRANSACTIONS_PER_PAYLOAD,
            MAX_WITHDRAWALS_PER_PAYLOAD,
        },
        ExecutionAddress, ExecutionPayload as DenebExecutionPayload,
    },
    ssz::prelude::{ssz_rs, ByteList, ByteVector, HashTreeRoot, List},
    types::mainnet::ExecutionPayload as ConsensusExecutionPayload,
};

use regex::Regex;
use reth_rpc_layer::{secret_to_bearer_header, JwtSecret};

use beacon_api_client::{presets::mainnet::Client as BeaconRPCClient, BlockId, StateId};
use reqwest::{Client, Url};
use serde_json::Value;

use crate::config::Config;

use super::builder::BuilderError;
use std::time::Duration;
use tokio::time::timeout;

const GET_BLOCK_TIMEOUT: Duration = Duration::from_secs(10);

/// Extra-data payload field used for locally built blocks, decoded in UTF-8.
///
const DEFAULT_EXTRA_DATA: [u8; 20] = [
    0x53, 0x65, 0x6c, 0x66, 0x2d, 0x62, 0x75, 0x69, 0x6c, 0x74, 0x20, 0x77, 0x69, 0x74, 0x68, 0x20,
    0x42, 0x6f, 0x6c, 0x74,
];

pub struct BlockBuilder {
    el_rpc_client: ExecutionRpcClient,
    beacon_rpc_client: BeaconRPCClient,
    extra_data: Bytes,
    fee_recipient: Address,
    engine_hinter: EngineHinter,
    slot_time_in_seconds: u64,
}

impl BlockBuilder {
    pub fn new(config: &Config) -> Self {
        let engine_hinter = EngineHinter {
            client: reqwest::Client::new(),
            jwt_hex: config.jwt_hex.to_string(),
            engine_rpc_url: config.engine_api_url.clone(),
            rpc_url: config.execution_api_url.clone()
        };

        Self {
            engine_hinter,
            extra_data: DEFAULT_EXTRA_DATA.into(),
            fee_recipient: config.fee_recipient,
            beacon_rpc_client: BeaconRPCClient::new(config.beacon_api_url.clone()),
            el_rpc_client: ExecutionRpcClient::new(config.execution_api_url.clone()),
            slot_time_in_seconds: config.chain.get_slot_time_in_seconds(),
        }
    }

    const MAX_RETRIES: u32 = 5;
    const RETRY_DELAY: Duration = Duration::from_secs(2);

    async fn get_latest_block(&self) -> Result<Block, BuilderError> {
        let mut retries = 0;
        loop {
            let res = timeout(GET_BLOCK_TIMEOUT, self.el_rpc_client.get_block(None, true)).await;

            match res {
                Ok(block) => {
                    tracing::debug!("got latest block");
                    return block.map_err(BuilderError::RpcError);
                }
                Err(_) if retries < Self::MAX_RETRIES => {
                    retries += 1;
                    tokio::time::sleep(Self::RETRY_DELAY).await;
                }
                Err(err) => {
                    return Err(BuilderError::Timeout(format!(
                        "Getting latest block timed out after {} retries: {}",
                        Self::MAX_RETRIES,
                        err
                    )))
                }
            }
        }
    }

    pub async fn build_sealed_block(
        &self,
        txs: &[TransactionSigned],
        slot: u64,
    ) -> Result<SealedBlock, BuilderError> {
        let latest_block = self.get_latest_block().await?;

        let genesis_time = latest_block.header.timestamp;

        let withdrawals = self
            .beacon_rpc_client
            .get_expected_withdrawals(StateId::Head, None)
            .await?
            .into_iter()
            .map(convert_withdrawal_from_consensus_to_alloy)
            .collect::<Vec<_>>();
        tracing::debug!("got withdrawals");

        let prev_randao = reqwest::Client::new()
            .get(
                self.beacon_rpc_client
                    .endpoint
                    .join("/eth/v1/beacon/states/head/randao")
                    .unwrap(),
            )
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap();
        let prev_randao = prev_randao
            .pointer("/data/randao")
            .unwrap()
            .as_str()
            .unwrap();
        let prev_randao = B256::from_hex(prev_randao).unwrap();
        tracing::debug!("got prev_randao");

        let parent_beacon_block_root: B256 = B256::from_slice(
            &self
                .beacon_rpc_client
                .get_beacon_block_root(BlockId::Head)
                .await
                .unwrap()
                .to_vec(),
        );
        tracing::debug!(parent = ?parent_beacon_block_root, "got parent_beacon_block_root");

        let versioned_hashes = txs
            .iter()
            .flat_map(|tx| tx.blob_versioned_hashes())
            .flatten()
            .collect::<Vec<_>>();
        tracing::info!(amount = ?versioned_hashes.len(), "got versioned_hashes");

        let base_fee = calc_next_block_base_fee(
            latest_block.header.gas_used,
            latest_block.header.gas_limit,
            latest_block.header.base_fee_per_gas.unwrap_or_default(),
            BaseFeeParams::ethereum(),
        ) as u64;

        let excess_blob_gas = calc_excess_blob_gas(
            latest_block.header.excess_blob_gas.unwrap_or_default(),
            latest_block.header.blob_gas_used.unwrap_or_default(),
        ) as u64;

        let blob_gas_used = txs
            .iter()
            .fold(0, |acc, tx| acc + tx.blob_gas_used().unwrap_or_default());

        let ctx = Context {
            base_fee,
            blob_gas_used,
            excess_blob_gas,
            parent_beacon_block_root,
            prev_randao,
            extra_data: self.extra_data.clone(),
            fee_recipient: self.fee_recipient,
            transactions_root: proofs::calculate_transaction_root(txs),
            withdrawals_root: proofs::calculate_withdrawals_root(&withdrawals),
            slot_time_in_seconds: self.slot_time_in_seconds,
        };

        let body = BlockBody {
            ommers: Vec::new(),
            transactions: txs.to_vec(),
            withdrawals: Some(Withdrawals::new(withdrawals)),
        };

        let mut hints = Hints::default();
        let max_iterations = 20;
        let mut i = 0;

        loop {
            let header = build_header_with_hints_and_context(
                &latest_block,
                genesis_time,
                slot,
                &hints,
                &ctx,
            );

            let sealed_hash = header.hash_slow();
            let sealed_header = SealedHeader::new(header, sealed_hash);
            let sealed_block = SealedBlock::new(sealed_header, body.clone());

            let block_hash = hints.block_hash.unwrap_or(sealed_block.hash());

            let exec_payload = create_alloy_execution_payload(&sealed_block, block_hash);

            let engine_hint = self
                .engine_hinter
                .fetch_next_payload_hint(&exec_payload, &versioned_hashes, parent_beacon_block_root)
                .await?;

            tracing::debug!("engine_hint: {:?}", engine_hint);

            match engine_hint {
                EngineApiHint::BlockHash(hash) => {
                    tracing::warn!("Should not receive block hash hint {:?}", hash);
                    hints.block_hash = Some(hash)
                }

                EngineApiHint::GasUsed(gas) => {
                    hints.gas_used = Some(gas);
                    hints.block_hash = None;
                }
                EngineApiHint::StateRoot(hash) => {
                    hints.state_root = Some(hash);
                    hints.block_hash = None
                }
                EngineApiHint::ReceiptsRoot(hash) => {
                    hints.receipts_root = Some(hash);
                    hints.block_hash = None
                }
                EngineApiHint::LogsBloom(bloom) => {
                    hints.logs_bloom = Some(bloom);
                    hints.block_hash = None
                }

                EngineApiHint::BaseFee(fee) => {
                    hints.base_fee = Some(fee);
                    hints.block_hash = None
                }
                EngineApiHint::ValidPayload => return Ok(sealed_block),
            }

            if i > max_iterations {
                return Err(BuilderError::Custom(
                    "Too many iterations: Failed to fetch all missing header values from geth error messages"
                        .to_string(),
                ));
            }

            i += 1;
        }
    }
}

pub struct ExecutionRpcClient(RpcClient<Http<Client>>);

impl ExecutionRpcClient {
    pub fn new<U: Into<Url>>(url: U) -> Self {
        let client = ClientBuilder::default().http(url.into());
        Self(client)
    }
    pub async fn get_block(&self, block_number: Option<u64>, full: bool) -> TransportResult<Block> {
        let tag = block_number.map_or(BlockNumberOrTag::Latest, BlockNumberOrTag::Number);

        self.0.request("eth_getBlockByNumber", (tag, full)).await
    }
}

/// convert a withdrawal from ethereum-consensus to Reth
pub(crate) fn convert_withdrawal_from_consensus_to_alloy(
    value: ethereum_consensus::capella::Withdrawal,
) -> alloy::eips::eip4895::Withdrawal {
    alloy::eips::eip4895::Withdrawal {
        index: value.index as u64,
        validator_index: value.validator_index as u64,
        address: Address::from_slice(value.address.as_ref()),
        amount: value.amount,
    }
}

/// convert a withdrawal from Reth to ethereum-consensus
pub(crate) fn convert_withdrawal_from_reth_to_consensus(
    value: &alloy::eips::eip4895::Withdrawal,
) -> ethereum_consensus::capella::Withdrawal {
    ethereum_consensus::capella::Withdrawal {
        index: value.index as usize,
        validator_index: value.validator_index as usize,
        address: ExecutionAddress::try_from(value.address.as_ref()).unwrap(),
        amount: value.amount,
    }
}

/// convert a sealed header into an ethereum-consensus execution payload header.
/// This requires recalculating the withdrals and transactions roots as SSZ instead of MPT roots.
pub(crate) fn create_execution_payload_header(
    sealed_block: &SealedBlock,
    transactions: Vec<TransactionSigned>,
) -> ConsensusExecutionPayloadHeader {
    // Transactions and withdrawals are treated as opaque byte arrays in consensus types
    let transactions_bytes = transactions
        .iter()
        .map(|t| t.encoded_2718())
        .collect::<Vec<_>>();

    let mut transactions_ssz: List<Transaction, MAX_TRANSACTIONS_PER_PAYLOAD> = List::default();

    for tx in transactions_bytes {
        transactions_ssz.push(Transaction::try_from(tx.as_ref()).unwrap());
    }

    let transactions_root = transactions_ssz
        .hash_tree_root()
        .expect("valid transactions root");

    let mut withdrawals_ssz: List<ConsensusWithdrawal, MAX_WITHDRAWALS_PER_PAYLOAD> =
        List::default();

    if let Some(withdrawals) = sealed_block.body.withdrawals.as_ref() {
        for w in withdrawals.iter() {
            withdrawals_ssz.push(convert_withdrawal_from_reth_to_consensus(w));
        }
    }

    let withdrawals_root = withdrawals_ssz
        .hash_tree_root()
        .expect("valid withdrawals root");

    let header = &sealed_block.header;

    ConsensusExecutionPayloadHeader {
        parent_hash: to_bytes32(header.parent_hash),
        fee_recipient: to_bytes20(header.beneficiary),
        state_root: to_bytes32(header.state_root),
        receipts_root: to_bytes32(header.receipts_root),
        logs_bloom: to_byte_vector(header.logs_bloom),
        prev_randao: to_bytes32(header.mix_hash),
        block_number: header.number,
        gas_limit: header.gas_limit,
        gas_used: header.gas_used,
        timestamp: header.timestamp,
        extra_data: ByteList::try_from(header.extra_data.as_ref()).unwrap(),
        base_fee_per_gas: ssz_rs::U256::from(header.base_fee_per_gas.unwrap_or_default()),
        block_hash: to_bytes32(header.hash()),
        blob_gas_used: header.blob_gas_used.unwrap_or_default(),
        excess_blob_gas: header.excess_blob_gas.unwrap_or_default(),
        transactions_root,
        withdrawals_root,
    }
}

/// create an Alloy execution payload from a sealedblock
pub(crate) fn create_alloy_execution_payload(
    block: &SealedBlock,
    block_hash: B256,
) -> AlloyExecutionPayload {
    let alloy_withdrawals = block
        .body
        .withdrawals
        .as_ref()
        .map(|withdrawals| {
            withdrawals
                .iter()
                .map(|w| Withdrawal {
                    index: w.index,
                    validator_index: w.validator_index,
                    address: w.address,
                    amount: w.amount,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    AlloyExecutionPayload::V3(ExecutionPayloadV3 {
        blob_gas_used: block.blob_gas_used(),
        excess_blob_gas: block.excess_blob_gas.unwrap_or_default(),
        payload_inner: ExecutionPayloadV2 {
            payload_inner: ExecutionPayloadV1 {
                base_fee_per_gas: U256::from(block.base_fee_per_gas.unwrap_or_default()),
                block_hash,
                block_number: block.number,
                extra_data: block.extra_data.clone(),
                transactions: block.raw_transactions(),
                fee_recipient: block.header.beneficiary,
                gas_limit: block.gas_limit,
                gas_used: block.gas_used,
                logs_bloom: block.logs_bloom,
                parent_hash: block.parent_hash,
                prev_randao: block.mix_hash,
                receipts_root: block.receipts_root,
                state_root: block.state_root,
                timestamp: block.timestamp,
            },
            withdrawals: alloy_withdrawals,
        },
    })
}

/// create an ethereum-consensus execution payload from a sealed block
pub(crate) fn create_consensus_execution_payload(value: &SealedBlock) -> ConsensusExecutionPayload {
    let hash = value.hash();
    let header = &value.header;
    let transactions = &value.body.transactions;
    let withdrawals = &value.body.withdrawals;
    let transactions = transactions
        .iter()
        .map(|t| spec::Transaction::try_from(t.encoded_2718().as_ref()).unwrap())
        .collect::<Vec<_>>();
    let withdrawals = withdrawals
        .as_ref()
        .unwrap_or(&Withdrawals::default())
        .iter()
        .map(|w| spec::Withdrawal {
            index: w.index as usize,
            validator_index: w.validator_index as usize,
            address: to_bytes20(w.address),
            amount: w.amount,
        })
        .collect::<Vec<_>>();

    let payload = DenebExecutionPayload {
        parent_hash: to_bytes32(header.parent_hash),
        fee_recipient: to_bytes20(header.beneficiary),
        state_root: to_bytes32(header.state_root),
        receipts_root: to_bytes32(header.receipts_root),
        logs_bloom: to_byte_vector(header.logs_bloom),
        prev_randao: to_bytes32(header.mix_hash),
        block_number: header.number,
        gas_limit: header.gas_limit,
        gas_used: header.gas_used,
        timestamp: header.timestamp,
        extra_data: ByteList::try_from(header.extra_data.as_ref()).unwrap(),
        base_fee_per_gas: ssz_rs::U256::from(header.base_fee_per_gas.unwrap_or_default()),
        block_hash: to_bytes32(hash),
        transactions: TryFrom::try_from(transactions).unwrap(),
        withdrawals: TryFrom::try_from(withdrawals).unwrap(),
        blob_gas_used: value.blob_gas_used(),
        excess_blob_gas: value.excess_blob_gas.unwrap_or_default(),
    };
    ConsensusExecutionPayload::Deneb(payload)
}

#[derive(Debug, Default)]
struct Context {
    extra_data: Bytes,
    base_fee: u64,
    blob_gas_used: u64,
    excess_blob_gas: u64,
    prev_randao: B256,
    fee_recipient: Address,
    transactions_root: B256,
    withdrawals_root: B256,
    parent_beacon_block_root: B256,
    slot_time_in_seconds: u64,
}

#[derive(Debug, Default)]
struct Hints {
    pub gas_used: Option<u64>,
    pub receipts_root: Option<B256>,
    pub logs_bloom: Option<Bloom>,
    pub state_root: Option<B256>,
    pub block_hash: Option<B256>,
    pub base_fee: Option<u64>,
}

/// Engine API hint values that can be fetched from the engine API
/// to complete the sealed block. These hints are used to fill in
/// missing values in the block header.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum EngineApiHint {
    BlockHash(B256),
    GasUsed(u64),
    StateRoot(B256),
    ReceiptsRoot(B256),
    LogsBloom(Bloom),
    ValidPayload,
    BaseFee(u64),
}

pub(crate) enum EngineType {
    Geth,
    Reth,
    Besu,
    Nethermind,
}

/// Engine hinter struct that is responsible for fetching hints from the
/// engine API to complete the sealed block. This struct is used by the
/// fallback payload builder to fetch missing header values.
#[derive(Debug)]
pub(crate) struct EngineHinter {
    client: reqwest::Client,
    jwt_hex: String,
    engine_rpc_url: Url,
    rpc_url: Url
}

impl EngineHinter {
    ///Check the type of the EL node. Throws an error if the EL node not responds
    pub async fn get_el_node_type(&self) -> Result<EngineType, BuilderError> {
        let auth_jwt = secret_to_bearer_header(&JwtSecret::from_hex(&self.jwt_hex)?);

        let body =
            format!(r#"{{"id":1,"jsonrpc":"2.0","method":"web3_clientVersion","params":[]}}"#,);

        let raw_version = self
            .client
            .post(self.rpc_url.as_str())
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await?
            .text()
            .await?;

        tracing::debug!(?raw_version, "raw version print");

        if raw_version.contains(r#""result":"Geth"#) {
            return Ok(EngineType::Geth);
        }

        Err(BuilderError::Custom("EL node is unreachable".to_owned()))
    }
    /// Fetch the next payload hint from the engine API to complete the sealed block.
    pub async fn fetch_next_payload_hint(
        &self,
        exec_payload: &AlloyExecutionPayload,
        versioned_hashes: &[B256],
        parent_beacon_root: B256,
    ) -> Result<EngineApiHint, BuilderError> {
        let auth_jwt = secret_to_bearer_header(&JwtSecret::from_hex(&self.jwt_hex)?);

        let body = format!(
            r#"{{"id":1,"jsonrpc":"2.0","method":"engine_newPayloadV3","params":[{}, {}, "{:?}"]}}"#,
            serde_json::to_string(&exec_payload)?,
            serde_json::to_string(&versioned_hashes)?,
            parent_beacon_root
        );

        let raw_hint = self
            .client
            .post(self.engine_rpc_url.as_str())
            .header("Content-Type", "application/json")
            .header("Authorization", auth_jwt.clone())
            .body(body)
            .send()
            .await?
            .text()
            .await?;

        let el_type = match self.get_el_node_type().await {
            Ok(el_type) => el_type,
            Err(err) => return Err(err),
        };

        match el_type {
            EngineType::Geth => {
                let Some(hint_value) = parse_geth_response(&raw_hint) else {
                    // If the hint is not found, it means that we likely got a VALID
                    // payload response or an error message that we can't parse.
                    if raw_hint.contains("\"status\":\"VALID\"") {
                        return Ok(EngineApiHint::ValidPayload);
                    } else {
                        return Err(BuilderError::InvalidEngineHint(raw_hint));
                    }
                };

                tracing::trace!("raw hint: {:?}", raw_hint);

                // Match the hint value to the corresponding header field and return it
                if raw_hint.contains("blockhash mismatch") {
                    return Ok(EngineApiHint::BlockHash(B256::from_hex(hint_value)?));
                } else if raw_hint.contains("invalid gas used") {
                    return Ok(EngineApiHint::GasUsed(hint_value.parse()?));
                } else if raw_hint.contains("invalid merkle root") {
                    return Ok(EngineApiHint::StateRoot(B256::from_hex(hint_value)?));
                } else if raw_hint.contains("invalid receipt root hash") {
                    return Ok(EngineApiHint::ReceiptsRoot(B256::from_hex(hint_value)?));
                } else if raw_hint.contains("invalid bloom") {
                    return Ok(EngineApiHint::LogsBloom(Bloom::from_hex(&hint_value)?));
                } else if raw_hint.contains("invalid baseFee") {
                    return Ok(EngineApiHint::BaseFee(hint_value.parse()?));
                };
            }
            _ => {
                return Err(BuilderError::Custom(
                    "The current EL node is not supported".to_string(),
                ));
            }
        }

        Err(BuilderError::Custom(
            "Unexpected: failed to parse any hint from engine response".to_string(),
        ))
    }
}

/// Geth Reference:
/// - [ValidateState](<https://github.com/ethereum/go-ethereum/blob/9298d2db884c4e3f9474880e3dcfd080ef9eacfa/core/block_validator.go#L122-L151>)
/// - [Blockhash Mismatch](<https://github.com/ethereum/go-ethereum/blob/9298d2db884c4e3f9474880e3dcfd080ef9eacfa/beacon/engine/types.go#L253-L256>)
pub(crate) fn parse_geth_response(error: &str) -> Option<String> {
    // Capture either the "local" or "got" value from the error message
    let re = Regex::new(r"(?:local:|got) ([0-9a-zA-Z]+)").expect("valid regex");

    re.captures(error)
        .and_then(|capture| capture.get(1).map(|matched| matched.as_str().to_string()))
}

/// Build a header with the given hints and context values.
fn build_header_with_hints_and_context(
    latest_block: &Block,
    genesis_time: u64,
    slot: u64,
    hints: &Hints,
    context: &Context,
) -> Header {
    // Use the available hints, or default to an empty value if not present.
    let gas_used = hints.gas_used.unwrap_or_default();
    let receipts_root = hints.receipts_root.unwrap_or_default();
    let logs_bloom = hints.logs_bloom.unwrap_or_default();
    let state_root = hints.state_root.unwrap_or_default();

    Header {
        parent_hash: latest_block.header.hash,
        ommers_hash: EMPTY_OMMER_ROOT_HASH,
        beneficiary: context.fee_recipient,
        state_root,
        transactions_root: context.transactions_root,
        receipts_root,
        withdrawals_root: Some(context.withdrawals_root),
        logs_bloom,
        difficulty: U256::ZERO,
        number: latest_block.header.number + 1,
        gas_limit: latest_block.header.gas_limit as u64,
        gas_used,
        timestamp: genesis_time + slot * context.slot_time_in_seconds,
        mix_hash: context.prev_randao,
        nonce: B64::ZERO,
        base_fee_per_gas: Some(context.base_fee),
        blob_gas_used: Some(context.blob_gas_used),
        excess_blob_gas: Some(context.excess_blob_gas),
        parent_beacon_block_root: Some(context.parent_beacon_block_root),
        requests_hash: None,
        extra_data: context.extra_data.clone(),
    }
}

pub(crate) fn to_bytes32(value: B256) -> spec::Bytes32 {
    spec::Bytes32::try_from(value.as_ref()).unwrap()
}

pub(crate) fn to_bytes20(value: Address) -> spec::ExecutionAddress {
    spec::ExecutionAddress::try_from(value.as_ref()).unwrap()
}

pub(crate) fn to_byte_vector(value: Bloom) -> ByteVector<256> {
    ByteVector::<256>::try_from(value.as_ref()).unwrap()
}

#[cfg(test)]
mod tests {
    use alloy::{
        eips::eip2718::Encodable2718,
        network::{EthereumWallet, TransactionBuilder},
        primitives::{hex, keccak256, Address},
        signers::{k256::ecdsa::SigningKey, local::PrivateKeySigner, Signer},
    };

    use crate::{
        commitment::request::PreconfRequest,
        constraints::{ConstraintsMessage, SignedConstraints},
        state::Block,
        test_utils::{default_test_transaction, get_test_config},
        BLSBytes, BLS_DST_PREFIX,
    };
    use crate::{constraints::Constraint, utils::create_random_bls_secretkey};
    use ethereum_consensus::crypto::PublicKey as ECBlsPublicKey;

    #[tokio::test]
    async fn test_build_fallback_payload() -> eyre::Result<()> {
        let _ = tracing_subscriber::fmt::try_init();

        let cfg = get_test_config();

        let raw_sk = "5d2344259f42259f82d2c140aa66102ba89b57b4883ee441a8b312622bd42491".to_string();
        let sk = SigningKey::from_slice(hex::decode(raw_sk)?.as_slice())?;
        let signer = PrivateKeySigner::from_signing_key(sk.clone());
        let wallet = EthereumWallet::from(signer.clone());

        let addy = Address::from_private_key(&sk);
        let tx = default_test_transaction(addy, Some(1)).with_chain_id(1);
        let tx_signed = tx.build(&wallet).await?;
        let raw_encoded = tx_signed.encoded_2718();

        let tx = Constraint::decode_enveloped(&mut raw_encoded.as_slice())?;
        let txs = vec![tx];

        let message_digest = {
            let mut data = Vec::new();
            // First field is the concatenation of all the transaction hashes
            data.extend_from_slice(
                &txs.iter()
                    .map(|tx| tx.tx.hash().as_slice())
                    .collect::<Vec<_>>()
                    .concat(),
            );
            keccak256(data)
        };

        let ecda_signature = signer.clone().sign_hash(&message_digest).await.unwrap();

        let request = PreconfRequest {
            signature: ecda_signature,
            txs,
            sender: addy,
            slot: 42,
            chain_id: 171000,
        };

        // println!("preconf request {:#?}", request);

        let validator_pubkey =
            ECBlsPublicKey::try_from(create_random_bls_secretkey().sk_to_pk().to_bytes().as_ref())
                .unwrap();

        let message = ConstraintsMessage::build(validator_pubkey, request);

        let signer_key = create_random_bls_secretkey();
        let signature = BLSBytes::from(
            signer_key
                .sign(&message.digest(), BLS_DST_PREFIX, &[])
                .to_bytes(),
        );
        let signed_constraints = SignedConstraints { message, signature };

        let mut block = Block::default();

        block.add_constraints(signed_constraints);

        assert_eq!(block.signed_constraints_list.len(), 1);
        Ok(())
    }
}
