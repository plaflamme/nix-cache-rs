use aws_sigv4::{http_request::SigningError, sign::v4::signing_params::BuildError};
use axum::response::IntoResponse;
use thiserror::Error;

use crate::narinfo::NarInfoError;

#[derive(Error, Debug)]
pub enum Error {
    #[error("CF worker error: {0}")]
    WorkerError(#[from] worker::Error),
    #[error("Failed to build signing params: {0}")]
    BuildError(#[from] BuildError),
    #[error("Failed to sign: {0}")]
    SiningError(#[from] SigningError),
    #[error("Invalid narinfo: {0}")]
    NarInfoError(#[from] NarInfoError),
    /// A client-supplied value failed validation. Rendered as HTTP 400.
    #[error("validation error: {field}: {message}")]
    Validation {
        field: &'static str,
        message: String,
    },
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        let status = match &self {
            Error::Validation { .. } => http::StatusCode::BAD_REQUEST,
            Error::NarInfoError { .. } => http::StatusCode::BAD_REQUEST,
            _ => http::StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, self.to_string()).into_response()
    }
}
