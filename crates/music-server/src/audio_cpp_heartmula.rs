//! HeartMuLa's offline music task on the shared audio.cpp server.
use super::*;
use acpp_server::{Mode, ModelEntry};
use std::io::Read;
use std::path::{Path, PathBuf};

pub(super) const FAMILY: &str = "heartmula";
pub(super) const PROFILE: &str = "heartmula_q8_0";
const WEIGHT_DIR: &str = "HeartMuLa-GGUF";
const WEIGHT_FILE: &str = "heartmula-q8_0.gguf";
const WEIGHT_BYTES: u64 = 7_659_762_592;
pub(super) const ASSET: downloads::Asset = downloads::Asset {
    id: PROFILE,
    label: "HeartMuLa Q8_0",
    kind: downloads::AssetKind::Model,
    url: "https://huggingface.co/audio-cpp/audio.cpp-gguf/resolve/32c1ce1fea57a513eb9386aa62408d47b1cf7ab2/HeartMuLa-GGUF/heartmula-q8_0.gguf",
    relative_path: "models/HeartMuLa-GGUF/heartmula-q8_0.gguf",
    bytes: WEIGHT_BYTES,
    unzip_into: None,
    marker: WEIGHT_FILE,
    pick: &[],
    vram_gb: None,
    note: "HeartMuLa music generation through audio.cpp.",
};
pub(super) const SHA256: &str = "5c3e42171ec012ba7420e3841e2a36e68e36177863b5e76793393afecd10cfbd";

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub(super) struct HeartMuLaOptions {
    pub temperature: Option<f64>,
    pub top_k: Option<u32>,
    pub codec_duration_sec: Option<f64>,
    pub codec_guidance_scale: Option<f64>,
}

pub(super) fn model_dir(root: &Path) -> PathBuf {
    root.join("audiocpp/models").join(WEIGHT_DIR)
}

pub(super) fn missing_files(directory: &Path) -> Vec<&'static str> {
    let mut magic = [0; 4];
    if std::fs::File::open(directory.join(WEIGHT_FILE)).is_ok_and(|mut file| {
        file.metadata().is_ok_and(|info| info.len() == WEIGHT_BYTES)
            && file.read_exact(&mut magic).is_ok()
            && &magic == b"GGUF"
    }) {
        Vec::new()
    } else {
        vec![WEIGHT_FILE]
    }
}

pub(super) fn catalog(root: Option<&Path>, runtime_ready: bool) -> Value {
    let mut missing = root.map_or_else(
        || vec!["Studio data folder"],
        |root| missing_files(&model_dir(root)),
    );
    if !runtime_ready {
        missing.push("audio.cpp runtime");
    }
    serde_json::json!({
        "id": FAMILY, "name": "HeartMuLa", "profile_id": PROFILE, "quantization": "Q8_0",
        "status": "supported", "ready": missing.is_empty(), "missing": missing,
        "requires": [], "output_formats": ["flac", "mp3", "wav32"],
        "bytes": WEIGHT_BYTES, "installable": true
    })
}

pub(super) fn entry(directory: PathBuf) -> ModelEntry {
    ModelEntry {
        id: FAMILY.into(),
        family: FAMILY.into(),
        task: "gen".into(),
        mode: Mode::Offline,
        path: directory.join(WEIGHT_FILE),
        load_options: Default::default(),
        session_options: Default::default(),
        default_request_options: Default::default(),
        busy_timeout_ms: None,
        output_rate: None,
    }
}

pub(super) fn task(request: &CreateMusicJobRequest) -> anyhow::Result<Value> {
    anyhow::ensure!(
        !request.style.trim().is_empty() || !request.lyrics.trim().is_empty(),
        "HeartMuLa needs style tags or lyrics."
    );
    anyhow::ensure!(
        request.cover_of.is_none()
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
        "HeartMuLa through audio.cpp accepts one generation from tags or lyrics with a single seed."
    );
    let mut body =
        serde_json::json!({"lyrics": request.lyrics, "options": {"tags": request.style}});
    if let Some(duration) = request.duration_seconds {
        anyhow::ensure!(
            duration.is_finite() && duration >= 0.1,
            "Duration must be at least 0.1 seconds."
        );
        body["duration_seconds"] = Value::from(duration);
    }
    if let Some(steps) = request.steps {
        anyhow::ensure!(steps > 0, "Inference needs at least one step.");
        body["num_inference_steps"] = Value::from(steps);
    }
    if let Some(seed) = request.seed {
        anyhow::ensure!(seed >= 0, "The seed must be nonnegative.");
        body["seed"] = Value::from(seed);
    }
    if let Some(guidance) = request.cfg_scale {
        anyhow::ensure!(
            guidance.is_finite() && guidance > 0.0,
            "Guidance scale must be positive."
        );
        body["guidance_scale"] = Value::from(guidance);
    }
    if let Some(options) = &request.heartmula {
        for (key, value) in [
            ("temperature", options.temperature),
            ("codec_duration_sec", options.codec_duration_sec),
            ("codec_guidance_scale", options.codec_guidance_scale),
        ] {
            if let Some(value) = value {
                anyhow::ensure!(value.is_finite() && value > 0.0, "{key} must be positive.");
                body["options"][key] = Value::from(value);
            }
        }
        if let Some(top_k) = options.top_k {
            anyhow::ensure!(top_k > 0, "top_k must be at least 1.");
            body["options"]["top_k"] = Value::from(top_k);
        }
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartmula_install_asset_lands_where_the_runtime_loads_it() {
        let root = tempfile::tempdir().unwrap();
        let dir = model_dir(root.path());
        let downloader = downloads::Downloader::new(root.path().join("audiocpp"));
        assert_eq!(downloader.path_of(&ASSET), entry(dir.clone()).path);
        assert_eq!(crate::pin_sha::sha256(ASSET.url), Some(SHA256));
        assert_eq!(catalog(Some(root.path()), true)["installable"], true);
        assert!(!downloader.is_installed(&ASSET));
        assert_eq!(missing_files(&dir), [WEIGHT_FILE]);
    }

    #[test]
    fn requested_heartmula_controls_reach_the_native_music_payload() {
        let request: CreateMusicJobRequest = serde_json::from_value(serde_json::json!({
            "music_source": {"kind":"audio_cpp","model":"heartmula"},
            "style":"pop,piano,drums","lyrics":"[Verse]\nWe rise",
            "duration_seconds":4,"steps":4,"seed":42,"cfg_scale":1.5,
            "heartmula":{"temperature":0.9,"top_k":30,"codec_duration_sec":12,"codec_guidance_scale":1.2}
        })).unwrap();
        assert_eq!(
            task(&request).unwrap(),
            serde_json::json!({
                "lyrics":"[Verse]\nWe rise","duration_seconds":4.0,"num_inference_steps":4,
                "seed":42,"guidance_scale":1.5,
                "options":{"tags":"pop,piano,drums","temperature":0.9,"top_k":30,"codec_duration_sec":12.0,"codec_guidance_scale":1.2}
            })
        );
    }

    #[test]
    fn style_only_uses_native_defaults_and_invalid_options_are_explicit() {
        let mut request = CreateMusicJobRequest {
            style: "instrumental,piano".into(),
            ..Default::default()
        };
        assert_eq!(
            task(&request).unwrap(),
            serde_json::json!({"lyrics":"","options":{"tags":"instrumental,piano"}})
        );
        request.heartmula = Some(HeartMuLaOptions {
            temperature: Some(0.0),
            ..Default::default()
        });
        assert_eq!(
            task(&request).unwrap_err().to_string(),
            "temperature must be positive."
        );
    }
}
