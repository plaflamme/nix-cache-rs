use std::str::FromStr;

use harmonia_store_nar_info::NarInfo;
use harmonia_store_path::StorePathHash;
use harmonia_store_path_info::NarHash;
use harmonia_utils_hash::HashFormat;
use harmonia_utils_signature::SecretKey;
use http::Method;
use url::Url;
use worker::{Bucket, HttpMetadata};

use crate::Compression;

/// Two styles for Nar filenames:
/// * UUID - used by cachix
/// * NarHash and compression - used by `nix copy`
///
/// This is necessary to support both write protocols. Cachix uses multipart uploads and expects a UUID back when it creates it.
/// Nix, on the other hand, simply GET/PUTs directly to nar filenames under `/nar`.
///
/// So we use this to parse and generate nar filenames for either style:
///
/// * Uuid: `123e4567-e89b-12d3-a456-426614174000`
/// * NarHash: `5g20bqhw379iw2vp2jxwzzsf5n1gmh4h.nar.zstd`
///
/// Consquently, URLs are R2 object keys have the following format:
/// * URL: `nar/{filename}`
/// * Cache object key: `{cache_name}/nar/{filename}`
pub enum NarFilename {
    Uuid(uuid::Uuid),
    NarHash(NarHash, Compression),
    // For compatibility with previous format
    Compat(uuid::Uuid, Compression),
}

impl NarFilename {
    /// Returns the nar file's bucket object key
    fn object_key(&self, cache_name: &str) -> String {
        format!("{cache_name}/nar/{self}")
    }

    /// Returns the URL value that should appear in the corresponding narinfo
    fn narinfo_url(&self) -> String {
        format!("nar/{self}")
    }
}

impl std::fmt::Display for NarFilename {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NarFilename::Uuid(nar_id) => write!(f, "{nar_id}"),

            NarFilename::NarHash(nar_hash, compression) => write!(
                f,
                "{}.nar{}",
                nar_hash.as_base32().bare(),
                compression.extension()
            ),

            NarFilename::Compat(nar_id, compression) => {
                write!(f, "{nar_id}.nar{}", compression.extension())
            }
        }
    }
}

impl FromStr for NarFilename {
    type Err = crate::Error;

    fn from_str(filename: &str) -> Result<Self, Self::Err> {
        let error = crate::Error::Validation {
            field: "filename",
            message: format!("unexpected nar filename: {filename}"),
        };
        match filename.split_once(".") {
            None => Ok(NarFilename::Uuid(
                uuid::Uuid::from_str(filename).map_err(|_| error)?,
            )),
            Some((nar_hash, extension)) => {
                let compression = match extension.split_once('.') {
                    None => Compression::None,
                    Some(("nar", compression)) => Compression::from_str(compression)?,
                    Some(_) => return Err(error),
                };
                match crate::narinfo::parse_nar_hash(nar_hash) {
                    Ok(nar_hash) => Ok(NarFilename::NarHash(nar_hash, compression)),
                    Err(e) => {
                        let Ok(uuid) = uuid::Uuid::from_str(nar_hash) else {
                            return Err(e);
                        };
                        Ok(NarFilename::Compat(uuid, compression))
                    }
                }
            }
        }
    }
}

struct NarinfoFilename;
impl NarinfoFilename {
    /// Returns the object key to use to scan all narinfo files in the bucket for the specified cache.
    fn scan_key(cache_name: &str) -> String {
        format!("{cache_name}/narinfo/")
    }
    /// Returns the object key to use for the specified StorePath hash.
    fn object_key(cache_name: &str, store_path_hash: &StorePathHash) -> String {
        format!("{cache_name}/narinfo/{store_path_hash}")
    }

    /// Parses an object key and returns the StorePath hash. Note that this function assumes that `object_key` was used to produce the key.
    fn from_object_key(object_key: &str) -> Result<&str, crate::Error> {
        let Some(store_path) = object_key.split('/').next_back() else {
            return Err(crate::Error::Validation {
                field: "store_path",
                message: format!("unexpected narinfo object key format: {object_key}"),
            });
        };
        Ok(store_path)
    }
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

    pub async fn narinfo_lookup(
        &self,
        cache_name: &str,
        hashes: &mut std::collections::BTreeSet<String>,
    ) -> Result<(), crate::Error> {
        // NOTE: this approach doesn't scale well since we effectively have to list all narinfo objects in R2
        // But it was chosen to avoid introducing another dependency, like KVStore or D1.
        // Using `head` on each key is too slow when the list of hashes is large.
        //
        // TODO: use both strategies dependening on the number of hashes to list. Under say 5 hashes, it's probably faster to make 5 HEAD requests than list all narinfo files.

        let bucket = &self.bucket;
        let mut cursor = None;
        while !hashes.is_empty() {
            let list_objects = bucket.list().prefix(NarinfoFilename::scan_key(cache_name));

            let objects = match cursor {
                Some(c) => list_objects.cursor(c),
                None => list_objects,
            }
            .execute()
            .await?;

            cursor = objects.cursor();

            for object in objects.objects().into_iter() {
                hashes.remove(NarinfoFilename::from_object_key(&object.key())?);
            }

            if !objects.truncated() {
                break;
            }
        }
        Ok(())
    }

