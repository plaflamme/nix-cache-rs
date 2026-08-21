//! # nix-cache-rs

mod cachix;
use tower_service::Service;

use worker::{Context, Env, HttpRequest};

#[derive(Clone)]
struct NixCacheApp {
    env: Env,
}

impl NixCacheApp {
    fn new(env: Env) -> Self {
        Self { env }
    }

    fn bucket(&self, _cache_name: &str) -> worker::Bucket {
        self.env.bucket("nix_cache_test").unwrap() // TODO
    }

    fn r2_credentials(&self) -> worker::Result<aws_credential_types::Credentials> {
        Ok(aws_credential_types::Credentials::builder()
            .access_key_id(self.env.var("R2_ACCESS_KEY_ID")?.to_string())
            .secret_access_key(self.env.var("R2_SECRET_ACCESS_KEY")?.to_string())
            .build())
    }

    fn r2_endpoint(&self) -> String {
        let var = self.env.var("R2_ENDPOINT").unwrap(); // TODO
        var.to_string()
    }
}

#[axum_macros::debug_handler]
async fn cache_info() -> &'static str {
    "StoreDir: /nix/store\nWantMassQuery: 1\nPriority: 40\n"
}

fn router(env: Env) -> axum::Router {
    let state = NixCacheApp::new(env);
    axum::Router::new()
        // binary cache
        .route("/", axum::routing::get(cache_info))
        .route("/nix-cache-info", axum::routing::get(cache_info))
        .with_state(state.clone())
        .nest("/api/v1", cachix::router(state))
}

#[worker::event(fetch)]
pub async fn fetch(
    req: HttpRequest,
    env: Env,
    _ctx: Context,
) -> worker::Result<axum::http::Response<axum::body::Body>> {
    Ok(router(env).call(req).await?)
}
