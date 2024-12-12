mod server;
mod collector;
mod constraints;
mod errors;

use env_file_reader::read_file;

pub use constraints::{Constraint, ConstraintsMessage, SignedConstraints};
pub use collector::{ConstraintsCollector, GetHeaderParams, SignedBuilderBid, GetPayloadResponse, VersionedValue};
pub use server::{
    STATUS_PATH, 
    REGISTER_VALIDATORS_PATH, 
    GET_HEADER_PATH, 
    GET_PAYLOAD_PATH, 
    CONSTRAINTS_PATH
};

pub use errors::{CollectorError, ErrorResponse};

use server::run_constraints_collector;
use tracing_subscriber::fmt::Subscriber;
#[tokio::main]
async fn main() {
    
    let subscriber = Subscriber::builder()
    .with_max_level(tracing::Level::DEBUG)
    .finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    let envs = read_file("/work/interstate-protocol/constraints-collector/.env").unwrap();
    let port = envs["PORT"].parse::<u16>().unwrap();
    let cb_url = envs["COMMIT_BOOST"].clone();
    run_constraints_collector(port, cb_url).await;
    println!("Hello, world!");
}
