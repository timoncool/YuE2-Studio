use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
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

#[derive(Clone, Debug)]
struct ChannelTransport(reqwest::Client);

#[async_trait::async_trait]
impl tough::Transport for ChannelTransport {
    async fn fetch(
        &self,
        url: reqwest::Url,
    ) -> Result<tough::TransportStream, tough::TransportError> {
        use futures_util::TryStreamExt;
        use tough::{TransportError, TransportErrorKind};

        let request = self.0.get(url.clone());
        let error = move |cause: reqwest::Error| {
            let kind = match cause.status() {
                Some(reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::GONE) => {
                    TransportErrorKind::FileNotFound
                }
                _ => TransportErrorKind::Other,
            };
            TransportError::new_with_cause(kind, url.as_str(), cause)
        };
        // Use the installer's existing client so all TUF requests follow the studio proxy settings.
        let response = request
            .send()
            .await
            .and_then(reqwest::Response::error_for_status);
        Ok(Box::pin(
            response.map_err(&error)?.bytes_stream().map_err(error),
        ))
    }
}

fn installed_channel() -> Result<PathBuf> {
    let executable = std::env::current_exe()?;
    let parent = executable.parent().context("missing installation folder")?;
    Ok(packaged_channel(parent, std::env::consts::OS))
}

fn packaged_channel(binary_dir: &Path, os: &str) -> PathBuf {
    if let Some(parent) = binary_dir.parent() {
        let resources = match os {
            "macos" if binary_dir.ends_with("Contents/MacOS") => Some(parent.join("Resources")),
            "linux" if binary_dir.ends_with("usr/bin") => Some(parent.join("lib/YuE2 Studio")),
            _ => None,
        };
        if let Some(resources) = resources {
            let channel = resources.join("component-channel");
            if channel.join("root.json").exists() {
                return channel;
            }
        }
    }
    binary_dir.join("component-channel")
}

async fn installed_root() -> Result<Option<Vec<u8>>> {
    match tokio::fs::read(installed_channel()?.join("root.json")).await {
        Ok(bytes) if bytes.trim_ascii() == b"null" => Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        result => result.map(Some).context("read installed channel root"),
    }
}

async fn channel_repository(
    http: &reqwest::Client,
    root: &[u8],
    store: &Path,
) -> Result<tough::Repository> {
    let endpoint = tokio::fs::read_to_string(installed_channel()?.join("url.txt")).await?;
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
    tokio::fs::create_dir_all(store).await?;
    Ok(tough::RepositoryLoader::new(&root, base.clone(), base.join("targets/")?)
        .transport(ChannelTransport(http.clone()))
        .datastore(store)
        .load()
        .await?)
}

