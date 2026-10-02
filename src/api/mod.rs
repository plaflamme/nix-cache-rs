use crate::NixCacheApp;

mod auth;
mod cachix;
mod extract;
mod nix;

pub(crate) use cachix::NarInfoCreate;

// Cachix supports multiple cache names, we don't fully support this currently.
// The cachix-side is fine, but the nix-side can't select different caches at the moment
// To support this on the nix-side we either have to require a top-level path (not sure if that's supported) or a different hostname per cache (like cachix does)
const DEFAULT_CACHE_NAME: &str = "default";

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
