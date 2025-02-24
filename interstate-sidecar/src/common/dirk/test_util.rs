use eyre::{bail, Context};
use rustls::crypto::CryptoProvider;
use std::{
    fs,
    process::{Child, Command},
    time::Duration,
};

use crate::cli::DirkTlsCredentials;

/// Initialize the default TLS provider for the tests if not already set.
pub fn try_init_tls_provider() {
    // Init the default rustls provider
    if CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
}

/// Start a DIRK test server for testing (run on localhost:9091).
/// This is a single instance (non distributed).
///
/// Returns the DIRK client URL and credentials, and the corresponding server process handle.
pub async fn start_single_dirk_test_server() -> eyre::Result<(String, DirkTlsCredentials, Child)> {
    try_init_tls_provider();

    // Check if dirk is installed (in $PATH)
    if Command::new("dirk").arg("--help").status().is_err() {
        bail!("DIRK is not installed in $PATH");
    }

    let test_data_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/test_data/dirk_single").to_string();

    // read the template json file from test_data
    let template_path = test_data_dir.clone() + "/dirk.template.json";
    let template = fs::read_to_string(template_path).wrap_err("Failed to read template")?;

    // change the occurrence of $PWD to the current working directory in the template
    let new_file = test_data_dir.clone() + "/dirk.json";
    let new_content = template.replace("$PWD", &test_data_dir);
    fs::write(new_file, new_content).wrap_err("Failed to write dirk config file")?;

    // Start the DIRK server in the background
    let dirk_proc = Command::new("dirk").arg("--base-dir").arg(&test_data_dir).spawn()?;

    // Wait for some time for the server to start up
    tokio::time::sleep(Duration::from_secs(3)).await;

    let url = "https://localhost:9091".to_string();

    let cred = DirkTlsCredentials {
        client_cert_path: test_data_dir.clone() + "/client1.crt",
        client_key_path: test_data_dir.clone() + "/client1.key",
        ca_cert_path: Some(test_data_dir.clone() + "/security/ca.crt"),
    };

    Ok((url, cred, dirk_proc))
}

/// Start a multi-node DIRK test server for testing.
/// This is a distributed instance with multiple nodes.
///
/// Returns the DIRK client URL and credentials,
/// and the corresponding server process handles.
///
/// NOTE: in order for the example certificates to work on your machine, you need to
/// modify the /etc/hosts file to include the following entry:
///
/// ```text
/// 127.0.0.1       localhost localhost-1 localhost-2 localhost-3
/// ```
///
/// This is because we need to map 3 different server certificates to localhost
/// to simulate multiple servers with their own hostnames.
pub async fn start_multi_dirk_test_server() -> eyre::Result<(String, DirkTlsCredentials, Vec<Child>)>
{
    try_init_tls_provider();

    // Check if dirk is installed (in $PATH)
    if Command::new("dirk").arg("--help").status().is_err() {
        bail!("DIRK is not installed in $PATH");
    }

    let test_data_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/test_data/dirk_multi").to_string();

    // directories containing the individual configuration for each instance
    let dirk_ids = ["1", "2", "3"];
    let mut dirk_procs = Vec::new();

    for dirk_id in dirk_ids {
        // Example: /test_data/dirk_multi/1
        let dirk_dir = test_data_dir.clone() + &format!("/{}", dirk_id);

        // read the template yml file from test_data
        let template_path = dirk_dir.clone() + "/dirk.template.yml";
        let template = fs::read_to_string(template_path).wrap_err("Failed to read template")?;

        // change the occurrence of $PWD to the current working directory in the template
        let new_file = dirk_dir.clone() + "/dirk.yml";
        let new_content = template.replace("$PWD", &test_data_dir);
        fs::write(new_file, new_content).wrap_err("Failed to write dirk config file")?;

        let dirk_proc = Command::new("dirk").arg("--base-dir").arg(&dirk_dir).spawn()?;
        dirk_procs.push(dirk_proc);

        // Wait for some time for each server to start up
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    // Note: the first server is used for the client connection
    let url = "https://localhost-1:8881".to_string();
    let cred = DirkTlsCredentials {
        client_cert_path: test_data_dir.clone() + "/client/localhost.crt",
        client_key_path: test_data_dir.clone() + "/client/localhost.key",
        ca_cert_path: Some(test_data_dir.clone() + "/1/security/ca.crt"),
    };

    Ok((url, cred, dirk_procs))
}
