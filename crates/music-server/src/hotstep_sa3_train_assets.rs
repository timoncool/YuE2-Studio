use crate::downloads::{Asset, AssetKind, Downloader};
use std::path::PathBuf;

const FOLDER: &str = "hotstep-1.5.2-cuda12.8";
const ZIP_URL: &str = "https://github.com/scragnog/HOT-Step-CPP/releases/download/v1.5.2/HOT-Step-CPP-v1.5.2-win-x64-cuda12.8.zip";
pub(super) const BASE: &str = "sa3-dit-base-BF16.gguf";

macro_rules! asset {
    ($url:expr, $path:expr, $name:literal, $bytes:literal, $sha:literal) => {
        (
            Asset {
                id: $name,
                label: $name,
                kind: AssetKind::Model,
                url: $url,
                relative_path: $path,
                bytes: $bytes,
                unzip_into: None,
                marker: $name,
                pick: &[],
                vram_gb: None,
                note: "HOT-Step Stable Audio 3 CUDA training component.",
            },
            $sha,
        )
    };
}

const TRAIN_BASE: (Asset, &str) = asset!(
    "https://huggingface.co/scragnog/Stable-Audio-3-Medium-Base-GGUF/resolve/dd7740c5e37ea21c2981cc48e7e53ca10fe5a577/sa3-dit-base-BF16.gguf",
    "models/sa3-dit-base-BF16.gguf",
    "sa3-dit-base-BF16.gguf",
    2_908_305_984,
    "9b75b6e470e756e867b50eb02f0174dea0db2d7f17790ec58a6aa9a411b45563"
);

macro_rules! cuda {
    ($name:literal, $bytes:literal, $sha:literal) => {
        asset!(
            concat!("https://huggingface.co/scragnog/HOT-Step-CPP-SuperSep/resolve/440487b8300dcd61453cc52ec244a38150b03456/runtime/cuda12/", $name),
            concat!("runtime/hotstep-1.5.2-cuda12.8/", $name),
            $name, $bytes, $sha
        )
    };
}

const CUDA: [(Asset, &str); 3] = [
    cuda!(
        "cublas64_12.dll",
        113_712_640,
        "0a60a7ecbf906f7f3842826fecaf412f1587ff6269729339245a1ec224364161"
    ),
    cuda!(
        "cublasLt64_12.dll",
        692_441_600,
        "a21ddfc1c9cd090ab07d2c6aad235aa4f15fe60fe896d5db28adf7a279c09ef3"
    ),
    cuda!(
        "cudart64_12.dll",
        573_952,
        "9d9b868955149875ac4ef43442aaaa8913d4a7b3e4d6dd60ee871626aa045768"
    ),
];

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
    if !has_size(downloader, &TRAIN_BASE.0) {
        weights.push(&TRAIN_BASE.0);
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
        list.extend(
            CUDA.iter()
                .map(|(asset, _)| asset)
                .filter(|asset| !has_size(downloader, asset)),
        );
    }
    list
}

pub(super) fn has_size(downloader: &Downloader, asset: &Asset) -> bool {
    std::fs::metadata(downloader.path_of(asset))
        .is_ok_and(|meta| meta.is_file() && meta.len() == asset.bytes)
}

pub(super) fn sha256(url: &str) -> Option<&'static str> {
    if url == ZIP_URL {
        return Some("a7a0d836dd053c74a3aeb5eb40014bbbf079f7f04dbf387423fa2f24a9a6e446");
    }
    std::iter::once(&TRAIN_BASE)
        .chain(CUDA.iter())
        .find_map(|(asset, sha)| (asset.url == url).then_some(*sha))
}
