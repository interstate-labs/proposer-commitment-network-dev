use alloy_v092::{
    consensus::{transaction::PooledTransaction, BlobTransactionSidecar, Signed, Transaction, TxType, Typed2718},
    eips::eip2718::{Decodable2718, Encodable2718},
    hex,
    primitives::{Address, U256},
};

use reth_primitives_v115::TransactionSigned;
use serde::{de, ser::SerializeSeq};
use std::{borrow::Cow, fmt};

use crate::state::{account_state::AccountState, execution::ValidationError};
/// Trait that exposes additional information on transaction types that don't already do it
/// by themselves (e.g. [`PooledTransaction`]).
pub trait TransactionExtForPooledTransaction {
    /// Returns the value of the transaction.
    fn value(&self) -> U256;

    /// Returns the blob sidecar of the transaction, if any.
    fn blob_sidecar(&self) -> Option<&BlobTransactionSidecar>;

    /// Returns the size of the transaction in bytes.
    fn size(&self) -> usize;
}

impl TransactionExtForPooledTransaction for PooledTransaction {
    fn value(&self) -> U256 {
        match self {
            Self::Legacy(transaction) => transaction.tx().value,
            Self::Eip1559(transaction) => transaction.tx().value,
            Self::Eip2930(transaction) => transaction.tx().value,
            Self::Eip4844(transaction) => transaction.tx().tx().value,
            Self::Eip7702(transaction) => transaction.tx().value,
        }
    }

    fn blob_sidecar(&self) -> Option<&BlobTransactionSidecar> {
        match self {
            Self::Eip4844(transaction) => Some(&transaction.tx().sidecar),
            _ => None,
        }
    }

    fn size(&self) -> usize {
        match self {
            Self::Legacy(transaction) => transaction.tx().size(),
            Self::Eip1559(transaction) => transaction.tx().size(),
            Self::Eip2930(transaction) => transaction.tx().size(),
            Self::Eip4844(blob_tx) => blob_tx.tx().tx().size(),
            Self::Eip7702(transaction) => transaction.tx().size(),
        }
    }
}

pub const fn tx_type_str(tx_type: TxType) -> &'static str {
    match tx_type {
        TxType::Legacy => "legacy",
        TxType::Eip2930 => "eip2930",
        TxType::Eip1559 => "eip1559",
        TxType::Eip4844 => "eip4844",
        TxType::Eip7702 => "eip7702",
    }
}
#[derive(Clone, PartialEq, Eq)]
pub struct FullTransaction {
    pub tx: PooledTransaction,
    pub sender: Option<Address>,
}

impl From<PooledTransaction> for FullTransaction {
    fn from(tx: PooledTransaction) -> Self {
        Self { tx, sender: None }
    }
}

impl fmt::Debug for FullTransaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug_struct = f.debug_struct("FullTransaction");

        if self.tx.is_eip4844() {
            if let Some(tx) = self.tx.as_eip4844_with_sidecar() {
                let sidecar = tx.sidecar();

                let shortened_blobs: Vec<String> =
                    sidecar.blobs.iter().map(|blob| format!("{blob:#}")).collect();

                debug_struct
                    .field("tx", &"BlobTransaction")
                    .field("hash", self.tx.hash())
                    .field("transaction", &tx.tx())
                    .field("signature", &self.tx.signature())
                    .field("sidecar_blobs", &shortened_blobs)
                    .field("sidecar_commitments", &sidecar.commitments)
                    .field("sidecar_proofs", &sidecar.proofs);
            } else {
                debug_struct
                    .field("tx", &"EIP-4844 Transaction (no sidecar)")
                    .field("hash", self.tx.hash());
            }
        }

        debug_struct.field("sender", &self.sender);
        debug_struct.finish()
    }
}

impl std::ops::Deref for FullTransaction {
    type Target = PooledTransaction;

    fn deref(&self) -> &Self::Target {
        &self.tx
    }
}

impl std::ops::DerefMut for FullTransaction {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.tx
    }
}

