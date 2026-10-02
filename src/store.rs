use std::str::FromStr;

use harmonia_store_nar_info::NarInfo;
use harmonia_store_path::StorePathHash;
use harmonia_utils_hash::HashFormat;
use harmonia_utils_signature::SecretKey;
use worker::{Bucket, HttpMetadata};

use crate::Compression;

fn narinfo_key(cache_name: &str, store_hash: &str) -> String {
    format!("{cache_name}/narinfo/{store_hash}")
}

fn narfile_key(cache_name: &str, nar_hash: &str, compression: Compression) -> String {
    format!("{cache_name}/nar/{nar_hash}.nar{}", compression.extension())
}

fn store_hash(narinfo_key: &str) -> Option<&str> {
    narinfo_key.split('/').next_back()
}

pub struct BucketStore {
    pub bucket: Bucket,
    signing_secret_key: SecretKey,
}

impl BucketStore {
    pub fn new(bucket: Bucket, signing_secret_key: SecretKey) -> Self {
        Self {
            bucket,
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
}
