use std::{net::SocketAddr, sync::{Arc}};
use axum::{debug_handler, extract::State, http::StatusCode, routing::post, Json, Router};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{mpsc::{self, Sender, Receiver}, oneshot};

use crate::commitment::request::{CommitmentRequestError, CommitmentRequestEvent, CommitmentRequestHandler, PreconfRequest};

pub async fn run_commitment_rpc_server ( event_sender: mpsc::Sender<CommitmentRequestEvent>) {

  let handler = CommitmentRequestHandler::new(event_sender);

  let app = Router::new()
  .route("/api/v1/preconfirmation", post(handle_preconfirmation))
  .with_state(handler.clone());

  // let (close_tx, close_rx) = oneshot::channel();
  
  let addr = SocketAddr::from(([127,0,0,1], 4000));
  let listener = tokio::net::TcpListener::bind(addr)
  .await
  .unwrap();

  let server_handle = tokio::spawn(async {
    axum::serve(listener, app)
        .await
        .unwrap();
  });
  tracing::info!("listening on .. {}", addr);
  // tracing::info!("telling server to shutdown");
  // _ = close_tx.send(());

  // println!("waiting for server to gracefully shutdown");
  // _ = server_handle.await;
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