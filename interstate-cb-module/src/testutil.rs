use alloy::primitives::{Bytes, B256};
use ssz_compat::Decode;
use types::{ExecPayload, MainnetEthSpec, SignedBeaconBlockDeneb};

const BLOCK_DATA: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/testdata/signed-mainnet-beacon-block.bin.ssz"
));

/// Loads and parses a signed beacon block from a binary file in `testdata`.
pub fn load_beacon_block() -> SignedBeaconBlockDeneb<MainnetEthSpec> {
    SignedBeaconBlockDeneb::from_ssz_bytes(BLOCK_DATA).unwrap()
}

/// Extracts the root hash of transactions and the list of transactions from the beacon block.
pub fn extract_transactions() -> (B256, Vec<Bytes>) {
    let beacon_block = load_beacon_block();

    let tx_list = beacon_block.message.body.execution_payload.transactions().unwrap();

    let transaction_data: Vec<Bytes> =
        tx_list.into_iter().map(|tx| Bytes::from(tx.to_vec())).collect();

    let root_hash = beacon_block
        .message
        .body
        .execution_payload
        .to_execution_payload_header()
        .transactions_root();

    (B256::from_slice(root_hash.as_ref()), transaction_data)
}
