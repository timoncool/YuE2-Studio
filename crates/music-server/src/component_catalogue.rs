use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

#[derive(Deserialize)]
pub(crate) struct Catalogue {
    schema: u32,
    revision: u64,
    archives: BTreeMap<String, Archive>,
}

#[derive(Deserialize)]
struct Archive {
    sources: Vec<String>,
}

impl Catalogue {
    /// Validate one complete revision before making any of its sources available.
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self> {
        let catalogue: Self = serde_json::from_slice(bytes).context("read component catalogue")?;
        ensure!(
            catalogue.schema == 1,
            "unsupported component catalogue schema"
        );
        ensure!(
            catalogue.revision > 0 && !catalogue.archives.is_empty(),
            "empty component catalogue or revision"
        );
        for (digest, archive) in &catalogue.archives {
            ensure!(
                digest.len() == 64
                    && digest
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "invalid archive SHA-256"
            );
            ensure!(
                !archive.sources.is_empty(),
                "no download source for {digest}"
            );
            let mut distinct = HashSet::new();
            for source in &archive.sources {
                let url = reqwest::Url::parse(source).context("invalid component mirror URL")?;
                ensure!(
                    url.scheme() == "https"
                        && url.host_str().is_some()
                        && url.username().is_empty()
                        && url.password().is_none()
                        && url.fragment().is_none(),
                    "invalid HTTPS mirror for {digest}"
                );
                ensure!(distinct.insert(url), "duplicate mirror for {digest}");
            }
        }
        Ok(catalogue)
    }

    pub(crate) fn sources(&self, digest: &str) -> Option<&[String]> {
        self.archives
            .get(digest)
            .map(|entry| entry.sources.as_slice())
    }
}

pub(crate) fn bundled() -> &'static Catalogue {
    static CATALOGUE: OnceLock<Catalogue> = OnceLock::new();
    CATALOGUE.get_or_init(|| {
        Catalogue::parse(include_bytes!(
            "../../../distribution/components/catalog.json"
        ))
        .expect("valid bundled component catalogue")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_the_whole_revision_before_returning_any_sources() {
        let digest = "a".repeat(64);
        let mut data = serde_json::json!({"schema": 1, "revision": 7, "archives": {
            (digest.clone()): {"sources": ["https://mirror.example/file"]}
        }});
        assert_eq!(
            Catalogue::parse(&serde_json::to_vec(&data).unwrap())
                .unwrap()
                .sources(&digest)
                .unwrap()
                .len(),
            1
        );
        for bad in [
            "http://mirror.example/file",
            "https://user:secret@mirror.example/file",
            "https://mirror.example/file#part",
            "https://mirror.example/file",
        ] {
            data["archives"][&digest]["sources"] =
                serde_json::json!(["https://mirror.example/file", bad]);
            assert!(Catalogue::parse(&serde_json::to_vec(&data).unwrap()).is_err());
        }
    }
}
