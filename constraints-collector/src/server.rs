use std::{net::SocketAddr, ops::Deref, sync::Arc};

use axum::{extract::{ws::{Message, WebSocket, WebSocketUpgrade}, Extension, Path, State}, response::{Html, IntoResponse}, routing::{ any, get, post }, Json, Router};
use ethereum_consensus::{
  builder::SignedValidatorRegistration, deneb::mainnet::SignedBlindedBeaconBlock
};
use reqwest::StatusCode;
use tokio::sync::broadcast;
use tower_http::{
  services::ServeDir,
  trace::{DefaultMakeSpan, TraceLayer},
};


use crate::{CollectorError, ConstraintsCollector, GetHeaderParams, GetPayloadResponse, SignedBuilderBid, SignedConstraints, VersionedValue };

/// The path to the builder API status endpoint.
pub const STATUS_PATH: &str = "/eth/v1/builder/status";
/// The path to the builder API register validators endpoint.
pub const REGISTER_VALIDATORS_PATH: &str = "/eth/v1/builder/validators";
/// The path to the builder API get header endpoint.
pub const GET_HEADER_PATH: &str = "/eth/v1/builder/header_with_proofs/:slot/:parent_hash/:pubkey";
/// The path to the builder API get payload endpoint.
pub const GET_PAYLOAD_PATH: &str = "/eth/v1/builder/blinded_blocks";
/// The path to the constraints API submit constraints endpoint.
pub const CONSTRAINTS_PATH: &str = "/constraints/v1/builder/constraints";
/// The path to the constraints API collect constraints endpoint.
pub const CONSTRAINTS_COLLECT_PATH: &str = "/constraints/v1/builder/constraints_collect";

pub async fn run_constraints_collector(port: u16, cb_url:String) {
    let collector = Arc::new(ConstraintsCollector::new(cb_url.parse().expect("Valid URL")));

    let (tx, mut _rx) = broadcast::channel::<String>(16);

    let router = Router::new()
      .route("/ws", any(ws_handler))
      .route("/", get(description))
      .route(STATUS_PATH, get(status))
      .route(
          REGISTER_VALIDATORS_PATH,
          post(register_validators),
      )
      .route(GET_HEADER_PATH, get(get_header))
      .route(GET_PAYLOAD_PATH, post(get_payload))
      .route(CONSTRAINTS_PATH, post(send_constraints))
      .route(CONSTRAINTS_COLLECT_PATH, post(collect_constraints))
      .layer(Extension(tx))
      // .layer(
      //   TraceLayer::new_for_http()
      //       .make_span_with(DefaultMakeSpan::default().include_headers(true)),
      // )
      .with_state(collector);

    //TODO: replace a port
    let addr: SocketAddr = SocketAddr::from(([0,0,0,0], port));

    //TODO: replace a listening port as a builder
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    tracing::info!("collector is running on .. {}", addr);
    axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
}

async fn description() -> Html<& 'static str> {
  tracing::debug!("description");
  Html("This is an endpoint to interact with constraints collector")
}

async fn status(State(collector):State<Arc<ConstraintsCollector>>) -> StatusCode {
  tracing::debug!("handling STATUS request");

  let status = match collector.status().await {
      Ok(status) => status,
      Err(err) => {
          tracing::error!(%err, "Failed in getting status from all builders");
          StatusCode::INTERNAL_SERVER_ERROR
      }
  };
  status
}

async fn get_header( State(collector):State<Arc<ConstraintsCollector>>, Path(params): Path<GetHeaderParams>) -> Result<Json<VersionedValue<SignedBuilderBid>>, CollectorError> {
  tracing::debug!("handling GET_HEADER request");
  match collector.get_header_with_proofs(params).await {
      Ok(header) => 
          return Ok(Json(header))
      ,
      Err(err) => {
          tracing::error!("Failed in getting header with proof from all builders");
          return Err(err);
      }
  }
}

async fn get_payload( State(collector): State<Arc<ConstraintsCollector>>, Json(signed_blinded_block):Json<SignedBlindedBeaconBlock>) -> Result<Json<GetPayloadResponse>, CollectorError> {
  tracing::debug!("handling GET_PAYLOAD request");

  match collector
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

async fn register_validators( State(collector):State<Arc<ConstraintsCollector>>, Json(registors):Json<Vec<SignedValidatorRegistration>>) -> Result<StatusCode, CollectorError> {
  tracing::debug!("handling REGISTER_VALIDATORS_REQUEST");
  match collector.register_validators(registors).await.map(|_| StatusCode::OK).map_err(|e| {
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

async fn send_constraints( State(collector):State<Arc<ConstraintsCollector>>, Json(constraints):Json<Vec<SignedConstraints>>) -> Result<StatusCode, CollectorError> {

  let slot = constraints[0].message.slot;

  match collector.send_constraints(slot).await.map(|_| StatusCode::OK).map_err(|e| {
    tracing::error!(%e, "Failed to send constraints to commit-boost");
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

async fn collect_constraints( State(collector):State<Arc<ConstraintsCollector>>, tx:Extension<broadcast::Sender<String>>,Json(constraints):Json<Vec<SignedConstraints>>) -> Result<StatusCode, CollectorError> {
  
  tracing::debug!(?constraints, "received constraints");

  let mut constraints_store = collector.collected_constraints.write();

  let slot: u64 = constraints[0].message.slot;
  let mut merged = Vec::new();
  if let Some(cs) = constraints_store.get_mut(&slot) {
      merged.append(&mut cs.clone());
      cs.append(&mut constraints.clone());
  } else {
    constraints_store.insert(slot, constraints.to_vec());
  }

  merged.append(&mut constraints.clone());

  tracing::debug!(?merged, "total constraints");

  let json_text = serde_json::to_string(&merged).unwrap();
 
  let _ = tx.send(json_text);

  Ok(StatusCode::OK)
}

#[axum::debug_handler]
async fn ws_handler(
  ws: WebSocketUpgrade,
  Extension(tx): Extension<broadcast::Sender<String>>,
) ->  impl IntoResponse {
  tracing::debug!(" ws connected.");
  ws.on_upgrade(move |socket| handle_socket(socket, tx))
}

async fn handle_socket(mut socket: WebSocket, tx: broadcast::Sender<String>) {
  let mut rx = tx.subscribe();

  loop {
    tokio::select! {
        // Handle broadcasting messages to the client
        broadcast_msg = rx.recv() => {
            if let Ok(merged_list_json) = broadcast_msg {
                if socket.send(Message::Text(merged_list_json)).await.is_err() {
                    break; // Client disconnected
                }
            }
        }
    }
  }
  
}