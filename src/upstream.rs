use std::collections::BTreeSet;

use futures::stream::StreamExt;
use url::Url;

/// An upstream nix cache.
///
/// When a client requests the hashes it needs to upload to the cache, we filter out any hashes already in the local cache.
/// But this means that a client may upload NARs that exist in other caches it uses.
///
/// As such, we also want to filter out hashes that upstream caches hold.
#[derive(Clone)]
pub struct Upstream {
    url: Url,
}

impl Upstream {
    pub fn cache_nixos_org() -> Self {
        Self {
            url: url::Url::parse("https://cache.nixos.org").expect("URL is valid"),
        }
    }

    pub async fn narinfo_lookup(&self, hashes: &mut BTreeSet<String>) -> Result<(), crate::Error> {
        let mut fetches = futures::stream::iter(hashes.clone())
            .map(|h| self.lookup(h))
            .buffer_unordered(50); // https://developers.cloudflare.com/workers/platform/limits/#subrequests

        while let Some(hash) = fetches.next().await {
            if let Some(hash) = hash? {
                hashes.remove(&hash);
            }
        }
        Ok(())
    }

    async fn lookup(&self, hash: String) -> Result<Option<String>, crate::Error> {
        let request = worker::Request::new(
            self.url.clone().join(&format!("/{hash}.narinfo"))?.as_str(),
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
