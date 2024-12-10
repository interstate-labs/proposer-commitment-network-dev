use alloy::eips::eip2718::Eip2718Error;
use serde::{ Serialize, Deserialize, Serializer};

use axum::{response::{IntoResponse, Response}, Json};
use reqwest::StatusCode;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    #[serde(serialize_with = "serialize_status_code")]
    code: u16,
    message: String,
}

impl ErrorResponse {
    pub fn new(code: u16, message: String) -> Self {
        Self { code, message }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CollectorError {
    #[error(transparent)]
    Decode(#[from] Eip2718Error),
    #[error("No validators could be registered: {0:?}")]
    FailedRegisteringValidators(ErrorResponse),
    #[error("Failed getting header: {0:?}")]
    FailedGettingHeader(ErrorResponse),
    #[error("Failed getting payload: {0:?}")]
    FailedGettingPayload(ErrorResponse),
    #[error("Failed submitting constraints: {0:?}")]
    FailedSubmittingConstraints(ErrorResponse),
    #[error("Failed to fetch local payload for slot {0}")]
    FailedToFetchLocalPayload(u64),
    #[error("Axum error: {0:?}")]
    AxumError(#[from] axum::Error),
    #[error("Json error: {0:?}")]
    JsonError(#[from] serde_json::Error),
    #[error("Reqwest error: {0:?}")]
    ReqwestError(#[from] reqwest::Error),
    #[error("API request timed out : {0:?}")]
    Timeout(#[from] tokio::time::error::Elapsed),
    #[error("Invalid fork: {0}")]
    InvalidFork(String),
    #[error("Generic error: {0}")]
    Generic(String),
}


impl IntoResponse for CollectorError {
    fn into_response(self) -> Response {
        match self {
            CollectorError::FailedRegisteringValidators(error) => {
                (StatusCode::from_u16(error.code).unwrap(), Json(error)).into_response()
            }
            CollectorError::FailedGettingHeader(error) => {
                (StatusCode::from_u16(error.code).unwrap(), Json(error)).into_response()
            }
            CollectorError::FailedGettingPayload(error) => {
                (StatusCode::from_u16(error.code).unwrap(), Json(error)).into_response()
            }
            CollectorError::FailedSubmittingConstraints(error) => {
                (StatusCode::from_u16(error.code).unwrap(), Json(error)).into_response()
            }
            CollectorError::AxumError(err) => {
                (StatusCode::BAD_REQUEST, err.to_string()).into_response()
            }
            CollectorError::JsonError(err) => {
                (StatusCode::BAD_REQUEST, err.to_string()).into_response()
            }
            CollectorError::FailedToFetchLocalPayload(_) => {
                (StatusCode::NO_CONTENT, self.to_string()).into_response()
            }
            CollectorError::ReqwestError(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                StatusCode::INTERNAL_SERVER_ERROR
                    .canonical_reason()
                    .unwrap(),
            )
                .into_response(),
            CollectorError::Timeout(_) => (
                StatusCode::GATEWAY_TIMEOUT,
                StatusCode::GATEWAY_TIMEOUT.canonical_reason().unwrap(),
            )
                .into_response(),
            CollectorError::InvalidFork(err) => {
                (StatusCode::BAD_REQUEST, Json(err)).into_response()
            }
            CollectorError::Generic(err) => {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response()
            }
            CollectorError::Decode(_eip2718_error) => {
                (StatusCode::INTERNAL_SERVER_ERROR, Json("EIP decoding error".to_string())).into_response()
            },
        }
    }
}

pub fn serialize_status_code<S>(value: &u16, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&value.to_string())
}