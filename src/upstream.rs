use std::collections::BTreeSet;

use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use url::Url;
use worker::console_error;

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
                let request = worker::Request::new_with_init(
                    &format!("https://app.cachix.org/api/v1/cache/{cache_name}/narinfo"),
                    worker::RequestInit::new()
                        .with_method(worker::Method::Post)
                        .with_headers(worker::Headers::from_iter([
                            (http::header::HOST.as_str(), "app.cachix.org"),
                            (http::header::CONTENT_TYPE.as_str(), "application/json"),
                        ]))
                        .with_body(Some(serde_wasm_bindgen::to_value(&hashes)?)),
                )?;

                match worker::Fetch::Request(request).send().await {
                    Err(e) => console_error!("{e}"),
                    Ok(mut response) => match response.json::<BTreeSet<String>>().await {
                        Err(e) => console_error!("{e}"),
                        Ok(remain) => *hashes = remain,
                    },
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
