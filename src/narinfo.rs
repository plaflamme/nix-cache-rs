//! Pure NarInfo rendering for pushed `NarInfoCreate` payloads.
//!
//! [`render_narinfo`] validates the hash and store-path fields of a pushed
//! [`NarInfoCreate`] and renders the `NarInfo` text that the
//! `complete_multipart_upload` handler (step 2) stores in R2. It is
//! deliberately pure: no I/O, no HTTP, no signing. The crate's
//! `build_narinfo` is intentionally not used because it mints its own `URL`
//! and signs the info with a key — neither applies to a pushed cache.

use std::{collections::BTreeSet, str::FromStr};

use uuid::Uuid;

use harmonia_store_nar_info::{NarInfo, UnkeyedNarInfo, format_narinfo_txt};
use harmonia_store_path::{FromStoreDirStr, StoreDir, StorePath};
use harmonia_store_path_info::{NarHash, UnkeyedValidPathInfo};
use harmonia_utils_hash::fmt::Any;

use crate::cachix::NarInfoCreate;

/// Validation/rendering errors for [`render_narinfo`].
///
/// Every variant maps to an HTTP 400 once wired into a handler (step 2).
#[derive(Debug, thiserror::Error)]
pub enum NarInfoError {
    #[error("invalid {field}: {value} ({message})")]
    Invalid {
        field: &'static str,
        value: String,
        message: String,
    },
}

/// Renders the pushed [`NarInfoCreate`] as `NarInfo` text.
///
/// * `create` — the `narInfoCreate` object from the multipart-complete
///   request.
/// * `nar_id` — the UUID from the `/complete` route, used in the `URL:` line.
/// * `origin` — the request origin, e.g. `https://cache.example.com`.
///
/// The rendered text uses the standard field set only: `StorePath`, `URL`,
/// `Compression`, `FileHash`, `FileSize`, `NarHash`, `NarSize`, and
/// `References` (when non-empty). No `Sig`, `Deriver`, or `CA` is emitted:
/// `c_sig` and `c_deriver` from the push payload are intentionally dropped.
///
/// The upload blob is assumed to be zstd-compressed (the only compression
/// this cache accepts at upload initiation), so `Compression: zstd` is
/// rendered and `FileHash`/`FileSize` describe the compressed upload.
pub(crate) fn render_narinfo(
    create: &NarInfoCreate,
    nar_id: &Uuid,
    origin: &str,
) -> Result<String, NarInfoError> {
    let nar_hash = parse_nar_hash("c_nar_hash", &create.c_nar_hash)?;
    let file_hash = parse_nar_hash("c_file_hash", &create.c_file_hash)?;

    let base = format!("{}-{}", create.c_store_hash, create.c_store_suffix);
    let store_path = base
        .parse::<StorePath>()
        .map_err(|e| NarInfoError::Invalid {
            field: "store_path",
            value: base,
            message: e.to_string(),
        })?;

    let mut references = BTreeSet::new();
    for reference in &create.c_references {
        match reference.parse::<StorePath>() {
            Ok(path) => {
                references.insert(path);
            }
            Err(e) => {
                return Err(NarInfoError::Invalid {
                    field: "c_references",
                    value: reference.clone(),
                    message: e.to_string(),
                });
            }
        }
    }

    let deriver = StorePath::from_store_dir_str(&StoreDir::default(), &create.c_deriver)
        .or_else(|_| StorePath::from_str(&create.c_deriver))
        .map_err(|e| NarInfoError::Invalid {
            field: "deriver",
            value: create.c_deriver.clone(),
            message: e.to_string(),
        })?;

    let narinfo = NarInfo {
        path: store_path,
        info: UnkeyedNarInfo {
            info: UnkeyedValidPathInfo {
                deriver: Some(deriver),
                nar_hash,
                references,
                registration_time: None,
                nar_size: create.c_nar_size,
                ultimate: false,
                signatures: BTreeSet::new(),
                ca: None,
                store_dir: StoreDir::default(),
            },
            url: Some(format!("{origin}/nar/{nar_id}.nar.zst")),
            compression: Some("zstd".to_string()),
            download_hash: Some(file_hash.into()),
            download_size: Some(create.c_file_size),
        },
    };

    let bytes = format_narinfo_txt(&StoreDir::default(), &narinfo);
    // The crate only emits ASCII (validated paths and hashes, plus the URL
    // built above), so this cannot fail.
    Ok(String::from_utf8(bytes).expect("NarInfo text is ASCII"))
}

