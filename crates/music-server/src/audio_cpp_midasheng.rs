//! MiDashengLM-Gen's offline structured-text request for audio.cpp.
use super::CreateMusicJobRequest;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub(super) const FAMILY: &str = "midashenglm_gen";
pub(super) const PROFILE: &str = "q8_0";
const FILE: &str = "midashenglm-gen-q8_0.gguf";
const FILE_BYTES: u64 = 3_381_370_752;
const DIRECTORY: &str = "audiocpp/models/midashenglm_gen";
const PACKAGE: &str = "https://huggingface.co/audio-cpp/audio.cpp-gguf/blob/33120a4cfdf0d56786db934a03a093d4675de938/MiDashengLM-Gen-GGUF/midashenglm-gen-q8_0.gguf";
const LICENSE: &str = "https://huggingface.co/mispeech/midashenglm-gen#license";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct MiDashengPrompt {
    pub caption: String,
    pub music: String,
    pub speech_text: String,
    pub speech_style: String,
    pub effects: String,
    pub environment: String,
}

pub(super) fn model_dir(root: &Path) -> PathBuf {
    root.join(DIRECTORY)
}

pub(super) fn missing_files(path: &Path) -> Vec<&'static str> {
    if crate::audio_cpp_music::matches_gguf_package(&path.join(FILE), FILE_BYTES, 637, 16) {
        vec![]
    } else {
        vec![FILE]
    }
}

pub(super) fn catalog(root: Option<&Path>, runtime_ready: bool) -> Value {
    let mut missing = root.map_or_else(
        || vec!["Studio data folder"],
        |root| missing_files(&model_dir(root)),
    );
    let missing_bytes = if missing.is_empty() { 0 } else { FILE_BYTES };
    if !runtime_ready {
        missing.push("audio.cpp runtime");
    }
    json!({
        "id": FAMILY, "name": "MiDashengLM-Gen", "profile_id": PROFILE,
        "quantization": "Q8_0", "status": "experimental", "ready": missing.is_empty(),
        "missing": missing, "requires": ["midasheng"], "output_formats": ["flac", "mp3", "wav32"],
        "duration_seconds": { "kind": "budget", "allows_early_stop": true },
        "bytes": FILE_BYTES, "missing_bytes": missing_bytes, "model_directory": DIRECTORY,
        "package_url": PACKAGE, "license_url": LICENSE,
    })
}

pub(super) fn entry(path: PathBuf) -> acpp_server::ModelEntry {
    acpp_server::ModelEntry {
        id: FAMILY.into(),
        family: FAMILY.into(),
        task: "gen".into(),
        mode: acpp_server::Mode::Offline,
        path: path.join(FILE),
        load_options: Default::default(),
        session_options: Default::default(),
        default_request_options: Default::default(),
        busy_timeout_ms: None,
        output_rate: None,
    }
}

impl MiDashengPrompt {
    pub(super) fn text(&self) -> Result<String> {
        let speech = self.speech_text.trim();
        let layers = [
            ("caption", self.caption.trim()),
            ("asr", speech),
            (
                "speech",
                if speech.is_empty() {
                    ""
                } else {
                    self.speech_style.trim()
                },
            ),
            ("music", self.music.trim()),
            ("sfx", self.effects.trim()),
            ("env", self.environment.trim()),
        ];
        ensure!(
            layers.iter().any(|(_, value)| !value.is_empty()),
            "Describe at least one sound or enter the speech text."
        );
        ensure!(
            !layers.iter().any(|(_, value)| value.contains("<|")),
            "MiDashengLM-Gen fields cannot contain model tags."
        );
        Ok(layers
            .map(|(tag, value)| {
                let value = if value.is_empty() {
                    "<|unknown|>"
                } else {
                    value
                };
                format!("<|{tag}|> {value}")
            })
            .join(" "))
    }
}

pub(super) fn task(request: &CreateMusicJobRequest) -> Result<Value> {
    ensure!(
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
            && request.steps.is_none()
            && request.synth_batch_size.is_none_or(|n| n == 1)
            && request.lm_batch_size.is_none_or(|n| n == 1),
        "MiDashengLM-Gen accepts one structured text request with a single seed."
    );
    let prompt = request
        .midasheng
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Enter the MiDashengLM-Gen audio layers."))?;
    let mut body = wire_task(prompt, request.duration_seconds, request.seed)?;
    if let Some(guidance) = request.cfg_scale {
        ensure!(
            (guidance as f32).is_finite(),
            "Guidance scale must be finite."
        );
        body["options"]["guidance_scale"] = json!(guidance.to_string());
    }
    Ok(body)
}

fn wire_task(prompt: &MiDashengPrompt, duration: Option<f64>, seed: Option<i64>) -> Result<Value> {
    let mut options = serde_json::Map::new();
    if let Some(duration) = duration {
        let runtime_duration = duration as f32;
        ensure!(
            runtime_duration.is_finite() && runtime_duration > 0.,
            "Enter a positive duration in seconds."
        );
        options.insert("duration_sec".into(), json!(duration.to_string()));
    }
    if let Some(seed) = seed {
        let seed = u32::try_from(seed)
            .map_err(|_| anyhow::anyhow!("Seed must be between 0 and 4294967295."))?;
        options.insert("seed".into(), json!(seed.to_string()));
    }
    Ok(json!({"text": prompt.text()?, "options": options}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_speech_keeps_both_tags_but_only_applies_filled_audio_layers() {
        let prompt = MiDashengPrompt {
            caption: " An arcade loop ".into(),
            speech_text: " \n ".into(),
            speech_style: "Quiet narrator in the last draft".into(),
            music: "Synth chords".into(),
            effects: "Coin pickup".into(),
            environment: "Arcade room".into(),
        };
        assert_eq!(
            wire_task(&prompt, Some(3.5), Some(7)).unwrap(),
            json!({
                "text": "<|caption|> An arcade loop <|asr|> <|unknown|> <|speech|> <|unknown|> <|music|> Synth chords <|sfx|> Coin pickup <|env|> Arcade room",
                "options": {"duration_sec": "3.5", "seed": "7"},
            })
        );
        assert_eq!(
            serde_json::to_value(&prompt).unwrap()["speech_style"],
            "Quiet narrator in the last draft"
        );
    }

    #[test]
    fn voiced_prompt_validates_fields_and_numeric_options() {
        let prompt = MiDashengPrompt {
            speech_text: "The door is open.".into(),
            speech_style: "Warm, quiet narrator".into(),
            ..Default::default()
        };
        assert_eq!(
            prompt.text().unwrap(),
            "<|caption|> <|unknown|> <|asr|> The door is open. <|speech|> Warm, quiet narrator <|music|> <|unknown|> <|sfx|> <|unknown|> <|env|> <|unknown|>"
        );
        assert!(wire_task(&prompt, Some(f64::INFINITY), None).is_err());
        assert!(wire_task(&prompt, None, Some(-1)).is_err());
        assert!(MiDashengPrompt::default().text().is_err());
        assert!(
            MiDashengPrompt {
                caption: "<|speech|> fake tag".into(),
                ..prompt
            }
            .text()
            .is_err()
        );
    }
}
