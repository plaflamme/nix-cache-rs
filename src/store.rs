use std::str::FromStr;

use axum::{
    Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::{get, put},
};
use http::StatusCode;

use crate::{NixCacheApp, cachix::bucket_key, extract::Method, r2_sig};

#[worker::send]
#[axum_macros::debug_handler]
async fn get_narinfo(
    State(app): State<NixCacheApp>,
    Path(store_hash): Path<String>,
    Method(method): Method,
) -> Result<axum::response::Response, crate::Error> {
    let bucket = app.bucket()?;
    let cache_name = "default"; // TODO: extract from worker URI
    let object = bucket
        .get(format!("{cache_name}/{store_hash}.narinfo"))
        .execute()
        .await?;

    let response = match object {
        None => axum::response::Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(axum::body::Body::empty())?,
        Some(obj) => {
            if let Some(body) = obj.body() {
                let res = axum::response::Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Length", obj.size());
                let res = if let Some(ct) = obj.http_metadata().content_type.as_ref() {
                    res.header("Content-Type", ct.clone())
                } else {
                    res
                };
                match method {
                    http::Method::GET => {
                        // TODO: stream the body instead, but this file is small, so it's fine
                        res.body(axum::body::Body::from(body.text().await?))?
                    }
                    http::Method::HEAD => res.body(axum::body::Body::empty())?,
                    _ => unreachable!(),
                }
            } else {
                axum::response::Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(axum::body::Body::empty())?
            }
        }
    };
    Ok(response)
}

async fn read(
    State(app): State<NixCacheApp>,
    Path(path): Path<String>,
    Method(method): Method,
) -> Result<axum::response::Response, crate::Error> {
    if let Some((store_path, "narinfo")) = path.split_once('.') {
        get_narinfo(State(app), Path(store_path.to_string()), Method(method)).await
    } else {
        Ok(axum::response::Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(axum::body::Body::from("Not implemented"))?)
    }
}

async fn get_nar(
    State(app): State<NixCacheApp>,
    Path(store_path): Path<String>,
) -> Result<axum::response::Response, crate::Error> {
    if let Some((nar_id, "nar.zst")) = store_path.split_once('.')
        && let Ok(nar_id) = uuid::Uuid::from_str(nar_id)
    {
        let mut download_url = app.r2_endpoint()?;
        download_url
            .path_segments_mut()
            .expect("url can be base")
            .push(&app.bucket_name()?)
            .extend(bucket_key("default", &nar_id).split('/')); // TODO: cache name from hostname

        // https://github.com/cachix/cachix/blob/5ecbf73e1e742f527c0d970bef0a4c0d359a5ea7/cachix/src/Cachix/Client/Push/S3.hs#L108-L116
        crate::r2_sig::sign_request(&app, &mut download_url, http::Method::GET, &[])?;

        Ok(axum::response::Redirect::temporary(download_url.as_str()).into_response())
    } else {
        Ok(axum::response::Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(axum::body::Body::empty())?)
    }
}

async fn put_nar(
    State(app): State<NixCacheApp>,
    Path(store_path): Path<String>,
) -> Result<axum::response::Redirect, crate::Error> {
    let mut upload_url = app.r2_endpoint()?;
    upload_url
        .path_segments_mut()
        .expect("url can be base")
        .push(&app.bucket_name()?)
        .push("default")
        .push("nar")
        .push(&store_path);

    r2_sig::sign_request(&app, &mut upload_url, http::Method::PUT, &[])?;
    Ok(axum::response::Redirect::temporary(upload_url.as_str()))
}

async fn put_narinfo(
    State(app): State<NixCacheApp>,
    Path(store_path): Path<String>,
) -> Result<axum::response::Response, crate::Error> {
    if let Some((_, "narinfo")) = store_path.split_once('.') {
        let mut upload_url = app.r2_endpoint()?;
        upload_url
            .path_segments_mut()
            .expect("url can be base")
            .push(&app.bucket_name()?)
            .push("default")
            .push(&store_path);
        r2_sig::sign_request(&app, &mut upload_url, http::Method::PUT, &[])?;
        Ok(axum::response::Redirect::temporary(upload_url.as_str()).into_response())
    } else {
        Ok(axum::response::Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(axum::body::Body::empty())?)
    }
}

pub fn router(state: NixCacheApp) -> axum::Router {
    // https://fzakaria.github.io/nix-http-binary-cache-api-spec
    Router::new()
        .route("/{store_hash}", get(read))
        .route("/{store_hash}", put(put_narinfo))
        .route("/nar/{store_hash}", get(get_nar))
        .route("/nar/{store_hash}", put(put_nar))
        .with_state(state)
}
