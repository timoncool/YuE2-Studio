//! The component catalogue: one JSON file listing every file the studio downloads (engines,
//! runtimes, model weights) and the model sets each engine offers, signed with the studio's
//! own updater key. New weights, a moved address or a new set are published by changing this
//! file (`scripts/publish-components.ps1`), without a studio release.
//!
//! The signed copy built into this binary is the starting point; the newest verified copy
//! downloaded from the studio's site replaces it, and an older one is never accepted.

use crate::downloads::{Asset, AssetKind};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

/// The minisign key the studio's updater already trusts (`desktop/src-tauri/tauri.conf.json`).
const PUBLIC_KEY: &str = "RWQqzubo1F9uGuZI4bU3aVssDQdhaF2CslTB/RwL7ScrRiVnOQQOjddd";
const URL: &str = "https://timoncool.github.io/YuE2-Studio/components/components.json";
const FILE: &str = "components.json";
const SIGNATURE: &str = "components.json.sig";
const LIMIT: usize = 4 * 1024 * 1024;
const BUILT_IN: &[u8] = include_bytes!("../../../docs/components/components.json");
const BUILT_IN_SIGNATURE: &[u8] = include_bytes!("../../../docs/components/components.json.sig");

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct FileEntry {
    pub label: String,
    pub kind: AssetKind,
    pub url: String,
    pub bytes: u64,
    pub sha256: String,
    /// Where the file lands, relative to the root of the part of the studio that uses it.
    pub path: String,
    #[serde(default)]
    pub unzip_into: Option<String>,
    #[serde(default)]
    pub marker: Option<String>,
    #[serde(default)]
    pub pick: Vec<String>,
    #[serde(default)]
    pub vram_gb: Option<u32>,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub mirrors: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct ModelSet {
    pub id: String,
    /// The set's name by language code.
    pub label: BTreeMap<String, String>,
    /// Video memory the set needs to run, in gigabytes.
    pub vram_gb: f64,
    /// What the set needs besides memory, such as `cuda13`.
    #[serde(default)]
    pub requires: Vec<String>,
    /// File ids by the role the engine gives them (`dit`, `lm`, ...).
    pub roles: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct Catalogue {
    pub sequence: u64,
    pub published: String,
    #[serde(default)]
    pub files: BTreeMap<String, FileEntry>,
    #[serde(default)]
    pub sets: BTreeMap<String, Vec<ModelSet>>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct State {
    pub sequence: u64,
    pub published: String,
    /// Where the catalogue in use came from: `built-in` or `downloaded`.
    pub source: &'static str,
    /// Why the last refresh did not replace it, when it did not.
    pub refresh_error: Option<String>,
}

struct Loaded {
    catalogue: Catalogue,
    source: &'static str,
    assets: Mutex<HashMap<String, &'static Asset>>,
}

static CURRENT: RwLock<Option<Arc<Loaded>>> = RwLock::new(None);
static REFRESH_ERROR: Mutex<Option<String>> = Mutex::new(None);

fn leak(text: &str) -> &'static str {
    Box::leak(text.to_owned().into_boxed_str())
}

fn verify(json: &[u8], signature: &[u8]) -> Result<Catalogue> {
    use base64::Engine;
    ensure!(json.len() <= LIMIT, "the component catalogue is larger than {LIMIT} bytes");
    let key = minisign_verify::PublicKey::from_base64(PUBLIC_KEY).context("component catalogue key")?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(signature.trim_ascii())
        .context("the component catalogue signature is not base64")?;
    let text = std::str::from_utf8(&decoded).context("the component catalogue signature is not text")?;
    let signature = minisign_verify::Signature::decode(text).context("read the component catalogue signature")?;
    key.verify(json, &signature, false).context("the component catalogue signature does not match")?;
    let catalogue: Catalogue = serde_json::from_slice(json).context("read the component catalogue")?;
    for (id, file) in &catalogue.files {
        ensure!(file.url.starts_with("https://"), "catalogue file {id} is not served over HTTPS");
        ensure!(file.sha256.len() == 64 && file.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "catalogue file {id} has no SHA-256");
    }
    for (engine, sets) in &catalogue.sets {
        for set in sets {
            for file in set.roles.values() {
                ensure!(catalogue.files.contains_key(file), "set {engine}/{} names unknown file {file}", set.id);
            }
        }
    }
    Ok(catalogue)
}

fn current() -> Arc<Loaded> {
    if let Some(loaded) = CURRENT.read().expect("component catalogue lock").clone() {
        return loaded;
    }
    let mut slot = CURRENT.write().expect("component catalogue lock");
    slot.get_or_insert_with(|| {
        let catalogue = verify(BUILT_IN, BUILT_IN_SIGNATURE).expect("the built-in component catalogue is signed");
        Arc::new(Loaded { catalogue, source: "built-in", assets: Default::default() })
    }).clone()
}

fn install(catalogue: Catalogue, source: &'static str) {
    *CURRENT.write().expect("component catalogue lock") =
        Some(Arc::new(Loaded { catalogue, source, assets: Default::default() }));
}

/// The file with this id, as a download the studio's downloader understands.
pub(crate) fn asset(id: &str) -> Result<&'static Asset> {
    let loaded = current();
    let mut assets = loaded.assets.lock().expect("component catalogue assets");
    if let Some(asset) = assets.get(id) {
        return Ok(asset);
    }
    let file = loaded.catalogue.files.get(id)
        .with_context(|| format!("the component catalogue {} has no file {id}", loaded.catalogue.sequence))?;
    let pick: Vec<&'static str> = file.pick.iter().map(|name| leak(name)).collect();
    let asset: &'static Asset = Box::leak(Box::new(Asset {
        id: leak(id),
        label: leak(&file.label),
        kind: file.kind,
        url: leak(&file.url),
        relative_path: leak(&file.path),
        bytes: file.bytes,
        unzip_into: file.unzip_into.as_deref().map(leak),
        marker: leak(file.marker.as_deref().unwrap_or("")),
        pick: Box::leak(pick.into_boxed_slice()),
        vram_gb: file.vram_gb,
        note: leak(&file.note),
    }));
    assets.insert(id.to_owned(), asset);
    Ok(asset)
}

