use std::str::FromStr;

use harmonia_utils_signature::SecretKey;
use worker::Env;

#[derive(Clone)]
pub struct NixCacheApp {
    env: Env,
}

impl NixCacheApp {
    pub(super) fn new(env: Env) -> Self {
        Self { env }
    }

    pub fn auth_token(&self) -> worker::Result<String> {
        Ok(self.env.secret("AUTH_TOKEN")?.to_string())
    }

    pub fn cache_endpoint(&self) -> worker::Result<url::Url> {
        Ok(url::Url::from_str(&format!(
            "https://{}",
            self.env.var("cache_hostname")?
        ))?)
    }

    pub fn bucket(&self) -> worker::Result<worker::Bucket> {
        self.env.bucket("nix-cache-bucket")
    }

    pub fn bucket_name(&self) -> worker::Result<String> {
        Ok(self.env.var("bucket_name")?.to_string())
    }

    pub fn github_username(&self) -> worker::Result<String> {
        Ok(self.env.var("github_username")?.to_string())
    }

    pub fn signing_secret_key(&self) -> Result<SecretKey, crate::Error> {
        Ok(SecretKey::from_str(&format!(
            "{}:{}",
            self.env.var("cache_hostname")?,
            &self.env.secret("SIGNING_PRIVATE_KEY")?
        ))?)
    }

    pub fn r2_credentials(&self) -> worker::Result<aws_credential_types::Credentials> {
        Ok(aws_credential_types::Credentials::builder()
            .access_key_id(self.env.var("R2_ACCESS_KEY_ID")?.to_string())
            .secret_access_key(self.env.var("R2_SECRET_ACCESS_KEY")?.to_string())
            .provider_name("provider_name")
            .build())
    }

    pub fn r2_endpoint(&self) -> worker::Result<url::Url> {
        Ok(url::Url::parse(
            &self.env.secret("R2_ENDPOINT")?.to_string(),
        )?)
    }
}
