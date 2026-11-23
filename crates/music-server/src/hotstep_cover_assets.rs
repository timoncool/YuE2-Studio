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

/// A HOT-Step set of the component catalogue (`distribution/components-src/hotstep.json`) with each file's SHA-256.
pub(super) fn set(id: &str) -> Vec<(&'static Asset, &'static str)> {
    crate::components::set_files("hotstep", id).unwrap_or_else(|| panic!("the component catalogue has no HOT-Step set {id}"))
}

/// The MuLaCover weights; the codec and tokenizer are HeartMuLa's.
fn models() -> Vec<(&'static Asset, &'static str)> {
    set("mulacover")
}

pub(super) fn sha256(url: &str) -> Option<&'static str> {
    (url == WIN_URL).then_some("92de8271d38f16922d44ba176bcad9bff71df24bae301ba3e2594a6c2893d4a3")
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
    let mut missing: Vec<_> = models()
        .into_iter()
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