/// The pinned SHA-256 of a catalogue file, by any of its addresses.
pub(crate) fn sha256(url: &str) -> Option<&'static str> {
    let loaded = current();
    loaded.catalogue.files.values()
        .find(|file| file.url == url || file.mirrors.iter().any(|mirror| mirror == url))
        .map(|file| leak(&file.sha256))
}

pub(crate) fn mirrors(url: &str) -> Vec<String> {
    let loaded = current();
    loaded.catalogue.files.values()
        .find(|file| file.url == url)
        .map(|file| file.mirrors.clone())
        .unwrap_or_default()
}

pub(crate) fn sets(engine: &str) -> Vec<ModelSet> {
    current().catalogue.sets.get(engine).cloned().unwrap_or_default()
}

pub(crate) fn file(id: &str) -> Option<FileEntry> {
    current().catalogue.files.get(id).cloned()
}

pub(crate) fn sequence() -> u64 {
    current().catalogue.sequence
}

pub(crate) fn state() -> State {
    let loaded = current();
    State {
        sequence: loaded.catalogue.sequence,
        published: loaded.catalogue.published.clone(),
        source: loaded.source,
        refresh_error: REFRESH_ERROR.lock().expect("component refresh error").clone(),
    }
}

fn cache_dir() -> Result<PathBuf> {
    Ok(crate::studio_data_root().context("missing studio data folder")?.join("components"))
}

