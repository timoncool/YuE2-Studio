//! The managed HOT-Step Stable Audio 3 Medium BF16 generation pack.
use crate::downloads::{Asset, Downloader};

pub(super) fn missing(downloader: &Downloader) -> Vec<&'static Asset> {
    super::hotstep_cover_assets::set("stable-audio-3")
        .into_iter()
        .map(|(asset, _)| asset)
        .filter(|asset| {
            !std::fs::metadata(downloader.path_of(asset))
                .is_ok_and(|file| file.is_file() && file.len() == asset.bytes)
        })
        .collect()
}
