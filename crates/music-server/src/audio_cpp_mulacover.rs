//! MuLaCover's offline reference-audio route on the shared audio.cpp server.
use super::*;
use acpp_server::{Mode, ModelEntry};
use std::io::Read;
use std::path::{Path, PathBuf};

pub(super) const FAMILY: &str = "mulacover";
pub(super) const NAME: &str = "MuLaCover";
pub(super) const PROFILE: &str = "mulacover_q8_0";
const DIRECTORY: &str = "MuLaCover-GGUF";
const WEIGHT: &str = "mulacover-q8_0.gguf";
const BYTES: u64 = 8_220_378_496;
const PACKAGE: &str = "https://huggingface.co/audio-cpp/audio.cpp-gguf/tree/32c1ce1fea57a513eb9386aa62408d47b1cf7ab2/MuLaCover-GGUF";
const LICENSE: &str = "https://github.com/HeartMuLa/MuLaCover/blob/e27fa7fa0da7c9817879cc6210cc5693f9327946/MODEL_LICENSE";

pub(super) fn model_dir(root: &Path) -> PathBuf {
    root.join("audiocpp/models").join(DIRECTORY)
}

pub(super) fn missing_files(dir: &Path) -> Vec<&'static str> {
    let mut magic = [0; 4];
    let valid = std::fs::File::open(dir.join(WEIGHT)).is_ok_and(|mut file| {
        file.metadata().is_ok_and(|info| info.len() == BYTES)
            && file.read_exact(&mut magic).is_ok()
            && &magic == b"GGUF"
    });
    if valid { vec![] } else { vec![WEIGHT] }
}

pub(super) fn catalog(root: Option<&Path>, runtime_ready: bool, soxr_ready: bool) -> Value {
    let mut missing = root.map_or_else(
        || vec!["Studio data folder"],
        |root| missing_files(&model_dir(root)),
    );
    if !runtime_ready {
        missing.push("audio.cpp runtime");
    }
    if !soxr_ready {
        missing.push("libsoxr");
    }
    serde_json::json!({
        "id": FAMILY, "name": NAME, "profile_id": PROFILE, "quantization": "Q8_0",
        "status": "experimental", "ready": missing.is_empty(), "missing": missing,
        "requires": ["cover_of"], "output_formats": ["flac", "mp3", "wav32"],
        "license_url": LICENSE, "package_url": PACKAGE, "model_directory": format!("audiocpp/models/{DIRECTORY}"),
    })
}

pub(super) async fn install_runtime(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    #[cfg(windows)]
    state
        .acpp
        .install_soxr()
        .await
        .map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, format!("{error:#}")))?;
    if !state.acpp.soxr_ready() {
        return Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Install libsoxr through your system's package manager.".into(),
        ));
    }
    Ok(Json(serde_json::json!({"ready": true})))
}

pub(super) fn entry(dir: PathBuf) -> ModelEntry {
    ModelEntry {
        id: FAMILY.into(),
        family: FAMILY.into(),
        task: "gen".into(),
        mode: Mode::Offline,
        path: dir.join(WEIGHT),
        load_options: Default::default(),
        session_options: Default::default(),
        default_request_options: Default::default(),
        busy_timeout_ms: None,
        output_rate: None,
    }
}

pub(super) struct CoverTask {
    pub body: Value,
    pub derived: Value,
    reference: PathBuf,
}

fn payload(request: &CreateMusicJobRequest) -> anyhow::Result<Value> {
    anyhow::ensure!(
        request.semantic_tokens.is_none()
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
        "MuLaCover accepts one cover from a library audio reference."
    );
    let seed = u32::try_from(request.seed.unwrap_or(1234))
        .context("The MuLaCover seed must fit in an unsigned 32-bit integer.")?;
    let duration = request.duration_seconds.unwrap_or(20.0);
    let steps = request.steps.unwrap_or(10);
    let guidance = request.cfg_scale.unwrap_or(1.5);
    anyhow::ensure!(
        duration.is_finite() && duration >= 0.08,
        "Duration must be at least 0.08 seconds."
    );
    anyhow::ensure!(steps > 0, "Inference needs at least one step.");
    anyhow::ensure!(
        guidance.is_finite() && guidance > 0.0,
        "Guidance scale must be positive."
    );
    Ok(serde_json::json!({
        "text": request.lyrics.trim(),
        "seed": seed, "num_inference_steps": steps, "guidance_scale": guidance,
        "options": {"tags": request.style.trim(), "duration_sec": duration}
    }))
}

