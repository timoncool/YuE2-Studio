//! The managed HOT-Step Stable Audio 3 Medium BF16 generation pack.
use crate::downloads::{Asset, AssetKind, Downloader};

macro_rules! model {
    ($name:literal, $folder:literal, $bytes:literal, $sha:literal) => {
        (
            Asset {
                id: $name,
                label: $name,
                kind: AssetKind::Model,
                url: concat!("https://huggingface.co/scragnog/HOT-Step-CPP-StableStep/resolve/22f23e05097301a50df2d4c1573514333069c8e0/", $name),
                relative_path: concat!("models/", $folder, $name),
                bytes: $bytes,
                unzip_into: None,
                marker: $name,
                pick: &[],
                vram_gb: None,
                note: "HOT-Step Stable Audio 3 Medium weights and tokenizer.",
            },
            $sha,
        )
    };
}

pub(super) const MODELS: [(Asset, &str); 7] = [
    model!("sa3-dit-BF16.gguf", "", 2_908_308_288, "550ecd0f7dd7b8bd30c3da8e6370e39393898fd1c9eace3e10714f5102e80492"),
    model!("sa3-same-enc-F16.gguf", "", 852_575_904, "465257fdea87aa80a3fe77459e6130a6eeeb230e4249aea5854edb6ce63fc7a5"),
    model!("sa3-same-dec-F16.gguf", "", 852_574_880, "c9e6ec019d0aa834c575e839f331ddca90cc3e080a8fddfcfd57db058b308a69"),
    model!("sa3-text-enc-BF16.gguf", "", 1_183_213_792, "9945540fe3d89d3a2736c7125cf9e677775013607e7a53840a7f2deed055a3ef"),
    model!("tokenizer.json", "onnx/sa3/", 34_362_429, "7794135caa3ea73918949c902a781cc61dab674a4b59c17d85931c77c1114cbd"),
    model!("tokenizer_config.json", "onnx/sa3/", 46_437, "9546baec0dfefec0e29873edea9488ecde153f846e18c06ea48cb44587bf408f"),
    model!("special_tokens_map.json", "onnx/sa3/", 636, "baec30ea10906f16adb8c18af7a34023002c1746542612b8b41c9f09e1351351"),
];

pub(super) fn missing(downloader: &Downloader) -> Vec<&'static Asset> {
    MODELS.iter().map(|(asset, _)| asset).filter(|asset| {
        !std::fs::metadata(downloader.path_of(asset))
            .is_ok_and(|file| file.is_file() && file.len() == asset.bytes)
    }).collect()
}

pub(super) fn sha256(url: &str) -> Option<&'static str> {
    MODELS.iter().find_map(|(asset, sha)| (asset.url == url).then_some(*sha))
}
