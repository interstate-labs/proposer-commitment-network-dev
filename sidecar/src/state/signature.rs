use std::str::FromStr;

use alloy_v092::{
    hex,
    primitives::{PrimitiveSignature, SignatureError as AlloySignatureError},
};
use derive_more::derive::{Deref, DerefMut, From, FromStr};
use serde::{de, Deserialize, Deserializer, Serialize};

#[derive(Debug, thiserror::Error)]
#[error("Invalid signature")]
pub struct SignatureError;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Deref, DerefMut, From, FromStr)]
pub struct AlloySignatureWrapper(pub PrimitiveSignature);

impl AlloySignatureWrapper {
    pub fn test_signature() -> Self {
        PrimitiveSignature::test_signature().into()
    }
}

impl<'a> TryFrom<&'a [u8]> for AlloySignatureWrapper {
    type Error = AlloySignatureError;

    fn try_from(bytes: &'a [u8]) -> Result<Self, Self::Error> {
        PrimitiveSignature::try_from(bytes).map(Self)
    }
}

impl Serialize for AlloySignatureWrapper {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let parity = self.v();
        let mut bytes = self.as_bytes();
        bytes[bytes.len() - 1] = if parity { 1 } else { 0 };
        serializer.serialize_str(&hex::encode_prefixed(bytes))
    }
}

impl<'de> Deserialize<'de> for AlloySignatureWrapper {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let s = Self::from_str(s.trim_start_matches("0x")).map_err(de::Error::custom)?;
        Ok(s)
    }
}

#[allow(dead_code)]
fn deserialize_sig<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: std::fmt::Display,
{
    let s = String::deserialize(deserializer)?;
    T::from_str(s.trim_start_matches("0x")).map_err(de::Error::custom)
}

#[allow(dead_code)]
fn serialize_sig<S: serde::Serializer>(
    sig: &AlloySignatureWrapper,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let parity = sig.v();
    let mut bytes = sig.as_bytes();
    bytes[bytes.len() - 1] = if parity { 1 } else { 0 };
    serializer.serialize_str(&hex::encode_prefixed(bytes))
}

pub trait ECDSASignatureExt {
    fn as_bytes_with_parity(&self) -> [u8; 65];
    fn to_hex(&self) -> String;
}

impl ECDSASignatureExt for AlloySignatureWrapper {
    fn as_bytes_with_parity(&self) -> [u8; 65] {
        let parity = self.v();
        let mut bytes = self.as_bytes();
        bytes[bytes.len() - 1] = if parity { 1 } else { 0 };

        bytes
    }

    fn to_hex(&self) -> String {
        hex::encode_prefixed(self.as_bytes_with_parity())
    }
}