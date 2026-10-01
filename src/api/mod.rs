use crate::{Compression, NixCacheApp};

mod auth;
mod cachix;
mod extract;
mod nix;

pub(crate) use cachix::NarInfoCreate;

fn narfile_key(cache_name: &str, nar_hash: &str, compression: Compression) -> String {
    format!("{cache_name}/nar/{nar_hash}.nar{}", compression.extension())
}

fn narinfo_key(cache_name: &str, store_hash: &str) -> String {
    format!("{cache_name}/narinfo/{store_hash}")
}

fn store_hash(narinfo_key: &str) -> Option<&str> {
    narinfo_key.split('/').next_back()
}

async fn cache_info() -> &'static str {
    "StoreDir: /nix/store\nPriority: 40\n"
}

pub fn router(state: NixCacheApp) -> axum::Router {
    axum::Router::new()
        .route("/nix-cache-info", axum::routing::get(cache_info))
        .merge(nix::router(state.clone()))
        .nest("/api/v1", cachix::router(state.clone()))
        .layer(axum::middleware::from_fn_with_state(
            state,
            auth::authenticate,
        ))
}
