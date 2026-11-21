use super::*;
use crate::downloads::{Asset, AssetKind, Downloader};
use sha2::{Digest, Sha256};

const PINS: &str = include_str!("rvc_burn_assets.json");
const SCOPE: &str = "rvc-burn-runtime";

#[derive(Deserialize)]
struct Pin<'a> {
    label: &'a str,
    url: &'a str,
    file: &'a str,
    bytes: u64,
    sha256: &'a str,
    #[serde(borrow)]
    pick: Vec<&'a str>,
    #[serde(default)]
    headers: bool,
}

fn pins() -> &'static std::collections::BTreeMap<&'static str, Pin<'static>> {
    static PINS_LOCK: std::sync::OnceLock<std::collections::BTreeMap<&str, Pin<'static>>> =
        std::sync::OnceLock::new();
    PINS_LOCK.get_or_init(|| serde_json::from_str(PINS).expect("valid Burn component pins"))
}

pub(super) fn sha256(url: &str) -> Option<&'static str> {
    pins()
        .values()
        .find(|pin| pin.url == url)
        .map(|pin| pin.sha256)
}

fn asset(key: &'static str, folder: Option<&'static str>) -> Asset {
    let pin = &pins()[key];
    Asset {
        id: key,
        label: pin.label,
        kind: AssetKind::Runtime,
        url: pin.url,
        relative_path: folder.map_or(pin.file, |flavour| {
            format!("packages/{flavour}/{}", pin.file).leak()
        }),
        bytes: pin.bytes,
        unzip_into: folder.filter(|_| !pin.headers),
        marker: pin.pick.first().copied().unwrap_or(pin.file),
        pick: &pin.pick,
        vram_gb: None,
        note: "Native RVC Burn component",
    }
}

#[derive(Clone, Copy)]
pub(super) enum Profile {
    Cuda12,
    Cuda13,
}

impl Profile {
    pub(super) fn folder(self) -> &'static str {
        match self {
            Self::Cuda12 => "cuda12",
            Self::Cuda13 => "cuda13",
        }
    }

    pub(super) fn from_folder(folder: &std::path::Path) -> Option<Self> {
        match folder.file_name()?.to_str()? {
            "cuda12" => Some(Self::Cuda12),
            "cuda13" => Some(Self::Cuda13),
            _ => None,
        }
    }

    fn runtime(self) -> &'static [Asset; 7] {
        static CUDA12: std::sync::OnceLock<[Asset; 7]> = std::sync::OnceLock::new();
        static CUDA13: std::sync::OnceLock<[Asset; 7]> = std::sync::OnceLock::new();
        let folder = Some(self.folder());
        match self {
            Self::Cuda12 => CUDA12.get_or_init(|| {
                [
                    "worker12", "nvrtc12", "ort", "ffmpeg", "cudart12", "crt12", "cccl12",
                ]
                .map(|key| asset(key, folder))
            }),
            Self::Cuda13 => CUDA13.get_or_init(|| {
                [
                    "worker13", "nvrtc13", "ort", "ffmpeg", "cudart13", "crt13", "cccl13",
                ]
                .map(|key| asset(key, folder))
            }),
        }
    }

    fn sdk_assets(self) -> impl Iterator<Item = &'static Asset> {
        self.runtime()
            .iter()
            .filter(|asset| pins()[asset.id].headers)
    }

    pub(super) fn sdk_dir(self, worker: &std::path::Path) -> PathBuf {
        sdk_dir(
            worker,
            self.sdk_assets().map(|asset| pins()[asset.id].sha256),
        )
    }

    pub(super) fn assets(self, training: bool) -> impl Iterator<Item = &'static Asset> {
        static MODELS: std::sync::OnceLock<[Asset; 4]> = std::sync::OnceLock::new();
        let models = MODELS.get_or_init(|| {
            ["content", "pitch", "generator", "discriminator"].map(|key| asset(key, None))
        });
        self.runtime()
            .iter()
            .chain(&models[..if training { 4 } else { 2 }])
    }
}

fn sdk_dir<'a>(worker: &std::path::Path, hashes: impl IntoIterator<Item = &'a str>) -> PathBuf {
    let mut digest = Sha256::new();
    for hash in hashes {
        digest.update(hash.as_bytes());
        digest.update([0]);
    }
    let hex: String = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    worker.join(format!("cuda-sdk-{hex}"))
}

// NVRTC expects the combined SDK layout, including NVIDIA's separate header archives.
pub(super) fn prepare_sdk(profile: Profile, downloader: &Downloader) -> anyhow::Result<()> {
    let worker = downloader.runtime_dir(profile.folder());
    let sdk = profile.sdk_dir(&worker);
    install_sdk(
        &worker,
        &sdk,
        profile.sdk_assets().map(|asset| downloader.path_of(asset)),
    )
}

