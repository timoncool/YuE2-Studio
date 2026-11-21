use std::fs;
use std::path::{Path as FsPath, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result, ensure};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;

use crate::{
    AppState, api_error,
    downloads::{Asset, AssetKind, Downloader},
    install_record,
};

const DATA: &str = include_str!("native_music_runtime_assets.json");

#[derive(Deserialize)]
struct Pin<'a> {
    #[serde(borrow)]
    engine: &'a str,
    platform: &'a str,
    revision: &'a str,
    directory: &'a str,
    archive: &'a str,
    url: &'a str,
    bytes: u64,
    sha256: &'a str,
    executable: &'a str,
    #[serde(default)]
    broken_windows_adapter_paths: bool,
    #[serde(borrow)]
    required: std::collections::BTreeMap<&'a str, &'a str>,
}

struct Runtime {
    pin: Pin<'static>,
    asset: Asset,
}

fn runtimes() -> &'static [Runtime] {
    static RUNTIMES: OnceLock<Vec<Runtime>> = OnceLock::new();
    RUNTIMES.get_or_init(|| {
        let pins: Vec<Pin<'_>> =
            serde_json::from_str(DATA).expect("valid native music runtime pins");
        pins.into_iter()
            .map(|pin| Runtime {
                asset: Asset {
                    id: pin.engine,
                    label: match pin.engine {
                        "acestep-cpp" => "ACE-Step runtime",
                        _ => "MiniMax runtime",
                    },
                    kind: AssetKind::Runtime,
                    url: pin.url,
                    relative_path: pin.archive,
                    bytes: pin.bytes,
                    unzip_into: Some(pin.directory),
                    marker: pin.executable,
                    pick: &[],
                    vram_gb: None,
                    note: "Native music engine",
                },
                pin,
            })
            .collect()
    })
}

fn host_runtimes(engine: &str) -> impl Iterator<Item = &'static Runtime> + '_ {
    let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    runtimes()
        .iter()
        .filter(move |runtime| runtime.pin.engine == engine && runtime.pin.platform == platform)
}

fn selected(engine: &str) -> Option<&'static Runtime> {
    host_runtimes(engine).next()
}

pub(super) fn sha256(url: &str) -> Option<&'static str> {
    runtimes()
        .iter()
        .find(|runtime| runtime.asset.url == url)
        .map(|runtime| runtime.pin.sha256)
}

pub(super) fn downloader(data_root: &FsPath) -> Downloader {
    Downloader::new(data_root.join("native-music-runtimes"))
}

fn installed_by(downloader: &Downloader, runtime: &Runtime) -> Option<PathBuf> {
    let record_path = downloader.record_of(&runtime.asset);
    if install_record::matches(&record_path, runtime.asset.url, downloader.root()) != Some(true)
        || install_record::read(&record_path)?.sha256 != runtime.pin.sha256
    {
        return None;
    }
    let folder = downloader.picked_into(&runtime.asset);
    if runtime.pin.required.keys().any(|path| {
        !fs::symlink_metadata(folder.join(path)).is_ok_and(|meta| meta.is_file() && meta.len() > 0)
    }) {
        return None;
    }
    Some(folder.join(runtime.pin.executable))
}

pub(super) fn installed(data_root: &FsPath, engine: &str) -> Option<PathBuf> {
    let downloader = downloader(data_root);
    // Keep earlier pins in the table behind the current pin. An app upgrade
    // can then run the previous verified install before the user downloads.
    host_runtimes(engine).find_map(|runtime| installed_by(&downloader, runtime))
}

pub(super) fn requires_adapter_update(data_root: &FsPath, engine: &str, executable: &FsPath) -> bool {
    let downloader = downloader(data_root);
    host_runtimes(engine).any(|runtime| {
        runtime.pin.broken_windows_adapter_paths
            && downloader.picked_into(&runtime.asset).join(runtime.pin.executable) == executable
    })
}

pub(super) fn cuda_build() -> Option<crate::hardware::CudaBuild> {
    if cfg!(target_os = "macos") {
        return None;
    }
    let machine = crate::hardware::hardware();
    crate::hardware::cuda_build(machine.compute_capability?, machine.driver_major?)
}

pub(super) fn cuda_backend(executable: &FsPath) -> Option<PathBuf> {
    let build = cuda_build()?;
    let library = if cfg!(windows) {
        "ggml-cuda.dll"
    } else {
        "libggml-cuda.so"
    };
    let backend = executable.parent()?.join(build.folder()).join(library);
    backend.is_file().then_some(backend)
}

