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

/// The minisign keys the studio's updater already trusts (`desktop/src-tauri/tauri.conf.json`).
/// A new key is added here in a release signed with the old one, before catalogues move to it.
const PUBLIC_KEYS: &[&str] = &["RWQqzubo1F9uGuZI4bU3aVssDQdhaF2CslTB/RwL7ScrRiVnOQQOjddd"];
/// The program a catalogue is for: the same key signs other studios' files.
const PRODUCT: &str = "yue2-studio";
/// Where the catalogue is published, tried in order. The `1` is the format: a change older studios
/// cannot read is published under `2/`, and `1/` keeps being served for them.
const URLS: &[&str] = &[
    "https://timoncool.github.io/YuE2-Studio/components/1/components.json",
    "https://raw.githubusercontent.com/timoncool/YuE2-Studio/main/docs/components/1/components.json",
];
const FORMAT: &str = "1";
const FILE: &str = "components.json";
const SIGNATURE: &str = "components.json.sig";
const LIMIT: usize = 4 * 1024 * 1024;
const REFRESH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const BUILT_IN: &[u8] = include_bytes!("../../../docs/components/1/components.json");
const BUILT_IN_SIGNATURE: &[u8] = include_bytes!("../../../docs/components/1/components.json.sig");

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
    /// SHA-256 of files inside an archive, by name: what a pick takes out, and what an
    /// unpacked archive must still hold to count as installed.
    #[serde(default)]
    pub contents: BTreeMap<String, String>,
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

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Catalogue {
    pub product: String,
    pub sequence: u64,
    pub published: String,
    pub files: BTreeMap<String, FileEntry>,
    pub sets: BTreeMap<String, Vec<ModelSet>>,
    /// Entries this studio cannot use (an unknown kind, a set without a role it needs), left out.
    #[serde(skip)]
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct State {
    pub sequence: u64,
    pub published: String,
    /// Where the catalogue in use came from: `built-in` or `downloaded`.
    pub source: &'static str,
    pub built_in_sequence: u64,
    pub files: usize,
    pub sets: usize,
    /// Entries of the catalogue in use this studio left out, and why.
    pub skipped: Vec<String>,
    /// When the published catalogue was last read, in seconds since 1970.
    pub checked_at: Option<i64>,
    /// The address it was read from.
    pub checked_url: Option<&'static str>,
    /// Why the last refresh did not replace it, when it did not.
    pub refresh_error: Option<String>,
}

struct Loaded {
    catalogue: Catalogue,
    source: &'static str,
    assets: Mutex<HashMap<String, &'static Asset>>,
    shas: Mutex<HashMap<String, &'static str>>,
    contents: Mutex<HashMap<String, &'static [(&'static str, &'static str)]>>,
}

impl Loaded {
    fn new(catalogue: Catalogue, source: &'static str) -> Arc<Self> {
        Arc::new(Self { catalogue, source, assets: Default::default(), shas: Default::default(), contents: Default::default() })
    }
}

/// The last look at the published catalogue: when, and at which address.
static CHECKED: Mutex<Option<(i64, &'static str)>> = Mutex::new(None);

static CURRENT: RwLock<Option<Arc<Loaded>>> = RwLock::new(None);
static REFRESH_ERROR: Mutex<Option<String>> = Mutex::new(None);

fn leak(text: &str) -> &'static str {
    Box::leak(text.to_owned().into_boxed_str())
}

/// The roles each engine's sets must fill for this studio to load them.
fn required_roles(engine: &str) -> &'static [&'static str] {
    match engine {
        crate::ace::ENGINE_ID => &crate::ace::ROLES,
        crate::native_minimax::ENGINE_ID => &crate::native_minimax::ROLES,
        _ => &[],
    }
}

/// Checks the signature, then reads what this studio understands. A file or set it cannot use
/// is left out and named in `skipped`, so a catalogue made for a newer studio still loads; the
/// publishing script holds every entry to the full rules before signing.
fn verify(json: &[u8], signature: &[u8]) -> Result<Catalogue> {
    use base64::Engine;
    ensure!(json.len() <= LIMIT, "the component catalogue is larger than {LIMIT} bytes");
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(signature.trim_ascii())
        .context("the component catalogue signature is not base64")?;
    let text = std::str::from_utf8(&decoded).context("the component catalogue signature is not text")?;
    let signature = minisign_verify::Signature::decode(text).context("read the component catalogue signature")?;
    let signed = PUBLIC_KEYS.iter().any(|key| {
        minisign_verify::PublicKey::from_base64(key).is_ok_and(|key| key.verify(json, &signature, false).is_ok())
    });
    ensure!(signed, "the component catalogue signature does not match");
    parse(json)
}

