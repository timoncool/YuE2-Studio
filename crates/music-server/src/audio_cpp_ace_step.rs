//! ACE-Step 1.5 Turbo text-to-music on the shared audio.cpp runtime.
use super::*;
use acpp_server::{Mode, ModelEntry};
use anyhow::ensure;
use std::path::Path as FsPath;

pub(super) const FAMILY: &str = "ace_step";
pub(super) const PROFILE: &str = "ace-step-1.5-turbo-q8_0";
const DIRECTORY: &str = "ACE-Step1.5-GGUF/turbo";
const FILE: &str = "ace-step-1.5-turbo-q8_0.gguf";

/// The packed weights in the component catalogue's ACE-Step set, with their SHA-256.
fn weights() -> (&'static downloads::Asset, &'static str) {
    crate::audio_cpp_music::set_file(FAMILY, "turbo/ace-step-1.5-turbo-q8_0.gguf")
}

pub(super) fn model_dir(root: &FsPath) -> PathBuf {
    root.join("audiocpp/models").join(DIRECTORY)
}

pub(super) fn missing_files(directory: &FsPath) -> Vec<&'static str> {
    // The packed GGUF carries DiT, the embedding encoder and VAE together.
    let path = directory.join(FILE);
    let (asset, sha256) = weights();
    if fs::metadata(&path).is_ok_and(|meta| meta.is_file() && meta.len() == asset.bytes)
        && crate::pin_sha::holds(&path, sha256)
    {
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
    let bytes = weights().0.bytes;
    let missing_bytes = if missing.is_empty() { 0 } else { bytes };
    if !runtime_ready {
        missing.push("audio.cpp runtime");
    }
    serde_json::json!({"id":FAMILY,"name":"ACE-Step 1.5","profile_id":PROFILE,"quantization":"Q8_0",
        "status":"experimental","ready":missing.is_empty(),"missing":missing,"missing_bytes":missing_bytes,
        "bytes":bytes,"installable":true,
        "license_url":"https://huggingface.co/ACE-Step/Ace-Step1.5"})
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
    ensure!(
        !request.style.trim().is_empty() || !request.lyrics.trim().is_empty(),
        "Enter a style or lyrics."
    );
    ensure!(
        request.ace.is_none()
            && request.adapters.is_empty()
            && request.cover_of.is_none()
            && request
                .abc
                .as_deref()
                .is_none_or(|abc| abc.trim().is_empty()),
        "The audio.cpp ACE-Step Turbo profile accepts style and lyrics for text-to-music."
    );
    let duration = request.duration_seconds.unwrap_or(30.0);
    let seed = request.seed.unwrap_or(42);
    let steps = request.steps.unwrap_or(8);
    let guidance = request.cfg_scale.unwrap_or(1.0);
    ensure!(
        (1.0..=300.0).contains(&duration)
            && (1..=100).contains(&steps)
            && seed >= 0
            && guidance > 0.0
            && guidance.is_finite(),
        "Enter valid ACE-Step duration, steps, seed and guidance."
    );
    let lyrics = if request.lyrics.trim().is_empty() {
        "[Instrumental]"
    } else {
        &request.lyrics
    };
    Ok(
        serde_json::json!({"text":request.style,"lyrics":lyrics,"duration_seconds":duration,
        "seed":seed,"num_inference_steps":steps,
        "options":{"route":"text2music","sampler_mode":"euler","guidance_scale":guidance.to_string(),"shift":"3"}}),
    )
}