/// Put the native engine's own libraries first. The CUDA dependencies are
/// shared with YuE2, and the Windows writable bundle may keep them app-local.
pub(super) fn runtime_paths(executable: &FsPath, data_root: &FsPath) -> Vec<PathBuf> {
    let mut paths = executable
        .parent()
        .into_iter()
        .map(FsPath::to_owned)
        .collect::<Vec<_>>();
    let shared = data_root.join("engine-runtime");
    if cfg!(target_os = "linux") {
        paths.push(shared.join("cuda"));
    } else if cfg!(windows) {
        paths.push(shared);
        paths.push(crate::engine_bundle_root());
    }
    paths
}

/// Only our native runtime archive pins take this path. Older downloads use
/// their existing flat extractor and update records.
pub(super) fn install_tree(
    url: &str,
    source: &FsPath,
    destination: &FsPath,
    record: &FsPath,
    root: &FsPath,
) -> Option<Result<()>> {
    let runtime = runtimes().iter().find(|runtime| runtime.asset.url == url)?;
    Some((|| {
        let pin = &runtime.pin;
        ensure!(
            fs::metadata(source)?.len() == pin.bytes,
            "native runtime archive size does not match its pin"
        );
        ensure!(
            install_record::sha256_of(source)? == pin.sha256,
            "native runtime archive hash does not match its pin"
        );
        let parent = destination
            .parent()
            .context("native runtime has no staging directory")?;
        fs::create_dir_all(parent)?;
        let staged = tempfile::tempdir_in(parent)?;
        let extracted = super::native_runtime_archive::unpack(source, staged.path())?;
        for (name, expected) in &pin.required {
            let path = staged.path().join(name);
            ensure!(
                install_record::sha256_of(&path)
                    .with_context(|| format!("verify runtime file {name}"))?
                    == *expected,
                "native runtime file {name} does not match its pin"
            );
        }
        super::downloads::make_programs_executable(pin.executable, &extracted)?;
        let paths: Vec<_> = extracted
            .iter()
            .map(|path| Ok(destination.join(path.strip_prefix(staged.path())?)))
            .collect::<Result<_>>()?;
        // A corrupt reinstall is staged beside the same pinned version. Move
        // its directory aside only after the new archive and files pass checks.
        let old = tempfile::tempdir_in(parent)?;
        let backup = old.path().join("previous");
        let had_old = fs::symlink_metadata(destination).is_ok();
        if had_old {
            fs::rename(destination, &backup)?;
        }
        if let Err(error) = fs::rename(staged.path(), destination) {
            if had_old {
                fs::rename(&backup, destination)?;
            }
            return Err(error.into());
        }
        if let Err(error) = install_record::write(record, url, pin.sha256, &paths, root) {
            fs::rename(destination, staged.path())?;
            if had_old {
                fs::rename(&backup, destination)?;
            }
            return Err(error);
        }
        Ok(())
    })())
}

pub(super) async fn status(
    State(state): State<AppState>,
    Path(engine): Path<String>,
) -> impl IntoResponse {
    let Some(runtime) = selected(&engine) else {
        return Json(serde_json::json!({ "supported": false })).into_response();
    };
    let downloader = &state.native_music_downloads;
    let installed = installed_by(downloader, runtime).is_some();
    let shared = state.engine_runtime.missing(cuda_build());
    let common_bytes: u64 = shared.iter().map(|asset| asset.bytes).sum();
    let archive_bytes = if installed { 0 } else { runtime.pin.bytes };
    let shared_download = state.engine_runtime.downloader().active_for(&engine).await;
    let download = if shared_download
        .as_ref()
        .is_some_and(|download| !download.done)
    {
        shared_download
    } else {
        downloader.active_for(&engine).await.or(shared_download)
    };
    Json(serde_json::json!({
        "supported": true, "installed": installed && shared.is_empty(),
        "revision": runtime.pin.revision, "bytes": archive_bytes + common_bytes,
        "download": download,
    }))
    .into_response()
}

pub(super) async fn start(
    State(state): State<AppState>,
    Path(engine): Path<String>,
) -> impl IntoResponse {
    let Some(runtime) = selected(&engine) else {
        return api_error(
            StatusCode::NOT_FOUND,
            "No native runtime for this engine and platform.".into(),
        )
        .into_response();
    };
    let Ok(guard) = state.native_music_setup.clone().try_lock_owned() else {
        return api_error(
            StatusCode::CONFLICT,
            "A native music runtime installation is already running.".into(),
        )
        .into_response();
    };
    state.native_music_downloads.clear_finished(&engine).await;
    state
        .engine_runtime
        .downloader()
        .clear_finished(&engine)
        .await;
    tokio::spawn(async move {
        let _guard = guard;
        let result = install(&state, &engine, runtime).await;
        if let Err(error) = result {
            state
                .native_music_downloads
                .fail(&engine, format!("{error:#}"))
                .await;
        }
    });
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "started": true })),
    )
        .into_response()
}

