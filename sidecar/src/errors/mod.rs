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
pub enum CommitBoostError {
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
    #[error("Failed to send delegating request {0:?}")]
    FailedDelegating(ErrorResponse),
    #[error("Failed to send revoking request {0:?}")]
    FailedRevoking(ErrorResponse),
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
    #[error("Locally-built payload does not match expected signed header")]
    LocalPayloadIntegrity(#[from] super::constraints::LocalPayloadIntegrityError),
    
}

impl IntoResponse for CommitBoostError {
    fn into_response(self) -> Response {
        match self {
            CommitBoostError::FailedRegisteringValidators(error) => {
                (StatusCode::from_u16(error.code).unwrap(), Json(error)).into_response()
            }
            CommitBoostError::FailedGettingHeader(error) => {
                (StatusCode::from_u16(error.code).unwrap(), Json(error)).into_response()
            }
            CommitBoostError::FailedGettingPayload(error) => {
                (StatusCode::from_u16(error.code).unwrap(), Json(error)).into_response()
            }
            CommitBoostError::FailedSubmittingConstraints(error) => {
                (StatusCode::from_u16(error.code).unwrap(), Json(error)).into_response()
            }
            CommitBoostError::FailedDelegating(error) => {
                (StatusCode::from_u16(error.code).unwrap(), Json(error)).into_response()
            }
            CommitBoostError::FailedRevoking(error) => {
                (StatusCode::from_u16(error.code).unwrap(), Json(error)).into_response()
            }
            CommitBoostError::AxumError(err) => {
                (StatusCode::BAD_REQUEST, err.to_string()).into_response()
            }
            CommitBoostError::JsonError(err) => {
                (StatusCode::BAD_REQUEST, err.to_string()).into_response()
            }
            CommitBoostError::FailedToFetchLocalPayload(_) => {
                (StatusCode::NO_CONTENT, self.to_string()).into_response()
            }
            CommitBoostError::ReqwestError(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                StatusCode::INTERNAL_SERVER_ERROR
                    .canonical_reason()
                    .unwrap(),
            )
                .into_response(),
            CommitBoostError::Timeout(_) => (
                StatusCode::GATEWAY_TIMEOUT,
                StatusCode::GATEWAY_TIMEOUT.canonical_reason().unwrap(),
            )
                .into_response(),
            CommitBoostError::InvalidFork(err) => {
                (StatusCode::BAD_REQUEST, Json(err)).into_response()
            }
            CommitBoostError::Generic(err) => {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response()
            }
            CommitBoostError::LocalPayloadIntegrity(local_payload_integrity_error) => {
                (StatusCode::BAD_REQUEST, local_payload_integrity_error.to_string()).into_response()
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