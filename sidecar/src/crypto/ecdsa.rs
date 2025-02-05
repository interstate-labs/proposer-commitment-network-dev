use std::fmt::Debug;

use alloy_v092::{
    primitives::Address,
    signers::{local::PrivateKeySigner, Signature as AlloySignature, Signer},
};
use secp256k1::{ecdsa::Signature, Message, PublicKey, SecretKey};

pub trait SignableECDSA {
    fn digest(&self) -> Message;

    fn sign(&self, key: &SecretKey) -> Signature {
        secp256k1::Secp256k1::new().sign_ecdsa(&self.digest(), key)
    }

    fn verify(&self, signature: &Signature, pubkey: &PublicKey) -> bool {
        secp256k1::Secp256k1::new().verify_ecdsa(&self.digest(), signature, pubkey).is_ok()
    }
}

#[derive(Clone, Debug)]
pub struct ECDSASigner {
    secp256k1_key: SecretKey,
}

impl ECDSASigner {
    pub fn new(secp256k1_key: SecretKey) -> Self {
        Self { secp256k1_key }
    }

    pub fn sign_ecdsa<T: SignableECDSA>(&self, obj: &T) -> Signature {
        obj.sign(&self.secp256k1_key)
    }

    #[allow(dead_code)]
    pub fn verify_ecdsa<T: SignableECDSA>(
        &self,
        obj: &T,
        sig: &Signature,
        pubkey: &PublicKey,
    ) -> bool {
        obj.verify(sig, pubkey)
    }
}

#[async_trait::async_trait]
pub trait SignerECDSA: Send + Debug {
    fn public_key(&self) -> Address;
    async fn sign_hash(&self, hash: &[u8; 32]) -> eyre::Result<AlloySignature>;
}

#[async_trait::async_trait]
impl SignerECDSA for PrivateKeySigner {
    fn public_key(&self) -> Address {
        self.address()
    }

    async fn sign_hash(&self, hash: &[u8; 32]) -> eyre::Result<AlloySignature> {
        let sig = Signer::sign_hash(self, hash.into()).await?;

        Ok(AlloySignature::try_from(sig.as_bytes().as_ref()).expect("signature conversion"))
    }
}