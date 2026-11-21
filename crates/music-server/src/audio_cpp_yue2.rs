//! Packed YuE2 with the tested V9 NAR companion on audio.cpp.
use super::*;
use acpp_server::{Mode, ModelEntry};
use anyhow::ensure;
use std::{io::Read, path::Path as FsPath};

pub(super) const FAMILY: &str = "yue2";
pub(super) const PROFILE: &str = "yue2-3b-q8_0-nar-v9";
const DIRECTORY: &str = "Yue2-3B-GGUF";
const NAR: &str = "nar_lora_joint_v9.safetensors";

macro_rules! asset {
    ($file:literal, $url:expr, $bytes:literal, $sha:literal) => {
        (
            downloads::Asset {
                id: $file,
                label: $file,
                kind: downloads::AssetKind::Model,
                url: $url,
                relative_path: concat!("models/Yue2-3B-GGUF/", $file),
                bytes: $bytes,
                unzip_into: None,
                marker: $file,
                pick: &[],
                vram_gb: None,
                note: "YuE2 Q8_0 for audio.cpp",
            },
            $sha,
        )
    };
}

macro_rules! packed {
    ($file:literal, $bytes:literal, $sha:literal) => {
        asset!($file, concat!("https://huggingface.co/audio-cpp/Yue2-3B-GGUF/resolve/cca74d8980c00f30ab8d52248a367c01ad633ec2/", $file), $bytes, $sha)
    };
}

pub(super) const ASSETS: [(downloads::Asset, &str); 7] = [
    packed!(
        "yue2-3b-q8_0.gguf",
        4264195616,
        "52be345b5155bf9ef0b2411e7df8d1d2b8e13b6d977a2f70c35612ec69b1f591"
    ),
    packed!(
        "yue2-vae-f16.gguf",
        265218656,
        "d4f4a05d8f291ae820cd1e43609da3fa91b56465810091a2b08c3350b751719d"
    ),
    packed!(
        "sidecars/yue2-model-config.json",
        959,
        "ad3477bbef890bf98ae196c1e4b44779494a6231c4ab66f32708eabadf265329"
    ),
    packed!(
        "sidecars/yue2-generation-config.json",
        466,
        "203830cebde6e3644eb291925362990d198e66c3b6006cd11f1aaa21904bcc61"
    ),
    packed!(
        "sidecars/yue2-qwen.tiktoken",
        2561218,
        "b2b1b8dfb5cc5f024bafc373121c6aba3f66f9a5a0269e243470a1de16a33186"
    ),
    packed!(
        "sidecars/yue2-vae-config.json",
        1378,
        "f0191bb9694009956de44e0c361a6f1334760be4c8f848e599bde242a54a0970"
    ),
    asset!(
        "nar_lora_joint_v9.safetensors",
        "https://huggingface.co/Mothersuperior/yue2-mothersuperior-realaudio-tokenizer-v4/resolve/e2e63d859f3af879baf1b4d4e9f22d1eeda6fde5/nar_lora_joint_v9.safetensors",
        140560592,
        "585f303da1d5252d228d1e8ac6d4c4d11d970df9297406935cc8bdafa49cfa7e"
    ),
];

pub(super) fn sha256(url: &str) -> Option<&'static str> {
    ASSETS
        .iter()
        .find_map(|(item, sha)| (item.url == url).then_some(*sha))
}

pub(super) fn model_dir(root: &FsPath) -> PathBuf {
    root.join("audiocpp/models").join(DIRECTORY)
}

pub(super) fn missing_files(directory: &FsPath) -> Vec<&'static str> {
    ASSETS
        .iter()
        .filter(|(asset, sha)| {
            let path = directory.join(asset.marker);
            if !fs::metadata(&path).is_ok_and(|meta| meta.is_file() && meta.len() == asset.bytes) {
                return true;
            }
            if asset.marker.ends_with(".gguf") {
                let mut header = [0; 4];
                return !fs::File::open(&path).is_ok_and(|mut file| {
                    file.read_exact(&mut header).is_ok() && header == *b"GGUF"
                });
            }
            asset.marker.starts_with("sidecars/") && !crate::pin_sha::holds(&path, sha)
        })
        .map(|(asset, _)| asset.marker)
        .collect()
}

