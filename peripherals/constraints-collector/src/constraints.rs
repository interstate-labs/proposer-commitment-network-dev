use serde::{de, Deserialize, Deserializer, Serialize};
use alloy::{hex, primitives::{keccak256, Address, FixedBytes}};
use reth_primitives::PooledTransactionsElement;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default, )]
pub struct SignedConstraints {
    /// The constraints that need to be signed.
    pub message: ConstraintsMessage,
    /// The signature of the proposer sidecar.
    pub signature: FixedBytes<96>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct ConstraintsMessage {
    /// The validator index of the proposer sidecar.
    pub validator_index: u64,
    /// The consensus slot at which the constraints are valid
    pub slot: u64,
    /// The constraints that need to be signed.
    pub constraints: Vec<Constraint>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Constraint {
    pub index: Option<u64>,
    #[serde(rename(serialize = "tx", deserialize = "tx"), serialize_with = "serialize_tx", deserialize_with = "deserialize_tx")]
    pub(crate) transaction: PooledTransactionsElement,
    pub(crate) sender: Address,
}

fn serialize_tx<S>(
  tx: &PooledTransactionsElement,
  serializer: S,
) -> Result<S::Ok, S::Error>
where
  S: serde::Serializer,
{
  tracing::debug!("start to serialize");
  let mut data = Vec::new();
  tx.encode_enveloped(&mut data);
  serializer.serialize_str(&format!("0x{}", hex::encode(&data)))
}

fn deserialize_tx<'de, D>(deserializer: D) -> Result<PooledTransactionsElement, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    let data = hex::decode(s.trim_start_matches("0x")).map_err(de::Error::custom)?;
    PooledTransactionsElement::decode_enveloped(&mut data.as_slice()).map_err(de::Error::custom)
}