fn install_sdk(
    worker: &std::path::Path,
    sdk: &std::path::Path,
    archives: impl IntoIterator<Item = PathBuf>,
) -> anyhow::Result<()> {
    static PREPARE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = PREPARE.lock().unwrap_or_else(|error| error.into_inner());
    let complete = |root: &std::path::Path| {
        root.join("include/cuda_runtime.h").is_file()
            && root.join("include/crt/host_config.h").is_file()
            && (root.join("include/cuda/std/type_traits").is_file()
                || root.join("include/cccl/cuda/std/type_traits").is_file())
    };
    if complete(sdk) {
        return Ok(());
    }
    let stage = tempfile::tempdir_in(worker)?;
    for archive in archives {
        use anyhow::Context as _;
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&archive)?)
            .with_context(|| format!("Read CUDA SDK archive {}", archive.display()))?;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index)?;
            let Some(path) = entry.enclosed_name() else {
                continue;
            };
            let relative: PathBuf = path.components().skip(1).collect();
            if entry.is_dir() || !relative.starts_with("include") {
                continue;
            }
            let path = stage.path().join(relative);
            std::fs::create_dir_all(path.parent().context("CUDA header path")?)?;
            std::io::copy(&mut entry, &mut std::fs::File::create(path)?)?;
        }
    }
    anyhow::ensure!(
        complete(stage.path()),
        "The CUDA Runtime Compilation headers are incomplete.",
    );
    // Use a same-volume temporary directory so only a complete SDK becomes visible.
    if sdk.is_dir() {
        let old = tempfile::tempdir_in(worker)?;
        std::fs::rename(sdk, old.path().join("cuda-sdk"))?;
    }
    match std::fs::rename(stage.path(), sdk) {
        Ok(()) => Ok(()),
        Err(_) if complete(sdk) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn selected() -> Result<Profile, &'static str> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Err("The managed RVC Burn runtime is available on Windows x64.");
    }
    let hardware = hardware::hardware();
    let Some(compute) = hardware.compute_capability else {
        return Err("RVC Burn needs an NVIDIA CUDA card.");
    };
    if hardware.driver_major.unwrap_or(0) >= 610 && compute >= (7, 5) {
        return Ok(Profile::Cuda13);
    }
    // Burn compiles kernels using NVRTC. Its PTX needs the toolkit's driver,
    // rather than the older minor-version minimum for precompiled kernels.
    if hardware.driver_major.unwrap_or(0) >= 577 && compute >= (5, 2) {
        return Ok(Profile::Cuda12);
    }
    Err("RVC Burn needs an NVIDIA driver from the 577 series or newer.")
}

pub(super) fn downloader(data: &std::path::Path) -> Downloader {
    let root = data.join("rvc/burn");
    Downloader::with_runtimes(root.clone(), root.join("runtime"))
}

pub(super) fn present(downloader: &Downloader, asset: &Asset) -> bool {
    downloader.is_installed(asset)
        && install_record::read(&downloader.record_of(asset))
            .is_some_and(|record| Some(record.sha256.as_str()) == sha256(asset.url))
}

pub(super) async fn status(state: &AppState) -> Value {
    let downloader = &state.rvc_burn_downloads;
    let profile = selected();
    let download = downloader.active_for(SCOPE).await;
    let missing_bytes = profile.as_ref().ok().map_or(0, |profile| {
        profile
            .assets(true)
            .filter(|asset| !present(downloader, asset))
            .map(|asset| asset.bytes.saturating_sub(downloader.partial_bytes(asset)))
            .sum()
    });
    serde_json::json!({
        "installable": profile.is_ok(),
        "profile": profile.as_ref().ok().map(|profile| profile.folder()),
        "missing_bytes": missing_bytes,
        "install_reason": profile.err(),
        "download": download,
    })
}

pub(super) async fn start(State(state): State<AppState>) -> impl IntoResponse {
    let profile = match selected() {
        Ok(profile) => profile,
        Err(reason) => return api_error(StatusCode::CONFLICT, reason.to_owned()).into_response(),
    };
    let downloader = state.rvc_burn_downloads.clone();
    if downloader
        .active_for(SCOPE)
        .await
        .is_some_and(|progress| !progress.done)
    {
        return (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({ "started": true })),
        )
            .into_response();
    }
    downloader.clear_finished(SCOPE).await;
    tokio::spawn(async move {
        let assets: Vec<_> = profile.assets(true).collect();
        if let Err(error) = downloader.install_all(SCOPE, &assets).await {
            studio_log!("RVC Burn install failed: {error:#}");
        }
    });
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "started": true })),
    )
        .into_response()
}

pub(super) async fn cancel(State(state): State<AppState>) -> Json<Value> {
    state.rvc_burn_downloads.cancel_for(SCOPE).await;
    rvc_burn::status(State(state)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn sdk_uses_current_pins_and_survives_concurrent_starts() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let make_archive = |name: &str, marker: &str| {
            let path = root.join(name);
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            for header in [
                "cuda_runtime.h",
                "crt/host_config.h",
                "cccl/cuda/std/type_traits",
            ] {
                zip.start_file(
                    format!("vendor/include/{header}"),
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
                zip.write_all(marker.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
            path
        };
        let old_archive = make_archive("old.zip", "old");
        let new_archive = make_archive("new.zip", "new");
        let old_pin = install_record::sha256_of(&old_archive).unwrap();
        let new_pin = install_record::sha256_of(&new_archive).unwrap();
        let old_sdk = sdk_dir(root, [old_pin.as_str(), "crt", "cccl"]);
        let new_sdk = sdk_dir(root, [new_pin.as_str(), "crt", "cccl"]);
        install_sdk(root, &old_sdk, [old_archive]).unwrap();
        std::thread::scope(|scope| {
            let first = scope.spawn(|| install_sdk(root, &new_sdk, [new_archive.clone()]));
            let second = scope.spawn(|| install_sdk(root, &new_sdk, [new_archive.clone()]));
            first.join().unwrap().unwrap();
            second.join().unwrap().unwrap();
        });
        assert_ne!(old_sdk, new_sdk);
        assert_eq!(
            std::fs::read_to_string(old_sdk.join("include/cuda_runtime.h")).unwrap(),
            "old"
        );
        assert_eq!(
            std::fs::read_to_string(new_sdk.join("include/cuda_runtime.h")).unwrap(),
            "new"
        );
    }
}