pub(super) fn task(
    request: &CreateMusicJobRequest,
    library: &library::Library,
    resumed: bool,
) -> anyhow::Result<CoverTask> {
    let id = request
        .cover_of
        .as_deref()
        .context("Choose the original song from the library.")?;
    let original =
        cover_source(library, id, resumed).map_err(|(_, reason)| anyhow::anyhow!(reason))?;
    let reference = library
        .media_path_for_song(&original)
        .filter(|file| file.is_file())
        .context("The original song has no available audio.")?;
    let body = payload(request)?;
    let derived = derivation(&original, "cover", serde_json::json!({"model": FAMILY}));
    Ok(CoverTask {
        body,
        derived,
        reference,
    })
}

impl CoverTask {
    pub(super) fn into_request(mut self) -> anyhow::Result<(Value, tempfile::TempDir)> {
        let temp = tempfile::tempdir()?;
        let wav = temp.path().join("reference.wav");
        let audio = audio_pcm::decode_stereo(&self.reference)?.resampled(24_000)?;
        audio_pcm::write_wav_f32(&wav, &audio)?;
        self.body["audio"] = serde_json::json!(wav);
        Ok((self.body, temp))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn audio_cpp_mulacover_uses_native_lyrics_tags_and_sampler_controls() {
        let request = serde_json::from_value(json!({
            "style":"genre:[folk]","lyrics":"[Verse] Morning comes",
            "duration_seconds":3,"seed":2026,"steps":7,"cfg_scale":1.5,
        }))
        .unwrap();
        let body = payload(&request).unwrap();
        assert_eq!(body["text"], "[Verse] Morning comes");
        assert_eq!(body["options"]["tags"], "genre:[folk]");
        assert_eq!(body["options"]["duration_sec"], 3.0);
        assert_eq!(body["seed"], 2026);
        assert_eq!(body["num_inference_steps"], 7);
        assert!(body.get("audio").is_none());
    }

    #[test]
    fn audio_cpp_mulacover_stages_a_wav_and_keeps_the_library_original() {
        let root = tempfile::tempdir().unwrap();
        let library =
            library::Library::open_at(root.path().join("songs.db"), root.path().join("media"))
                .unwrap();
        let bytes = audio_pcm::wav24_bytes(&audio_post::Stereo {
            rate: 24_000,
            left: vec![0.2; 2048],
            right: vec![0.2; 2048],
        })
        .unwrap();
        let imported = library
            .import_audio_song(library::AudioImportInput {
                title: "Source recording".into(),
                caption: String::new(),
                lyrics: String::new(),
                metadata: Value::Null,
                generation_settings: Value::Null,
                engine_id: "imported-audio".into(),
                profile_id: None,
                source: "audio_import".into(),
                audio_extension: "wav".into(),
                audio: bytes.clone(),
            })
            .unwrap()
            .song;
        let request = serde_json::from_value(json!({"cover_of": imported.id})).unwrap();
        let cover = task(&request, &library, false).unwrap();
        assert_eq!(cover.derived["from"], imported.id);
        assert_eq!(cover.derived["tool"], "cover");
        let (body, temp) = cover.into_request().unwrap();
        let staged = Path::new(body["audio"].as_str().unwrap());
        assert!(staged.is_file());
        drop(temp);
        assert!(!staged.exists());
        let original = library.media_path_for_song(&imported).unwrap();
        assert_eq!(std::fs::read(original).unwrap(), bytes);
    }

    #[test]
    fn audio_cpp_mulacover_requires_soxr_and_a_complete_q8_package() {
        let folder = tempfile::tempdir().unwrap();
        let row = catalog(Some(folder.path()), true, false);
        assert_eq!(row["missing"], json!([WEIGHT, "libsoxr"]));
        let model = entry(model_dir(folder.path()));
        assert_eq!(model.task, "gen");
        assert!(matches!(model.mode, Mode::Offline));
        assert_eq!(model.path.file_name().unwrap(), WEIGHT);
    }
}
