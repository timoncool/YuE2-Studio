//! Download state and hardware selection for the optional TensorRT-RTX worker.

use std::path::{Path, PathBuf};

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::Serialize;

use crate::{
    AppState, api_error,
    downloads::{Asset, AssetKind, DownloadProgress, Downloader},
    hardware, install_record,
};

const SCOPE: &str = "rvc-native";

static HUBERT: Asset = Asset {
    // These weights are runtime components and use the verified install record.
    id: "rvc-native-hubert",
    label: "HuBERT",
    kind: AssetKind::Runtime,
    url: "https://modelscope.cn/api/v1/models/MosRat/vfrm-models/repo?Revision=9527a1bcf861c70717d4aba619ba92f2b5f42396&FilePath=shared%2Fhubert.vfrm",
    relative_path: "v1/hubert.vfrm",
    bytes: 378_427_648,
    unzip_into: None,
    marker: "hubert.vfrm",
    pick: &[],
    vram_gb: None,
    note: "Acoustic features for native RVC",
};
static RMVPE: Asset = Asset {
    id: "rvc-native-rmvpe",
    label: "RMVPE",
    kind: AssetKind::Runtime,
    url: "https://modelscope.cn/api/v1/models/MosRat/vfrm-models/repo?Revision=9527a1bcf861c70717d4aba619ba92f2b5f42396&FilePath=shared%2Frmvpe.vfrm",
    relative_path: "v1/rmvpe.vfrm",
    bytes: 362_064_656,
    unzip_into: None,
    marker: "rmvpe.vfrm",
    pick: &[],
    vram_gb: None,
    note: "Pitch estimation for native RVC",
};

const RELEASE: &str =
    "https://github.com/timoncool/vocal-fast-rs/releases/download/component-64b4f5dfc088/";
const RUNTIME_DATA: &str = include_str!("rvc_fast_assets.json");

#[derive(serde::Deserialize)]
struct RuntimePin<'a> {
    #[serde(borrow)]
    file: &'a str,
    sha256: &'a str,
    bytes: u64,
}

#[derive(serde::Deserialize)]
struct RuntimePins<'a> {
    #[serde(borrow)]
    windows: RuntimePin<'a>,
    linux12: RuntimePin<'a>,
    linux13: RuntimePin<'a>,
}

fn runtimes() -> &'static [(Asset, &'static str); 3] {
    static RUNTIMES: std::sync::OnceLock<[(Asset, &'static str); 3]> = std::sync::OnceLock::new();
    RUNTIMES.get_or_init(|| {
        let pins: RuntimePins<'_> =
            serde_json::from_str(RUNTIME_DATA).expect("valid native RVC asset pins");
        [
            (pins.windows, "cuda13", "vocal-fast.exe"),
            (pins.linux12, "cuda12", "vocal-fast"),
            (pins.linux13, "cuda13", "vocal-fast"),
        ]
        .map(|(pin, folder, marker)| {
            let url = format!("{RELEASE}{}", pin.file).leak();
            (
                Asset {
                    id: "rvc-native-worker",
                    label: "TensorRT-RTX",
                    kind: AssetKind::Runtime,
                    url,
                    relative_path: pin.file,
                    bytes: pin.bytes,
                    unzip_into: Some(folder),
                    marker,
                    pick: &[],
                    vram_gb: None,
                    note: "Native RVC worker for NVIDIA RTX",
                },
                pin.sha256,
            )
        })
    })
}

pub(crate) fn sha256(url: &str) -> Option<&'static str> {
    if url == HUBERT.url {
        return Some("a84fed477f49a040d53b44b7c27184d711fb246d5f9119c7c046941aaee65c59");
    }
    if url == RMVPE.url {
        return Some("462ee5494525ed1029abfad368abb56ca20285478e24aa846575ed400bff4734");
    }
    if !url.starts_with(RELEASE) {
        return None;
    }
    runtimes()
        .iter()
        .find(|(asset, _)| asset.url == url)
        .map(|(_, sha)| *sha)
}

fn selected() -> Result<[&'static Asset; 3], String> {
    if !cfg!(all(
        target_arch = "x86_64",
        any(target_os = "windows", target_os = "linux")
    )) {
        return Err("TensorRT-RTX requires x86-64 Windows or Linux.".into());
    }
    let hardware = hardware::hardware();
    if !hardware.nvidia
        || hardware
            .compute_capability
            .is_none_or(|compute| compute < (8, 6))
        || !hardware
            .gpu_name
            .as_ref()
            .is_some_and(|name| name.contains("RTX"))
    {
        return Err("This TensorRT-RTX component requires an NVIDIA RTX GPU with compute capability 8.6 or newer.".into());
    }
    let driver = hardware.driver_major.unwrap_or_default();
    let runtime = if cfg!(windows) {
        if driver < 580 {
            return Err(
                "Update the NVIDIA graphics driver to R580 or newer for TensorRT-RTX.".into(),
            );
        }
        0
    } else if driver >= 580 {
        2
    } else if driver >= 575 {
        1
    } else {
        return Err("Update the NVIDIA graphics driver to R575 or newer for TensorRT-RTX.".into());
    };
    Ok([&runtimes()[runtime].0, &HUBERT, &RMVPE])
}

pub(crate) fn downloader(data_root: &Path) -> Downloader {
    let root = data_root.join("rvc/native");
    Downloader::with_runtimes(root.clone(), root.join("v1"))
}

pub(crate) fn installed_worker(data_root: &Path) -> Result<Option<PathBuf>, String> {
    let assets = selected()?;
    let downloader = downloader(data_root);
    if assets.iter().all(|asset| present(&downloader, asset)) {
        return Ok(Some(downloader.picked_into(assets[0])));
    }
    Ok(None)
}

fn present(downloader: &Downloader, asset: &Asset) -> bool {
    let record = downloader.record_of(asset);
    install_record::matches(&record, asset.url, downloader.root()) == Some(true)
        && install_record::read(&record)
            .is_some_and(|record| Some(record.sha256.as_str()) == sha256(asset.url))
}

#[derive(Serialize)]
pub(crate) struct Setup {
    supported: bool,
    installed: bool,
    missing_bytes: u64,
    reason: Option<String>,
    download: Option<DownloadProgress>,
}

pub(crate) async fn get(State(state): State<AppState>) -> Json<Setup> {
    let downloader = &state.rvc_native_downloads;
    let download = downloader.active_for(SCOPE).await;
    let assets = match selected() {
        Ok(assets) => assets,
        Err(reason) => {
            return Json(Setup {
                supported: false,
                installed: false,
                missing_bytes: 0,
                reason: Some(reason),
                download,
            });
        }
    };
    let missing: Vec<_> = assets
        .iter()
        .filter(|asset| !present(downloader, asset))
        .collect();
    Json(Setup {
        supported: true,
        installed: missing.is_empty(),
        missing_bytes: missing
            .iter()
            .map(|asset| asset.bytes.saturating_sub(downloader.partial_bytes(asset)))
            .sum(),
        reason: None,
        download,
    })
}

pub(crate) async fn start(State(state): State<AppState>) -> impl IntoResponse {
    let assets = match selected() {
        Ok(assets) => assets,
        Err(reason) => return api_error(StatusCode::CONFLICT, reason).into_response(),
    };
    let downloader = state.rvc_native_downloads.clone();
    downloader.clear_finished(SCOPE).await;
    tokio::spawn(async move {
        if let Err(error) = downloader.install_all(SCOPE, &assets).await {
            eprintln!("Could not install native RVC: {error:#}");
        }
    });
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "started": true })),
    )
        .into_response()
}

