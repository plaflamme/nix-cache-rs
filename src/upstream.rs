use std::collections::BTreeSet;

use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use url::Url;
use worker::console_error;
use worker::console_log;

/// An upstream nix cache.
///
/// When a client requests the hashes it needs to upload to the cache, we filter out any hashes already in the local cache.
/// But this means that a client may upload NARs that exist in other caches it uses.
///
/// As such, we also want to filter out hashes that upstream caches hold.
#[derive(Clone)]
pub enum Upstream {
    Cachix { cache_name: String },
    Nix { url: Url },
}

impl Upstream {
    pub fn cache_nixos_org() -> Self {
        Self::Nix {
            url: url::Url::parse("https://cache.nixos.org").expect("URL is valid"),
        }
    }

    pub fn nix_community_cachix_org() -> Self {
        Self::Cachix {
            cache_name: "nix-community".to_string(),
        }
    }

    pub async fn narinfo_lookup(&self, hashes: &mut BTreeSet<String>) -> Result<(), crate::Error> {
        match self {
            Self::Cachix { cache_name } => {
                let client = reqwest::ClientBuilder::new().build()?;

                let response = client
                    .post(format!(
                        "https://app.cachix.org/api/v1/cache/{cache_name}/narinfo"
                    ))
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .json(hashes)
                    .send()
                    .await?;

                if response.status().is_success() {
                    let remain = response.json::<BTreeSet<String>>().await?;
                    console_log!(
                        "hashes requested upstream: {}, hashes remaining: {}",
                        hashes.len(),
                        remain.len()
                    );
                    *hashes = remain;
                } else {
                    console_error!(
                        "Upstream request failed ({}): {}",
                        response.status(),
                        response.text().await?
                    )
                }
                Ok(())
            }
            Self::Nix { url } => {
                let mut fetches = FuturesUnordered::new();
                for h in hashes.iter() {
                    fetches.push(self.lookup(url, h.clone()));
                }
                while let Some(hash) = fetches.next().await {
                    if let Some(hash) = hash? {
                        hashes.remove(&hash);
                    }
                }
                Ok(())
            }
        }
    }

    async fn lookup(&self, base_url: &Url, hash: String) -> Result<Option<String>, crate::Error> {
        let request = worker::Request::new(
            base_url.join(&format!("/{hash}.narinfo"))?.as_str(),
            worker::Method::Head,
        )?;
        let response = worker::Fetch::Request(request).send().await;
        if let Ok(response) = response // ignore failed requests, worse thing is we cache something already cached upstream
            && response.status_code() < 400
        {
            Ok(Some(hash))
        } else {
            Ok(None)
        }
    }
}
