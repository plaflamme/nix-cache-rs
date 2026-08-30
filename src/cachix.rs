use crate::Compression;
use crate::NixCacheApp;
use crate::cache_info;

use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use http::StatusCode;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;
use worker::HttpMetadata;
use worker::UploadedPart;

pub fn narfile_key(cache_name: &str, nar_hash: &str, compression: Compression) -> String {
    match compression {
        Compression::None => format!("{cache_name}/nar/{nar_hash}.nar"),
        _ => format!(
            "{cache_name}/nar/{nar_hash}.nar.{}",
            compression.extension()
        ),
    }
}

pub fn narinfo_key(cache_name: &str, store_hash: &str) -> String {
    format!("{cache_name}/{store_hash}.narinfo")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GetCacheResponse {
    github_username: String,
    is_public: bool,
    name: String,
    permission: String,
    preferred_compression_method: String,
    public_signing_keys: Vec<String>,
    uri: String,
}

async fn get_cache(
    Path(name): Path<String>,
    State(app): State<NixCacheApp>,
) -> Json<GetCacheResponse> {
    Json(GetCacheResponse {
        github_username: app.github_username().unwrap_or("".to_string()),
        is_public: true,
        name,
        permission: "Write".to_string(),
        preferred_compression_method: Compression::Zstd.name().to_ascii_uppercase(),
        public_signing_keys: Vec::new(),
        uri: app.cache_endpoint().unwrap().to_string(),
    })
}

#[worker::send]
async fn missing_narinfo(
    State(app): State<NixCacheApp>,
    Path(name): Path<String>,
    Json(hashes): Json<Vec<String>>,
) -> Result<Json<Vec<String>>, crate::Error> {
    let bucket = app.bucket()?;
    let mut missing_hashes = Vec::new();
    for store_hash in hashes {
        let narinfo_object = bucket.head(narinfo_key(&name, &store_hash)).await?;
        if narinfo_object.is_none() {
            missing_hashes.push(store_hash);
        }
    }
    Ok(Json(missing_hashes))
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
    Path(name): Path<String>,
    State(app): State<NixCacheApp>,
    Query(param): Query<CompressionParam>,
) -> Result<Json<CreateMultipartUploadResponse>, crate::Error> {
    let compression = param.compression.unwrap_or(Compression::None);
    let bucket = app.bucket()?;
    let nar_id = Uuid::new_v4();
    let metadata = HttpMetadata {
        content_type: Some("application/x-nix-nar".to_string()),
        ..Default::default()
    };
    let multipart_upload = bucket
        .create_multipart_upload(narfile_key(&name, &nar_id.to_string(), compression))
        .http_metadata(metadata)
        .execute()
        .await?;
    let upload_id = multipart_upload.upload_id().await;
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
    Path((name, nar_id)): Path<(String, Uuid)>,
    Query(params): Query<RetrievePreSignedUrlParameters>,
    Json(request): Json<RetrievePreSignedUrlRequest>,
) -> Result<Json<RetrievePreSignedUrlResponse>, crate::Error> {
    let mut upload_url = app.r2_endpoint()?;
    upload_url
        .path_segments_mut()
        .expect("url can be base")
        .push(&app.bucket_name()?)
        .extend(narfile_key(&name, &nar_id.to_string(), Compression::Zstd).split('/')); // TODO: how do we know what compression is used?
    upload_url
        .query_pairs_mut()
        .append_pair("uploadId", &params.upload_id)
        .append_pair("partNumber", &params.part_number.to_string())
        .finish();

    // https://github.com/cachix/cachix/blob/5ecbf73e1e742f527c0d970bef0a4c0d359a5ea7/cachix/src/Cachix/Client/Push/S3.hs#L108-L116
    crate::r2_sig::sign_request(
        &app,
        &mut upload_url,
        http::Method::PUT,
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
    Path((name, nar_id)): Path<(String, Uuid)>,
    Query(params): Query<CompleteMultipartUploadParameterss>,
    Json(request): Json<CompleteMultipartUploadRequest>,
) -> Result<StatusCode, crate::Error> {
    let bucket = app.bucket()?;
    let upload = bucket.resume_multipart_upload(
        narfile_key(&name, &nar_id.to_string(), Compression::Zstd), // TODO: how do we know what compression is being used?
        params.upload_id,
    );

    let upload = match upload {
        Ok(u) => u,
        Err(e) => {
            worker::console_warn!("no such multipart upload: {e}");
            return Ok(StatusCode::BAD_REQUEST);
        }
    };

    let result = upload
        .complete(request.parts.into_iter().map(|part| {
            UploadedPart::new(
                part.part_number,
                part.e_tag.trim_prefix('"').trim_suffix('"').to_string(),
            )
        }))
        .await;

    let nar_info_txt = crate::narinfo::render_narinfo(
        &request.nar_info_create,
        &nar_id,
        Compression::Zstd, // TODO: how do we know what compression is being used?
        &app.signing_secret_key()?,
    )?;

    bucket
        .put(
            narinfo_key(&name, &request.nar_info_create.c_store_hash),
            nar_info_txt,
        )
        .http_metadata(HttpMetadata {
            content_type: Some("text/x-nix-narinfo".to_string()),
            ..Default::default()
        })
        .execute()
        .await?;

    match result {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => {
            worker::console_error!("cannot complete: {e}");
            Ok(StatusCode::INTERNAL_SERVER_ERROR) // TODO: some errors are client errors
        }
    }
}

pub fn router(state: NixCacheApp) -> axum::Router {
    // cachix API
    // https://app.cachix.org/api/v1/
    Router::new()
        .route("/cache/{name}", get(get_cache))
        .route("/cache/{name}/nix-cache-info", get(cache_info))
        .route("/cache/{name}/narinfo", post(missing_narinfo))
        .route("/cache/{name}/multipart-nar", post(create_multipart_upload))
        .route(
            "/cache/{name}/multipart-nar/{nar_id}",
            post(retrieve_presigned_url),
        )
        .route(
            "/cache/{name}/multipart-nar/{nar_id}/complete",
            post(complete_multipart_upload),
        )
        .with_state(state)
}
