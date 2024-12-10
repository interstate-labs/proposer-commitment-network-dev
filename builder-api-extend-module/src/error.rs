use axum::{response::{IntoResponse, Response}, Json};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize, Serializer};

/// A response object for errors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    #[serde(serialize_with = "serialize_status_code")]
    code: u16,
    message: String,
}

#[derive(Debug, thiserror::Error)]
#[allow(missing_docs)]
#[non_exhaustive]
pub enum BuilderApiError {
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
    #[error("Failed in joinning handlers in extender in request {0}")]
    FailedJoinningInExtender(String),
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

impl IntoResponse for BuilderApiError {
    fn into_response(self) -> Response {
        match self {
            BuilderApiError::FailedRegisteringValidators(error) => {
                (StatusCode::from_u16(error.code).unwrap(), Json(error)).into_response()
            }
            BuilderApiError::FailedGettingHeader(error) => {
                (StatusCode::from_u16(error.code).unwrap(), Json(error)).into_response()
            }
            BuilderApiError::FailedGettingPayload(error) => {
                (StatusCode::from_u16(error.code).unwrap(), Json(error)).into_response()
            }
            BuilderApiError::FailedSubmittingConstraints(error) => {
                (StatusCode::from_u16(error.code).unwrap(), Json(error)).into_response()
            }
            BuilderApiError::AxumError(err) => {
                (StatusCode::BAD_REQUEST, err.to_string()).into_response()
            }
            BuilderApiError::JsonError(err) => {
                (StatusCode::BAD_REQUEST, err.to_string()).into_response()
            }
            BuilderApiError::FailedToFetchLocalPayload(_) => {
                (StatusCode::NO_CONTENT, self.to_string()).into_response()
            }
            BuilderApiError::ReqwestError(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                StatusCode::INTERNAL_SERVER_ERROR
                    .canonical_reason()
                    .unwrap(),
            )
                .into_response(),
            BuilderApiError::Timeout(_) => (
                StatusCode::GATEWAY_TIMEOUT,
                StatusCode::GATEWAY_TIMEOUT.canonical_reason().unwrap(),
            )
                .into_response(),
            BuilderApiError::InvalidFork(err) => {
                (StatusCode::BAD_REQUEST, Json(err)).into_response()
            }
            BuilderApiError::Generic(err) => {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response()
            }
            BuilderApiError::FailedJoinningInExtender(err) => {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response()
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