/// Nix bianry cache endpoints.
///
/// See https://fzakaria.github.io/nix-http-binary-cache-api-spec/
use std::str::FromStr;

use axum::{
    Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::{get, put},
};
use harmonia_store_path::StorePathHash;
use http::StatusCode;

use crate::{
    Compression, NixCacheApp,
    api::{DEFAULT_CACHE_NAME, extract::Method},
};

#[worker::send]
async fn get_narinfo(
    State(app): State<NixCacheApp>,
    Path(path): Path<String>,
    Method(method): Method,
) -> Result<axum::response::Response, crate::Error> {
    let Some((store_path_hash, "narinfo")) = path.split_once('.') else {
        return Ok(StatusCode::BAD_REQUEST.into_response());
    };
    let store_hash = StorePathHash::from_str(store_path_hash)?;
    let narinfo = app
        .store
        .get_narinfo(DEFAULT_CACHE_NAME, store_hash)
        .await?;

    let response = match narinfo {
        None => StatusCode::NOT_FOUND.into_response(),
        Some(narinfo) => {
            let narinfo_txt = crate::narinfo::render_narinfo_text(&narinfo);

            let res = axum::response::Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "text/x-nix-narinfo")
                .header("Content-Length", narinfo_txt.len());

            match method {
                http::Method::GET => res.body(axum::body::Body::from(narinfo_txt))?,
                http::Method::HEAD => res.body(axum::body::Body::empty())?,
                _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
            }
        }
    };
    Ok(response)
}

#[worker::send]
async fn put_narinfo(
    State(app): State<NixCacheApp>,
    Path(store_path_hash): Path<String>,
    narinfo_txt: String,
) -> Result<axum::response::Response, crate::Error> {
    let narinfo = crate::narinfo::parse_narinfo(&narinfo_txt)?;
    let store_path_hash = StorePathHash::from_str(&store_path_hash)?;
    if narinfo.path.hash() != store_path_hash {
        return Ok(StatusCode::BAD_REQUEST.into_response());
    }

    app.store.put_narinfo(DEFAULT_CACHE_NAME, &narinfo).await?;
    Ok(StatusCode::OK.into_response())
}

async fn presigned_nar_redirect(
    State(app): State<NixCacheApp>,
    Path(narfile): Path<String>,
    Method(method): Method,
) -> Result<axum::response::Response, crate::Error> {
    if let Some((nar_id, extension)) = narfile.split_once('.')
        && let Some(("nar", compression)) = extension.split_once('.') // TODO: handle no compression
        && let Ok(compression) = Compression::from_str(compression)
    {
        Ok(axum::response::Redirect::temporary(
            app.store
                .presigned_nar_url(DEFAULT_CACHE_NAME, nar_id, compression, method, &[], &[])?
                .as_str(),
        )
        .into_response())
    } else {
        Ok(StatusCode::BAD_REQUEST.into_response())
    }
}

pub fn router(state: NixCacheApp) -> axum::Router {
    // https://fzakaria.github.io/nix-http-binary-cache-api-spec
    Router::new()
        .route("/{store_hash}", get(get_narinfo))
        .route("/{store_hash}", put(put_narinfo))
        .route("/nar/{store_hash}", get(presigned_nar_redirect))
        .route("/nar/{store_hash}", put(presigned_nar_redirect))
        .with_state(state)
}
