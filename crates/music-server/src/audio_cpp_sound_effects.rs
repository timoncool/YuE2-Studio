//! Text-to-sound-effects through the shared audio.cpp process.
use super::*;
use acpp_server::{Mode, ModelEntry};
use std::path::{Path, PathBuf};

pub(super) const FAMILY: &str = "moss_soundeffect";
pub(super) const NAME: &str = "MOSS-SoundEffect v2";
pub(super) const PROFILE: &str = "moss_soundeffect_q8_0";
const FILE: &str = "moss-soundeffect-v2-q8_0.gguf";
const BYTES: u64 = 4_001_175_520;
const DIRECTORY: &str = "audiocpp/models/moss_soundeffect";
const PACKAGE: &str = "https://huggingface.co/audio-cpp/audio.cpp-gguf/blob/33120a4cfdf0d56786db934a03a093d4675de938/MOSS-SoundEffect-v2-GGUF/moss-soundeffect-v2-q8_0.gguf";
const LICENSE: &str = "https://huggingface.co/OpenMOSS-Team/MOSS-SoundEffect-v2.0";
const DEFAULT_DURATION: f64 = 10.0;

pub(super) fn model_dir(root: &Path) -> PathBuf {
    root.join(DIRECTORY)
}

pub(super) fn missing_files(directory: &Path) -> Vec<&'static str> {
    let valid = audio_cpp_music::matches_gguf_package(&directory.join(FILE), BYTES, 1247, 16);
    if valid { vec![] } else { vec![FILE] }
}

pub(super) fn catalog(root: Option<&Path>, runtime_ready: bool) -> Value {
    let mut missing = root.map_or_else(
        || vec!["Studio data folder"],
        |root| missing_files(&model_dir(root)),
    );
    let missing_bytes = if missing.is_empty() { 0 } else { BYTES };
    if !runtime_ready {
        missing.push("audio.cpp runtime");
    }
    serde_json::json!({
        "id": FAMILY, "name": NAME, "profile_id": PROFILE, "ready": missing.is_empty(), "missing": missing,
        "bytes": BYTES, "missing_bytes": missing_bytes, "model_directory": DIRECTORY,
        "package_url": PACKAGE, "license_url": LICENSE, "duration_seconds": { "min": 0.1, "max": 30, "default": DEFAULT_DURATION },
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
    let description = request.style.trim();
    anyhow::ensure!(!description.is_empty(), "Describe the sound effect.");
    anyhow::ensure!(
        request.lyrics.trim().is_empty() && request.cover_of.is_none() && request.abc.is_none(),
        "MOSS-SoundEffect takes a sound description."
    );
    let duration = request.duration_seconds.unwrap_or(DEFAULT_DURATION);
    anyhow::ensure!(
        (0.1..=30.0).contains(&duration),
        "Enter a duration from 0.1 to 30 seconds."
    );
    let steps = request.steps.unwrap_or(100);
    anyhow::ensure!(steps > 0, "Use at least one generation step.");
    let seed =
        u32::try_from(request.seed.unwrap_or(0)).context("Use a seed from 0 to 4294967295.")?;
    let duration = (duration * 10.0).round() / 10.0;
    Ok(serde_json::json!({
        "text": description,
        "options": {
            "duration_sec": duration.to_string(),
            "num_steps": steps.to_string(),
            "seed": seed.to_string(),
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sound_effect_validates_input_and_passes_runtime_options() {
        let mut request = CreateMusicJobRequest {
            style: "  A ceramic cup tapping a table.  ".into(),
            duration_seconds: Some(2.04),
            steps: Some(8),
            seed: Some(42),
            ..Default::default()
        };
        assert_eq!(
            task(&request).unwrap(),
            serde_json::json!({"text":"A ceramic cup tapping a table.","options":{"duration_sec":"2","num_steps":"8","seed":"42"}})
        );
        request.duration_seconds = Some(30.1);
        assert!(task(&request).is_err());
        request.duration_seconds = Some(2.0);
        request.style.clear();
        assert!(task(&request).is_err());
    }
}
