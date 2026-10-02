use std::{collections::BTreeSet, str::FromStr};

use harmonia_utils_signature::SecretKey;
use uuid::Uuid;

use harmonia_store_nar_info::{NarInfo, UnkeyedNarInfo, format_narinfo_txt, parse_narinfo_txt};
use harmonia_store_path::{FromStoreDirStr, StoreDir, StorePath};
use harmonia_store_path_info::{NarHash, UnkeyedValidPathInfo, fingerprint_path};
use harmonia_utils_hash::fmt::Any;

use crate::{Compression, api::NarInfoCreate};

/// Renders the pushed [`NarInfoCreate`] as `NarInfo` text.
/// This will also sign the fingerprint and add it as a `Sig` entry of the resulting narinfo.
pub(crate) fn build_narinfo(
    create: &NarInfoCreate,
    nar_id: &Uuid,
    compression: Compression,
) -> Result<NarInfo, crate::Error> {
    let nar_hash = parse_nar_hash_field("c_nar_hash", &create.c_nar_hash)?;
    let file_hash = parse_nar_hash_field("c_file_hash", &create.c_file_hash)?;

    let base = format!("{}-{}", create.c_store_hash, create.c_store_suffix);
    let store_path = base
        .parse::<StorePath>()
        .map_err(|e| crate::Error::Validation {
            field: "store_path",
            message: e.to_string(),
        })?;

    let mut references = BTreeSet::new();
    for reference in &create.c_references {
        match reference.parse::<StorePath>() {
            Ok(path) => {
                references.insert(path);
            }
            Err(e) => {
                return Err(crate::Error::Validation {
                    field: "c_references",
                    message: e.to_string(),
                });
            }
        }
    }

    let deriver = match create.c_deriver.as_str() {
        "unknown-deriver" => None,
        other => StorePath::from_store_dir_str(&StoreDir::default(), other)
            .or_else(|_| StorePath::from_str(other))
            .ok(),
    };

    Ok(NarInfo {
        path: store_path,
        info: UnkeyedNarInfo {
            info: UnkeyedValidPathInfo {
                deriver,
                nar_hash,
                references,
                registration_time: None,
                nar_size: create.c_nar_size,
                ultimate: false,
                signatures: BTreeSet::default(),
                ca: None,
                store_dir: StoreDir::default(),
            },
            url: Some(format!("nar/{nar_id}.nar{}", compression.extension())),
            compression: Some(compression.to_string()),
            download_hash: Some(file_hash.into()),
            download_size: Some(create.c_file_size),
        },
    })
}

pub(crate) fn render_narinfo_text(narinfo: &NarInfo) -> String {
    let bytes = format_narinfo_txt(&StoreDir::default(), narinfo);
    // The crate only emits ASCII (validated paths and hashes, plus the URL
    // built above), so this cannot fail.
    String::from_utf8(bytes).expect("NarInfo text is ASCII")
}

pub(crate) fn sign_narinfo(narinfo: &mut NarInfo, secret_key: &SecretKey) {
    let fingerprint = fingerprint_path(
        &StoreDir::default(),
        &narinfo.path,
        &narinfo.info.info.nar_hash,
        narinfo.info.info.nar_size,
        &narinfo.info.info.references,
    );
    let signature = secret_key.sign(fingerprint);
    narinfo.info.info.signatures.insert(signature);
}

pub(crate) fn parse_narinfo(txt: &str) -> Result<NarInfo, crate::Error> {
    Ok(parse_narinfo_txt(&StoreDir::default(), txt)?)
}

fn parse_nar_hash_field(field: &'static str, value: &str) -> Result<NarHash, crate::Error> {
    parse_nar_hash(value).map_err(|e| crate::Error::Validation {
        field,
        message: e.to_string(),
    })
}

/// Parses a hash field in any of the encodings the client sends:
/// `sha256:<base32>`, bare base32, or bare hex (see `Any`).
pub fn parse_nar_hash(value: &str) -> Result<NarHash, crate::Error> {
    Ok(value.parse::<Any<NarHash>>().map(Into::into)?)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use harmonia_store_nar_info::parse_narinfo_txt;
    use harmonia_store_path::StorePathName;

    fn secret_key() -> SecretKey {
        SecretKey::from_str(include_str!("../tests/cache.example.com-1.sk")).unwrap()
    }

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
        let mut narinfo = build_narinfo(&create, &nar_id, Compression::Zstd).unwrap();
        sign_narinfo(&mut narinfo, &secret_key());
        let text = render_narinfo_text(&narinfo);
        let expected = format!(
            "StorePath: /nix/store/4myf3s1i9rahd2my1zs2cqify7y930sk-readline-8.3p3\n\
             URL: nar/{nar_id}.nar.zst\n\
             Compression: zstd\n\
             FileHash: sha256:1gryjhjb0kwdbv8xrap77ac7nbx2w39zl24g6pq3s5kh8rjayqp3\n\
             FileSize: 212516\n\
             NarHash: sha256:19ns4kqd5prk1039zibr9j75q28x9jj2mk7aqa27j7bx8ximc4gf\n\
             NarSize: 506392\n\
             References: 0d8g8n0a11v6f5m2h416ajyxmnkwc3md-glibc-2.42-67 \
             zlvs6miv8wfki399pmxri7x0sjd3429c-ncurses-6.6\n\
             Deriver: 28544zr6433qkx35zq4yq54kq0b8zj5f-readline.drv\n\
             Sig: cache.example.com-1:TaiCdsGXu8o3TzbTmvGg40M159q5jdlw5dd7QOHGbKHXqyeYWSURLEEKn6olV2nYOukq3tsp1sRQnFvVRjGSAg==\n"
        );
        assert_eq!(text, expected);
    }

    #[test]
    fn omits_references_line_when_empty() {
        let mut create = sample_create();
        create.c_references = vec![];
        let narinfo = build_narinfo(&create, &Uuid::nil(), Compression::Zstd).unwrap();
        assert!(narinfo.info.info.references.is_empty());
    }

    #[test]
    fn round_trips_through_crate_parser() {
        let create = sample_create();
        let narinfo = build_narinfo(&create, &Uuid::nil(), Compression::Zstd).unwrap();
        let text = render_narinfo_text(&narinfo);
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
        let err = build_narinfo(&create, &Uuid::nil(), Compression::Zstd).unwrap_err();
        assert!(
            matches!(err, crate::Error::Validation { .. }),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn rejects_invalid_file_hash() {
        let mut create = sample_create();
        create.c_file_hash = "xyz".to_string();
        let err = build_narinfo(&create, &Uuid::nil(), Compression::Zstd).unwrap_err();
        assert!(
            matches!(err, crate::Error::Validation { .. }),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn rejects_invalid_store_path() {
        let mut create = sample_create();
        create.c_store_hash = "!!!not-base32!!!".to_string();
        let err = build_narinfo(&create, &Uuid::nil(), Compression::Zstd).unwrap_err();
        assert!(
            matches!(err, crate::Error::Validation { .. }),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn rejects_invalid_reference() {
        let mut create = sample_create();
        create.c_references = vec!["not-a-store-path".to_string()];
        let err = build_narinfo(&create, &Uuid::nil(), Compression::Zstd).unwrap_err();
        assert!(
            matches!(err, crate::Error::Validation { .. }),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn accepts_unknown_deriver() {
        let mut create = sample_create();
        create.c_deriver = "unknown-deriver".to_string();
        let result = build_narinfo(&create, &Uuid::nil(), Compression::Zstd);
        assert!(result.is_ok());
    }
}