fn parse(json: &[u8]) -> Result<Catalogue> {
    #[derive(Deserialize)]
    struct Raw {
        product: String,
        sequence: u64,
        published: String,
        #[serde(default)]
        files: BTreeMap<String, serde_json::Value>,
        #[serde(default)]
        sets: BTreeMap<String, Vec<serde_json::Value>>,
    }
    let raw: Raw = serde_json::from_slice(json).context("read the component catalogue")?;
    ensure!(raw.product == PRODUCT, "the component catalogue is for {}, not {PRODUCT}", raw.product);
    let mut skipped = Vec::new();
    let mut files = BTreeMap::new();
    for (id, value) in raw.files {
        let file = match serde_json::from_value::<FileEntry>(value) {
            Ok(file) => file,
            Err(error) => {
                skipped.push(format!("file {id}: {error}"));
                continue;
            }
        };
        if !file.url.starts_with("https://") {
            skipped.push(format!("file {id}: not served over HTTPS"));
        } else if ![&file.sha256].into_iter().chain(file.contents.values()).all(|sha| sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit())) {
            skipped.push(format!("file {id}: no SHA-256"));
        } else {
            files.insert(id, file);
        }
    }
    let mut sets = BTreeMap::new();
    for (engine, values) in raw.sets {
        let mut usable = Vec::new();
        for value in values {
            let set = match serde_json::from_value::<ModelSet>(value) {
                Ok(set) => set,
                Err(error) => {
                    skipped.push(format!("set {engine}: {error}"));
                    continue;
                }
            };
            let missing_role = required_roles(&engine).iter().find(|role| !set.roles.contains_key(**role));
            let unknown_file = set.roles.values().find(|file| !files.contains_key(*file));
            if let Some(role) = missing_role {
                skipped.push(format!("set {engine}/{}: no {role}", set.id));
            } else if let Some(file) = unknown_file {
                skipped.push(format!("set {engine}/{}: file {file} is not usable", set.id));
            } else if !set.label.contains_key("en") {
                skipped.push(format!("set {engine}/{}: no English name", set.id));
            } else {
                usable.push(set);
            }
        }
        sets.insert(engine, usable);
    }
    Ok(Catalogue { product: raw.product, sequence: raw.sequence, published: raw.published, files, sets, skipped })
}

/// The signed copy built into this binary.
fn built_in() -> Arc<Loaded> {
    static BUILT: std::sync::OnceLock<Arc<Loaded>> = std::sync::OnceLock::new();
    BUILT.get_or_init(|| {
        Loaded::new(verify(BUILT_IN, BUILT_IN_SIGNATURE).expect("the built-in component catalogue is signed"), "built-in")
    }).clone()
}

fn current() -> Arc<Loaded> {
    CURRENT.read().expect("component catalogue lock").clone().unwrap_or_else(built_in)
}

fn install(catalogue: Catalogue, source: &'static str) {
    for entry in &catalogue.skipped {
        crate::studio_log!("[components] left out of catalogue {}: {entry}", catalogue.sequence);
    }
    *CURRENT.write().expect("component catalogue lock") = Some(Loaded::new(catalogue, source));
}

/// The entry for `id` on this platform: `id@windows`, `id@linux` or `id@macos` where the
/// file differs by platform, else `id` itself. The studio keeps calling it `id`.
fn entry<'a>(catalogue: &'a Catalogue, id: &str) -> Option<&'a FileEntry> {
    catalogue.files.get(&format!("{id}@{}", std::env::consts::OS)).or_else(|| catalogue.files.get(id))
}

/// The catalogue in use first, then the one built in: an id a newer catalogue dropped still
/// resolves for the code of this studio that asks for it.
fn both() -> [Arc<Loaded>; 2] {
    [current(), built_in()]
}

/// The file with this id, as a download the studio's downloader understands.
pub(crate) fn asset(id: &str) -> Result<&'static Asset> {
    for loaded in both() {
        let mut assets = loaded.assets.lock().expect("component catalogue assets");
        if let Some(asset) = assets.get(id) {
            return Ok(asset);
        }
        let Some(file) = entry(&loaded.catalogue, id) else { continue };
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
        return Ok(asset);
    }
    bail!("the component catalogue {} has no file {id}", sequence())
}


