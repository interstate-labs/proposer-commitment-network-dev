use alloy_v092::{primitives::FixedBytes, rpc::types::beacon::constants::BLS_PUBLIC_KEY_BYTES_LEN};
use ethereum_consensus::crypto::PublicKey as BlsPublicKey;

pub const BLS_DST_PREFIX: &[u8] = b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_POP_";

pub type BLSSig = FixedBytes<96>;

pub trait SignableBLS {
    fn digest(&self) -> [u8; 32];
}

pub fn cl_public_key_to_arr(pubkey: impl AsRef<BlsPublicKey>) -> [u8; BLS_PUBLIC_KEY_BYTES_LEN] {
    pubkey
        .as_ref()
        .as_ref()
        .try_into()
        .expect("BLS keys are 48 bytes")
}
