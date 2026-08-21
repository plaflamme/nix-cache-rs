use std::time::SystemTime;

use crate::NixCacheApp;
use crate::cache_info;
use aws_sigv4::http_request::SignableBody;
use aws_sigv4::http_request::SignableRequest;
use aws_sigv4::http_request::SignatureLocation;
use aws_sigv4::http_request::SigningSettings;
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
    github_username: String,
    is_public: bool,
    name: String,
    permission: String,
    preferred_compression_method: String,
    public_signing_keys: Vec<String>,
    uri: String,
}
async fn get_cache(Path(name): Path<String>) -> Json<GetCacheResponse> {
    Json(GetCacheResponse {
        github_username: "plaflamme".to_string(),
        is_public: true,
        name,
        permission: "Write".to_string(),
        preferred_compression_method: "ZSTD".to_string(),
        public_signing_keys: Vec::new(),
        uri: "https://nix-cache-rs.philippe-e68.workers.dev/".to_string(),
    })
}

async fn missing_narinfo(Json(hashes): Json<Vec<String>>) -> (StatusCode, Json<Vec<String>>) {
    (StatusCode::OK, Json(hashes))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateMultipartUploadResponse {
    nar_id: Uuid,
    upload_id: String,
}

#[derive(Deserialize)]
struct CompressionParam {
    compression: Option<String>,
}

#[worker::send]
#[axum_macros::debug_handler]
async fn create_multipart_upload(
    Path(name): Path<String>,
    State(app): State<NixCacheApp>,
    Query(_param): Query<CompressionParam>,
) -> axum::response::Result<Json<CreateMultipartUploadResponse>> {
    let bucket = app.bucket(&name);
    let nar_id = Uuid::new_v4();
    let multipart_upload = bucket
        .create_multipart_upload(nar_id.to_string())
        .execute()
        .await
        .unwrap(); // TODO
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
    Path((_name, _nar_id)): Path<(String, Uuid)>,
    Query(_params): Query<RetrievePreSignedUrlParameters>,
    Json(request): Json<RetrievePreSignedUrlRequest>,
) -> Json<RetrievePreSignedUrlResponse> {
    let identity = app.r2_credentials().unwrap().into(); // TODO;
    let mut settings = SigningSettings::default();
    settings.signature_location = SignatureLocation::QueryParams;

    let params = aws_sigv4::http_request::SigningParams::V4(
        aws_sigv4::sign::v4::SigningParams::builder()
            .identity(&identity)
            .region("")
            .name("nix-cache-rs")
            .time(SystemTime::now())
            .settings(settings)
            .build()
            .unwrap(), // TODO
    );

    // https://github.com/cachix/cachix/blob/5ecbf73e1e742f527c0d970bef0a4c0d359a5ea7/cachix/src/Cachix/Client/Push/S3.hs#L108-L116
    let request = SignableRequest::new(
        "PUT",
        app.r2_endpoint(),
        [
            ("Content-Type", "application/octet-stream"),
            ("Content-MD5", request.content_md5.as_str()),
        ]
        .into_iter(),
        SignableBody::UnsignedPayload,
    )
    .unwrap(); // TODO

    let result = aws_sigv4::http_request::sign(request, &params).unwrap();
    let signed_params = result.output().params();
    let query_params = signed_params
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<String>>()
        .join("&");

    let upload_url = format!("{}?{query_params}", app.r2_endpoint());
    Json(RetrievePreSignedUrlResponse { upload_url })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NarInfoCreate {
    c_deriver: String,
    c_file_hash: String,
    c_file_size: u64,
    c_nar_hash: String,
    c_nar_size: u64,
    c_references: Vec<String>,
    c_sig: Option<String>,
    c_store_hash: String,
    c_store_suffix: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompletedPart {
    e_tag: String,
    part_number: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompleteMultipartUploadRequest {
    nar_info_create: NarInfoCreate,
    parts: Vec<CompletedPart>,
}

async fn complete_multipart_upload(
    State(_app): State<NixCacheApp>,
    Path((_name, _nar_id)): Path<(String, Uuid)>,
    Json(_request): Json<CompleteMultipartUploadRequest>,
) {
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