/// The catalogue ids this build of the studio downloads by name. Each is in the built-in
/// catalogue (a test holds it to that), so `required` cannot miss.
pub(crate) const REQUIRED: &[&str] = &[
    "engine-cuda-cublas", "engine-cuda12-cublas",
    "engine-cuda-cudart-linux", "engine-cuda-cublas-linux", "engine-cuda12-cudart-linux", "engine-cuda12-cublas-linux",
    "crisperwhisper-turbo", "crisperwhisper-turbo-license", "crisperwhisper-turbo-readme", "crisperwhisper-turbo-added-tokens",
    "crisperwhisper-turbo-config", "crisperwhisper-turbo-generation-config", "crisperwhisper-turbo-merges",
    "crisperwhisper-turbo-normalizer", "crisperwhisper-turbo-preprocessor-config", "crisperwhisper-turbo-special-tokens-map",
    "crisperwhisper-turbo-tokenizer", "crisperwhisper-turbo-tokenizer-config", "crisperwhisper-turbo-vocab",
    "parakeet-tdt-int8", "parakeet-ultra-int8", "onnxruntime",
    "gemma-4-e4b-q4_0", "gemma-4-12b-q4_0", "gemma-4-12b-q5_k_m", "gemma-4-12b-q6_k", "gemma-4-12b-q8_0",
    "llama-cuda", "llama-cuda-runtime", "llama-cuda12", "llama-cuda12-runtime", "llama-vulkan", "llama-cpu",
    "htdemucs-6s", "roformer", "apollo", "rvc-f16",
];

/// A file this build downloads by id: one of `REQUIRED`.
pub(crate) fn required(id: &str) -> &'static Asset {
    debug_assert!(REQUIRED.contains(&id), "{id} is not in components::REQUIRED");
    asset(id).unwrap_or_else(|error| panic!("{error:#}"))
}

/// The pinned SHA-256 of a catalogue file, by any of its addresses.
pub(crate) fn sha256(url: &str) -> Option<&'static str> {
    both().into_iter().find_map(|loaded| {
        let mut shas = loaded.shas.lock().expect("component catalogue hashes");
        if let Some(sha) = shas.get(url) {
            return Some(*sha);
        }
        let file = loaded.catalogue.files.values()
            .find(|file| file.url == url || file.mirrors.iter().any(|mirror| mirror == url))?;
        let sha = leak(&file.sha256);
        shas.insert(url.to_owned(), sha);
        Some(sha)
    })
}

pub(crate) fn mirrors(url: &str) -> Vec<String> {
    both().into_iter()
        .find_map(|loaded| loaded.catalogue.files.values().find(|file| file.url == url).map(|file| file.mirrors.clone()))
        .unwrap_or_default()
}


/// The files inside the archive at `url` with their SHA-256, as the catalogue lists them.
pub(crate) fn contents(url: &str) -> Option<&'static [(&'static str, &'static str)]> {
    both().into_iter().find_map(|loaded| {
        let mut cache = loaded.contents.lock().expect("component catalogue contents");
        if let Some(list) = cache.get(url) {
            return Some(*list);
        }
        let file = loaded.catalogue.files.values().find(|file| file.url == url && !file.contents.is_empty())?;
        let list: Vec<(&'static str, &'static str)> = file.contents.iter().map(|(name, sha)| (leak(name), leak(sha))).collect();
        let list: &'static [(&'static str, &'static str)] = Box::leak(list.into_boxed_slice());
        cache.insert(url.to_owned(), list);
        Some(list)
    })
}

pub(crate) fn sets(engine: &str) -> Vec<ModelSet> {
    both().into_iter()
        .find_map(|loaded| loaded.catalogue.sets.get(engine).filter(|sets| !sets.is_empty()).cloned())
        .unwrap_or_default()
}

pub(crate) fn file(id: &str) -> Option<FileEntry> {
    both().into_iter().find_map(|loaded| entry(&loaded.catalogue, id).cloned())
}

pub(crate) fn sequence() -> u64 {
    current().catalogue.sequence
}

pub(crate) fn state() -> State {
    let loaded = current();
    let checked = *CHECKED.lock().expect("component check");
    State {
        sequence: loaded.catalogue.sequence,
        published: loaded.catalogue.published.clone(),
        source: loaded.source,
        built_in_sequence: built_in().catalogue.sequence,
        files: loaded.catalogue.files.len(),
        sets: loaded.catalogue.sets.values().map(Vec::len).sum(),
        skipped: loaded.catalogue.skipped.clone(),
        checked_at: checked.map(|(at, _)| at),
        checked_url: checked.map(|(_, url)| url),
        refresh_error: REFRESH_ERROR.lock().expect("component refresh error").clone(),
    }
}

