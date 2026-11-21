pub(crate) mod rollback;
pub(crate) mod updates;

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use axum::{
    Json,
    extract::{Path as RoutePath, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use super::{Catalogue, EnginePackage, Failure, ModelPackage, PackageFile, parse, portable_name};
use crate::{AppState, api_error, component_catalogue as channel, net, update_data_root};

trait Package: Serialize + Sized {
    const FOLDER: &'static str;
    fn select(catalog: Catalogue, id: &str) -> Result<Self>;
    fn id(&self) -> &str;
    fn files(&self) -> &[PackageFile];
    fn executable(&self) -> Option<&str> {
        None
    }
}

impl Package for EnginePackage {
    const FOLDER: &'static str = "music-engines";
    fn select(catalog: Catalogue, id: &str) -> Result<Self> {
        let platform = host_platform();
        catalog
            .engines
            .into_iter()
            .find(|p| p.id == id && p.platform == platform)
            .context("engine is absent for this platform")
    }
    fn id(&self) -> &str {
        &self.id
    }
    fn files(&self) -> &[PackageFile] {
        &self.files
    }
    fn executable(&self) -> Option<&str> {
        Some(&self.executable)
    }
}

impl Package for ModelPackage {
    const FOLDER: &'static str = "music-models";
    fn select(catalog: Catalogue, id: &str) -> Result<Self> {
        catalog
            .models
            .into_iter()
            .find(|model| model.id == id)
            .context("model is absent from the signed catalogue")
    }
    fn id(&self) -> &str {
        &self.id
    }
    fn files(&self) -> &[PackageFile] {
        &self.files
    }
}

#[derive(Default, Deserialize, Serialize)]
pub(crate) struct Selection {
    active: String,
    previous: Option<String>,
    #[serde(default)]
    automatic_updates_paused: bool,
}

pub(super) fn host_platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

fn read_selection(root: &Path) -> Result<Selection> {
    match std::fs::read(root.join("active.json")) {
        Ok(bytes) => {
            let selection: Selection = serde_json::from_slice(&bytes)?;
            for id in std::iter::once(&selection.active).chain(selection.previous.iter()) {
                ensure!(
                    id.starts_with("generation-") && portable_name(id),
                    "invalid installed generation"
                );
            }
            Ok(selection)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Selection::default()),
        Err(error) => Err(error.into()),
    }
}

async fn verified(path: PathBuf, file: &PackageFile) -> Result<bool> {
    let expected = file.sha256.clone();
    let size = file.bytes;
    tokio::task::spawn_blocking(move || {
        if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && m.len() == size) {
            return Ok(false);
        }
        Ok(crate::install_record::sha256_of(&path)? == expected)
    })
    .await?
}

async fn only_signed_files(directory: &Path, package: &impl Package) -> Result<bool> {
    if !tokio::fs::symlink_metadata(directory).await?.is_dir() {
        return Ok(false);
    }
    let expected: std::collections::HashSet<_> = package
        .files()
        .iter()
        .map(|file| file.name.as_str())
        .collect();
    let mut entries = tokio::fs::read_dir(directory).await?;
    let mut count = 0;
    while let Some(entry) = entries.next_entry().await? {
        if !entry.file_type().await?.is_file()
            || !entry
                .file_name()
                .to_str()
                .is_some_and(|name| expected.contains(name))
        {
            return Ok(false);
        }
        count += 1;
    }
    Ok(count == expected.len())
}
async fn download(
    file: &PackageFile,
    path: &Path,
    client: &reqwest::Client,
    transfer: &super::progress::Transfer,
) -> Result<()> {
    transfer.stage("downloading", Some(&file.name));
    let mut response = client.get(&file.url).send().await?.error_for_status()?;
    let mut writer = tokio::fs::File::create_new(path).await?;
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            chunk.len() as u64 <= file.bytes - bytes,
            "component exceeds its signed size"
        );
        bytes += chunk.len() as u64;
        hash.update(&chunk);
        writer.write_all(&chunk).await?;
        transfer.downloaded(chunk.len() as u64);
    }
    ensure!(
        bytes == file.bytes
            && hash
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
                == file.sha256,
        "component integrity check failed for {}",
        file.name
    );
    writer.sync_all().await?;
    Ok(())
}

