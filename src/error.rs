use aws_sigv4::{http_request::SigningError, sign::v4::signing_params::BuildError};
use axum::response::IntoResponse;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("CF worker error: {0}")]
    WorkerError(#[from] worker::Error),
    #[error("Failed to build signing params: {0}")]
    BuildError(#[from] BuildError),
    #[error("Failed to sign: {0}")]
    SiningError(#[from] SigningError),
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        (http::StatusCode::INTERNAL_SERVER_ERROR, self.to_string()).into_response()
    }
}
