//! MiniMax-H3's audio-only text-to-audio/video route on the shared server.
use super::*;
use acpp_server::{Mode, ModelEntry};
use std::io::Read;
use std::path::{Path, PathBuf};

pub(super) const FAMILY: &str = "minimax_h3";
pub(super) const NAME: &str = "MiniMax-H3";
pub(super) const PROFILE: &str = "minimax_h3_q4_k";
pub(super) const QUANTIZATION: &str = "Q4_K";
const MODEL_DIR: &str = "MiniMax-H3-Q4-GGUF";
const ENTRY_FILE: &str = "dit.gguf";
const MIN_SECONDS: f64 = 1.0;
const MAX_SECONDS: f64 = 20.0;
const DEFAULT_SECONDS: f64 = 4.0;
const FPS: f64 = 24.0;
const WEIGHTS: [(&str, u64); 4] = [
    (ENTRY_FILE, 15_502_530_720),
    ("text_encoder_q4_k.gguf", 15_270_376_000),
    ("audio_vae_folded_f16.gguf", 284_562_816),
    ("video_vae.gguf", 1_374_245_472),
];
const SIDECARS: [(&str, u64); 8] = [
    ("configuration.json", 34),
    ("FL2VA/processor/chat_template.json", 5_499),
    ("FL2VA/processor/merges.txt", 1_671_839),
    ("FL2VA/processor/preprocessor_config.json", 390),
    ("FL2VA/processor/tokenizer.json", 7_032_403),
    ("FL2VA/processor/tokenizer_config.json", 11_003),
    ("FL2VA/processor/video_preprocessor_config.json", 385),
    ("FL2VA/processor/vocab.json", 2_776_833),
];
const MODEL_SOURCE: &str = "https://huggingface.co/audio-cpp/audio.cpp-gguf/tree/32c1ce1fea57a513eb9386aa62408d47b1cf7ab2/MiniMax-H3-Q4-GGUF";
const MODEL_LICENSE: &str = "https://huggingface.co/MiniMaxAI/MiniMax-H3/blob/main/LICENSE";

pub(super) fn model_dir(root: &Path) -> PathBuf {
    root.join("audiocpp/models").join(MODEL_DIR)
}

pub(super) fn missing_files(directory: &Path) -> Vec<&'static str> {
    let sidecars = SIDECARS.into_iter().filter_map(|(name, _)| {
        let present = std::fs::metadata(directory.join(name))
            .is_ok_and(|info| info.is_file() && info.len() > 0);
        (!present).then_some(name)
    });
    let weights = WEIGHTS.into_iter().filter_map(|(name, bytes)| {
        let mut magic = [0; 4];
        let present = std::fs::File::open(directory.join(name)).is_ok_and(|mut file| {
            file.metadata().is_ok_and(|info| info.len() == bytes)
                && file.read_exact(&mut magic).is_ok()
                && &magic == b"GGUF"
        });
        (!present).then_some(name)
    });
    sidecars.chain(weights).collect()
}

pub(super) fn catalog(root: Option<&Path>, runtime_ready: bool) -> Value {
    let mut missing = root.map_or_else(
        || vec!["Studio data folder"],
        |root| missing_files(&model_dir(root)),
    );
    let files = WEIGHTS.into_iter().chain(SIDECARS);
    let bytes: u64 = files.clone().map(|(_, bytes)| bytes).sum();
    let missing_bytes: u64 = files
        .filter(|(file, _)| missing.contains(file))
        .map(|(_, bytes)| bytes)
        .sum();
    if !runtime_ready {
        missing.push("audio.cpp runtime");
    }
    serde_json::json!({
        "id": FAMILY, "name": NAME, "profile_id": PROFILE, "quantization": QUANTIZATION,
        "status": "experimental", "ready": missing.is_empty(), "missing": missing,
        "requires": ["style"], "output_formats": ["flac", "mp3", "wav32"],
        "bytes": bytes, "missing_bytes": missing_bytes,
        "package_url": MODEL_SOURCE, "license_url": MODEL_LICENSE,
        "model_directory": format!("audiocpp/models/{MODEL_DIR}"),
        "duration_seconds": {"min": MIN_SECONDS, "max": MAX_SECONDS, "default": DEFAULT_SECONDS}
    })
}

