use std::fs;
use std::path::Path;

fn main() {
    let source_keys = "/home/proposer-commitment-network-dev/sidecar/keystores/keys";
    let source_secrets = "/home/proposer-commitment-network-dev/sidecar/keystores/secrets";
    let dest_keys = "/home/proposer-commitment-network-dev/sidecar/web3signer/keystore/keys";
    let dest_secrets = "/home/proposer-commitment-network-dev/sidecar/web3signer/keystore/secrets";

    // Ensure destination directories exist
    fs::create_dir_all(dest_keys).expect("Failed to create keys directory");
    fs::create_dir_all(dest_secrets).expect("Failed to create secrets directory");

    // Move and rename keystore files
    for entry in fs::read_dir(source_keys).expect("Failed to read keys directory") {
        let entry = entry.expect("Failed to read entry");
        let path = entry.path();
        if path.is_dir() {
            let dir_name = path.file_name().unwrap().to_str().unwrap();
            let keystore_file = path.join("voting-keystore.json");
            if keystore_file.exists() {
                let new_path = format!("{}/{}.json", dest_keys, dir_name);
                fs::rename(&keystore_file, &new_path).expect("Failed to move keystore file");
            }
        }
    }

    // Move and rename secret files
    for entry in fs::read_dir(source_secrets).expect("Failed to read secrets directory") {
        let entry = entry.expect("Failed to read entry");
        let path = entry.path();
        if path.is_file() {
            let filename = path.file_name().unwrap().to_str().unwrap();
            let new_path = format!("{}/{}.txt", dest_secrets, filename);
            fs::rename(&path, &new_path).expect("Failed to move secret file");
        }
    }
    
    println!("Files moved successfully!");
}
