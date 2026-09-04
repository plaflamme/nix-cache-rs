use std::{str::FromStr, sync::Arc};

use harmonia_utils_signature::{PublicKey, SecretKey};
use url::Url;
use worker::{Bucket, Env};

#[derive(Clone)]
pub struct NixCacheApp {
    pub auth_token: String,
    pub bucket: Arc<Bucket>,
    pub bucket_name: String,
    pub signing_public_key: PublicKey,
    pub signing_secret_key: Arc<SecretKey>,
    pub r2_endpoint: url::Url,
    r2_access_key_id: String,
    r2_secret_access_key: String,
}

impl NixCacheApp {
    pub fn auth_token(&self) -> &str {
        &self.auth_token
    }

    pub fn r2_credentials(&self) -> worker::Result<aws_credential_types::Credentials> {
        Ok(aws_credential_types::Credentials::builder()
            .access_key_id(self.r2_access_key_id.clone())
            .secret_access_key(self.r2_secret_access_key.clone())
            .provider_name("provider_name")
            .build())
    }
}

impl TryFrom<Env> for NixCacheApp {
    type Error = crate::Error;

    fn try_from(env: Env) -> Result<Self, Self::Error> {
        Ok(Self {
            auth_token: env.secret("AUTH_TOKEN")?.to_string(),
            bucket: Arc::new(env.bucket("nix-cache-bucket")?),
            bucket_name: env.var("bucket_name")?.to_string(),
            signing_public_key: PublicKey::from_str(
                &env.secret("SIGNING_PUBLIC_KEY")?.to_string(),
            )?,
            signing_secret_key: Arc::new(SecretKey::from_str(
                &env.secret("SIGNING_PRIVATE_KEY")?.to_string(),
            )?),
            r2_access_key_id: env.secret("R2_ACCESS_KEY_ID")?.to_string(),
            r2_secret_access_key: env.secret("R2_SECRET_ACCESS_KEY")?.to_string(),
            r2_endpoint: Url::from_str(&env.secret("R2_ENDPOINT")?.to_string()).map_err(|e| {
                crate::Error::Validation {
                    field: "R2_ENDPOINT",
                    message: e.to_string(),
                }
            })?,
        })
    }
}
