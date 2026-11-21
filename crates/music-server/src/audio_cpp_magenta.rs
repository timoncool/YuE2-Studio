//! Offline Magenta RealTime 2 generation with text or a library style reference.
use crate::{CreateMusicJobRequest, audio_pcm, library};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::Read;
use std::path::{Path, PathBuf};

pub(super) const FAMILY: &str = "magenta_realtime_2";
pub(super) const PROFILE: &str = "small-f16";
const FILE: &str = "magenta-rt2-small-f16.gguf";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct MagentaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_song_id: Option<String>,
}

pub(super) fn model_dir(root: &Path) -> PathBuf {
    root.join("audiocpp/models").join(FAMILY)
}

pub(super) fn missing_files(dir: &Path) -> Vec<&'static str> {
    let mut magic = [0; 4];
    if std::fs::File::open(dir.join(FILE)).is_ok_and(|mut model| {
        model.metadata().is_ok_and(|info| info.len() > 24)
            && model.read_exact(&mut magic).is_ok()
            && &magic == b"GGUF"
    }) {
        vec![]
    } else {
        vec![FILE]
    }
}

pub(super) fn catalog(dir: &Path, runtime_ready: bool) -> Value {
    let mut missing = missing_files(dir);
    if !runtime_ready {
        missing.push("audio.cpp runtime");
    }
    json!({
        "id": FAMILY, "name": "Magenta RealTime 2", "profile_id": PROFILE,
        "quantization": "F16", "status": "experimental", "ready": missing.is_empty(),
        "missing": missing, "requires": [], "output_formats": ["flac", "mp3", "wav32"],
    })
}

pub(super) fn entry(dir: PathBuf) -> acpp_server::ModelEntry {
    acpp_server::ModelEntry {
        id: FAMILY.into(),
        family: FAMILY.into(),
        task: "gen".into(),
        mode: acpp_server::Mode::Offline,
        path: dir.join(FILE),
        load_options: Default::default(),
        session_options: Default::default(),
        default_request_options: Default::default(),
        busy_timeout_ms: None,
        output_rate: None,
    }
}

pub(super) struct MagentaTask {
    body: Value,
    reference_dir: Option<tempfile::TempDir>,
}

impl MagentaTask {
    pub(super) fn into_request(self) -> (Value, Option<tempfile::TempDir>) {
        (self.body, self.reference_dir)
    }
}

pub(super) fn task(
    request: &CreateMusicJobRequest,
    library: &library::Library,
) -> Result<MagentaTask> {
    ensure!(
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
            && request.steps.is_none()
            && request.synth_batch_size.is_none_or(|n| n == 1)
            && request.lm_batch_size.is_none_or(|n| n == 1),
        "Magenta RealTime 2 generates one instrumental with a text style or an audio style reference."
    );
    let mut body = request_body(
        &request.style,
        request
            .magenta
            .as_ref()
            .and_then(|options| options.reference_song_id.as_deref())
            .is_some(),
        request.duration_seconds,
        request.seed,
        request.cfg_scale,
    )?;

    let reference_dir = if let Some(id) = request
        .magenta
        .as_ref()
        .and_then(|options| options.reference_song_id.as_deref())
    {
        let song = library
            .get_song(id)?
            .filter(|song| library::trashed_at(song) == 0)
            .context("Choose an available Magenta style reference from the library.")?;
        let source = library
            .media_path_for_song(&song)
            .filter(|path| path.is_file())
            .context("The Magenta style reference has no available audio file.")?;
        let (path, directory) = stage_reference(&source)?;
        body["audio"] = json!(path);
        Some(directory)
    } else {
        None
    };
    Ok(MagentaTask {
        body,
        reference_dir,
    })
}

fn request_body(
    style: &str,
    audio_reference: bool,
    duration: Option<f64>,
    seed: Option<i64>,
    guidance: Option<f64>,
) -> Result<Value> {
    let mut options = serde_json::Map::new();
    if let Some(duration) = duration {
        ensure!(
            (duration as f32).is_finite() && duration >= 0.04,
            "Enter a Magenta duration of at least 0.04 seconds."
        );
        options.insert("duration_sec".into(), json!(duration.to_string()));
    }
    if let Some(seed) = seed {
        let seed = u32::try_from(seed)
            .context("The Magenta seed must fit in an unsigned 32-bit integer.")?;
        options.insert("seed".into(), json!(seed.to_string()));
    }
    if let Some(guidance) = guidance {
        ensure!(
            (-1.0..=7.0).contains(&guidance),
            "Magenta guidance must be between -1 and 7."
        );
        options.insert("guidance_scale".into(), json!(guidance.to_string()));
    }

    let mut body = json!({"options": options});
    if !audio_reference {
        let style = style.trim();
        ensure!(
            !style.is_empty(),
            "Describe a Magenta style or choose a library audio reference."
        );
        body["text"] = json!(style);
    }
    Ok(body)
}

fn stage_reference(source: &Path) -> Result<(PathBuf, tempfile::TempDir)> {
    let audio = audio_pcm::decode_stereo(source)
        .context("The Magenta style reference has unreadable audio.")?;
    ensure!(
        audio.frames() > 0,
        "The Magenta style reference contains no audio."
    );
    let directory = tempfile::tempdir().context("Creating the Magenta style reference folder")?;
    let path = directory.path().join("style.wav");
    std::fs::write(&path, audio_pcm::wav24_bytes(&audio)?)
        .context("Preparing the Magenta style reference audio")?;
    Ok((path, directory))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magenta_style_methods_are_exclusive_and_options_match_runtime() {
        assert_eq!(
            request_body(" jazz ", false, Some(4.25), Some(7), Some(-1.0)).unwrap(),
            json!({
                "text": "jazz",
                "options": {"duration_sec": "4.25", "seed": "7", "guidance_scale": "-1"}
            }),
        );
        assert_eq!(
            request_body("stale style", true, None, None, None).unwrap(),
            json!({"options": {}}),
        );
        assert!(request_body(" ", false, None, None, None).is_err());
        assert!(request_body("jazz", false, Some(0.02), None, None).is_err());
        assert!(request_body("jazz", false, None, Some(-1), None).is_err());
        assert!(request_body("jazz", false, None, None, Some(8.)).is_err());
    }

    #[test]
    fn magenta_reference_is_decoded_and_unreadable_audio_is_rejected() {
        let source = tempfile::tempdir().unwrap();
        let invalid = source.path().join("broken.wav");
        std::fs::write(&invalid, b"unreadable").unwrap();
        assert!(
            stage_reference(&invalid)
                .unwrap_err()
                .to_string()
                .contains("unreadable audio")
        );

        let valid = source.path().join("valid.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 22050,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&valid, spec).unwrap();
        writer.write_sample(10_000_i16).unwrap();
        writer.finalize().unwrap();
        let (staged, guard) = stage_reference(&valid).unwrap();
        assert_eq!(
            hound::WavReader::open(&staged).unwrap().spec().sample_rate,
            22050
        );
        drop(guard);
        assert!(!staged.exists());
    }
}
