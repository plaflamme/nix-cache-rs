use std::str::FromStr;

use harmonia_store_nar_info::NarInfo;
use harmonia_store_path::StorePathHash;
use harmonia_utils_hash::HashFormat;
use harmonia_utils_signature::SecretKey;
use http::Method;
use url::Url;
use worker::{Bucket, HttpMetadata};

use crate::Compression;

fn narinfo_key(cache_name: &str, store_hash: &str) -> String {
    format!("{cache_name}/narinfo/{store_hash}")
}

fn narfile_key(cache_name: &str, nar_hash: &str, compression: Compression) -> String {
    format!("{cache_name}/nar/{nar_hash}.nar{}", compression.extension())
}

fn narfile_url(nar_hash: &str, compression: Compression) -> String {
    format!("nar/{nar_hash}.nar{}", compression.extension())
}

fn store_hash(narinfo_key: &str) -> Option<&str> {
    narinfo_key.split('/').next_back()
}

pub struct Part(pub u16, pub String);
impl From<Part> for worker::UploadedPart {
    fn from(value: Part) -> Self {
        Self::new(value.0, value.1)
    }
}

pub struct BucketStore {
    pub bucket: Bucket,
    bucket_name: String,
    credentials: aws_credential_types::Credentials,
    r2_endpoint: Url,
    signing_secret_key: SecretKey,
}

impl BucketStore {
    pub fn new(
        bucket: Bucket,
        bucket_name: String,
        credentials: aws_credential_types::Credentials,
        r2_endpoint: Url,
        signing_secret_key: SecretKey,
    ) -> Self {
        Self {
            bucket,
            bucket_name,
            credentials,
            r2_endpoint,
            signing_secret_key,
        }
    }

    /// Reads the narinfo file for the specified store path hash and appends the store's signature.
    pub async fn get_narinfo(
        &self,
        cache_name: &str,
        store_hash: StorePathHash,
    ) -> Result<Option<NarInfo>, crate::Error> {
        let narinfo_object = self
            .bucket
            .get(narinfo_key(cache_name, &store_hash.to_string()))
            .execute()
            .await?;

        match narinfo_object {
            None => Ok(None),
            Some(object) => match object.body() {
                None => Ok(None),
                Some(body) => {
                    let narinfo_txt = body.text().await?;
                    let mut narinfo = crate::narinfo::parse_narinfo(&narinfo_txt)?;
                    crate::narinfo::sign_narinfo(&mut narinfo, &self.signing_secret_key);
                    Ok(Some(narinfo))
                }
            },
        }
    }

    /// Write the specified narinfo data to the store.
    ///
    /// Returns an error if any of the fields are invalid or if the nar file referenced by the narinfo doesn't exist.
    pub async fn put_narinfo(
        &self,
        cache_name: &str,
        narinfo: &NarInfo,
    ) -> Result<(), crate::Error> {
        let store_path = &narinfo.path;

        // The hash of the actual file that will be downloaded.
        // This may differ from the nar's hash when the nar is compressed for example.
        let file_hash = narinfo
            .info
            .download_hash
            .unwrap_or_else(|| narinfo.info.info.nar_hash.into());

        let compression = narinfo
            .info
            .compression
            .as_deref()
            .map(Compression::from_str)
            .unwrap_or(Ok(Compression::None))?;

        let narfile_key = narfile_key(
            cache_name,
            &file_hash.as_base32().bare().to_string(),
            compression,
        );

        let nar_url = narfile_key.trim_prefix(cache_name).trim_prefix("/"); // TODO: this is stupid

        let narinfo_url = narinfo.info.url.clone().ok_or(crate::Error::Validation {
            field: "URL",
            message: "missing URL".to_string(),
        })?;

        if narinfo_url != nar_url {
            return Err(crate::Error::Validation {
                field: "URL",
                message: format!("expected {nar_url}, got {narinfo_url}"),
            });
        }

        let narfile = self
            .bucket
            .head(narfile_key)
            .await?
            .ok_or(crate::Error::Validation {
                field: "nar",
                message: "nar file not in store".to_string(),
            })?;

        let filesize = narinfo
            .info
            .download_size
            .unwrap_or(narinfo.info.info.nar_size);

        if narfile.size() != filesize {
            return Err(crate::Error::Validation {
                field: "FileSize",
                message: format!("expected {}, got {filesize}", narfile.size()),
            });
        }

        self.bucket
            .put(
                narinfo_key(cache_name, &store_path.hash().to_string()),
                crate::narinfo::render_narinfo_text(narinfo),
            )
            .http_metadata(HttpMetadata {
                content_type: Some("text/x-nix-narinfo".to_string()),
                ..Default::default()
            })
            .execute()
            .await?;

        Ok(())
    }

