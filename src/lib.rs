//! # nix-cache-rs
#![feature(trim_prefix_suffix)]

mod cachix;
mod compression;
mod error;
mod extract;
mod narinfo;
mod r2_sig;
mod store;
mod time;

pub use compression::Compression;
pub use error::Error;

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

    fn cache_endpoint(&self) -> worker::Result<String> {
        Ok(self.env.var("cache_endpoint")?.to_string())
    }

    fn bucket(&self) -> worker::Result<worker::Bucket> {
        self.env.bucket("nix-cache-bucket")
    }

    fn bucket_name(&self) -> worker::Result<String> {
        Ok(self.env.var("bucket_name")?.to_string())
    }

    fn github_username(&self) -> worker::Result<String> {
        Ok(self.env.var("github_username")?.to_string())
    }

    fn r2_credentials(&self) -> worker::Result<aws_credential_types::Credentials> {
        Ok(aws_credential_types::Credentials::builder()
            .access_key_id(self.env.var("R2_ACCESS_KEY_ID")?.to_string())
            .secret_access_key(self.env.var("R2_SECRET_ACCESS_KEY")?.to_string())
            .provider_name("provider_name")
            .build())
    }

    fn r2_endpoint(&self) -> worker::Result<url::Url> {
        Ok(url::Url::parse(
            &self.env.secret("R2_ENDPOINT")?.to_string(),
        )?)
    }
}

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
}

#[worker::event(fetch)]
pub async fn fetch(
    req: HttpRequest,
    env: Env,
    _ctx: Context,
) -> worker::Result<axum::http::Response<axum::body::Body>> {
    Ok(router(env).call(req).await?)
}
