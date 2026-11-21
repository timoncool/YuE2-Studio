//! Stable Audio 3 Small Music text-to-audio on the shared audio.cpp server.
use super::*;
use acpp_server::{Mode, ModelEntry};
use std::{
    io::Read,
    path::{Path as FsPath, PathBuf},
};

pub(super) const FAMILY: &str = "stable_audio";
pub(super) const PROFILE: &str = "stable_audio_3_small_music_q8_0";
const DIRECTORY: &str = "Stable-Audio-3-Small-Music-GGUF";
const FILE: &str = "stable-audio-3-small-music-q8_0.gguf";
pub(super) const SHA256: &str = "a6d9176e11b857409b28cf5a69f4fefd8f3e61907b3b10d867f1f4c0a7eb9418";
pub(super) const ASSET: downloads::Asset = downloads::Asset {
    id: PROFILE,
    label: "Stable Audio 3 Small Music Q8_0",
    kind: downloads::AssetKind::Model,
    url: "https://huggingface.co/audio-cpp/audio.cpp-gguf/resolve/33120a4cfdf0d56786db934a03a093d4675de938/Stable-Audio-3-Small-Music-GGUF/stable-audio-3-small-music-q8_0.gguf",
    relative_path: "models/Stable-Audio-3-Small-Music-GGUF/stable-audio-3-small-music-q8_0.gguf",
    bytes: 1_683_576_608,
    unzip_into: None,
    marker: FILE,
    pick: &[],
    vram_gb: None,
    note: "Stable Audio 3 Small Music under the Stability AI Community License.",
};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct Options {
    pub negative_prompt: Option<String>,
    pub sampler_mode: Option<String>,
    pub apg_scale: Option<f64>,
    pub duration_padding_sec: Option<f64>,
}

pub(super) fn model_dir(root: &FsPath) -> PathBuf {
    root.join("audiocpp/models").join(DIRECTORY)
}

pub(super) fn missing_files(directory: &FsPath) -> Vec<&'static str> {
    let mut magic = [0; 4];
    if std::fs::File::open(directory.join(FILE)).is_ok_and(|mut file| {
        file.metadata().is_ok_and(|m| m.len() == ASSET.bytes)
            && file.read_exact(&mut magic).is_ok()
            && &magic == b"GGUF"
    }) {
        vec![]
    } else {
        vec![FILE]
    }
}

pub(super) fn catalog(root: Option<&FsPath>, runtime_ready: bool) -> Value {
    let mut missing = root.map_or_else(
        || vec!["Studio data folder"],
        |root| missing_files(&model_dir(root)),
    );
    if !runtime_ready {
        missing.push("audio.cpp runtime");
    }
    serde_json::json!({
        "id": FAMILY, "name": "Stable Audio 3", "profile_id": PROFILE, "quantization": "Q8_0",
        "status": "supported", "ready": missing.is_empty(), "missing": missing,
        "requires": ["style"], "output_formats": ["flac", "mp3", "wav32"], "bytes": ASSET.bytes,
        "installable": true, "license_url": "https://huggingface.co/stabilityai/stable-audio-3-small-music"
    })
}

pub(super) fn entry(directory: PathBuf) -> ModelEntry {
    ModelEntry {
        id: FAMILY.into(),
        family: FAMILY.into(),
        task: "gen".into(),
        mode: Mode::Offline,
        path: directory.join(FILE),
        load_options: Default::default(),
        session_options: Default::default(),
        default_request_options: Default::default(),
        busy_timeout_ms: None,
        output_rate: None,
    }
}

pub(super) fn task(request: &CreateMusicJobRequest) -> anyhow::Result<Value> {
    let prompt = request.style.trim();
    anyhow::ensure!(!prompt.is_empty(), "Stable Audio 3 needs a music prompt.");
    anyhow::ensure!(
        !prompt.contains('|'),
        "Use one Stable Audio 3 music prompt."
    );
    anyhow::ensure!(
        request.lyrics.trim().is_empty()
            && request.cover_of.is_none()
            && request.semantic_tokens.is_none()
            && request
                .abc
                .as_deref()
                .is_none_or(|abc| abc.trim().is_empty())
            && !request.abc_continue
            && request.adapters.is_empty()
            && request.ace.is_none()
            && !request.vocals_only
            && request.solver.is_none()
            && request.lm_seed.is_none()
            && request.synth_batch_size.is_none_or(|n| n == 1)
            && request.lm_batch_size.is_none_or(|n| n == 1),
        "Stable Audio 3 accepts one music prompt with no lyrics, reference audio or adapters."
    );
    let duration = request.duration_seconds.unwrap_or(120.0);
    let steps = request.steps.unwrap_or(8);
    let seed = request
        .seed
        .unwrap_or_else(|| i64::from(uuid::Uuid::now_v7().as_u128() as u32));
    let guidance = request.cfg_scale.unwrap_or(1.0);
    anyhow::ensure!(
        duration.is_finite() && duration > 0.0,
        "Duration must be positive."
    );
    anyhow::ensure!(steps > 0, "Inference needs at least one step.");
    anyhow::ensure!(seed >= 0, "The seed must be nonnegative.");
    anyhow::ensure!(guidance.is_finite(), "Guidance scale must be finite.");
    let mut body = serde_json::json!({
        "text": prompt, "duration_seconds": duration, "seed": seed,
        "num_inference_steps": steps, "guidance_scale": guidance, "options": {}
    });
    if let Some(options) = &request.stable_audio {
        if let Some(negative) = options
            .negative_prompt
            .as_deref()
            .filter(|text| !text.trim().is_empty())
        {
            anyhow::ensure!(
                !negative.contains('|'),
                "Use one Stable Audio 3 negative prompt."
            );
            body["options"]["negative_prompt"] = Value::from(negative);
        }
        if let Some(sampler) = options.sampler_mode.as_deref() {
            anyhow::ensure!(
                matches!(sampler, "pingpong" | "euler"),
                "Stable Audio 3 sampler must be pingpong or euler."
            );
            body["options"]["sampler_mode"] = Value::from(sampler);
        }
        if let Some(apg) = options.apg_scale {
            anyhow::ensure!(apg.is_finite(), "APG scale must be finite.");
            body["options"]["apg_scale"] = Value::from(apg);
        }
        if let Some(padding) = options.duration_padding_sec {
            anyhow::ensure!(
                padding.is_finite() && padding >= 0.0,
                "Duration padding must be nonnegative."
            );
            body["options"]["duration_padding_sec"] = Value::from(padding);
        }
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_audio_serializes_native_controls_and_pins_the_install_destination() {
        let request: CreateMusicJobRequest = serde_json::from_value(serde_json::json!({
            "style": "instrumental piano", "duration_seconds": 4, "steps": 4, "seed": 42, "cfg_scale": 2,
            "stable_audio": {"negative_prompt":"vocals","sampler_mode":"euler","apg_scale":0.5,"duration_padding_sec":1}
        })).unwrap();
        assert_eq!(
            task(&request).unwrap(),
            serde_json::json!({
                "text":"instrumental piano","duration_seconds":4.0,"num_inference_steps":4,"seed":42,
                "guidance_scale":2.0,"options":{"negative_prompt":"vocals","sampler_mode":"euler","apg_scale":0.5,"duration_padding_sec":1.0}
            })
        );
        let root = tempfile::tempdir().unwrap();
        let downloads = downloads::Downloader::new(root.path().join("audiocpp"));
        assert_eq!(
            downloads.path_of(&ASSET),
            entry(model_dir(root.path())).path
        );
        assert_eq!(crate::pin_sha::sha256(ASSET.url), Some(SHA256));
    }
}