pub(super) fn catalog(root: Option<&FsPath>, runtime_ready: bool) -> Value {
    let mut missing = root.map_or_else(
        || vec!["Studio data folder"],
        |root| missing_files(&model_dir(root)),
    );
    let missing_bytes = ASSETS
        .iter()
        .filter(|(asset, _)| missing.contains(&asset.marker) || root.is_none())
        .map(|(asset, _)| asset.bytes)
        .sum::<u64>();
    if !runtime_ready {
        missing.push("audio.cpp runtime");
    }
    serde_json::json!({"id":FAMILY,"name":"YuE2","profile_id":PROFILE,"quantization":"Q8_0",
        "status":"experimental","ready":missing.is_empty(),"missing":missing,"missing_bytes":missing_bytes,
        "bytes":ASSETS.iter().map(|(asset, _)| asset.bytes).sum::<u64>(),"installable":true,
        "license_url":"https://huggingface.co/Mothersuperior/yue2-mothersuperior-realaudio-tokenizer-v4/tree/e2e63d859f3af879baf1b4d4e9f22d1eeda6fde5"})
}

pub(super) fn entry(path: PathBuf) -> ModelEntry {
    let companion = path.join(NAR);
    ModelEntry {
        id: FAMILY.into(),
        family: FAMILY.into(),
        task: "gen".into(),
        mode: Mode::Offline,
        path,
        load_options: Default::default(),
        default_request_options: Default::default(),
        session_options: serde_json::Map::from_iter([
            ("yue2.nar_lora".into(), serde_json::json!(companion)),
            ("yue2.nar_lora_scale".into(), serde_json::json!("1.0")),
        ]),
        busy_timeout_ms: None,
        output_rate: None,
    }
}

pub(super) fn task(request: &CreateMusicJobRequest) -> anyhow::Result<Value> {
    ensure!(
        !request.style.trim().is_empty(),
        "Enter a YuE2 style prompt."
    );
    ensure!(
        request.adapters.is_empty() && request.cover_of.is_none() && !request.abc_continue,
        "This YuE2 audio.cpp profile accepts style, lyrics and a complete ABC score or semantic prefix."
    );
    let seed = request.seed.unwrap_or(1234);
    let steps = request.steps.unwrap_or(8);
    let duration = request.duration_seconds.unwrap_or(30.0);
    let cot = request.cot.as_deref().unwrap_or("full");
    ensure!(
        seed >= 0 && (1..=100).contains(&steps) && (1.0..=300.0).contains(&duration),
        "Enter a nonnegative seed, 1–100 steps and a duration between 1 and 300 seconds."
    );
    ensure!(
        matches!(cot, "off" | "melody" | "full"),
        "Choose a valid YuE2 planning mode."
    );
    let cap = (duration * 25.0).round() as u32;
    let mut options =
        serde_json::json!({"style":request.style,"cot":cot,"semantic_max_tokens":cap.to_string()});
    if let Some(abc) = request.abc.as_deref().filter(|abc| !abc.trim().is_empty()) {
        ensure!(cot != "off", "ABC needs melody or full mode.");
        options["abc"] = serde_json::json!(abc);
    }
    if let Some(tokens) = &request.semantic_tokens {
        let codes = tokens
            .split(',')
            .map(|token| token.trim().parse::<u16>())
            .collect::<Result<Vec<_>, _>>()?;
        ensure!(
            !codes.is_empty()
                && codes.len() <= cap as usize
                && codes.iter().all(|&code| code < 32768),
            "Enter a semantic prefix within the duration and YuE2 vocabulary."
        );
        options["semantic_prefix"] = serde_json::json!(serde_json::to_string(&codes)?);
    }
    if let Some(sampling) = &request.semantic_sampling {
        for (name, setting) in [
            (
                "semantic_temperature",
                sampling.temperature.map(|value| value.to_string()),
            ),
            (
                "semantic_top_p",
                sampling.top_p.map(|value| value.to_string()),
            ),
            (
                "semantic_top_k",
                sampling.top_k.map(|value| value.to_string()),
            ),
            (
                "semantic_repetition_penalty",
                sampling.repetition_penalty.map(|value| value.to_string()),
            ),
            (
                "semantic_penalty_window",
                sampling.penalty_window.map(|value| value.to_string()),
            ),
        ] {
            if let Some(value) = setting {
                options[name] = Value::from(value);
            }
        }
        let minimum = sampling.min_tokens.unwrap_or(0);
        let maximum = sampling.max_tokens.unwrap_or(cap);
        ensure!(
            minimum <= maximum && maximum <= cap,
            "YuE2 semantic token limits must fit the duration."
        );
        options["semantic_min_tokens"] = serde_json::json!(minimum.to_string());
        options["semantic_max_tokens"] = serde_json::json!(maximum.to_string());
    }
    let mut body = serde_json::json!({"lyrics":request.lyrics,"seed":seed,"num_inference_steps":steps,"options":options});
    if let Some(cfg) = request.cfg_scale {
        ensure!(
            (0.0..=20.0).contains(&cfg),
            "YuE2 guidance must be between 0 and 20."
        );
        body["guidance_scale"] = serde_json::json!(cfg);
    }
    Ok(body)
}
