pub mod request;
use axum::{
    debug_handler,
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use axum_client_ip::SecureClientIpSource;
use serde::Serialize;
use std::{net::SocketAddr, sync::Arc, time::Instant};
use tokio::sync::mpsc;

use crate::config::Config;
use crate::{
    commitment::request::{
        CommitmentRequestError, CommitmentRequestEvent, CommitmentRequestHandler, PreconfRequest,
    },
    metrics::ApiMetrics,
};

pub async fn run_commitment_rpc_server(
    event_sender: mpsc::Sender<CommitmentRequestEvent>,
    config: &Config,
) {
    let handler = CommitmentRequestHandler::new(
        event_sender,
        config.execution_api_url.clone(),
        config.gateway_contract,
    );

    let app = Router::new()
        .route("/api/v1/preconfirmation", post(handle_preconfirmation))
        .route_layer(middleware::from_fn(track_metrics))
        .layer(SecureClientIpSource::ConnectInfo.into_extension())
        .with_state(handler.clone());

    let addr: SocketAddr = SocketAddr::from(([0, 0, 0, 0], config.commitment_port));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    tokio::spawn(async {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    tracing::info!("commitment RPC server is listening on .. {}", addr);
}

#[debug_handler]
// async fn handle_preconfirmation (insecure_ip: InsecureClientIp, secure_ip: SecureClientIp, State(handler):State<Arc<CommitmentRequestHandler>>, Json(body):Json<PreconfRequest>) -> Result<Json<PreconfResponse>, CommitmentRequestError>{
async fn handle_preconfirmation(
    State(handler): State<Arc<CommitmentRequestHandler>>,
    Json(body): Json<PreconfRequest>,
) -> Result<Json<PreconfResponse>, CommitmentRequestError> {
    match handler.handle_commitment_request(&body).await {
        Ok(_) => {
            let response = PreconfResponse { ok: true };

            return Ok(Json(response));
        }
        Err(e) => return Err(e),
    };

    // let client_ip = insecure_ip.0.to_string();
    // match handler.verify_ip(client_ip.clone()).await{
    //   Ok(validity) => {
    //     if validity {
    //       match handler.handle_commitment_request(&body).await {
    //         Ok(_) => {
    //           let response = PreconfResponse {
    //             ok: true
    //           };

    //           return Ok(Json(response))
    //         },
    //         Err(e)=> return Err(e)
    //       };
    //     }else{
    //       tracing::warn!("Received preconf request from not allowed ip {}", client_ip.clone());
    //       return Err(CommitmentRequestError::NotAllowedIP(client_ip));
    //     }
    //   }
    //   Err(err) => {
    //     return Err(CommitmentRequestError::Custom(err.to_string()));
    //   }
    // }
}

#[derive(Serialize)]
pub struct PreconfResponse {
    pub ok: bool,
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
            CommitmentRequestError::NotAllowedIP(ip) => {
                (StatusCode::INTERNAL_SERVER_ERROR, ip).into_response()
            }
        }
    }
}

pub async fn track_metrics(req: Request, next: Next) -> impl IntoResponse {
    let path = req.uri().path().to_owned();
    let method = req.method().to_string();

    let start = Instant::now();
    let response = next.run(req).await;
    let latency = start.elapsed();
    let status = response.status().as_u16().to_string();

    ApiMetrics::observe_http_request(latency, method, path, status);

    response
}
