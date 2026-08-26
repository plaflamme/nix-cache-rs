use axum::{
    Router,
    extract::{Path, State},
    routing::get,
};
use http::StatusCode;

use crate::NixCacheApp;

#[worker::send]
#[axum_macros::debug_handler]
async fn get_narinfo(
    State(app): State<NixCacheApp>,
    Path(store_hash): Path<String>,
) -> Result<axum::response::Response, crate::Error> {
    let bucket = app.bucket()?;
    let cache_name = "default"; // TODO: extract from worker URI
    let object = bucket
        .get(format!("{cache_name}/nix/store/{store_hash}.narinfo"))
        .execute()
        .await?;

    let response = match object {
        None => axum::response::Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(axum::body::Body::empty())?,
        Some(obj) => {
            let res = axum::response::Response::builder();
            let res = if let Some(ct) = obj.http_metadata().content_type.as_ref() {
                res.header("Content-Type", ct.clone())
            } else {
                res
            };
            res.status(StatusCode::OK)
                .body(axum::body::Body::from(obj.body().unwrap().text().await?))? // TODO: stream the body instead, but this file is small, so it's fine
        }
    };
    Ok(response)
}

async fn read(
    State(app): State<NixCacheApp>,
    Path(path): Path<String>,
) -> Result<axum::response::Response, crate::Error> {
    if let Some((store_path, "narinfo")) = path.split_once('.') {
        get_narinfo(State(app), Path(store_path.to_string())).await
    } else {
        unimplemented!()
    }
}

pub fn router(state: NixCacheApp) -> axum::Router {
    // https://fzakaria.github.io/nix-http-binary-cache-api-spec
    Router::new()
        .route("/{store_hash}", get(read))
        .with_state(state)
}