pub(crate) async fn cancel(State(state): State<AppState>) -> Json<Setup> {
    state.rvc_native_downloads.cancel_for(SCOPE).await;
    get(State(state)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_runtime_has_a_platform_archive_and_sha256_pin() {
        for (asset, digest) in runtimes() {
            assert_eq!(sha256(asset.url), Some(*digest));
            assert!(asset.bytes > 0);
            assert_eq!(digest.len(), 64);
            assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
            assert!(
                asset
                    .relative_path
                    .ends_with(if asset.is_windows_runtime() {
                        ".zip"
                    } else {
                        ".tar.gz"
                    })
            );
        }
    }

    #[test]
    fn status_requires_the_current_install_pin_and_every_recorded_file() {
        let scratch = tempfile::tempdir().unwrap();
        let downloader = downloader(scratch.path());
        let path = downloader.path_of(&HUBERT);
        let record = downloader.record_of(&HUBERT);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"test weights").unwrap();
        assert!(!present(&downloader, &HUBERT));
        assert!(!record.exists());
        install_record::write(
            &record,
            HUBERT.url,
            "obsolete",
            std::slice::from_ref(&path),
            downloader.root(),
        )
        .unwrap();
        assert!(!present(&downloader, &HUBERT));
        install_record::write(
            &record,
            HUBERT.url,
            sha256(HUBERT.url).unwrap(),
            std::slice::from_ref(&path),
            downloader.root(),
        )
        .unwrap();
        assert!(present(&downloader, &HUBERT));
        std::fs::remove_file(&path).unwrap();
        assert!(!present(&downloader, &HUBERT));
    }

    #[tokio::test]
    #[ignore = "requires an RTX host and the verified runtime, acoustic models and indexed RVC fixture"]
    async fn installs_and_converts_from_the_pinned_runtime() {
        // Supply the release archive, hubert.vfrm, rmvpe.vfrm, input.wav,
        // voice.pth and retrieval.index.
        let fixture = PathBuf::from(std::env::var_os("YUE2_RVC_INSTALL_FIXTURE").unwrap());
        let scratch = tempfile::Builder::new()
            .prefix("rvc-голос-")
            .tempdir_in(&fixture)
            .unwrap();
        let downloader = downloader(scratch.path());
        let assets = selected().unwrap();
        for asset in assets {
            let destination = downloader.path_of(asset);
            std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
            std::fs::hard_link(fixture.join(destination.file_name().unwrap()), destination)
                .unwrap();
        }
        assert!(!scratch.path().join("rvc/native/cache-v1").exists());
        downloader.install_all(SCOPE, &assets).await.unwrap();
        for asset in assets {
            assert!(present(&downloader, asset), "unverified {}", asset.label);
        }
        assert_eq!(crate::rvc_fast::status(scratch.path()), Ok(false));
        assert!(!scratch.path().join("rvc/native/cache-v1").exists());
        assert!(
            installed_worker(scratch.path())
                .unwrap()
                .unwrap()
                .join(assets[0].marker)
                .is_file()
        );
        let vocal = fixture.join("input.wav");
        let voice = fixture.join("voice.pth");
        let index = fixture.join("retrieval.index");
        let audio = crate::rvc_fast::convert(
            scratch.path(),
            &crate::rvc_fast::Request {
                vocal: &vocal,
                voice: &voice,
                index: Some(&index),
                semitone_shift: 0,
                retrieval_blend: 0.5,
            },
        )
        .unwrap();
        assert_eq!(audio.rate, 40_000);
        assert_eq!(audio.frames(), 159_200);
        assert!(audio.peak() > 0.1);
        assert_eq!(crate::rvc_fast::status(scratch.path()), Ok(true));
    }
}