async fn lock_channel(store: &Path) -> Result<std::fs::File> {
    tokio::fs::create_dir_all(store).await?;
    let lock = std::fs::File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(store.join("refresh.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock).context("component channel is refreshing")?;
    Ok(lock)
}

fn channel_store() -> Result<PathBuf> {
    Ok(crate::studio_data_root()
        .context("missing studio data folder")?
        .join("component-channel"))
}

/// Read trust anchors only from files installed alongside the executable.
pub(crate) async fn channel_sources(
    http: &reqwest::Client,
    digest: &str,
) -> Result<Option<Vec<String>>> {
    use futures_util::TryStreamExt;
    use tough::TargetName;

    let Some(root) = installed_root().await? else {
        return Ok(None);
    };
    let store = channel_store()?;
    let _lock = lock_channel(&store).await?;
    let repo = channel_repository(http, &root, &store).await?;
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

const RVC_TARGET: &str = "rvc-voice-catalog.json";
const RVC_LIMIT: usize = 8 * 1024 * 1024;

async fn read_rvc_snapshot(
    root: &[u8],
    snapshot: &Path,
    refreshing: Option<&tough::Repository>,
) -> Result<Vec<u8>> {
    use futures_util::TryStreamExt;
    use tough::{ExpirationEnforcement, FilesystemTransport, RepositoryLoader, TargetName};

    let base = |folder| {
        reqwest::Url::from_directory_path(snapshot.join(folder))
            .map_err(|_| anyhow::anyhow!("invalid RVC catalogue cache path"))
    };
    // Offline use rechecks signatures and target hashes. Only a refresh can accept new metadata.
    let repo = RepositoryLoader::new(&root, base("metadata")?, base("targets")?)
        .transport(FilesystemTransport)
        .expiration_enforcement(ExpirationEnforcement::from(refreshing.is_some()))
        .load()
        .await?;
    if let Some(online) = refreshing {
        ensure!(
            repo.root().signed.version == online.root().signed.version
                && repo.timestamp().signed.version == online.timestamp().signed.version
                && repo.snapshot().signed.version == online.snapshot().signed.version
                && repo.targets().signed.version == online.targets().signed.version,
            "component channel changed while caching the RVC catalogue"
        );
    }
    let stream = repo
        .read_target(&TargetName::new(RVC_TARGET)?)
        .await?
        .context("missing signed RVC catalogue")?;
    futures_util::pin_mut!(stream);
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.try_next().await? {
        ensure!(chunk.len() <= RVC_LIMIT - bytes.len(), "RVC catalogue exceeds 8 MiB");
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn rvc_snapshot_name(name: &str) -> bool {
    name.strip_prefix("snapshot-").is_some_and(|suffix| {
        suffix.len() == 6 && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
    })
}

pub(crate) async fn cached_rvc_catalogue() -> Result<Option<Vec<u8>>> {
    let Some(root) = installed_root().await? else {
        return Ok(None);
    };
    let store = channel_store()?.join("rvc");
    let id = match tokio::fs::read_to_string(store.join("accepted")).await {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        result => result?,
    };
    ensure!(rvc_snapshot_name(&id), "invalid RVC catalogue cache pointer");
    Ok(Some(read_rvc_snapshot(&root, &store.join(id), None).await?))
}

pub(crate) async fn refresh_rvc_catalogue<T>(
    http: &reqwest::Client,
    validate: impl FnOnce(&[u8]) -> Result<T>,
) -> Result<T> {
    use std::io::Write;

    let root = installed_root()
        .await?
        .context("component channel is not installed")?;
    let channel = channel_store()?;
    let _lock = lock_channel(&channel).await?;
    let store = channel.join("rvc");
    let repo = channel_repository(http, &root, &store.join("datastore")).await?;
    let target = tough::TargetName::new(RVC_TARGET)?;
    ensure!(
        repo.targets().signed.find_target(&target, false)?.length <= RVC_LIMIT as u64,
        "RVC catalogue exceeds 8 MiB"
    );
    let snapshot = tempfile::Builder::new()
        .prefix("snapshot-")
        .rand_bytes(6)
        .tempdir_in(&store)?;
    let id = snapshot
        .path()
        .file_name()
        .and_then(|name| name.to_str())
        .context("invalid RVC catalogue cache path")?;
    repo.cache(
        snapshot.path().join("metadata"),
        snapshot.path().join("targets"),
        Some(&[RVC_TARGET]),
        true,
    )
    .await?;
    let bytes = read_rvc_snapshot(&root, snapshot.path(), Some(&repo)).await?;
    let result = validate(&bytes)?;
    let pointer = store.join("accepted");
    let previous = std::fs::read_to_string(&pointer).ok();
    let mut next = tempfile::NamedTempFile::new_in(&store)?;
    next.write_all(id.as_bytes())?;
    next.as_file().sync_all()?;
    next.persist(&pointer)?;
    let id = id.to_owned();
    let _ = snapshot.keep();

    // Leave the previous generation available for list/install requests already reading it.
    for entry in std::fs::read_dir(store).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name != id && previous.as_deref() != Some(name) && rvc_snapshot_name(name) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_uses_the_package_resource_directory_or_standalone_layout() {
        let root = tempfile::tempdir().unwrap();
        for (os, bin, resources) in [
            ("macos", "Studio.app/Contents/MacOS", "Studio.app/Contents/Resources"),
            ("linux", "AppDir/usr/bin", "AppDir/usr/lib/YuE2 Studio"),
        ] {
            let bin = root.path().join(bin);
            let channel = root.path().join(resources).join("component-channel");
            assert_eq!(packaged_channel(&bin, os), bin.join("component-channel"));
            std::fs::create_dir_all(&channel).unwrap();
            std::fs::write(channel.join("root.json"), b"null").unwrap();
            assert_eq!(packaged_channel(&bin, os), channel);
        }
        assert_eq!(packaged_channel(root.path(), "windows"), root.path().join("component-channel"));
    }

    #[tokio::test]
    async fn metadata_uses_the_installers_proxy_and_distinguishes_missing_roots() {
        use axum::{Router, http::StatusCode, routing::get};
        use tough::{Transport, TransportErrorKind};

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let proxy = listener.local_addr().unwrap();
        let router = Router::new()
            .route("/2.root.json", get(|| async { StatusCode::NOT_FOUND }))
            .route(
                "/timestamp.json",
                get(|| async { StatusCode::SERVICE_UNAVAILABLE }),
            );
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let client = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(format!("http://{proxy}")).unwrap())
            .build()
            .unwrap();
        let transport = ChannelTransport(client);
        for (file, expected) in [
            ("2.root.json", TransportErrorKind::FileNotFound),
            ("timestamp.json", TransportErrorKind::Other),
        ] {
            let url = reqwest::Url::parse(&format!("http://catalogue.invalid/{file}")).unwrap();
            let error = transport.fetch(url).await.err().expect("proxy error");
            assert_eq!(error.kind(), expected);
        }
        server.abort();
    }

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
