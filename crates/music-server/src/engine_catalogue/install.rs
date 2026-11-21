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

use super::{EnginePackage, Failure, PackageFile, parse, portable_name};
use crate::{AppState, api_error, component_catalogue as channel, net, update_data_root};

#[derive(Default, Deserialize, Serialize)]
struct Selection {
    active: String,
    previous: Option<String>,
}

fn host_platform() -> String {
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

async fn only_signed_files(directory: &Path, package: &EnginePackage) -> Result<bool> {
    if !tokio::fs::symlink_metadata(directory).await?.is_dir() {
        return Ok(false);
    }
    let expected: std::collections::HashSet<_> = package
        .files
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
async fn download(file: &PackageFile, path: &Path, client: &reqwest::Client) -> Result<()> {
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

async fn install(root: &Path, package: &EnginePackage) -> Result<serde_json::Value> {
    let root = root.join(&package.id);
    tokio::fs::create_dir_all(&root).await?;
    let lock = std::fs::File::options()
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("install.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)
        .context("an installation is already running for this engine")?;
    let selected = read_selection(&root)?;
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
    let trusted = signed_package(&proof, &package.id)?;
    ensure!(
        serde_json::to_vec(&trusted)? == manifest,
        "catalogue changed before installation"
    );
    let directory = stage.path().join("files");
    tokio::fs::create_dir(&directory).await?;
    let client = net::download_client();
    let mut downloaded = 0u64;
    let mut reused = 0u64;
    for file in &package.files {
        let to = directory.join(&file.name);
        let from = prior.join("files").join(&file.name);
        if !selected.active.is_empty() && verified(from.clone(), file).await? {
            if file.name == package.executable || tokio::fs::hard_link(&from, &to).await.is_err() {
                tokio::fs::copy(&from, &to).await?;
            }
            reused = reused
                .checked_add(file.bytes)
                .context("package size overflow")?;
        } else {
            download(file, &to, &client).await?;
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
                signed_package(&proof, &package.id).is_ok_and(|signed| {
                    serde_json::to_vec(&signed).ok().as_deref() == Some(manifest.as_slice())
                })
            })
    {
        return Ok(
            serde_json::json!({"generation":selected.active,"downloaded_bytes":0,"reused_bytes":reused}),
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let executable = directory.join(&package.executable);
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
    };
    let mut next = tempfile::NamedTempFile::new_in(&root)?;
    serde_json::to_writer(next.as_file_mut(), &selection)?;
    next.as_file().sync_all()?;
    // Files from an old generation may still be loaded by an active engine.
    // Publish its replacement only after every signed artifact is present.
    let _generation_path = stage.keep();
    next.persist(root.join("active.json"))?;
    Ok(
        serde_json::json!({"generation":generation,"downloaded_bytes":downloaded,"reused_bytes":reused}),
    )
}

fn signed_package(bytes: &[u8], id: &str) -> Result<EnginePackage> {
    let platform = host_platform();
    parse(bytes)?
        .engines
        .into_iter()
        .find(|p| p.id == id && p.platform == platform)
        .context("engine is absent from the signed catalogue for this platform")
}

pub(crate) struct InstalledEngine {
    pub(crate) executable: PathBuf,
    pub(crate) generation: String,
    pub(crate) version: String,
    pub(crate) protocol: String,
}

pub(crate) async fn resolve_generation(
    data_root: &Path,
    id: &str,
    generation: Option<&str>,
) -> Result<InstalledEngine> {
    ensure!(portable_name(id), "invalid engine id");
    let root = data_root.join("music-engines").join(id);
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
    let package = signed_package(&proof, id)?;
    ensure!(
        package.protocol == "music-http-v1",
        "incompatible engine protocol"
    );
    // Resolve the actual files from the signed catalogue. Editable local
    // manifest fields must never decide which binary runs for a saved song.
    let files = dir.join("files");
    ensure!(
        only_signed_files(&files, &package).await?,
        "engine directory contains missing or undeclared files"
    );
    for item in &package.files {
        ensure!(
            verified(files.join(&item.name), item).await?,
            "installed engine file failed verification: {}",
            item.name
        );
    }
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
) -> Result<Json<serde_json::Value>, Failure> {
    let current = resolve_generation(update_data_root(&state), &id, None)
        .await
        .map_err(|error| api_error(StatusCode::CONFLICT, format!("{error:#}")))?;
    Ok(Json(
        serde_json::json!({"id": id,"generation":current.generation,"version":current.version,
        "protocol":current.protocol,"executable":current.executable.file_name().and_then(|name| name.to_str())}),
    ))
}
pub(crate) async fn install_selected(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
) -> Result<Json<serde_json::Value>, Failure> {
    let bytes = channel::cached_catalogue(channel::ENGINES)
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| {
            api_error(
                StatusCode::NOT_FOUND,
                "Refresh the signed engine catalogue first.".into(),
            )
        })?;
    let catalog =
        parse(&bytes).map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
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
    install(&root, package)
        .await
        .map(Json)
        .map_err(|e| api_error(StatusCode::CONFLICT, format!("{e:#}")))
}
