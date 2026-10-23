use std::collections::BTreeSet;

use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use http::request;
use url::Url;

/// An upstream nix cache.
///
/// When a client requests the hashes it needs to upload to the cache, we filter out any hashes already in the local cache.
/// But this means that a client may upload NARs that exist in other caches it uses.
///
/// As such, we also want to filter out hashes that upstream caches hold.
struct Upstream {
    url: Url,
}

impl Upstream {
    async fn narinfo_lookup(&self, hashes: &mut BTreeSet<String>) -> Result<(), crate::Error> {
        let mut fetches = FuturesUnordered::new();
        for h in hashes.iter() {
            fetches.push(lookup(h));
        }
        while let Some(hash) = fetches.await? {
            hashes.remove(hash);
        }
        Ok(())
    }

    async fn lookup(&self, hash: &str) -> Result<Option<&str>, crate::Error> {
        let request = worker::request::Builder::new()
            .uri(self.url.clone().join(format!("/{hash}.narinfo")?))
            .method("HEAD")
            .body(())?;
        let response = worker::Fetch::Request(request).send().await?;
        if response.status_code() < 400 {
            Ok(Some(hash))
        } else {
            Ok(None)
        }
    }
}