    pub async fn create_nar_upload(
        &self,
        cache_name: &str,
        compression: Compression,
    ) -> Result<(uuid::Uuid, String), crate::Error> {
        let bucket = &self.bucket;
        let nar_id = uuid::Uuid::new_v4();
        let metadata = HttpMetadata {
            content_type: Some("application/x-nix-nar".to_string()),
            ..Default::default()
        };
        let multipart_upload = bucket
            .create_multipart_upload(narfile_key(cache_name, &nar_id.to_string(), compression))
            .http_metadata(metadata)
            .execute()
            .await?;
        let upload_id = multipart_upload.upload_id().await;
        Ok((nar_id, upload_id))
    }

    pub(crate) async fn complete_nar_upload(
        &self,
        cache_name: &str,
        nar_id: uuid::Uuid,
        upload_id: &str,
        compression: Compression,
        mut nar_info: NarInfo,
        parts: impl IntoIterator<Item = Part>,
    ) -> Result<(), crate::Error> {
        let bucket = &self.bucket;
        let upload = bucket.resume_multipart_upload(
            narfile_key(cache_name, &nar_id.to_string(), compression), // TODO: how do we know what compression is being used?
            upload_id,
        );

        let upload = match upload {
            Ok(u) => u,
            Err(e) => {
                return Err(crate::Error::Validation {
                    field: "uploadId",
                    message: format!("cannot resume upload {e}"),
                });
            }
        };

        let _object = match upload.complete(parts.into_iter().map(Into::into)).await {
            Ok(object) => object,
            Err(e) => {
                return Err(crate::Error::Validation {
                    field: "parts",
                    message: format!("cannot complete upload {e}"),
                });
            }
        };

        // TODO: validate narinfo matches _object
        nar_info.info.url = Some(narfile_url(&nar_id.to_string(), compression));
        let narinfo_txt = crate::narinfo::render_narinfo_text(&nar_info);

        bucket
            .put(
                narinfo_key(cache_name, &nar_info.path.hash().to_string()),
                narinfo_txt,
            )
            .http_metadata(HttpMetadata {
                content_type: Some("text/x-nix-narinfo".to_string()),
                ..Default::default()
            })
            .execute()
            .await?;

        Ok(())
    }

    pub fn presigned_nar_url(
        &self,
        cache_name: &str,
        nar_id: &str,
        compression: Compression,
        method: Method,
        query_params: &[(&str, &str)],
        headers: &[(&str, &str)],
    ) -> Result<Url, crate::Error> {
        let mut narfile_url = self.r2_endpoint.clone();
        narfile_url
            .path_segments_mut()
            .expect("url can be base")
            .push(&self.bucket_name)
            .extend(narfile_key(cache_name, nar_id, compression).split('/'));
        query_params
            .iter()
            .fold(&mut narfile_url.query_pairs_mut(), |qp, (key, value)| {
                qp.append_pair(key, value)
            })
            .finish();
        // https://github.com/cachix/cachix/blob/5ecbf73e1e742f527c0d970bef0a4c0d359a5ea7/cachix/src/Cachix/Client/Push/S3.hs#L108-L116
        crate::r2_sig::sign_request(self.credentials.clone(), &mut narfile_url, method, headers)?;
        Ok(narfile_url)
    }
}
