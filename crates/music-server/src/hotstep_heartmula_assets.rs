use crate::downloads::{Asset, Downloader};

/// HeartMuLa 3B with HeartCodec and its tokenizer, and for training the encoders: HOT-Step sets of the component
/// catalogue.
pub(super) fn missing(downloader: &Downloader, training: bool) -> Vec<&'static Asset> {
    super::hotstep_cover_assets::set("heartmula")
        .into_iter()
        .chain(training.then(|| super::hotstep_cover_assets::set("heartmula-train")).into_iter().flatten())
        .map(|(asset, _)| asset)
        .filter(|asset| !super::hotstep_sa3_train_assets::has_size(downloader, asset))
        .collect()
}
