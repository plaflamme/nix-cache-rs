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
use harmonia_utils_hash::fmt::Base32;
use http::StatusCode;
use worker::HttpMetadata;

use super::{narfile_key, narinfo_key};
use crate::{
    Compression, NixCacheApp,
    api::{DEFAULT_CACHE_NAME, extract::Method},
    r2_sig,
};

#[worker::send]
#[axum_macros::debug_handler]
async fn get_narinfo(
    State(app): State<NixCacheApp>,
    Path(store_hash): Path<String>,
    Method(method): Method,
) -> Result<axum::response::Response, crate::Error> {
    let bucket = app.bucket;
    let cache_name = DEFAULT_CACHE_NAME;
    let object = bucket
        .get(narinfo_key(cache_name, &store_hash))
        .execute()
        .await?;

    let response = match object {
        None => axum::response::Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(axum::body::Body::empty())?,
        Some(obj) => {
            if let Some(body) = obj.body() {
                // NOTE: we cannot set Content-Length because we add a signature on read
                // TODO: is the signature length constant? If so, we can know how much more bytes we'll be adding to the object and can compute its resulting size
                let res = axum::response::Response::builder().status(StatusCode::OK);
                let res = if let Some(ct) = obj.http_metadata().content_type.as_ref() {
                    res.header("Content-Type", ct.clone())
                } else {
                    res
                };
                match method {
                    http::Method::GET => {
                        // Sign on read instead of write so we can more easily change keys
                        let narinfo_txt = body.text().await?;
                        let mut narinfo = crate::narinfo::parse_narinfo(&narinfo_txt)?;
                        crate::narinfo::sign_narinfo(&mut narinfo, &app.signing_secret_key);
                        let narinfo_txt = crate::narinfo::render_narinfo_text(&narinfo);
                        res.body(axum::body::Body::from(narinfo_txt))?
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

#[worker::send]
async fn put_narinfo(
    State(app): State<NixCacheApp>,
    Path(store_path): Path<String>,
    narinfo_txt: String,
) -> Result<axum::response::Response, crate::Error> {
    if let Some((store_hash, "narinfo")) = store_path.split_once('.') {
        let bucket = app.bucket;
        let narinfo = crate::narinfo::parse_narinfo(&narinfo_txt)?;
        if let Some(nar_hash) = narinfo.info.download_hash {
            let compression = narinfo
                .info
                .compression
                .map(|c| Compression::from_str(&c))
                .unwrap_or(Ok(Compression::None))?;

            let narfile_hash = Base32::from_hash(nar_hash).bare().to_string();
            let narfile_key = narfile_key(DEFAULT_CACHE_NAME, &narfile_hash, compression);
            let nar_url = narfile_key.trim_prefix(DEFAULT_CACHE_NAME).trim_prefix("/"); // TODO: this is stupid

            let narinfo_url = narinfo.info.url.ok_or(crate::Error::Validation {
                field: "URL",
                message: "missing URL".to_string(),
            })?;
            if narinfo_url != nar_url {
                return Err(crate::Error::Validation {
                    field: "URL",
                    message: format!("expected {nar_url}, got {narinfo_url}"),
                });
            }

            let narfile = bucket
                .head(narfile_key)
                .await?
                .ok_or(crate::Error::Validation {
                    field: "nar",
                    message: "nar file not in store".to_string(),
                })?;

            let filesize = narinfo.info.download_size.ok_or(crate::Error::Validation {
                field: "FileSize",
                message: "missing".to_string(),
            })?;

            if narfile.size() != filesize {
                return Err(crate::Error::Validation {
                    field: "FileSize",
                    message: format!("expected {}, got {filesize}", narfile.size()),
                });
            }
        }

        bucket
            .put(narinfo_key(DEFAULT_CACHE_NAME, store_hash), narinfo_txt)
            .http_metadata(HttpMetadata {
                content_type: Some("text/x-nix-narinfo".to_string()),
                ..Default::default()
            })
            .execute()
            .await?;

        Ok(StatusCode::OK.into_response())
    } else {
        Ok(axum::response::Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(axum::body::Body::empty())?)
    }
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
        .route("/{store_hash}", get(read))
        .route("/{store_hash}", put(put_narinfo))
        .route("/nar/{store_hash}", get(get_nar))
        .route("/nar/{store_hash}", put(put_nar))
        .with_state(state)
}
