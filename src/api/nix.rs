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

use super::narfile_key;
use crate::{
    Compression, NixCacheApp,
    api::{DEFAULT_CACHE_NAME, extract::Method},
    r2_sig,
};

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

async fn get_nar(
    State(app): State<NixCacheApp>,
    Path(store_path): Path<String>,
    Method(method): Method,
) -> Result<axum::response::Response, crate::Error> {
    if let Some((nar_hash, extension)) = store_path.split_once('.')
        && let Some(("nar", compression)) = extension.split_once('.') // TODO: handle no compression
        && let Ok(compression) = Compression::from_str(compression)
    {
        let mut download_url = app.r2_endpoint.clone();
        download_url
            .path_segments_mut()
            .expect("url can be base")
            .push(&app.bucket_name)
            .extend(narfile_key(DEFAULT_CACHE_NAME, nar_hash, compression).split('/'));

        // https://github.com/cachix/cachix/blob/5ecbf73e1e742f527c0d970bef0a4c0d359a5ea7/cachix/src/Cachix/Client/Push/S3.hs#L108-L116
        crate::r2_sig::sign_request(&app, &mut download_url, method, &[])?;

        Ok(axum::response::Redirect::temporary(download_url.as_str()).into_response())
    } else {
        Ok(axum::response::Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(axum::body::Body::empty())?)
    }
}

async fn put_nar(
    State(app): State<NixCacheApp>,
    Path(narfile): Path<String>,
) -> Result<axum::response::Redirect, crate::Error> {
    let mut upload_url = app.r2_endpoint.clone();
    upload_url
        .path_segments_mut()
        .expect("url can be base")
        .push(&app.bucket_name)
        .push(DEFAULT_CACHE_NAME)
        .push("nar")
        .push(&narfile);

    r2_sig::sign_request(&app, &mut upload_url, http::Method::PUT, &[])?;
    Ok(axum::response::Redirect::temporary(upload_url.as_str()))
}

pub fn router(state: NixCacheApp) -> axum::Router {
    // https://fzakaria.github.io/nix-http-binary-cache-api-spec
    Router::new()
        .route("/{store_hash}", get(get_narinfo))
        .route("/{store_hash}", put(put_narinfo))
        .route("/nar/{store_hash}", get(get_nar))
        .route("/nar/{store_hash}", put(put_nar))
        .with_state(state)
}
