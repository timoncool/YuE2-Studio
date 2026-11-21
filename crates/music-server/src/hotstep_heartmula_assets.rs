use crate::downloads::{Asset, AssetKind, Downloader};

macro_rules! weight {
    ($name:literal, $bytes:literal, $sha:literal) => {
        (Asset {
            id: $name, label: $name, kind: AssetKind::Model,
            url: concat!("https://huggingface.co/scragnog/HeartMuLa-GGUF/resolve/fa50b8e6a972214e6ea10decf711c252f88c8d4c/", $name),
            relative_path: concat!("models/", $name), bytes: $bytes, unzip_into: None,
            marker: $name, pick: &[], vram_gb: None, note: "HOT-Step HeartMuLa 3B and HeartCodec.",
        }, $sha)
    };
}

const BACKBONE: [(Asset, &str); 2] = [
    weight!(
        "heartmula-3b-global-Q8_0.gguf",
        3_658_162_880,
        "54546b73a4a1a47af91d064c9cf36f9f58ab817bb5b8782aa1d49f952aba247e"
    ),
    weight!(
        "heartmula-3b-local-Q8_0.gguf",
        528_309_344,
        "0533e957c05feb704f32cc422a17818035e63dda64ce41f878cd6bfb6d823724"
    ),
];

const ENCODER: [(Asset, &str); 4] = [
    weight!(
        "heartcodec-muencoder-F16.gguf",
        651_074_464,
        "871ac1e66785a1ed6eee6b4f79c32f69556965223c5f701f2967123abde7e864"
    ),
    weight!(
        "heartcodec-wavlm-F16.gguf",
        189_171_488,
        "afe8f3f6da22339da19d9c80c6605a69a0c03077f04d9193d4e8f0e5b82476c6"
    ),
    weight!(
        "heartcodec-whisper-F16.gguf",
        178_950_976,
        "2ab07c9e51c64d9d474b5e4e7c549e4c8d815194945cf91dd6e57bc9f46885fc"
    ),
    weight!(
        "heartcodec-query-F16.gguf",
        59_549_952,
        "446d0ce24ef7f314f4f59e4f9d941f27081f50ae0625e6e4dcfbecd1262dce98"
    ),
];

pub(super) fn missing(downloader: &Downloader, training: bool) -> Vec<&'static Asset> {
    BACKBONE
        .iter()
        .chain(training.then_some(ENCODER.iter()).into_iter().flatten())
        .map(|(asset, _)| asset)
        .chain(super::hotstep_cover_assets::heartmula_shared())
        .filter(|asset| !super::hotstep_sa3_train_assets::has_size(downloader, asset))
        .collect()
}

pub(super) fn sha256(url: &str) -> Option<&'static str> {
    BACKBONE
        .iter()
        .chain(ENCODER.iter())
        .find_map(|(asset, sha)| (asset.url == url).then_some(*sha))
}
