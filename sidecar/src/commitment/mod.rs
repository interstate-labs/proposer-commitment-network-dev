pub mod request;
use std::{net::SocketAddr, sync::{Arc}};
use axum::{debug_handler, extract::State, http::StatusCode, routing::post, Json, Router};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{mpsc::{self, Sender, Receiver}, oneshot};

use crate::commitment::request::{CommitmentRequestError, CommitmentRequestEvent, CommitmentRequestHandler, PreconfRequest};
use crate::config::Config;

pub async fn run_commitment_rpc_server ( event_sender: mpsc::Sender<CommitmentRequestEvent>, config: &Config) {

  let handler = CommitmentRequestHandler::new(event_sender);

  let app = Router::new()
  .route("/api/v1/preconfirmation", post(handle_preconfirmation))
  .with_state(handler.clone());
  
  let addr: SocketAddr = SocketAddr::from(([0,0,0,0], config.commitment_port));
  let listener = tokio::net::TcpListener::bind(addr)
  .await
  .unwrap();

  tokio::spawn(async {
    axum::serve(listener, app)
        .await
        .unwrap();
  });
  tracing::info!("commitment RPC server is listening on .. {}", addr);
}

#[debug_handler]
async fn handle_preconfirmation (State(handler):State<Arc<CommitmentRequestHandler>>, Json(body):Json<PreconfRequest>) -> Result<Json<PreconfResponse>, CommitmentRequestError>{
  match handler.handle_commitment_request(&body).await {
    Ok(_) => {
      let response = PreconfResponse {
        ok: true
      };
    
      return Ok(Json(response))
    },
    Err(e)=> return Err(e)
  };

}

#[derive(Serialize)]
pub struct  PreconfResponse {
  pub ok: bool
}

impl axum::response::IntoResponse for CommitmentRequestError {
  fn into_response(self) -> axum::response::Response {
     match self {
      CommitmentRequestError::Custom(err) => {
          (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
      }
      CommitmentRequestError::Parse(err) => {
          (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
      }
     }
  }
}