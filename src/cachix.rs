use std::time::Duration;

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
use worker::UploadedPart;

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
        .create_multipart_upload(format!("/nar/{nar_id}"))
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
    Path((_name, nar_id)): Path<(String, Uuid)>,
    Query(_params): Query<RetrievePreSignedUrlParameters>,
    Json(request): Json<RetrievePreSignedUrlRequest>,
) -> Result<Json<RetrievePreSignedUrlResponse>, crate::Error> {
    let identity = app.r2_credentials()?.into();
    let mut settings = SigningSettings::default();
    settings.signature_location = SignatureLocation::QueryParams;
    settings.expires_in = Some(Duration::from_hours(1));

    let params = aws_sigv4::http_request::SigningParams::V4(
        aws_sigv4::sign::v4::SigningParams::builder()
            .identity(&identity)
            .region("auto")
            .name("s3")
            .time(crate::time::now())
            .settings(settings)
            .build()?,
    );

    let upload_url = format!("{}/nar/{nar_id}", app.r2_endpoint());

    // https://github.com/cachix/cachix/blob/5ecbf73e1e742f527c0d970bef0a4c0d359a5ea7/cachix/src/Cachix/Client/Push/S3.hs#L108-L116
    let request = SignableRequest::new(
        "PUT",
        &upload_url,
        [
            ("Content-Type", "application/octet-stream"),
            ("Content-MD5", request.content_md5.as_str()),
        ]
        .into_iter(),
        SignableBody::UnsignedPayload,
    )?;

    let result = aws_sigv4::http_request::sign(request, &params)?;
    let signed_params = result.output().params();
    let query_params = signed_params
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<String>>()
        .join("&");

    let upload_url = format!("{upload_url}?{query_params}");
    Ok(Json(RetrievePreSignedUrlResponse { upload_url }))
}

#[derive(Deserialize, Debug)]
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
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct CompletedPart {
    e_tag: String,
    part_number: u16,
}
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
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
    Path((_name, nar_id)): Path<(String, Uuid)>,
    Query(params): Query<CompleteMultipartUploadParameterss>,
    Json(request): Json<CompleteMultipartUploadRequest>,
) -> StatusCode {
    worker::console_log!("{nar_id}: {request:?}");
    let upload = app
        .bucket("cache_name")
        .resume_multipart_upload(format!("/nar/{nar_id}"), params.upload_id);

    let upload = match upload {
        Ok(u) => u,
        Err(e) => {
            worker::console_error!("no such multipart upload: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR;
        }
    };

    let result = upload
        .complete(request.parts.into_iter().map(|part| {
            let e_tag = part.e_tag.trim_prefix("\"").trim_suffix("\"").to_string();
            worker::console_log!("e_tag: {e_tag}");
            UploadedPart::new(part.part_number, e_tag)
        }))
        .await;

    match result {
        Ok(_) => {
            worker::console_log!("success");
            StatusCode::OK
        }
        Err(e) => {
            worker::console_error!("cannot complete: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
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
