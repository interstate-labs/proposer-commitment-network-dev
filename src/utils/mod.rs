pub use blst::min_pk::SecretKey as BlsSecretKey;
use rand::RngCore;

/// create a random BLS secret key.
pub fn create_bls_secret() -> BlsSecretKey {
  let mut rng = rand::thread_rng();
  let mut ikm = [0u8; 32];
  rng.fill_bytes(&mut ikm);
  BlsSecretKey::key_gen(&ikm, &[]).unwrap()
}