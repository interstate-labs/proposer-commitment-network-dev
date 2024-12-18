use alloy::{hex, primitives::FixedBytes};
use lighthouse_bls::Keypair;
use std::{
  collections::HashSet,
  ffi::OsString,
  fmt::Debug,
  fs::{self, DirEntry, ReadDir},
  io,
  path::{Path, PathBuf},
};
use lighthouse_eth2_keystore::Keystore;
use ethereum_consensus::{crypto::PublicKey as ECBlsPublicKey};
use ssz::Encode;

use crate::config::ChainConfig;
use crate::constraints::signature::compute_signing_root;

#[derive(Debug, thiserror::Error)]
#[allow(missing_docs)]
pub enum KeystoreError {
    #[error("failed to read keystore directory: {0}")]
    ReadFromDirectory(#[from] std::io::Error),
    #[error("failed to read keystore from JSON file {0}: {1}")]
    ReadFromJSON(PathBuf, String),
    #[error("failed to read keystore secret from file: {0}")]
    ReadFromSecretFile(String),
    #[error("failed to decrypt keypair from JSON file {0} with the provided password: {1}")]
    KeypairDecryption(PathBuf, String),
    #[error("could not find private key associated to public key {0}")]
    UnknownPublicKey(String),
    #[error("invalid signature key length -- signature: {0} -- message: {1}")]
    SignatureLength(String, String),
}

pub struct Keystores{
  keypairs: Vec<Keypair>,
  chain: ChainConfig
}

impl Keystores {
  pub fn new(pubkeys_root_path: &Path, secrets_path: &Path, chain: &ChainConfig) -> Self {
    let mut keystore_paths = Vec::new();

    for dir_entry in read_dir(&pubkeys_root_path.to_path_buf()).unwrap() {
        let path = read_path(dir_entry).unwrap();
        if path.is_dir() {
            for dir_entry in read_dir(&path).unwrap() {
                let path = read_path(dir_entry).unwrap();
                if path.is_file() && path.extension() == Some(&OsString::from("json")) {
                  keystore_paths.push(path);
                }
            }
        }
    }

    let mut keypairs = Vec::with_capacity(keystore_paths.len());

    for path in keystore_paths {
      let keystore = Keystore::from_json_file(path.clone()).unwrap();

      let pubkey = format!("0x{}", keystore.pubkey());

      let mut secret_path = secrets_path.to_path_buf();
      secret_path.push(pubkey);

      let password = fs::read_to_string(secret_path).unwrap();

      let keypair = keystore.decrypt_keypair(password.as_bytes()).unwrap();

      keypairs.push(keypair);   
    }

    Self { keypairs, chain:chain.clone() }
  }

  pub fn get_pubkeys(&self) -> HashSet<ECBlsPublicKey> {
    self.keypairs
    .iter()
    .map(|kp| {
      ECBlsPublicKey::try_from(kp.pk.serialize().to_vec().as_ref()).expect("valid pubkey")
    })
    .collect::<HashSet<_>>()
  }

  /// Signs a message with the keystore signer and the Commit Boost domain
  pub fn sign_commit_boost_root(
    &self,
    root: [u8; 32],
    public_key: &ECBlsPublicKey,
  ) -> Result<BLSSig, KeystoreError> {
      self.sign_root(root, public_key, self.chain.commit_boost_domain())
  }

  /// Signs a message with the keystore signer.
  fn sign_root(
      &self,
      root: [u8; 32],
      public_key: &ECBlsPublicKey,
      domain: [u8; 32],
  ) -> Result<BLSSig, KeystoreError> {
      let sk = self
          .keypairs
          .iter()
          // `as_ssz_bytes` returns the raw bytes we need
          .find(|kp| kp.pk.as_ssz_bytes() == public_key.as_ref())
          .ok_or(KeystoreError::UnknownPublicKey(public_key.to_string()))?;

      let signing_root = compute_signing_root(root, domain);

      let sig = sk.sk.sign(signing_root.into()).as_ssz_bytes();
      let sig = BLSSig::try_from(sig.as_slice())
          .map_err(|e| KeystoreError::SignatureLength(hex::encode(sig), format!("{e:?}")))?;

      Ok(sig)
  }

}

fn read_dir(path: &PathBuf) -> Result<ReadDir, std::io::Error> {
  fs::read_dir(path)
}

fn read_path(entry: std::result::Result<DirEntry, io::Error>) -> Result<PathBuf, std::io::Error>  {
  Ok(entry?.path())
}

/// A fixed-size byte array for BLS signatures.
pub type BLSSig = FixedBytes<96>;


#[cfg(test)]
mod tests {
    use std::{
        fs::File,
        io::Write,
        path::{Path, PathBuf},
    };

    use crate::config::ChainConfig;

    use super::Keystores;
    /// The str path of the root of the project
    pub const CARGO_MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

    const KEYSTORES_DEFAULT_PATH_TEST: &str = "mock_data/keys";
    const KEYSTORES_SECRETS_DEFAULT_PATH_TEST: &str = "mock_data/secrets";

