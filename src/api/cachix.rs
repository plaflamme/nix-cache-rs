use std::collections::BTreeSet;

use super::{cache_info, narinfo_key, store_hash};
use crate::Compression;
use crate::NixCacheApp;

use axum::extract::OriginalUri;
use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use http::StatusCode;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GetCacheResponse {
    github_username: &'static str,
    is_public: bool,
    name: String,
    permission: String,
    preferred_compression_method: String,
    public_signing_keys: Vec<String>,
    uri: url::Url,
}

async fn get_cache(
    uri: OriginalUri,
    State(app): State<NixCacheApp>,
    Path(cache_name): Path<String>,
) -> Result<Json<GetCacheResponse>, crate::Error> {
    let mut uri = url::Url::parse(&uri.to_string()).expect("the original URI is a valid URL");
    uri.path_segments_mut()
        .expect("original uri can be base")
        .clear(); // We assume that if the client reached this endpoint using `https://whatever.com/api/v1/cache/foo`, then the cache is reachable at `https://whatever.com`

    Ok(Json(GetCacheResponse {
        github_username: "",
        is_public: false,
        name: cache_name,
        permission: "Write".to_string(),
        preferred_compression_method: Compression::Zstd.name().to_ascii_uppercase(),
        public_signing_keys: vec![app.signing_public_key.to_string()],
        uri,
    }))
}

#[worker::send]
async fn missing_narinfo(
    State(app): State<NixCacheApp>,
    Path(cache_name): Path<String>,
    Json(mut hashes): Json<BTreeSet<String>>,
) -> Result<Json<BTreeSet<String>>, crate::Error> {
    // NOTE: this approach doesn't scale well since we effectively have to list all narinfo objects in R2
    // But it was chosen to avoid introducing another dependency, like KVStore or D1.
    // Using `head` on each key is too slow

    let bucket = &app.store.bucket;
    let mut cursor = None;
    while !hashes.is_empty() {
        let list_objects = bucket.list().prefix(narinfo_key(&cache_name, ""));

        let objects = match cursor {
            Some(c) => list_objects.cursor(c),
            None => list_objects,
        }
        .execute()
        .await?;

        cursor = objects.cursor();

        for hash in objects.objects().into_iter().map(|o| {
            store_hash(&o.key())
                .expect("narinfo_key has a valid format")
                .to_string()
        }) {
            hashes.remove(&hash);
        }

        if !objects.truncated() {
            break;
        }
    }
    Ok(Json(hashes))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateMultipartUploadResponse {
    nar_id: Uuid,
    upload_id: String,
}

#[derive(Deserialize)]
#[allow(unused)]
struct CompressionParam {
    compression: Option<Compression>,
}

#[worker::send]
#[axum_macros::debug_handler]
async fn create_multipart_upload(
    Path(cache_name): Path<String>,
    State(app): State<NixCacheApp>,
    Query(param): Query<CompressionParam>,
) -> Result<Json<CreateMultipartUploadResponse>, crate::Error> {
    let compression = param.compression.unwrap_or(Compression::None);
    let (nar_id, upload_id) = app
        .store
        .create_nar_upload(&cache_name, compression)
        .await?;
    Ok(Json(CreateMultipartUploadResponse { nar_id, upload_id }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RetrievePreSignedUrlParameters {
    upload_id: String,
    part_number: u64,
}
#[derive(Deserialize)]
struct RetrievePreSignedUrlRequest {
    #[serde(rename = "contentMD5")]
    content_md5: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RetrievePreSignedUrlResponse {
    upload_url: String,
}

async fn retrieve_presigned_url(
    State(app): State<NixCacheApp>,
    Path((cache_name, nar_id)): Path<(String, Uuid)>,
    Query(params): Query<RetrievePreSignedUrlParameters>,
    Json(request): Json<RetrievePreSignedUrlRequest>,
) -> Result<Json<RetrievePreSignedUrlResponse>, crate::Error> {
    let upload_url = app.store.presigned_nar_url(
        &cache_name,
        &nar_id.to_string(),
        Compression::Zstd,
        http::Method::PUT,
        &[
            ("uploadId", &params.upload_id),
            ("partNumber", &params.part_number.to_string()),
        ],
        &[
            ("Content-Type", "application/octet-stream"),
            ("Content-MD5", request.content_md5.as_str()),
        ],
    )?;

    Ok(Json(RetrievePreSignedUrlResponse {
        upload_url: upload_url.to_string(),
    }))
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
#[allow(unused)]
pub(crate) struct NarInfoCreate {
    pub(crate) c_deriver: String,
    pub(crate) c_file_hash: String,
    pub(crate) c_file_size: u64,
    pub(crate) c_nar_hash: String,
    pub(crate) c_nar_size: u64,
    pub(crate) c_references: Vec<String>,
    pub(crate) c_sig: Option<String>,
    pub(crate) c_store_hash: String,
    pub(crate) c_store_suffix: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct CompletedPart {
    e_tag: String,
    part_number: u16,
}
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
#[allow(unused)]
struct CompleteMultipartUploadRequest {
    nar_info_create: NarInfoCreate,
    parts: Vec<CompletedPart>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompleteMultipartUploadParameterss {
    upload_id: String,
}

#[worker::send]
#[axum_macros::debug_handler]
async fn complete_multipart_upload(
    State(app): State<NixCacheApp>,
    Path((cache_name, nar_id)): Path<(String, Uuid)>,
    Query(params): Query<CompleteMultipartUploadParameterss>,
    Json(request): Json<CompleteMultipartUploadRequest>,
) -> Result<StatusCode, crate::Error> {
    app.store
        .complete_nar_upload(
            &cache_name,
            nar_id,
            &params.upload_id,
            crate::narinfo::build_narinfo(&request.nar_info_create, &nar_id, Compression::Zstd)?,
            request.parts.into_iter().map(|part| {
                crate::store::Part(
                    part.part_number,
                    part.e_tag.trim_prefix('"').trim_suffix('"').to_string(),
                )
            }),
        )
        .await?;
    Ok(StatusCode::OK)
}

pub fn router(state: NixCacheApp) -> axum::Router {
    // cachix API
    // https://app.cachix.org/api/v1/
    Router::new()
        .route("/cache/{cache_name}", get(get_cache))
        .route("/cache/{cache_name}/nix-cache-info", get(cache_info))
        .route("/cache/{cache_name}/narinfo", post(missing_narinfo))
        .route(
            "/cache/{cache_name}/multipart-nar",
            post(create_multipart_upload),
        )
        .route(
            "/cache/{cache_name}/multipart-nar/{nar_id}",
            post(retrieve_presigned_url),
        )
        .route(
            "/cache/{cache_name}/multipart-nar/{nar_id}/complete",
            post(complete_multipart_upload),
        )
        .with_state(state)
}