impl FullTransaction {
    pub fn decode_enveloped(data: impl AsRef<[u8]>) -> eyre::Result<Self> {
        let tx = PooledTransaction::decode_2718(&mut data.as_ref())?;
        Ok(Self { tx, sender: None })
    }

    pub fn into_inner(self) -> PooledTransaction {
        self.tx
    }

    pub fn into_signed(self) -> TransactionSigned {
        match self.tx {
            PooledTransaction::Legacy(tx) => tx.into(),
            PooledTransaction::Eip1559(tx) => tx.into(),
            PooledTransaction::Eip2930(tx) => tx.into(),
            PooledTransaction::Eip4844(tx) => {
                let sig = *tx.signature();
                let hash = *tx.hash();
                let inner_tx = tx.into_parts().0.into_parts().0;
                Signed::new_unchecked(inner_tx, sig, hash).into()
            }
            PooledTransaction::Eip7702(tx) => tx.into(),
        }
    }

    pub fn sender(&self) -> Option<&Address> {
        self.sender.as_ref()
    }

    pub fn effective_tip_per_gas(&self, base_fee: u128) -> Option<u128> {
        let max_fee_per_gas = self.max_fee_per_gas();

        if max_fee_per_gas < base_fee {
            return None;
        }
        let fee = max_fee_per_gas - base_fee;

        if let Some(priority_fee) = self.max_priority_fee_per_gas() {
            Some(fee.min(priority_fee))
        } else {
            Some(fee)
        }
    }
}

pub fn serialize_txs<S: serde::Serializer>(
    txs: &[FullTransaction],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut seq = serializer.serialize_seq(Some(txs.len()))?;
    for tx in txs {
        let encoded = tx.tx.encoded_2718();
        seq.serialize_element(&hex::encode_prefixed(encoded))?;
    }
    seq.end()
}

pub fn deserialize_txs<'de, D>(deserializer: D) -> Result<Vec<FullTransaction>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let hex_strings = <Vec<Cow<'_, str>> as de::Deserialize>::deserialize(deserializer)?;
    let mut txs = Vec::with_capacity(hex_strings.len());

    for s in hex_strings {
        let data = hex::decode(s.trim_start_matches("0x")).map_err(de::Error::custom)?;
        let tx = PooledTransaction::decode_2718(&mut data.as_slice())
            .map_err(de::Error::custom)
            .map(|tx| FullTransaction { tx, sender: None })?;
        txs.push(tx);
    }

    Ok(txs)
}

pub fn calculate_max_basefee(current: u128, block_diff: u64) -> Option<u128> {
    let multiplier: u128 = 1125;
    let divisor: u128 = 1000;
    let mut max_basefee = current;

    for _ in 0..block_diff {
        if max_basefee > u128::MAX / multiplier {
            return None;
        }

        max_basefee = max_basefee * multiplier / divisor + 1;
    }

    Some(max_basefee)
}

pub fn max_transaction_cost(transaction: &PooledTransaction) -> U256 {
    let gas_limit = transaction.gas_limit() as u128;

    let mut fee_cap = transaction.max_fee_per_gas();
    fee_cap += transaction.max_priority_fee_per_gas().unwrap_or(0);

    if let Some(eip4844) = transaction.as_eip4844() {
        fee_cap += eip4844.max_fee_per_blob_gas + eip4844.blob_gas() as u128;
    }

    U256::from(gas_limit * fee_cap) + <PooledTransaction as TransactionExtForPooledTransaction>::value(transaction)
}

pub fn validate_transaction(
    account_state: &AccountState,
    transaction: &PooledTransaction,
) -> Result<(), ValidationError> {
    if transaction.nonce() < account_state.transaction_count {
        return Err(ValidationError::NonceTooLow(
            account_state.transaction_count,
            transaction.nonce(),
        ));
    }

    if transaction.nonce() > account_state.transaction_count {
        return Err(ValidationError::NonceTooHigh(
            account_state.transaction_count,
            transaction.nonce(),
        ));
    }

    if max_transaction_cost(transaction) > account_state.balance {
        return Err(ValidationError::InsufficientBalance);
    }

    if account_state.has_code {
        return Err(ValidationError::AccountHasCode);
    }

    Ok(())
}