async fn install(state: &AppState, engine: &str, runtime: &'static Runtime) -> Result<()> {
    if let Some(cuda) = cuda_build() {
        state
            .engine_runtime
            .install_missing_for(Some(cuda), engine)
            .await?;
    }
    let downloader = &state.native_music_downloads;
    let cache = std::env::var_os("STUDIO_NATIVE_RUNTIME_CACHE")
        .map(PathBuf::from)
        .map(|dir| dir.join(runtime.pin.archive))
        .filter(|path| path.is_file());
    match cache {
        Some(path) => {
            downloader
                .install_local_archive(engine, &runtime.asset, &path)
                .await
        }
        None => downloader.install_all(engine, &[&runtime.asset]).await,
    }
}

pub(super) async fn cancel(
    State(state): State<AppState>,
    Path(engine): Path<String>,
) -> impl IntoResponse {
    let cancelled_runtime = state.native_music_downloads.cancel_for(&engine).await;
    let cancelled_shared = state.engine_runtime.downloader().cancel_for(&engine).await;
    Json(serde_json::json!({ "cancelled": cancelled_runtime || cancelled_shared }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_pins_include_two_windows_and_linux_backends() {
        for runtime in runtimes() {
            let pin = &runtime.pin;
            assert_eq!(pin.revision.len(), 40);
            assert_eq!(pin.sha256.len(), 64);
            assert_eq!(sha256(pin.url), Some(pin.sha256));
            assert!(pin.required.contains_key(pin.executable));
            if !pin.platform.starts_with("macos-") {
                let library = if pin.platform.starts_with("windows-") {
                    "ggml-cuda.dll"
                } else {
                    "libggml-cuda.so"
                };
                for backend in ["cuda12", "cuda13"] {
                    assert!(
                        pin.required
                            .contains_key(format!("{backend}/{library}").as_str())
                    );
                }
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_ace_fallback_stays_available_for_requests_without_adapters() {
        let root = tempfile::tempdir().unwrap();
        let downloads = downloader(root.path());
        let engine = "acestep-cpp";
        let current = selected(engine).unwrap();
        assert!(!current.pin.broken_windows_adapter_paths);
        let previous = host_runtimes(engine)
            .find(|runtime| runtime.pin.broken_windows_adapter_paths)
            .unwrap();
        let folder = downloads.picked_into(&previous.asset);
        let files: Vec<_> = previous.pin.required.keys().map(|name| folder.join(name)).collect();
        for file in &files {
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, b"fixture").unwrap();
        }
        install_record::write(
            &downloads.record_of(&previous.asset),
            previous.pin.url,
            previous.pin.sha256,
            &files,
            downloads.root(),
        )
        .unwrap();
        let fallback = installed(root.path(), engine).unwrap();
        assert_eq!(fallback, folder.join(previous.pin.executable));
        assert!(requires_adapter_update(root.path(), engine, &fallback));
        let executable = downloads.picked_into(&current.asset).join(current.pin.executable);
        assert!(!requires_adapter_update(root.path(), engine, &executable));
    }

    #[tokio::test]
    #[ignore = "uses the pinned release archive or local runtime cache"]
    async fn pinned_runtime_installs_its_verified_members() {
        let cache = std::env::var_os("STUDIO_NATIVE_RUNTIME_CACHE").map(PathBuf::from);
        let engine = std::env::var("STUDIO_NATIVE_RUNTIME_TEST_ENGINE")
            .unwrap_or_else(|_| "acestep-cpp".into());
        let temporary = tempfile::tempdir().unwrap();
        let downloads = downloader(temporary.path());
        let runtime = selected(&engine).expect("supported host and test engine");
        if let Some(cache) = cache {
            let archive = cache.join(runtime.pin.archive);
            downloads
                .install_local_archive(&engine, &runtime.asset, &archive)
                .await
                .unwrap();
            assert!(archive.is_file());
        } else {
            downloads
                .install_all(&engine, &[&runtime.asset])
                .await
                .unwrap();
        }
        let executable = installed_by(&downloads, runtime).unwrap();
        let folder = executable.parent().unwrap();
        for (member, sha) in &runtime.pin.required {
            assert_eq!(
                install_record::sha256_of(&folder.join(member)).unwrap(),
                *sha
            );
        }
        // A rejected reinstall must leave the good generation untouched.
        let corrupt = temporary.path().join("damaged.zip");
        fs::write(&corrupt, b"damaged").unwrap();
        assert!(
            install_tree(
                runtime.pin.url,
                &corrupt,
                folder,
                &downloads.record_of(&runtime.asset),
                downloads.root()
            )
            .unwrap()
            .is_err()
        );
        assert_eq!(
            installed_by(&downloads, runtime).as_deref(),
            Some(executable.as_path())
        );
    }
}
