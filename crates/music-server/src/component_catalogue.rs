use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

#[derive(Deserialize)]
pub(crate) struct Catalogue {
    schema: u32,
    revision: u64,
    #[serde(deserialize_with = "unique_archives")]
    archives: BTreeMap<String, Archive>,
}

#[derive(Deserialize)]
struct Archive {
    sources: Vec<String>,
}

fn unique_archives<'de, D>(deserializer: D) -> Result<BTreeMap<String, Archive>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Archives;
    impl<'de> serde::de::Visitor<'de> for Archives {
        type Value = BTreeMap<String, Archive>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("an archive map with distinct SHA-256 keys")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> Result<Self::Value, M::Error> {
            let mut archives = BTreeMap::new();
            while let Some((digest, archive)) = map.next_entry()? {
                if archives.insert(digest, archive).is_some() {
                    return Err(serde::de::Error::custom("duplicate archive SHA-256"));
                }
            }
            Ok(archives)
        }
    }
    deserializer.deserialize_map(Archives)
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

/// Read trust anchors only from files installed alongside the executable.
/// The writable data directory holds tough's rollback protection across restarts.
pub(crate) async fn channel_sources(digest: &str) -> Result<Option<Vec<String>>> {
    use futures_util::TryStreamExt;
    use tough::{RepositoryLoader, TargetName};

    let executable = std::env::current_exe()?;
    let channel = executable
        .parent()
        .context("missing installation folder")?
        .join("component-channel");
    let root = match tokio::fs::read(channel.join("root.json")).await {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        result => result.context("read installed channel root")?,
    };
    let endpoint = tokio::fs::read_to_string(channel.join("url.txt")).await?;
    let base = reqwest::Url::parse(endpoint.trim())?;
    ensure!(
        base.scheme() == "https"
            && base.host_str().is_some()
            && base.username().is_empty()
            && base.password().is_none()
            && base.fragment().is_none()
            && base.query().is_none()
            && base.path().ends_with('/'),
        "invalid channel endpoint"
    );
    let store = crate::studio_data_root()
        .context("missing studio data folder")?
        .join("component-channel");
    tokio::fs::create_dir_all(&store).await?;
    let lock = std::fs::File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(store.join("refresh.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock).context("component channel is refreshing")?;
    let repo = RepositoryLoader::new(&root, base.clone(), base.join("targets/")?)
        .datastore(store)
        .load()
        .await?;
    let stream = repo
        .read_target(&TargetName::new("mirrors.json")?)
        .await?
        .context("missing mirrors.json")?;
    futures_util::pin_mut!(stream);
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.try_next().await? {
        ensure!(
            chunk.len() <= 1024 * 1024 - bytes.len(),
            "mirror catalogue exceeds 1 MiB"
        );
        bytes.extend_from_slice(&chunk);
    }
    let catalogue = Catalogue::parse(&bytes)?;
    ensure!(
        catalogue.revision >= bundled().revision,
        "channel predates bundled catalogue"
    );
    Ok(catalogue.sources(digest).map(<[String]>::to_vec))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_archive_cannot_replace_a_source_list() {
        let entry = format!(
            r#""{}":{{"sources":["https://mirror.example/file"]}}"#,
            "a".repeat(64)
        );
        let json = format!(r#"{{"schema":1,"revision":1,"archives":{{{entry},{entry}}}}}"#);
        assert!(
            format!("{:#}", Catalogue::parse(json.as_bytes()).err().unwrap())
                .contains("duplicate archive SHA-256")
        );
    }

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