    /// If `path` is `Some`, returns a clone of it. Otherwise, returns the path to the
    /// `fallback_relative_path` starting from the root of the cargo project.
    fn make_path(relative_path: &str) -> PathBuf {
        let project_root = env!("CARGO_MANIFEST_DIR");
        Path::new(project_root).join(relative_path)
    }

    #[test]
    fn test_keystore_signer() {
        // 0. Test data setup

        // Reference: https://eips.ethereum.org/EIPS/eip-2335#test-cases
        let tests_keystore_json = [
            r#"
            {
                "crypto": {
                    "kdf": {
                        "function": "scrypt",
                        "params": {
                            "dklen": 32,
                            "n": 262144,
                            "p": 1,
                            "r": 8,
                            "salt": "d4e56740f876aef8c010b86a40d5f56745a118d0906a34e69aec8c0db1cb8fa3"
                        },
                        "message": ""
                    },
                    "checksum": {
                        "function": "sha256",
                        "params": {},
                        "message": "d2217fe5f3e9a1e34581ef8a78f7c9928e436d36dacc5e846690a5581e8ea484"
                    },
                    "cipher": {
                        "function": "aes-128-ctr",
                        "params": {
                            "iv": "264daa3f303d7259501c93d997d84fe6"
                        },
                        "message": "06ae90d55fe0a6e9c5c3bc5b170827b2e5cce3929ed3f116c2811e6366dfe20f"
                    }
                },
                "description": "This is a test keystore that uses scrypt to secure the secret.",
                "pubkey": "9612d7a727c9d0a22e185a1c768478dfe919cada9266988cb32359c11f2b7b27f4ae4040902382ae2910c15e2b420d07",
                "path": "m/12381/60/3141592653/589793238",
                "uuid": "1d85ae20-35c5-4611-98e8-aa14a633906f",
                "version": 4
            }
        "#,
            r#"
            {
                "crypto": {
                    "kdf": {
                        "function": "pbkdf2",
                        "params": {
                            "dklen": 32,
                            "c": 262144,
                            "prf": "hmac-sha256",
                            "salt": "d4e56740f876aef8c010b86a40d5f56745a118d0906a34e69aec8c0db1cb8fa3"
                        },
                        "message": ""
                    },
                    "checksum": {
                        "function": "sha256",
                        "params": {},
                        "message": "8a9f5d9912ed7e75ea794bc5a89bca5f193721d30868ade6f73043c6ea6febf1"
                    },
                    "cipher": {
                        "function": "aes-128-ctr",
                        "params": {
                            "iv": "264daa3f303d7259501c93d997d84fe6"
                        },
                        "message": "cee03fde2af33149775b7223e7845e4fb2c8ae1792e5f99fe9ecf474cc8c16ad"
                    }
                },
                "description": "This is a test keystore that uses PBKDF2 to secure the secret.",
                "pubkey": "9612d7a727c9d0a22e185a1c768478dfe919cada9266988cb32359c11f2b7b27f4ae4040902382ae2910c15e2b420d07",
                "path": "m/12381/60/0/0",
                "uuid": "64625def-3331-4eea-ab6f-782f3ed16a83",
                "version": 4
            }
        "#,
        ];

        // Reference: https://eips.ethereum.org/EIPS/eip-2335#test-cases
        let password = r#"𝔱𝔢𝔰𝔱𝔭𝔞𝔰𝔰𝔴𝔬𝔯𝔡🔑"#;
        let public_key = "0x9612d7a727c9d0a22e185a1c768478dfe919cada9266988cb32359c11f2b7b27f4ae4040902382ae2910c15e2b420d07";
        let chain_config = ChainConfig::default();

        let keystore_path =
            format!("{}/{}/{}", CARGO_MANIFEST_DIR, KEYSTORES_DEFAULT_PATH_TEST, public_key);

        println!("{} keystore path", keystore_path);
        let keystore_path = PathBuf::from(keystore_path);


        for test_keystore_json in tests_keystore_json {

            let mut tmp_keystore_file =
                File::create(keystore_path.join("test-voting-keystore.json"))
                    .expect("to create new keystore file");

            tmp_keystore_file
                .write_all(test_keystore_json.as_bytes())
                .expect("to write to temp file");

            // Create a file for the secret, we are going to test it as well
            let keystores_secrets_path = make_path(KEYSTORES_SECRETS_DEFAULT_PATH_TEST);
            let mut tmp_secret_file = File::create(keystores_secrets_path.join(public_key))
                .expect("to create secret file");

            tmp_secret_file.write_all(password.as_bytes()).expect("to write to temp file");

            let keys_path = make_path(KEYSTORES_DEFAULT_PATH_TEST);

            let keystore_signer_from_directory = Keystores::new(
                &keys_path,
                &keystores_secrets_path,
                &chain_config,
            );

            assert_eq!(keystore_signer_from_directory.keypairs.len(), 3);
            assert_eq!(
                keystore_signer_from_directory
                    .keypairs
                    .first()
                    .expect("to get keypair")
                    .pk
                    .to_string(),
                public_key
            );
        }
    }
}