pub(super) fn entry(directory: PathBuf) -> ModelEntry {
    ModelEntry {
        id: FAMILY.into(),
        family: FAMILY.into(),
        task: "gen".into(),
        mode: Mode::Offline,
        path: directory.join(ENTRY_FILE),
        load_options: Default::default(),
        session_options: Default::default(),
        default_request_options: Default::default(),
        busy_timeout_ms: None,
        output_rate: None,
    }
}

pub(super) fn task(request: &CreateMusicJobRequest) -> anyhow::Result<Value> {
    let prompt = request.style.trim();
    anyhow::ensure!(
        !prompt.is_empty(),
        "MiniMax-H3 needs an instrumental music prompt."
    );
    anyhow::ensure!(
        request.lyrics.trim().is_empty(),
        "MiniMax-H3 takes a single prompt. Put any instructions in the prompt field."
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
        "MiniMax-H3 through audio.cpp accepts one audio generation from a text prompt."
    );

    let duration = request.duration_seconds.unwrap_or(DEFAULT_SECONDS);
    let steps = request.steps.unwrap_or(20);
    let seed = u32::try_from(request.seed.unwrap_or(42))
        .context("The MiniMax-H3 seed must fit in an unsigned 32-bit integer.")?;
    let guidance = request.cfg_scale.unwrap_or(1.0);
    anyhow::ensure!(
        (MIN_SECONDS..=MAX_SECONDS).contains(&duration),
        "MiniMax-H3 duration must be between 1 and 20 seconds."
    );
    anyhow::ensure!(steps > 0, "Inference needs at least one step.");
    anyhow::ensure!(
        guidance.is_finite() && guidance > 0.0,
        "Guidance scale must be positive."
    );

    // H3's denoiser still uses video latents for audio, so keep the canvas at
    // the documented audio-only geometry. The first frame plus N intervals
    // describes the requested span before the model applies its alignment.
    let frames = (duration * FPS).round() as u32 + 1;
    Ok(serde_json::json!({
        "text": prompt, "seed": seed, "duration_seconds": duration,
        "num_inference_steps": steps, "guidance_scale": guidance,
        "options": {
            "height": 32, "width": 32, "num_frames": frames, "return_video": false
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn audio_cpp_minimax_h3_maps_prompt_to_audio_only_request() {
        let request = serde_json::from_value(json!({
            "music_source":{"kind":"audio_cpp","model":"minimax_h3"},
            "style":" Funk groove ", "duration_seconds":4,
            "steps":20, "seed":2026, "cfg_scale":1.0
        }))
        .unwrap();
        let body = task(&request).unwrap();
        assert_eq!(body["text"], "Funk groove");
        assert_eq!(body["seed"], 2026);
        assert_eq!(body["num_inference_steps"], 20);
        assert_eq!(
            body["options"],
            json!({"height":32,"width":32,"num_frames":97,"return_video":false})
        );
        assert!(body.get("lyrics").is_none());
    }

    #[test]
    fn audio_cpp_minimax_h3_rejects_unsupported_input() {
        for payload in [
            json!({"style":" ", "lyrics":"lyrics only"}),
            json!({"style":"instrumental", "lyrics":"lyrics"}),
            json!({"style":"instrumental", "seed":-1}),
            json!({"style":"instrumental", "duration_seconds":21}),
        ] {
            let request = serde_json::from_value(payload).unwrap();
            assert!(task(&request).is_err());
        }
    }

    #[test]
    fn audio_cpp_minimax_h3_requires_a_complete_gguf_bundle() {
        let root = tempfile::tempdir().unwrap();
        let directory = model_dir(root.path());
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(ENTRY_FILE), b"GGUF truncated file").unwrap();
        let expected = catalog(Some(root.path()), true);
        assert_eq!(expected["ready"], false);
        assert_eq!(expected["status"], "experimental");
        assert_eq!(expected["bytes"], 32_443_213_394_u64);
        assert_eq!(expected["missing_bytes"], expected["bytes"]);
        assert_eq!(expected["package_url"], MODEL_SOURCE);
        assert_eq!(expected["license_url"], MODEL_LICENSE);
        assert_eq!(
            expected["model_directory"],
            "audiocpp/models/MiniMax-H3-Q4-GGUF"
        );
        for (weight, _) in WEIGHTS {
            assert!(
                expected["missing"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(weight))
            );
        }
        let entry = entry(directory);
        assert_eq!(entry.family, "minimax_h3");
        assert_eq!(entry.task, "gen");
        assert!(matches!(entry.mode, Mode::Offline));
        assert_eq!(entry.path.file_name().unwrap(), ENTRY_FILE);
    }
}
