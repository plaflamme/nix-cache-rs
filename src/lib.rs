//! # nix-cache-rs
#![feature(trim_prefix_suffix)]

mod app;
mod auth;
mod cachix;
mod compression;
mod error;
mod extract;
mod narinfo;
mod r2_sig;
mod store;
mod time;

pub use app::NixCacheApp;
pub use compression::Compression;
pub use error::Error;

use tower_service::Service;

use worker::{Context, Env, HttpRequest};

#[axum_macros::debug_handler]
async fn cache_info() -> &'static str {
    "StoreDir: /nix/store\nPriority: 40\n"
}

fn router(env: Env) -> axum::Router {
    let state = NixCacheApp::new(env);
    axum::Router::new()
        .route("/nix-cache-info", axum::routing::get(cache_info))
        .merge(store::router(state.clone()))
        .nest("/api/v1", cachix::router(state.clone()))
        .layer(axum::middleware::from_fn_with_state(
            state,
            auth::authenticate,
        ))
}

#[worker::event(fetch)]
pub async fn fetch(
    req: HttpRequest,
    env: Env,
    _ctx: Context,
) -> worker::Result<axum::http::Response<axum::body::Body>> {
    Ok(router(env).call(req).await?)
}
