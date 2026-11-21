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

fn selected(engine: &str) -> Option<&'static Runtime> {
    let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    runtimes()
        .iter()
        .find(|runtime| runtime.pin.engine == engine && runtime.pin.platform == platform)
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
    installed_by(&downloader(data_root), selected(engine)?)
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
    Json(serde_json::json!({
        "supported": true, "installed": installed_by(downloader, runtime).is_some(),
        "revision": runtime.pin.revision, "bytes": runtime.pin.bytes,
        "download": downloader.active_for(&engine).await,
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
    let downloader = &state.native_music_downloads;
    let cache = std::env::var_os("STUDIO_NATIVE_RUNTIME_CACHE")
        .map(PathBuf::from)
        .map(|dir| dir.join(runtime.pin.archive))
        .filter(|path| path.is_file());
    let result = match cache {
        Some(path) => {
            downloader
                .install_local_archive(&engine, &runtime.asset, &path)
                .await
        }
        None => downloader.install_all(&engine, &[&runtime.asset]).await,
    };
    let result = match (result, cuda_build()) {
        (Ok(()), Some(cuda)) => {
            state
                .engine_runtime
                .install_missing_for(Some(cuda), &engine)
                .await
        }
        (result, _) => result,
    };
    match result {
        Ok(()) => Json(serde_json::json!({ "installed": true })).into_response(),
        Err(error) => api_error(StatusCode::CONFLICT, format!("{error:#}")).into_response(),
    }
}

pub(super) async fn cancel(
    State(state): State<AppState>,
    Path(engine): Path<String>,
) -> impl IntoResponse {
    Json(serde_json::json!({ "cancelled": state.native_music_downloads.cancel_for(&engine).await }))
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

    #[tokio::test]
    #[ignore = "needs the pinned archives in STUDIO_NATIVE_RUNTIME_CACHE"]
    async fn pinned_runtime_cache_installs_both_cuda_modules() {
        let cache = PathBuf::from(std::env::var_os("STUDIO_NATIVE_RUNTIME_CACHE").unwrap());
        let temporary = tempfile::tempdir().unwrap();
        let downloads = downloader(temporary.path());
        for engine in ["acestep-cpp", "minimaxmusic-cpp"] {
            let Some(runtime) = selected(engine) else {
                continue;
            };
            let archive = cache.join(runtime.pin.archive);
            downloads
                .install_local_archive(engine, &runtime.asset, &archive)
                .await
                .unwrap();
            let executable = installed_by(&downloads, runtime).unwrap();
            let folder = executable.parent().unwrap();
            for (member, sha) in &runtime.pin.required {
                assert_eq!(
                    install_record::sha256_of(&folder.join(member)).unwrap(),
                    *sha
                );
            }
            assert!(archive.is_file());
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
}