/// Models from different publishers can refer to the same pinned weight. An
/// object becomes reusable only after verification; concurrent installs share
/// the per-digest lock even when their model ids differ.
async fn materialize_model_file(
    store: &Path,
    file: &PackageFile,
    to: &Path,
    client: &reqwest::Client,
    transfer: &super::progress::Transfer,
) -> Result<bool> {
    tokio::fs::create_dir_all(store).await?;
    let object = store.join(&file.sha256);
    let lock_path = store.join(format!("{}.lock", file.sha256));
    let _lock = tokio::task::spawn_blocking(move || -> Result<std::fs::File> {
        let lock = std::fs::File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(lock_path)?;
        fs2::FileExt::lock_exclusive(&lock)?;
        Ok(lock)
    })
    .await??;
    let reused = verified(object.clone(), file).await?;
    if !reused {
        let staged = tempfile::tempdir_in(store)?;
        let fresh = staged.path().join("weight");
        download(file, &fresh, client, transfer).await?;
        tempfile::TempPath::try_from_path(fresh)?.persist(&object)?;
    }
    if tokio::fs::hard_link(&object, to).await.is_err() {
        tokio::fs::copy(&object, to).await?;
    }
    Ok(reused)
}

async fn install<P: Package>(
    root: &Path,
    package: &P,
    automatic: bool,
) -> Result<serde_json::Value> {
    let total = package
        .files()
        .iter()
        .try_fold(0u64, |sum, f| sum.checked_add(f.bytes))
        .context("package size overflow")?;
    let (job, mut cancelled) =
        super::progress::Installation::begin(P::FOLDER, package.id(), total)?;
    tokio::select! {
        biased;
        _ = cancelled.changed() => anyhow::bail!("Package installation cancelled"),
        result = stage_package(root, package, &job.transfer, automatic) => result,
    }
}

