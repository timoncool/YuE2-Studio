//! Vevo2's audio.cpp adapter for singing text with a library voice reference.
use crate::{CreateMusicJobRequest, audio_pcm, library};
use acpp_server::{Mode, ModelEntry};
use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

pub(super) const FAMILY: &str = "vevo2";
pub(super) const NAME: &str = "Vevo2";
pub(super) const PROFILE: &str = "quality-q8";
pub(super) const QUANTIZATION: &str = "Q8_0";
pub(super) const ROUTE: &str = "text_to_singing";
const WEIGHTS: &str = "vevo2-q8_0.gguf";
const LICENSE: &str = "https://huggingface.co/RMSnow/Vevo2";
const FOLDER_SETTING: &str = "vevo2-model-directory.json";
const DEFAULT_FOLDER: &str = "audiocpp/models/vevo2";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DirectorySelection {
    directory: Value,
}

fn configured_directory(root: &Path) -> Option<PathBuf> {
    serde_json::from_slice(&std::fs::read(root.join(FOLDER_SETTING)).ok()?).ok()
}

fn select_directory(root: &Path, directory: Option<&Path>) -> Result<()> {
    if let Some(directory) = directory {
        anyhow::ensure!(
            directory.is_absolute(),
            "Choose an absolute folder path on the Studio computer."
        );
        anyhow::ensure!(
            missing_files(directory).is_empty(),
            "The folder needs a readable {WEIGHTS} file."
        );
    }
    let mut temp = tempfile::NamedTempFile::new_in(root)?;
    serde_json::to_writer(&mut temp, &directory)?;
    temp.flush()?;
    temp.persist(root.join(FOLDER_SETTING))?;
    Ok(())
}

