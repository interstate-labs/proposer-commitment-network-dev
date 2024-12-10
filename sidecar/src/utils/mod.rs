use blst::min_pk::SecretKey;
use rand::RngCore;

pub fn create_random_bls_secretkey() -> SecretKey {
  let mut rng = rand::thread_rng();
  let mut ikm = [0u8; 32];
  rng.fill_bytes(&mut ikm);
  SecretKey::key_gen(&ikm, &[]).unwrap()
}
