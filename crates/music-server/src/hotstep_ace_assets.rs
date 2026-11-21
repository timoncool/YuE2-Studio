use crate::downloads::{Asset, AssetKind};

macro_rules! item {
    ($name:literal, $url:expr, $path:expr, $size:literal, $sha:literal) => {
        (
            Asset {
                id: $name,
                label: $name,
                kind: AssetKind::Model,
                url: $url,
                relative_path: $path,
                bytes: $size,
                unzip_into: None,
                marker: $name,
                pick: &[],
                vram_gb: None,
                note: "ACE-Step 1.5 Turbo for the published HOT-Step profile",
            },
            $sha,
        )
    };
}

macro_rules! model {
    ($name:literal, $size:literal, $sha:literal) => {
        item!($name,
            concat!("https://huggingface.co/Serveurperso/ACE-Step-1.5-GGUF/resolve/666ac70204440867d8c01ba4b119cc79c95b370a/", $name),
            concat!("models/ace-step-1.5/", $name), $size, $sha)
    };
}

macro_rules! plugin {
    ($folder:literal, $name:literal, $size:literal, $sha:literal) => {
        item!($name,
            concat!("https://raw.githubusercontent.com/scragnog/HOT-Step-CPP/cdef1b0d67afa96b4b202ce69d0aed793af2be2e/engine/plugins/", $folder, "/", $name),
            concat!("runtime/hotstep-1.5.2-cpu/plugins/", $folder, "/", $name), $size, $sha)
    };
}

pub(super) const ASSETS: [(Asset, &str); 5] = [
    model!(
        "acestep-v15-turbo-Q8_0.gguf",
        2549528000,
        "288f708a61cfc241013a98a62f98ba331f83fe34d0d3559acdd9b0f6a2f7cd6b"
    ),
    model!(
        "Qwen3-Embedding-0.6B-BF16.gguf",
        1196890592,
        "6f57900773fbd447b2943bc8126e2dc68ed009151ba89a47c14d1be05d4c73df"
    ),
    model!(
        "vae-BF16.gguf",
        337420928,
        "0599862ac5d15cd308e1d2e368373aea6c02e25ebd1737ad4a4562a0901b0ef8"
    ),
    plugin!(
        "solvers",
        "euler.lua",
        521,
        "db90641436e49d2d1f74ebcb26b5919903190b0dc4a4b69b28387885bed4e557"
    ),
    plugin!(
        "guidance",
        "apg.lua",
        1073,
        "ea804bb957fd99724acb7b199d5214526c350c0355353de155fb8fc5aa869f4b"
    ),
];
