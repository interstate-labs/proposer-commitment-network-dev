use server::run_builder_extend_modular;
use tracing_subscriber::fmt::Subscriber;

use env_file_reader::read_file;
use utils::get_urls;

mod builder;
mod extender;
mod server;
mod error;
mod utils;
mod auth;

#[tokio::main]
async fn main(){
    let subscriber = Subscriber::builder()
    .with_max_level(tracing::Level::DEBUG)
    .finish();
    // need to fix path
    let envs = read_file(".env").unwrap();

    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    let urls = get_urls(&envs["SIDECAR_URLS"]);

    run_builder_extend_modular(envs["EXTENDER_PORT"].parse::<u16>().unwrap(), urls).await;
}