/// Use the newest verified catalogue on this computer: the one downloaded last, when it is
/// newer than the one built in. A downloaded copy that does not verify is reported and skipped.
pub(crate) fn load_downloaded() {
    let built_in = current().catalogue.sequence;
    let read = || -> Result<Option<Catalogue>> {
        let dir = cache_dir()?;
        let json = match std::fs::read(dir.join(FILE)) {
            Ok(json) => json,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let signature = std::fs::read(dir.join(SIGNATURE))?;
        verify(&json, &signature).map(Some)
    };
    match read() {
        Ok(Some(catalogue)) if catalogue.sequence > built_in => install(catalogue, "downloaded"),
        Ok(_) => {}
        Err(error) => {
            crate::studio_log!("The downloaded component catalogue was rejected: {error:#}");
            *REFRESH_ERROR.lock().expect("component refresh error") = Some(format!("{error:#}"));
        }
    }
}

/// Download the published catalogue, check its signature and keep it when it is newer.
pub(crate) async fn refresh(http: &reqwest::Client) -> Result<State> {
    let result = async {
        let fetch = |url: String| async move {
            let response = http.get(&url).header("Cache-Control", "no-cache").send().await
                .with_context(|| format!("download {url}"))?.error_for_status()?;
            let bytes = response.bytes().await?;
            ensure!(bytes.len() <= LIMIT, "{url} is larger than {LIMIT} bytes");
            anyhow::Ok(bytes)
        };
        let json = fetch(URL.to_owned()).await?;
        let signature = fetch(format!("{URL}.sig")).await?;
        let catalogue = verify(&json, &signature)?;
        let accepted = current().catalogue.sequence;
        if catalogue.sequence < accepted {
            bail!("the published component catalogue {} is older than the one in use ({accepted})", catalogue.sequence);
        }
        if catalogue.sequence > accepted {
            let dir = cache_dir()?;
            std::fs::create_dir_all(&dir)?;
            for (name, bytes) in [(SIGNATURE, &signature), (FILE, &json)] {
                let mut next = tempfile::NamedTempFile::new_in(&dir)?;
                std::io::Write::write_all(&mut next, bytes)?;
                next.as_file().sync_all()?;
                next.persist(dir.join(name))?;
            }
            install(catalogue, "downloaded");
        }
        anyhow::Ok(())
    }.await;
    *REFRESH_ERROR.lock().expect("component refresh error") = result.as_ref().err().map(|error| format!("{error:#}"));
    result.map(|()| state())
}


/// Checks for a newer published catalogue at startup and once a day after.
pub(crate) async fn watch() {
    let mut every = tokio::time::interval(std::time::Duration::from_secs(24 * 3600));
    loop {
        every.tick().await;
        if let Err(error) = refresh(&crate::net::download_client()).await {
            crate::studio_log!("[ERROR] the component catalogue was not refreshed: {error:#}");
        }
    }
}

pub(crate) async fn status() -> axum::Json<State> {
    axum::Json(state())
}

pub(crate) async fn refresh_now() -> Result<axum::Json<State>, (axum::http::StatusCode, axum::Json<crate::ApiError>)> {
    refresh(&crate::net::download_client())
        .await
        .map(axum::Json)
        .map_err(|error| crate::api_error(axum::http::StatusCode::BAD_GATEWAY, format!("{error:#}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_catalogue_verifies_and_every_set_names_known_files() {
        let catalogue = verify(BUILT_IN, BUILT_IN_SIGNATURE).unwrap();
        assert!(catalogue.sequence >= 1);
        for (engine, sets) in &catalogue.sets {
            for set in sets {
                assert!(set.label.contains_key("en"), "{engine}/{} has no English name", set.id);
            }
        }
    }

    #[test]
    fn one_changed_byte_breaks_the_signature() {
        let mut json = BUILT_IN.to_vec();
        let digit = json.iter().position(u8::is_ascii_digit).unwrap();
        json[digit] = if json[digit] == b'9' { b'8' } else { json[digit] + 1 };
        assert!(verify(&json, BUILT_IN_SIGNATURE).is_err());
    }

    #[test]
    fn a_file_becomes_a_download_with_its_catalogue_address() {
        let catalogue = verify(BUILT_IN, BUILT_IN_SIGNATURE).unwrap();
        let (id, file) = catalogue.files.iter().next().unwrap();
        let asset = asset(id).unwrap();
        assert_eq!(asset.url, file.url);
        assert_eq!(asset.bytes, file.bytes);
        assert_eq!(sha256(&file.url), Some(file.sha256.as_str()));
    }
}