    /// Reads the narinfo file for the specified store path hash and appends the store's signature.
    pub async fn get_narinfo(
        &self,
        cache_name: &str,
        store_hash: StorePathHash,
    ) -> Result<Option<NarInfo>, crate::Error> {
        let narinfo_object = self
            .bucket
            .get(NarinfoFilename::object_key(cache_name, &store_hash))
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
            .map(|hash| {
                hash.try_into().map_err(|e| crate::Error::Validation {
                    field: "download_hash",
                    message: format!("invalid nar_hash: {e}"),
                })
            })
            .unwrap_or_else(|| Ok(narinfo.info.info.nar_hash))?;

        let compression = narinfo
            .info
            .compression
            .as_deref()
            .map(Compression::from_str)
            .unwrap_or(Ok(Compression::None))?;

        let nar_filename = NarFilename::NarHash(file_hash, compression);

        let narinfo_url = narinfo.info.url.clone().ok_or(crate::Error::Validation {
            field: "URL",
            message: "missing URL".to_string(),
        })?;

        if narinfo_url != nar_filename.narinfo_url() {
            return Err(crate::Error::Validation {
                field: "URL",
                message: format!("expected {}, got {narinfo_url}", nar_filename.narinfo_url()),
            });
        }

        let narfile = self
            .bucket
            .head(nar_filename.object_key(cache_name))
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
                NarinfoFilename::object_key(cache_name, store_path.hash()),
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
        let nar_filename = NarFilename::Uuid(nar_id);
        let metadata = HttpMetadata {
            content_type: Some("application/x-nix-nar".to_string()),
            ..Default::default()
        };
        let multipart_upload = bucket
            .create_multipart_upload(nar_filename.object_key(cache_name))
            .http_metadata(metadata)
            .custom_metadata([("compression".to_string(), compression.to_string())])
            .execute()
            .await?;
        let upload_id = multipart_upload.upload_id().await;
        Ok((nar_id, upload_id))
    }

    pub(crate) async fn complete_nar_upload(
        &self,
        cache_name: &str,
        nar_filename: NarFilename,
        upload_id: &str,
        mut nar_info: NarInfo,
        parts: impl IntoIterator<Item = Part>,
    ) -> Result<(), crate::Error> {
        let bucket: &Bucket = &self.bucket;
        let upload = bucket.resume_multipart_upload(nar_filename.object_key(cache_name), upload_id);

        let upload = match upload {
            Ok(u) => u,
            Err(e) => {
                return Err(crate::Error::Validation {
                    field: "uploadId",
                    message: format!("cannot resume upload {e}"),
                });
            }
        };

        let object = match upload.complete(parts.into_iter().map(Into::into)).await {
            Ok(object) => object,
            Err(e) => {
                return Err(crate::Error::Validation {
                    field: "parts",
                    message: format!("cannot complete upload {e}"),
                });
            }
        };

        let nar_size = nar_info
            .info
            .download_size
            .unwrap_or(nar_info.info.info.nar_size);
        if nar_size != object.size() {
            return Err(crate::Error::Validation {
                field: "nar_size",
                message: format!(
                    "invalid nar_size, expected {}, got {nar_size}",
                    object.size()
                ),
            });
        }
        let expected_compression = object
            .custom_metadata()?
            .get("compression")
            .map(|v| Compression::from_str(v))
            .unwrap_or(Ok(Compression::None))?;
        let actual_compression = nar_info
            .info
            .compression
            .as_ref()
            .map(|v| Compression::from_str(v))
            .unwrap_or(Ok(Compression::None))?;

        if expected_compression != actual_compression {
            return Err(crate::Error::Validation {
                field: "compression",
                message: format!(
                    "invalid compression, expected {expected_compression}, got {actual_compression}"
                ),
            });
        }

        nar_info.info.url = Some(nar_filename.narinfo_url());
        let narinfo_txt = crate::narinfo::render_narinfo_text(&nar_info);

        bucket
            .put(
                NarinfoFilename::object_key(cache_name, nar_info.path.hash()),
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
        nar_filename: NarFilename,
        method: Method,
        query_params: &[(&str, &str)],
        headers: &[(&str, &str)],
    ) -> Result<Url, crate::Error> {
        let mut narfile_url = self.r2_endpoint.clone();
        narfile_url
            .path_segments_mut()
            .expect("url can be base")
            .push(&self.bucket_name)
            .extend(nar_filename.object_key(cache_name).split('/'));
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
