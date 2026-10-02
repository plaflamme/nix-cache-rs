use aws_sigv4::{http_request::SigningError, sign::v4::signing_params::BuildError};
use axum::response::IntoResponse;
use harmonia_store_nar_info::NarInfoParseError;
use harmonia_store_path::StorePathError;
use harmonia_utils_signature::ParseKeyError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("CF worker error: {0}")]
    WorkerError(#[from] worker::Error),
    #[error("Failed to build signing params: {0}")]
    BuildError(#[from] BuildError),
    #[error("Failed to sign: {0}")]
    SiningError(#[from] SigningError),
    #[error("Failed to parse signing key: {0}")]
    ParseKeyError(#[from] ParseKeyError),
    #[error("Failed to parse hash: {0}")]
    ParseHashError(#[from] harmonia_utils_hash::fmt::ParseHashError),
    #[error("Http error: {0}")]
    HttpError(#[from] http::Error),
    #[error("Invalid narinfo: {0}")]
    NarInfoParseError(#[from] NarInfoParseError),
    #[error("Invalid store path or store path hash: {0}")]
    StorePathError(#[from] StorePathError),

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
            Error::NarInfoParseError { .. } => http::StatusCode::BAD_REQUEST,
            Error::StorePathError { .. } => http::StatusCode::BAD_REQUEST,
            _ => http::StatusCode::INTERNAL_SERVER_ERROR,
        };
        worker::console_warn!("{status}: {self}");
        status.into_response()
    }
}
