use std::{sync::Arc, net::SocketAddr};

use axum::{Router, extract::{Path, State}, response::Html, routing::{ get, post }, Json};

use crate::{builder::{ GetHeaderParams, GetPayloadResponse, SignedBuilderBid, VersionedValue}, error::BuilderApiError};
use ethereum_consensus::{
  builder::SignedValidatorRegistration, deneb::mainnet::SignedBlindedBeaconBlock
};
use reqwest::StatusCode;

use crate::{builder::{GET_HEADER_PATH, GET_PAYLOAD_PATH, REGISTER_VALIDATORS_PATH, STATUS_PATH}, extender::Extender};

pub async fn run_builder_extend_modular(port: u16, urls:Vec<String>) {
    let extender = Arc::new(Extender::new(urls));
    let router = Router::new()
      .route("/", get(description))
      .route(STATUS_PATH, get(status))
      .route(
          REGISTER_VALIDATORS_PATH,
          post(register_validators),
      )
      .route(GET_HEADER_PATH, get(get_header))
      .route(GET_PAYLOAD_PATH, post(get_payload))
      .with_state(extender);

    //TODO: replace a port
    let addr: SocketAddr = SocketAddr::from(([0,0,0,0], port));

    //TODO: replace a listening port as a builder
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    tracing::info!("extender is running on .. {}", addr);
    axum::serve(listener, router).await.unwrap();
}

async fn description() -> Html<& 'static str> {
  Html("This is an endpoint to interact with commit-boost")
}

async fn status(State(extender):State<Arc<Extender>>) -> StatusCode {
  tracing::debug!("handling STATUS request");

  let status = match extender.status().await {
      Ok(status) => status,
      Err(err) => {
          tracing::error!(%err, "Failed in getting status from all builders");
          StatusCode::INTERNAL_SERVER_ERROR
      }
  };
  status
}


async fn get_header( State(extender):State<Arc<Extender>>, Path(params): Path<GetHeaderParams>) -> Result<Json<VersionedValue<SignedBuilderBid>>, BuilderApiError> {
  tracing::debug!("handling GET_HEADER request");
  match extender.get_header(params).await {
      Ok(header) => 
          return Ok(Json(header))
      ,
      Err(err) => {
          tracing::error!("Failed in getting header with proof from all builders");
          return Err(err);
      }
  }
}

async fn get_payload( State(extender): State<Arc<Extender>>, Json(signed_blinded_block):Json<SignedBlindedBeaconBlock>) -> Result<Json<GetPayloadResponse>, BuilderApiError> {
  tracing::debug!("handling GET_PAYLOAD request");

  match extender
          .get_payload(signed_blinded_block)
          .await
          .map(Json)
          .map_err(|e| {
              tracing::error!(%e, "Failed to get payload from all builders");
              e
          })
  {
      Ok(payload) => return Ok(payload),
      Err(err) => {
          tracing::error!("Failed in getting payload from all builders");
          return Err(err);
      }
  };
}

async fn register_validators( State(extender):State<Arc<Extender>>, Json(registors):Json<Vec<SignedValidatorRegistration>>) -> Result<StatusCode, BuilderApiError> {
  tracing::debug!("handling REGISTER_VALIDATORS_REQUEST");
  match extender.register_validators(registors).await.map(|_| StatusCode::OK).map_err(|e| {
    tracing::error!(%e, "Failed to submit validators to all builders");
    e
  })
  {
    Ok(_) => return Ok(StatusCode::OK),
    Err(err) => {
        tracing::error!("Failed to submit validators to all builders");
        return Err(err);
    }
  };
}
