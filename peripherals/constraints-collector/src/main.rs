mod collector;
mod constraints;
mod errors;
mod server;

use env_file_reader::read_file;

pub use collector::{
    ConstraintsCollector, GetHeaderParams, GetPayloadResponse, SignedBuilderBid, VersionedValue,
};
pub use constraints::{Constraint, ConstraintsMessage, SignedConstraints};
pub use server::{
    CONSTRAINTS_PATH, GET_HEADER_PATH, GET_PAYLOAD_PATH, REGISTER_VALIDATORS_PATH, STATUS_PATH,
};

pub use errors::{CollectorError, ErrorResponse};

use server::run_constraints_collector;
use tracing_subscriber::fmt::Subscriber;
#[tokio::main]
async fn main() {
    let subscriber = Subscriber::builder()
        .with_max_level(tracing::Level::DEBUG)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let envs = read_file("/work/interstate-protocol/constraints-collector/.env").unwrap();
    let port = envs["PORT"].parse::<u16>().unwrap();
    let cb_url = envs["COMMIT_BOOST"].clone();
    run_constraints_collector(port, cb_url).await;
    println!("Hello, world!");
}
