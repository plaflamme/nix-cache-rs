//! # nix-cache-rs
#![feature(trim_prefix_suffix)]

mod app;
mod api;
mod compression;
mod error;
mod narinfo;
mod r2_sig;
mod time;

pub use app::NixCacheApp;
use axum::response::IntoResponse;
pub use compression::Compression;
pub use error::Error;

use tower_service::Service;

use worker::{Context, Env, HttpRequest};


#[worker::event(fetch)]
pub async fn fetch(
    req: HttpRequest,
    env: Env,
    _ctx: Context,
) -> worker::Result<axum::http::Response<axum::body::Body>> {
    match NixCacheApp::try_from(env) {
        Ok(state) => Ok(api::router(state).call(req).await?),
        Err(e) => Ok(e.into_response()),
    }
}