pub(super) async fn set_directory(
    axum::Json(selection): axum::Json<DirectorySelection>,
) -> Result<axum::Json<Value>, (axum::http::StatusCode, axum::Json<crate::ApiError>)> {
    use axum::http::StatusCode;
    let root = crate::studio_data_root().ok_or_else(|| {
        crate::api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Studio data folder is unavailable.".into(),
        )
    })?;
    let directory: Option<PathBuf> = serde_json::from_value(selection.directory)
        .map_err(|error| crate::api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    select_directory(&root, directory.as_deref())
        .map_err(|error| crate::api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(axum::Json(json!({ "configured_directory": directory })))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Options {
    pub voice_song_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub melody_song_id: Option<String>,
}

pub(super) fn model_dir(root: &Path) -> PathBuf {
    configured_directory(root).unwrap_or_else(|| root.join(DEFAULT_FOLDER))
}

pub(super) fn missing_files(dir: &Path) -> Vec<&'static str> {
    let valid = std::fs::File::open(dir.join(WEIGHTS)).is_ok_and(|mut file| {
        let mut magic = [0; 4];
        file.metadata().is_ok_and(|metadata| metadata.len() > 24)
            && file.read_exact(&mut magic).is_ok()
            && &magic == b"GGUF"
    });
    if valid { vec![] } else { vec![WEIGHTS] }
}

pub(super) fn catalog(root: Option<&Path>, runtime_ready: bool) -> Value {
    let mut missing = root.map_or_else(
        || vec!["Studio data folder"],
        |root| missing_files(&model_dir(root)),
    );
    if !runtime_ready {
        missing.push("audio.cpp runtime");
    }
    json!({
        "id": FAMILY, "name": NAME, "profile_id": PROFILE, "quantization": QUANTIZATION,
        "status": "experimental", "ready": missing.is_empty(), "missing": missing,
        "requires": ["lyrics", "voice_song_id"], "output_formats": ["flac", "mp3", "wav32"],
        "route": ROUTE, "license_url": LICENSE, "model_directory": DEFAULT_FOLDER,
        "configured_directory": root.and_then(configured_directory),
    })
}

pub(super) fn entry(dir: PathBuf) -> ModelEntry {
    ModelEntry {
        id: FAMILY.into(),
        family: FAMILY.into(),
        task: "tts".into(),
        mode: Mode::Offline,
        path: dir.join(WEIGHTS),
        load_options: Default::default(),
        session_options: Default::default(),
        default_request_options: Default::default(),
        busy_timeout_ms: None,
        output_rate: None,
    }
}

fn reference(library: &library::Library, id: &str, role: &str) -> Result<PathBuf> {
    let song = library
        .get_song(id)?
        .filter(|song| library::trashed_at(song) == 0)
        .with_context(|| format!("Choose a {role} reference from the library."))?;
    library
        .media_path_for_song(&song)
        .filter(|path| path.is_file())
        .with_context(|| format!("The {role} reference has no available audio."))
}

pub(super) struct SingingTask {
    text: String,
    voice: PathBuf,
    melody: Option<PathBuf>,
    pub seed: Option<u32>,
}

pub(super) fn task(
    request: &CreateMusicJobRequest,
    library: &library::Library,
) -> Result<SingingTask> {
    let text = request.lyrics.trim();
    anyhow::ensure!(!text.is_empty(), "Vevo2 needs text to sing.");
    let options = request
        .vevo2
        .as_ref()
        .context("Choose a voice reference from the library.")?;
    let seed = request
        .seed
        .map(u32::try_from)
        .transpose()
        .context("The Vevo2 seed must fit in an unsigned 32-bit integer.")?;
    let voice = reference(library, &options.voice_song_id, "voice")?;
    let melody = options
        .melody_song_id
        .as_deref()
        .map(|id| reference(library, id, "melody"))
        .transpose()?;
    Ok(SingingTask {
        text: text.into(),
        voice,
        melody,
        seed,
    })
}

fn write_reference(source: &Path, destination: &Path) -> Result<()> {
    let audio = audio_pcm::decode_stereo(source)?.resampled(24_000)?;
    std::fs::write(destination, audio_pcm::wav24_bytes(&audio)?)?;
    Ok(())
}

impl SingingTask {
    pub(super) fn into_request(self) -> Result<(Value, tempfile::TempDir)> {
        let temp = tempfile::tempdir()?;
        let voice = temp.path().join("voice.wav");
        write_reference(&self.voice, &voice).context("Preparing the Vevo2 voice reference")?;
        let mut body = json!({
            "text": self.text,
            "options": {
                "route": ROUTE,
                "target_voice": voice,
                "use_prosody_code": self.melody.is_some()
            }
        });
        if let Some(seed) = self.seed {
            body["options"]["seed"] = json!(seed);
        }
        if let Some(source) = self.melody {
            let melody = temp.path().join("melody.wav");
            write_reference(&source, &melody).context("Preparing the Vevo2 melody reference")?;
            body["options"]["prosody_ref"] = json!(melody);
        }
        Ok((body, temp))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(path: &Path) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for index in 0..1600 {
            writer.write_sample((index % 90) as i16 * 200).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn vevo2_registers_as_offline_tts_and_checks_gguf_readiness() {
        let root = tempfile::tempdir().unwrap();
        let directory = model_dir(root.path());
        std::fs::create_dir_all(&directory).unwrap();
        assert_eq!(catalog(Some(root.path()), true)["ready"], false);
        let path = directory.join(WEIGHTS);
        std::fs::write(&path, b"GGUF").unwrap();
        assert_eq!(catalog(Some(root.path()), true)["ready"], false);
        std::fs::write(&path, [b"GGUF".as_slice(), &[0; 32]].concat()).unwrap();
        assert_eq!(catalog(Some(root.path()), true)["ready"], true);
        assert_eq!(catalog(Some(root.path()), false)["ready"], false);
        let entry = entry(directory);
        assert_eq!(entry.path, path);
        assert_eq!(entry.task, "tts");
        assert!(matches!(entry.mode, Mode::Offline));
    }

    #[test]
    fn vevo2_keeps_the_selected_local_folder_and_can_restore_the_default() {
        let root = tempfile::tempdir().unwrap();
        let supplied = tempfile::tempdir().unwrap();
        let path = supplied.path().join(WEIGHTS);
        std::fs::write(&path, [b"GGUF".as_slice(), &[0; 32]].concat()).unwrap();
        assert_eq!(catalog(Some(root.path()), true)["ready"], false);
        select_directory(root.path(), Some(supplied.path())).unwrap();
        assert_eq!(catalog(Some(root.path()), true)["ready"], true);
        assert_eq!(entry(model_dir(root.path())).path, path);
        assert!(select_directory(root.path(), Some(Path::new("relative"))).is_err());
        assert!(select_directory(root.path(), Some(root.path())).is_err());
        assert_eq!(model_dir(root.path()), supplied.path());
        select_directory(root.path(), None).unwrap();
        assert_eq!(model_dir(root.path()), root.path().join(DEFAULT_FOLDER));
        assert_eq!(catalog(Some(root.path()), true)["ready"], false);
    }

    #[test]
    fn vevo2_request_keeps_staged_wav_alive_and_enables_prosody_only_with_melody() {
        let root = tempfile::tempdir().unwrap();
        let recording = root.path().join("library-input.wav");
        wav(&recording);
        for melody in [None, Some(recording.clone())] {
            let has_melody = melody.is_some();
            let (body, temp) = SingingTask {
                text: "Sing this".into(),
                voice: recording.clone(),
                melody,
                seed: Some(123),
            }
            .into_request()
            .unwrap();
            assert_eq!(body["text"], "Sing this");
            assert_eq!(body["options"]["route"], "text_to_singing");
            assert_eq!(body["options"]["seed"], 123);
            assert_eq!(body["options"]["use_prosody_code"], has_melody);
            assert_eq!(body["options"]["prosody_ref"].is_string(), has_melody);
            let voice = PathBuf::from(body["options"]["target_voice"].as_str().unwrap());
            assert_eq!(
                hound::WavReader::open(&voice).unwrap().spec().sample_rate,
                24000
            );
            if let Some(melody) = body["options"]["prosody_ref"].as_str() {
                assert!(Path::new(melody).is_file());
            }
            drop(temp);
            assert!(!voice.exists());
        }
    }
}