async fn stage_package<P: Package>(
    root: &Path,
    package: &P,
    transfer: &super::progress::Transfer,
    automatic: bool,
) -> Result<serde_json::Value> {
    let objects = root.with_file_name("music-model-objects");
    let root = root.join(package.id());
    tokio::fs::create_dir_all(&root).await?;
    let lock = std::fs::File::options()
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("install.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)
        .context("an installation is already running for this engine")?;
    let selected = read_selection(&root)?;
    if automatic {
        ensure!(
            !selected.active.is_empty() && !selected.automatic_updates_paused,
            "automatic updates paused for this package"
        );
    }
    let prior = root.join(&selected.active);
    let manifest = serde_json::to_vec(package)?;
    let same_manifest = !selected.active.is_empty()
        && tokio::fs::read(prior.join("manifest.json"))
            .await
            .ok()
            .as_deref()
            == Some(&manifest);
    let stage = tempfile::Builder::new()
        .prefix("generation-")
        .tempdir_in(&root)?;
    let proof = channel::save_engine_proof(&stage.path().join("proof")).await?;
    let trusted = P::select(parse(&proof)?, package.id())?;
    ensure!(
        serde_json::to_vec(&trusted)? == manifest,
        "catalogue changed before installation"
    );
    let directory = stage.path().join("files");
    tokio::fs::create_dir(&directory).await?;
    let client = net::download_client();
    let mut downloaded = 0u64;
    let mut reused = 0u64;
    for file in package.files() {
        transfer.stage("verifying", Some(&file.name));
        let to = directory.join(&file.name);
        let from = prior.join("files").join(&file.name);
        if !selected.active.is_empty() && verified(from.clone(), file).await? {
            if Some(file.name.as_str()) == package.executable()
                || tokio::fs::hard_link(&from, &to).await.is_err()
            {
                tokio::fs::copy(&from, &to).await?;
            }
            transfer.reused(file.bytes);
            reused = reused
                .checked_add(file.bytes)
                .context("package size overflow")?;
        } else {
            if P::FOLDER == ModelPackage::FOLDER {
                if materialize_model_file(&objects, file, &to, &client, transfer).await? {
                    transfer.reused(file.bytes);
                    reused = reused
                        .checked_add(file.bytes)
                        .context("package size overflow")?;
                    continue;
                }
            } else {
                download(file, &to, &client, transfer).await?;
            }
            downloaded = downloaded
                .checked_add(file.bytes)
                .context("package size overflow")?;
        }
    }
    if same_manifest
        && downloaded == 0
        && only_signed_files(&prior.join("files"), package)
            .await
            .unwrap_or(false)
        && channel::verify_engine_proof(&prior.join("proof"))
            .await
            .is_ok_and(|proof| {
                parse(&proof)
                    .and_then(|catalog| P::select(catalog, package.id()))
                    .is_ok_and(|signed| {
                        serde_json::to_vec(&signed).ok().as_deref() == Some(manifest.as_slice())
                    })
            })
    {
        return Ok(
            serde_json::json!({"generation":selected.active,"downloaded_bytes":0,"reused_bytes":reused}),
        );
    }
    #[cfg(unix)]
    if let Some(name) = package.executable() {
        use std::os::unix::fs::PermissionsExt;
        let executable = directory.join(name);
        tokio::fs::set_permissions(executable, std::fs::Permissions::from_mode(0o755)).await?;
    }
    let mut record = std::fs::File::create(stage.path().join("manifest.json"))?;
    record.write_all(&manifest)?;
    record.sync_all()?;
    let generation = stage
        .path()
        .file_name()
        .and_then(|name| name.to_str())
        .context("invalid generation directory")?
        .to_owned();
    let selection = Selection {
        active: generation.clone(),
        previous: (!selected.active.is_empty()).then_some(selected.active),
        automatic_updates_paused: false,
    };
    let mut next = tempfile::NamedTempFile::new_in(&root)?;
    serde_json::to_writer(next.as_file_mut(), &selection)?;
    next.as_file().sync_all()?;
    // Files from an old generation may still be loaded by an active engine.
    // Publish its replacement only after every signed artifact is present.
    transfer.stage("activating", None);
    let _generation_path = stage.keep();
    next.persist(root.join("active.json"))?;
    Ok(
        serde_json::json!({"generation":generation,"downloaded_bytes":downloaded,"reused_bytes":reused}),
    )
}

pub(crate) struct InstalledEngine {
    pub(crate) executable: PathBuf,
    pub(crate) generation: String,
    pub(crate) version: String,
    pub(crate) protocol: String,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Probe {
    #[default]
    Sha256,
    Presence,
}

#[derive(Default, Deserialize)]
pub(crate) struct StatusQuery {
    #[serde(default)]
    probe: Probe,
}

async fn resolve_package<P: Package>(
    data_root: &Path,
    id: &str,
    generation: Option<&str>,
    probe: Probe,
) -> Result<(P, PathBuf, String)> {
    ensure!(portable_name(id), "invalid engine id");
    let root = data_root.join(P::FOLDER).join(id);
    let selected = match generation {
        Some(value) => value.to_owned(),
        None => read_selection(&root)?.active,
    };
    ensure!(
        selected.starts_with("generation-") && portable_name(&selected),
        "no valid installed generation selected"
    );
    let dir = root.join(&selected);
    let proof = channel::verify_engine_proof(&dir.join("proof")).await?;
    let package = P::select(parse(&proof)?, id)?;
    // Resolve the actual files from the signed catalogue. Editable local
    // manifest fields must never decide which binary runs for a saved song.
    let files = dir.join("files");
    ensure!(
        only_signed_files(&files, &package).await?,
        "engine directory contains missing or undeclared files"
    );
    for item in package.files() {
        let path = files.join(&item.name);
        let valid = match probe {
            Probe::Sha256 => verified(path, item).await?,
            Probe::Presence => tokio::fs::symlink_metadata(path)
                .await
                .is_ok_and(|meta| meta.is_file() && meta.len() == item.bytes),
        };
        ensure!(
            valid,
            "installed engine file failed verification: {}",
            item.name
        );
    }
    Ok((package, files, selected))
}

pub(crate) async fn resolve_generation(
    data_root: &Path,
    id: &str,
    generation: Option<&str>,
) -> Result<InstalledEngine> {
    inspect_engine(data_root, id, generation, Probe::Sha256).await
}

async fn inspect_engine(
    data_root: &Path,
    id: &str,
    generation: Option<&str>,
    probe: Probe,
) -> Result<InstalledEngine> {
    let (package, files, selected) =
        resolve_package::<EnginePackage>(data_root, id, generation, probe).await?;
    ensure!(
        package.protocol == "music-http-v1",
        "incompatible engine protocol"
    );
    Ok(InstalledEngine {
        executable: files.join(package.executable),
        generation: selected,
        version: package.version,
        protocol: package.protocol,
    })
}

pub(crate) async fn status(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
    axum::extract::Query(query): axum::extract::Query<StatusQuery>,
) -> Result<Json<serde_json::Value>, Failure> {
    let current = inspect_engine(update_data_root(&state), &id, None, query.probe)
        .await
        .map_err(|error| api_error(StatusCode::CONFLICT, format!("{error:#}")))?;
    let selection = read_selection(
        &update_data_root(&state)
            .join(EnginePackage::FOLDER)
            .join(&id),
    )
    .map_err(|e| api_error(StatusCode::CONFLICT, format!("{e:#}")))?;
    Ok(Json(
        serde_json::json!({"id": id,"previous_generation":selection.previous.filter(|_| selection.active == current.generation),"generation":current.generation,"version":current.version,
        "verification":query.probe,"protocol":current.protocol,"executable":current.executable.file_name().and_then(|name| name.to_str())}),
    ))
}
pub(crate) async fn install_selected(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
) -> Result<Json<serde_json::Value>, Failure> {
    update_engine(&state, &id, false).await
}

async fn update_engine(
    state: &AppState,
    id: &str,
    automatic: bool,
) -> Result<Json<serde_json::Value>, Failure> {
    let catalog = current_catalogue().await?;
    let platform = host_platform();
    let package = catalog
        .engines
        .iter()
        .find(|p| p.id == id && p.platform == platform)
        .ok_or_else(|| {
            api_error(
                StatusCode::NOT_FOUND,
                "No signed engine package for this platform.".into(),
            )
        })?;
    if package.protocol != "music-http-v1" {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "This engine needs a different Studio protocol.".into(),
        ));
    }
    let root = update_data_root(&state).join("music-engines");
    install(&root, package, automatic)
        .await
        .map(Json)
        .map_err(|e| api_error(StatusCode::CONFLICT, format!("{e:#}")))
}

