use super::*;
use crate::downloads::{Asset, AssetKind, Downloader};

const PINS: &str = include_str!("rvc_burn_assets.json");
const SCOPE: &str = "rvc-burn-runtime";

#[derive(Deserialize)]
struct Pin {
    label: &'static str,
    url: &'static str,
    file: &'static str,
    bytes: u64,
    sha256: &'static str,
    pick: Vec<&'static str>,
}

fn pins() -> &'static std::collections::BTreeMap<&'static str, Pin> {
    static PINS_LOCK: std::sync::OnceLock<std::collections::BTreeMap<&str, Pin>> = std::sync::OnceLock::new();
    PINS_LOCK.get_or_init(|| serde_json::from_str(PINS).expect("valid Burn component pins"))
}

pub(super) fn sha256(url: &str) -> Option<&'static str> {
    pins().values().find(|pin| pin.url == url).map(|pin| pin.sha256)
}

fn asset(key: &'static str, folder: Option<&'static str>) -> Asset {
    let pin = &pins()[key];
    Asset {
        id: key,
        label: pin.label,
        kind: AssetKind::Runtime,
        url: pin.url,
        relative_path: folder.map_or(pin.file, |flavour| format!("packages/{flavour}/{}", pin.file).leak()),
        bytes: pin.bytes,
        unzip_into: folder,
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
        match self { Self::Cuda12 => "cuda12", Self::Cuda13 => "cuda13" }
    }

    fn runtime(self) -> &'static [Asset; 4] {
        fn assets(profile: Profile, keys: [&'static str; 2]) -> [Asset; 4] {
            let folder = Some(profile.folder());
            [asset(keys[0], folder), asset(keys[1], folder), asset("ort", folder), asset("ffmpeg", folder)]
        }
        static CUDA12: std::sync::OnceLock<[Asset; 4]> = std::sync::OnceLock::new();
        static CUDA13: std::sync::OnceLock<[Asset; 4]> = std::sync::OnceLock::new();
        match self {
            Self::Cuda12 => CUDA12.get_or_init(|| assets(self, ["worker12", "nvrtc12"])),
            Self::Cuda13 => CUDA13.get_or_init(|| assets(self, ["worker13", "nvrtc13"])),
        }
    }

    pub(super) fn assets(self, training: bool) -> impl Iterator<Item = &'static Asset> {
        static MODELS: std::sync::OnceLock<[Asset; 4]> = std::sync::OnceLock::new();
        let models = MODELS.get_or_init(|| ["content", "pitch", "generator", "discriminator"].map(|key| asset(key, None)));
        self.runtime().iter().chain(&models[..if training { 4 } else { 2 }])
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
        profile.assets(true).filter(|asset| !present(downloader, asset))
            .map(|asset| asset.bytes.saturating_sub(downloader.partial_bytes(asset))).sum()
    });
    serde_json::json!({
        "installable": profile.is_ok(),
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
    if downloader.active_for(SCOPE).await.is_some_and(|progress| !progress.done) {
        return (StatusCode::ACCEPTED, Json(serde_json::json!({ "started": true }))).into_response();
    }
    downloader.clear_finished(SCOPE).await;
    tokio::spawn(async move {
        let assets: Vec<_> = profile.assets(true).collect();
        if let Err(error) = downloader.install_all(SCOPE, &assets).await {
            studio_log!("RVC Burn install failed: {error:#}");
        }
    });
    (StatusCode::ACCEPTED, Json(serde_json::json!({ "started": true }))).into_response()
}

pub(super) async fn cancel(State(state): State<AppState>) -> Json<Value> {
    state.rvc_burn_downloads.cancel_for(SCOPE).await;
    rvc_burn::status(State(state)).await
}
