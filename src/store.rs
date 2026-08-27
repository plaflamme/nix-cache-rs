use std::str::FromStr;

use aws_sigv4::http_request::{SignableBody, SignableRequest};
use axum::{
    Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::get,
};
use http::StatusCode;

use crate::{NixCacheApp, cachix::bucket_key, extract::Method};

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
        .get(format!("{cache_name}/nix/store/{store_hash}.narinfo"))
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
        let download_url = format!(
            "{}/{}/{}?",
            app.r2_endpoint()?,
            app.bucket_name()?,
            bucket_key("default", &nar_id), // TODO: cache name from hostname
        );

        // https://github.com/cachix/cachix/blob/5ecbf73e1e742f527c0d970bef0a4c0d359a5ea7/cachix/src/Cachix/Client/Push/S3.hs#L108-L116
        let request = SignableRequest::new(
            "GET",
            &download_url,
            std::iter::empty(),
            SignableBody::UnsignedPayload,
        )?;

        let result = crate::r2_sig::sign_request(&app, request)?;

        let signed_params = result.output().params();
        let query_params = signed_params
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<String>>()
            .join("&");

        let download_url = format!("{download_url}&{query_params}");

        Ok(axum::response::Redirect::temporary(&download_url).into_response())
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
        .route("/nar/{store_hash}", get(get_nar))
        .with_state(state)
}