/// Parses a hash field in any of the encodings the client sends:
/// `sha256:<base32>`, bare base32, or bare hex (see `Any`).
fn parse_nar_hash(field: &'static str, value: &str) -> Result<NarHash, NarInfoError> {
    value
        .parse::<Any<NarHash>>()
        .map(NarHash::from)
        .map_err(|e| NarInfoError::Invalid {
            field,
            value: value.to_string(),
            message: e.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use harmonia_store_nar_info::parse_narinfo_txt;
    use harmonia_store_path::StorePathName;

    /// Real `narInfoCreate` payload captured from the push client pushing
    /// `/nix/store/4myf3s1i9rahd2my1zs2cqify7y930sk-readline-8.3p3`.
    fn sample_create() -> NarInfoCreate {
        NarInfoCreate {
            c_deriver: "/nix/store/28544zr6433qkx35zq4yq54kq0b8zj5f-readline.drv".to_string(),
            c_file_hash: "e362af644670163df0358f08fad3e0a22f7b983ae7aadcd15e8d4fb024943ebf"
                .to_string(),
            c_file_size: 212516,
            c_nar_hash: "sha256:19ns4kqd5prk1039zibr9j75q28x9jj2mk7aqa27j7bx8ximc4gf".to_string(),
            c_nar_size: 506392,
            c_references: vec![
                "zlvs6miv8wfki399pmxri7x0sjd3429c-ncurses-6.6".to_string(),
                "0d8g8n0a11v6f5m2h416ajyxmnkwc3md-glibc-2.42-67".to_string(),
            ],
            c_sig: Some("placeholder".to_string()),
            c_store_hash: "4myf3s1i9rahd2my1zs2cqify7y930sk".to_string(),
            c_store_suffix: "readline-8.3p3".to_string(),
        }
    }

    #[test]
    fn renders_real_readline_payload() {
        let create = sample_create();
        let nar_id = Uuid::nil();
        let text = render_narinfo(&create, &nar_id, "https://cache.example.com").unwrap();
        let expected = format!(
            "StorePath: /nix/store/4myf3s1i9rahd2my1zs2cqify7y930sk-readline-8.3p3\n\
             URL: https://cache.example.com/nar/{nar_id}.nar.zst\n\
             Compression: zstd\n\
             FileHash: sha256:1gryjhjb0kwdbv8xrap77ac7nbx2w39zl24g6pq3s5kh8rjayqp3\n\
             FileSize: 212516\n\
             NarHash: sha256:19ns4kqd5prk1039zibr9j75q28x9jj2mk7aqa27j7bx8ximc4gf\n\
             NarSize: 506392\n\
             References: 0d8g8n0a11v6f5m2h416ajyxmnkwc3md-glibc-2.42-67 \
             zlvs6miv8wfki399pmxri7x0sjd3429c-ncurses-6.6\n\
             Deriver: 28544zr6433qkx35zq4yq54kq0b8zj5f-readline.drv\n"
        );
        assert_eq!(text, expected);
    }

    #[test]
    fn url_uses_origin_and_nar_id() {
        let create = sample_create();
        let nar_id = Uuid::new_v4();
        let text = render_narinfo(&create, &nar_id, "https://cache.other-host.org").unwrap();
        let url = format!("URL: https://cache.other-host.org/nar/{nar_id}.nar.zst\n");
        assert!(text.contains(&url), "missing {url:?} in:\n{text}");
    }

    #[test]
    fn omits_references_line_when_empty() {
        let mut create = sample_create();
        create.c_references = vec![];
        let text = render_narinfo(&create, &Uuid::nil(), "https://cache.example.com").unwrap();
        assert!(!text.contains("References:"));
    }

    #[test]
    fn round_trips_through_crate_parser() {
        let create = sample_create();
        let text = render_narinfo(&create, &Uuid::nil(), "https://cache.example.com").unwrap();
        let parsed = parse_narinfo_txt(&StoreDir::default(), &text).unwrap();
        assert_eq!(
            parsed.path.name(),
            StorePathName::from_str("readline-8.3p3").unwrap()
        );
        assert_eq!(parsed.info.info.nar_size, 506392);
        assert_eq!(parsed.info.compression.as_deref(), Some("zstd"));
        assert_eq!(parsed.info.download_size, Some(212516));
        assert_eq!(parsed.info.info.references.len(), 2);
        assert_eq!(
            parsed.info.info.deriver,
            Some(StorePath::from_str("28544zr6433qkx35zq4yq54kq0b8zj5f-readline.drv").unwrap())
        );
        assert!(parsed.info.info.signatures.is_empty());
    }

    #[test]
    fn rejects_invalid_nar_hash() {
        let mut create = sample_create();
        create.c_nar_hash = "not-a-nar-hash".to_string();
        let err = render_narinfo(&create, &Uuid::nil(), "https://cache.example.com").unwrap_err();
        assert!(
            matches!(err, NarInfoError::Invalid { .. }),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn rejects_invalid_file_hash() {
        let mut create = sample_create();
        create.c_file_hash = "xyz".to_string();
        let err = render_narinfo(&create, &Uuid::nil(), "https://cache.example.com").unwrap_err();
        assert!(
            matches!(err, NarInfoError::Invalid { .. }),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn rejects_invalid_store_path() {
        let mut create = sample_create();
        create.c_store_hash = "!!!not-base32!!!".to_string();
        let err = render_narinfo(&create, &Uuid::nil(), "https://cache.example.com").unwrap_err();
        assert!(
            matches!(err, NarInfoError::Invalid { .. }),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn rejects_invalid_reference() {
        let mut create = sample_create();
        create.c_references = vec!["not-a-store-path".to_string()];
        let err = render_narinfo(&create, &Uuid::nil(), "https://cache.example.com").unwrap_err();
        assert!(
            matches!(err, NarInfoError::Invalid { .. }),
            "unexpected error: {err:?}"
        );
    }
}