fn cache_dir() -> Result<PathBuf> {
    Ok(crate::studio_data_root().context("missing studio data folder")?.join("components").join(FORMAT))
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
/// The addresses are tried in order; the first that serves a verified catalogue decides.
pub(crate) async fn refresh(http: &reqwest::Client) -> Result<State> {
    let fetch = |url: String| async move {
        let response = http.get(&url).header("Cache-Control", "no-cache").send().await
            .with_context(|| format!("download {url}"))?.error_for_status()?;
        let bytes = response.bytes().await?;
        ensure!(bytes.len() <= LIMIT, "{url} is larger than {LIMIT} bytes");
        anyhow::Ok(bytes)
    };
    let mut failures = Vec::new();
    let mut found = None;
    for url in URLS {
        let read = async {
            let json = fetch((*url).to_owned()).await?;
            let signature = fetch(format!("{url}.sig")).await?;
            let catalogue = verify(&json, &signature)?;
            anyhow::Ok((json, signature, catalogue))
        };
        match tokio::time::timeout(REFRESH_TIMEOUT, read).await {
            Ok(Ok(read)) => {
                found = Some((*url, read));
                break;
            }
            Ok(Err(error)) => failures.push(format!("{url}: {error:#}")),
            Err(_) => failures.push(format!("{url}: no answer in {} s", REFRESH_TIMEOUT.as_secs())),
        }
    }
    let result = match found {
        None => Err(anyhow::anyhow!("the component catalogue could not be read: {}", failures.join("; "))),
        Some((url, (json, signature, catalogue))) => {
            *CHECKED.lock().expect("component check") = Some((chrono::Utc::now().timestamp(), url));
            let accepted = current().catalogue.sequence;
            if catalogue.sequence < accepted {
                Err(anyhow::anyhow!("the published component catalogue {} is older than the one in use ({accepted})", catalogue.sequence))
            } else if catalogue.sequence == accepted {
                Ok(())
            } else {
                let saved = (|| -> Result<()> {
                    let dir = cache_dir()?;
                    std::fs::create_dir_all(&dir)?;
                    for (name, bytes) in [(SIGNATURE, &signature), (FILE, &json)] {
                        let mut next = tempfile::NamedTempFile::new_in(&dir)?;
                        std::io::Write::write_all(&mut next, bytes)?;
                        next.as_file().sync_all()?;
                        next.persist(dir.join(name))?;
                    }
                    Ok(())
                })();
                install(catalogue, "downloaded");
                saved.context("the newer component catalogue is in use until the studio closes, but was not saved")
            }
        }
    };
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
    fn the_built_in_catalogue_verifies_and_leaves_nothing_out() {
        let catalogue = verify(BUILT_IN, BUILT_IN_SIGNATURE).unwrap();
        assert!(catalogue.sequence >= 1);
        assert!(catalogue.skipped.is_empty(), "{:?}", catalogue.skipped);
        for (engine, sets) in &catalogue.sets {
            assert!(!sets.is_empty(), "{engine} lost its sets");
        }
    }

    #[test]
    fn every_required_id_is_in_the_built_in_catalogue() {
        let catalogue = verify(BUILT_IN, BUILT_IN_SIGNATURE).unwrap();
        let missing: Vec<_> = REQUIRED.iter().filter(|id| entry(&catalogue, id).is_none()).collect();
        assert!(missing.is_empty(), "not in the built-in catalogue: {missing:?}");
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
        assert!(std::ptr::eq(sha256(&file.url).unwrap(), sha256(&file.url).unwrap()), "a hash is kept, not copied again");
    }

    fn sample(product: &str, kind: &str, roles: serde_json::Value) -> Vec<u8> {
        let file = |kind: &str| serde_json::json!({"label": "x", "kind": kind, "url": "https://example.org/x",
            "bytes": 1, "sha256": "0".repeat(64), "path": "models/x"});
        serde_json::to_vec(&serde_json::json!({
            "product": product, "sequence": 9, "published": "2026-11-23",
            "files": {"known": file("model"), "future": file(kind)},
            "sets": {crate::ace::ENGINE_ID: [{"id": "s", "label": {"en": "S"}, "vram_gb": 4.0, "roles": roles}]}
        })).unwrap()
    }

    #[test]
    fn what_a_newer_studio_adds_is_left_out_and_the_rest_loads() {
        let all = serde_json::json!({"dit": "known", "lm": "known", "text_encoder": "known", "vae": "known"});
        let catalogue = parse(&sample(PRODUCT, "hologram", all)).unwrap();
        assert!(catalogue.files.contains_key("known"));
        assert!(!catalogue.files.contains_key("future"));
        assert_eq!(catalogue.sets[crate::ace::ENGINE_ID].len(), 1);
        assert_eq!(catalogue.skipped.len(), 1, "{:?}", catalogue.skipped);
    }

    #[test]
    fn a_set_missing_a_role_or_naming_a_dropped_file_is_left_out() {
        let short = serde_json::json!({"dit": "known", "lm": "known", "text_encoder": "known"});
        assert!(parse(&sample(PRODUCT, "model", short)).unwrap().sets[crate::ace::ENGINE_ID].is_empty());
        let dropped = serde_json::json!({"dit": "future", "lm": "known", "text_encoder": "known", "vae": "known"});
        assert!(parse(&sample(PRODUCT, "hologram", dropped)).unwrap().sets[crate::ace::ENGINE_ID].is_empty());
    }

    #[test]
    fn another_programs_catalogue_is_refused() {
        let all = serde_json::json!({"dit": "known", "lm": "known", "text_encoder": "known", "vae": "known"});
        assert!(parse(&sample("dub-studio", "model", all)).is_err());
    }
}


/// One-off export of the studio's built-in download definitions into a table of
/// `distribution/components-src`, run when a family moves into the catalogue:
/// `cargo test -p music-server --lib components::export:: -- --ignored`.
#[cfg(test)]
mod export {
    use crate::downloads::{Asset, AssetKind};
    use serde_json::{Value, json};

    fn entry(asset: &Asset, mirrors: &[&str]) -> Value {
        // hashes the studio never pinned, read from the Hugging Face LFS record of the pinned revision
        const LOOKED_UP: &[(&str, &str)] = &[
            ("https://huggingface.co/audio-cpp/audio.cpp-gguf/resolve/73ce4162f6cce370dd533c6782b772135c1b2a8e/Apollo-GGUF/apollo-orig.gguf", "41ee9eb6c5c265f126c1baaa3aca94d506dd604f4750f6da46d3cec00310df00"),
        ];
        let sha = crate::pin_sha::sha256(asset.url)
            .or_else(|| LOOKED_UP.iter().find(|(url, _)| *url == asset.url).map(|(_, sha)| *sha))
            .unwrap_or_else(|| panic!("{} has no pinned SHA-256", asset.id));
        let mut entry = json!({
            "label": asset.label,
            "kind": match asset.kind { AssetKind::Model => "model", AssetKind::Runtime => "runtime" },
            "url": asset.url, "bytes": asset.bytes, "sha256": sha, "path": asset.relative_path,
        });
        let object = entry.as_object_mut().unwrap();
        if let Some(folder) = asset.unzip_into { object.insert("unzip_into".into(), json!(folder)); }
        if !asset.marker.is_empty() { object.insert("marker".into(), json!(asset.marker)); }
        if !asset.pick.is_empty() { object.insert("pick".into(), json!(asset.pick)); }
        if let Some(vram) = asset.vram_gb { object.insert("vram_gb".into(), json!(vram)); }
        if !asset.note.is_empty() { object.insert("note".into(), json!(asset.note)); }
        if !mirrors.is_empty() { object.insert("mirrors".into(), json!(mirrors)); }
        let mut contents = serde_json::Map::new();
        for (name, sha) in crate::pin_sha::runtime_libraries(asset.url) { contents.insert((*name).into(), json!(sha)); }
        for entry in asset.pick {
            let name = crate::downloads::picked_file_name(entry);
            if let Some(sha) = crate::pin_sha::picked(asset.url, name) { contents.insert(name.into(), json!(sha)); }
        }
        if !contents.is_empty() { object.insert("contents".into(), Value::Object(contents)); }
        entry
    }

    fn write(table: &str, assets: &[(String, &Asset, Vec<&str>)]) {
        let mut files = serde_json::Map::new();
        for (key, asset, mirrors) in assets {
            assert!(files.insert(key.clone(), entry(asset, mirrors)).is_none(), "{key} twice");
        }
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../distribution/components-src").join(format!("{table}.json"));
        std::fs::write(&path, serde_json::to_string_pretty(&json!({ "files": files })).unwrap() + "\n").unwrap();
    }

    /// The other addresses Codex's channel listed for a pinned archive.
    fn channel_mirrors(asset: &Asset) -> Vec<&'static str> {
        let sha = crate::pin_sha::sha256(asset.url).unwrap();
        crate::component_catalogue::bundled().sources(sha).unwrap_or_default().iter()
            .filter(|url| url.as_str() != asset.url).map(|url| &*Box::leak(url.clone().into_boxed_str())).collect()
    }





}
