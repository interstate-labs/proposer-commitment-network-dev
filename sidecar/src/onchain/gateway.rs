
use alloy::{
  primitives::Address,
  providers::{ProviderBuilder, RootProvider},
  sol,
  transports::http::Http,
};
use eyre::bail;
use reqwest::{Client, Url};
use serde::Serialize;

use GatewayContract::GatewayContractInstance;

#[derive(Debug, Clone)]
pub struct GatewayController (GatewayContractInstance<Http<Client>, RootProvider<Http<Client>>>);

impl GatewayController {
  pub fn from_address<U: Into<Url>>(execution_client_url: U, contract_address: Address) -> Self {
    let provider = ProviderBuilder::new().on_http(execution_client_url.into());
    let gateway = GatewayContract::new(contract_address, provider);

    Self(gateway)
  }

  pub async fn check_ip(&self, ip: String) -> eyre::Result<bool>  {
    let data =  match self.0.getGatewayIPs().call().await {
      Ok(content) => content,
      Err(_err) => bail!("Failed to fetch a whitelist from a contract")
    };
    Ok(data.whitelist.contains(&ip))
  }

}

sol! {
  #[allow(missing_docs)]
  #[sol(rpc)]
  interface GatewayContract{
    #[derive(Debug, Default, Serialize)]
    function getGatewayIPs() public view returns (string[] memory whitelist);
  }
}