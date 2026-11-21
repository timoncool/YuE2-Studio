//! SHA-pinned HOT-Step runtime and the shared HeartMuLa cover model files.
use crate::downloads::{Asset, AssetKind, Downloader};
use std::path::{Path, PathBuf};

const RUNTIME_FOLDER: &str = "hotstep-1.5.2-cpu";
const REVISION: &str = "cdef1b0d67af";
const WIN_URL: &str = "https://github.com/scragnog/HOT-Step-CPP/releases/download/v1.5.2/HOT-Step-CPP-v1.5.2-win-x64-cpu.zip";

#[cfg(windows)]
macro_rules! win_member {
    ($name:literal) => {
        concat!("app/v1.5.2-cdef1b0d67af-cpu/engine/", $name)
    };
}
#[cfg(windows)]
const WIN: Asset = Asset {
    id: "hotstep-1.5.2-win-cpu",
    label: "HOT-Step CPU runtime 1.5.2",
    kind: AssetKind::Runtime,
    url: WIN_URL,
    relative_path: "runtime/HOT-Step-CPP-v1.5.2-win-x64-cpu.zip",
    bytes: 158_725_706,
    unzip_into: Some(RUNTIME_FOLDER),
    marker: "ace-server.exe",
    pick: &[
        win_member!("ace-server.exe"),
        win_member!("ggml.dll"),
        win_member!("ggml-base.dll"),
        win_member!("ggml-cpu-alderlake.dll"),
        win_member!("ggml-cpu-cannonlake.dll"),
        win_member!("ggml-cpu-cascadelake.dll"),
        win_member!("ggml-cpu-haswell.dll"),
        win_member!("ggml-cpu-icelake.dll"),
        win_member!("ggml-cpu-sandybridge.dll"),
        win_member!("ggml-cpu-skylakex.dll"),
        win_member!("ggml-cpu-sse42.dll"),
        win_member!("ggml-cpu-x64.dll"),
        win_member!("mp3-codec.exe"),
        win_member!("msvcp140.dll"),
        win_member!("msvcp140_1.dll"),
        win_member!("vcomp140.dll"),
        win_member!("vcruntime140.dll"),
        win_member!("vcruntime140_1.dll"),
    ],
    vram_gb: None,
    note: "Native cover runtime, with the best CPU backend selected for this processor.",
};

macro_rules! model {
    ($repo:literal, $revision:literal, $name:literal, $bytes:literal, $sha:literal) => {
        (Asset {
            id: $name, label: $name, kind: AssetKind::Model,
            url: concat!("https://huggingface.co/", $repo, "/resolve/", $revision, "/", $name),
            relative_path: concat!("models/", $name), bytes: $bytes, unzip_into: None,
            marker: $name, pick: &[], vram_gb: None,
            note: "HOT-Step HeartMuLa MuLaCover weights; codec and tokenizer are shared with HeartMuLa.",
        }, $sha)
    };
}

const MODELS: [(Asset, &str); 7] = [
    model!(
        "scragnog/HeartMuLa-GGUF",
        "fa50b8e6a972214e6ea10decf711c252f88c8d4c",
        "heartmula-mulacover-global-BF16.gguf",
        7755339840,
        "6b7a7d889242ae121f7c1b8d2a9ecd6de3d09eba9e831ee9bd42a0ce020f77a7"
    ),
    model!(
        "scragnog/HeartMuLa-GGUF",
        "fa50b8e6a972214e6ea10decf711c252f88c8d4c",
        "heartmula-mulacover-local-BF16.gguf",
        994370080,
        "5d35548e2474ca5bcc28fde9ed54629ea80d14c8181abc5c1b5f990c84a810d3"
    ),
    model!(
        "scragnog/HeartMuLa-GGUF",
        "fa50b8e6a972214e6ea10decf711c252f88c8d4c",
        "heartcodec-flow-Q8_0.gguf",
        1609869536,
        "f9b67fd65b6873c371b62680cd0ae2d3064d0b1081e8758a4f538d076734f83c"
    ),
    model!(
        "scragnog/HeartMuLa-GGUF",
        "fa50b8e6a972214e6ea10decf711c252f88c8d4c",
        "heartcodec-scalar-decoder-F16.gguf",
        167671648,
        "cc53325408d2527b99f09cfdf65451bee72cb952b0c52ff135d673b4aacb087f"
    ),
    model!(
        "scragnog/HeartMuLa-GGUF",
        "fa50b8e6a972214e6ea10decf711c252f88c8d4c",
        "heartcodec-rvq-F32.gguf",
        17194624,
        "d292f57cbcb204a06f998c8bf0f4a2ef671ba82b1a0071904d102435e3525b44"
    ),
    model!(
        "scragnog/HeartMuLa-GGUF",
        "fa50b8e6a972214e6ea10decf711c252f88c8d4c",
        "heartmula-tokenizer.gguf",
        7852064,
        "d4881e7fd90d6bc80367d909cc1453da93b666f29d4ab89a8befbba00f4c3cb1"
    ),
    model!(
        "Serveurperso/ACE-Step-1.5-GGUF",
        "666ac70204440867d8c01ba4b119cc79c95b370a",
        "Qwen3-Embedding-0.6B-BF16.gguf",
        1196890592,
        "6f57900773fbd447b2943bc8126e2dc68ed009151ba89a47c14d1be05d4c73df"
    ),
];

pub(super) fn sha256(url: &str) -> Option<&'static str> {
    if url == WIN_URL {
        return Some("92de8271d38f16922d44ba176bcad9bff71df24bae301ba3e2594a6c2893d4a3");
    }
    MODELS
        .iter()
        .find_map(|(asset, sha)| (asset.url == url).then_some(*sha))
}

fn server_override() -> Option<std::ffi::OsString> {
    std::env::var_os("YUE_HOTSTEP_SERVER").filter(|path| !path.is_empty())
}

pub(super) fn managed_revision() -> Option<&'static str> {
    (cfg!(windows) && server_override().is_none()).then_some(REVISION)
}

pub(super) fn server(downloader: &Downloader) -> Option<PathBuf> {
    if let Some(path) = server_override() {
        return Some(path.into());
    }
    #[cfg(windows)]
    {
        Some(downloader.runtime_dir(RUNTIME_FOLDER).join(WIN.marker))
    }
    #[cfg(not(windows))]
    {
        let _ = downloader;
        None
    }
}

pub(super) fn model_folder(downloader: &Downloader) -> PathBuf {
    downloader.root().join("models")
}

pub(super) fn missing(downloader: &Downloader) -> Vec<&'static Asset> {
    let mut missing: Vec<_> = MODELS
        .iter()
        .map(|(asset, _)| asset)
        .filter(|asset| {
            !std::fs::metadata(downloader.path_of(asset))
                .is_ok_and(|file| file.is_file() && file.len() == asset.bytes)
        })
        .collect();
    missing.extend(missing_runtime(downloader));
    missing
}

pub(super) fn missing_runtime(downloader: &Downloader) -> Option<&'static Asset> {
    #[cfg(windows)]
    { (server_override().is_none() && !downloader.is_installed(&WIN)).then_some(&WIN) }
    #[cfg(not(windows))]
    { let _ = downloader; None }
}

pub(super) fn ready(downloader: &Downloader) -> bool {
    server(downloader).as_deref().is_some_and(Path::is_file) && missing(downloader).is_empty()
}