pub(crate) struct InstalledModel {
    pub(crate) files: PathBuf,
    pub(crate) generation: String,
    pub(crate) version: String,
}

pub(crate) async fn resolve_model_generation(
    data_root: &Path,
    engine_id: &str,
    model_id: &str,
    generation: Option<&str>,
) -> Result<InstalledModel> {
    inspect_model(data_root, engine_id, model_id, generation, Probe::Sha256).await
}

async fn inspect_model(
    data_root: &Path,
    engine_id: &str,
    model_id: &str,
    generation: Option<&str>,
    probe: Probe,
) -> Result<InstalledModel> {
    let (model, files, generation) =
        resolve_package::<ModelPackage>(data_root, model_id, generation, probe).await?;
    ensure!(
        model.engines.iter().any(|id| id == engine_id),
        "model is incompatible with the selected engine"
    );
    Ok(InstalledModel {
        files,
        generation,
        version: model.version,
    })
}

pub(crate) async fn install_model(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
) -> Result<Json<serde_json::Value>, Failure> {
    update_model(&state, &id, false).await
}

async fn update_model(
    state: &AppState,
    id: &str,
    automatic: bool,
) -> Result<Json<serde_json::Value>, Failure> {
    let catalog = current_catalogue().await?;
    let model = ModelPackage::select(catalog, &id)
        .map_err(|e| api_error(StatusCode::NOT_FOUND, e.to_string()))?;
    install(
        &update_data_root(state).join(ModelPackage::FOLDER),
        &model,
        automatic,
    )
    .await
    .map(Json)
    .map_err(|e| api_error(StatusCode::CONFLICT, format!("{e:#}")))
}

async fn current_catalogue() -> Result<Catalogue, Failure> {
    let bytes = channel::cached_catalogue(channel::ENGINES)
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| {
            api_error(
                StatusCode::NOT_FOUND,
                "Refresh the signed engine catalogue first.".into(),
            )
        })?;
    parse(&bytes).map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

pub(crate) async fn model_status(
    State(state): State<AppState>,
    RoutePath((engine, id)): RoutePath<(String, String)>,
    axum::extract::Query(query): axum::extract::Query<StatusQuery>,
) -> Result<Json<serde_json::Value>, Failure> {
    let model = inspect_model(update_data_root(&state), &engine, &id, None, query.probe)
        .await
        .map_err(|e| api_error(StatusCode::CONFLICT, format!("{e:#}")))?;
    let selection = read_selection(
        &update_data_root(&state)
            .join(ModelPackage::FOLDER)
            .join(&id),
    )
    .map_err(|e| api_error(StatusCode::CONFLICT, format!("{e:#}")))?;
    Ok(Json(
        serde_json::json!({"id":id,"previous_generation":selection.previous.filter(|_| selection.active == model.generation),"engine":engine,"generation":model.generation,"version":model.version,"verification":query.probe}),
    ))
}
