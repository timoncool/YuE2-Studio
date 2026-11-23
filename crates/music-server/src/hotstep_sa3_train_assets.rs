use crate::downloads::{Asset, AssetKind, Downloader};
use std::path::PathBuf;

const FOLDER: &str = "hotstep-1.5.2-cuda12.8";
const ZIP_URL: &str = "https://github.com/scragnog/HOT-Step-CPP/releases/download/v1.5.2/HOT-Step-CPP-v1.5.2-win-x64-cuda12.8.zip";
pub(super) const BASE: &str = "sa3-dit-base-BF16.gguf";

/// The Stable Audio 3 base the trainer starts from, in the component catalogue's HOT-Step sets.
fn train_base() -> &'static Asset {
    super::hotstep_cover_assets::set("stable-audio-3-train")[0].0
}

/// The CUDA 12 libraries the trainer loads beside itself.
#[cfg(windows)]
fn cuda() -> Vec<&'static Asset> {
    super::hotstep_cover_assets::set("trainer-cuda12").into_iter().map(|(asset, _)| asset).collect()
}

#[cfg(windows)]
macro_rules! member {
    ($name:literal) => {
        concat!("app/v1.5.2-cdef1b0d67af-cuda12.8/engine/", $name)
    };
}

#[cfg(windows)]
const TRAINER: Asset = Asset {
    id: "hotstep-1.5.2-win-cuda12",
    label: "HOT-Step CUDA 12.8 trainer 1.5.2",
    kind: AssetKind::Runtime,
    url: ZIP_URL,
    relative_path: "runtime/HOT-Step-CPP-v1.5.2-win-x64-cuda12.8.zip",
    bytes: 304_618_168,
    unzip_into: Some(FOLDER),
    marker: "ace-train.exe",
    pick: &[
        member!("ace-train.exe"),
        member!("ggml.dll"),
        member!("ggml-base.dll"),
        member!("ggml-cuda.dll"),
        member!("ggml-cpu-alderlake.dll"),
        member!("ggml-cpu-cannonlake.dll"),
        member!("ggml-cpu-cascadelake.dll"),
        member!("ggml-cpu-haswell.dll"),
        member!("ggml-cpu-icelake.dll"),
        member!("ggml-cpu-sandybridge.dll"),
        member!("ggml-cpu-skylakex.dll"),
        member!("ggml-cpu-sse42.dll"),
        member!("ggml-cpu-x64.dll"),
        member!("msvcp140.dll"),
        member!("msvcp140_1.dll"),
        member!("vcomp140.dll"),
        member!("vcruntime140.dll"),
        member!("vcruntime140_1.dll"),
    ],
    vram_gb: None,
    note: "Native HOT-Step CUDA trainer. The CPU server remains available for generation.",
};

fn trainer_override() -> Option<PathBuf> {
    std::env::var_os("YUE_HOTSTEP_TRAINER")
        .filter(|path| !path.is_empty())
        .map(Into::into)
}

pub(super) fn trainer(downloader: &Downloader) -> Option<PathBuf> {
    if let Some(path) = trainer_override() {
        return Some(path);
    }
    #[cfg(windows)]
    {
        Some(downloader.runtime_dir(FOLDER).join(TRAINER.marker))
    }
    #[cfg(not(windows))]
    {
        let _ = (downloader, FOLDER);
        None
    }
}

pub(super) fn missing_models(downloader: &Downloader) -> Vec<&'static Asset> {
    let mut weights = super::hotstep_sa3_assets::missing(downloader);
    let base = train_base();
    if !has_size(downloader, base) {
        weights.push(base);
    }
    weights
}

pub(super) fn missing(downloader: &Downloader) -> Vec<&'static Asset> {
    let mut list = missing_models(downloader);
    list.extend(missing_runtime(downloader));
    list
}

pub(super) fn missing_runtime(downloader: &Downloader) -> Vec<&'static Asset> {
    let mut list = Vec::new();
    #[cfg(windows)]
    if trainer_override().is_none() {
        if !downloader.is_installed(&TRAINER) {
            list.push(&TRAINER);
        }
        list.extend(cuda().into_iter().filter(|asset| !has_size(downloader, asset)));
    }
    list
}

pub(super) fn has_size(downloader: &Downloader, asset: &Asset) -> bool {
    std::fs::metadata(downloader.path_of(asset))
        .is_ok_and(|meta| meta.is_file() && meta.len() == asset.bytes)
}

pub(super) fn sha256(url: &str) -> Option<&'static str> {
    (url == ZIP_URL).then_some("a7a0d836dd053c74a3aeb5eb40014bbbf079f7f04dbf387423fa2f24a9a6e446")
}
