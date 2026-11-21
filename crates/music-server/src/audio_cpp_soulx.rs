use crate::{CreateMusicJobRequest, audio_pcm, downloads, library};
use acpp_server::{Mode, ModelEntry};
use anyhow::{Context as _, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::Read as _,
    path::{Path, PathBuf},
};

pub(super) const FAMILY: &str = "soulx_singer";
pub(super) const SYNTHESIS: &str = FAMILY;
pub(super) const CONVERSION: &str = "soulx_singer_svc";
pub(super) const PROFILE: &str = "quality-q8";
const MAX_SCORE_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Variant {
    Synthesis,
    Conversion,
}

impl Variant {
    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Synthesis => SYNTHESIS,
            Self::Conversion => CONVERSION,
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Synthesis => "SoulX-Singer SVS",
            Self::Conversion => "SoulX-Singer SVC",
        }
    }

    pub(super) fn engine_task(self) -> &'static str {
        match self {
            Self::Synthesis => "gen",
            Self::Conversion => "svc",
        }
    }

    pub(super) fn asset(self) -> (&'static downloads::Asset, &'static str) {
        match self {
            Self::Synthesis => (&SVS, SVS_SHA),
            Self::Conversion => (&SVC, SVC_SHA),
        }
    }

    pub(super) fn missing(self, dir: &Path) -> Vec<&'static str> {
        let (asset, _) = self.asset();
        let name = asset
            .relative_path
            .rsplit('/')
            .next()
            .expect("SoulX asset filename");
        let ready = std::fs::File::open(dir.join(name)).is_ok_and(|mut file| {
            let mut magic = [0; 4];
            file.metadata()
                .is_ok_and(|metadata| metadata.len() == asset.bytes)
                && file.read_exact(&mut magic).is_ok()
                && &magic == b"GGUF"
        });
        if ready { vec![] } else { vec![name] }
    }

    pub(super) fn entry(self, dir: PathBuf) -> ModelEntry {
        let (asset, _) = self.asset();
        let name = asset
            .relative_path
            .rsplit('/')
            .next()
            .expect("SoulX asset filename");
        ModelEntry {
            id: self.id().into(),
            family: FAMILY.into(),
            task: self.engine_task().into(),
            mode: Mode::Offline,
            path: dir.join(name),
            load_options: Default::default(),
            session_options: Default::default(),
            default_request_options: Default::default(),
            busy_timeout_ms: None,
            output_rate: None,
        }
    }

    pub(super) fn catalog(
        self,
        root: Option<&Path>,
        runtime_ready: bool,
        soxr_ready: bool,
    ) -> Value {
        let mut missing = root.map_or_else(
            || vec!["Studio data folder"],
            |root| self.missing(&model_dir(root)),
        );
        if !runtime_ready {
            missing.push("audio.cpp runtime");
        }
        if self == Self::Conversion && !soxr_ready {
            missing.push("libsoxr runtime");
        }
        let asset = self.asset().0;
        json!({"id": self.id(), "name": self.name(), "profile_id": PROFILE, "quantization": "Q8_0",
            "status": "experimental", "ready": missing.is_empty(), "missing": missing,
            "bytes": asset.bytes, "installable": true, "license_url": "https://huggingface.co/audio-cpp/SoulX-Singer-GGUF",
            "output_formats": ["flac", "mp3", "wav32"]})
    }
}

pub(super) const SVS_URL: &str = "https://huggingface.co/audio-cpp/SoulX-Singer-GGUF/resolve/ded36295f789661e590e4bf6ad49a25470cf830d/SoulX-Singer-GGUF/soulx-singer-q8_0.gguf";
pub(super) const SVS_SHA: &str = "c146c09427f88202db37a71869cdd498e6c0e92fa2cd08065bd5c2f735eb82fd";
const SVS: downloads::Asset = downloads::Asset {
    id: SYNTHESIS,
    label: "SoulX-Singer SVS Q8_0",
    kind: downloads::AssetKind::Model,
    url: SVS_URL,
    relative_path: "models/soulx_singer/soulx-singer-q8_0.gguf",
    bytes: 1_173_681_632,
    unzip_into: None,
    marker: "",
    pick: &[],
    vram_gb: None,
    note: "Singing synthesis from aligned notes or pitch.",
};
pub(super) const SVC_URL: &str = "https://huggingface.co/audio-cpp/SoulX-Singer-GGUF/resolve/ded36295f789661e590e4bf6ad49a25470cf830d/SoulX-Singer-SVC-GGUF/soulx-singer-svc-q8_0.gguf";
pub(super) const SVC_SHA: &str = "1ed47055d338ddaadd7951fd8e19057fb9c367725c91e21b1b3761515db39bbb";
const SVC: downloads::Asset = downloads::Asset {
    id: CONVERSION,
    label: "SoulX-Singer SVC Q8_0",
    kind: downloads::AssetKind::Model,
    url: SVC_URL,
    relative_path: "models/soulx_singer/soulx-singer-svc-q8_0.gguf",
    bytes: 1_593_903_776,
    unzip_into: None,
    marker: "",
    pick: &[],
    vram_gb: None,
    note: "Singing voice conversion with reference audio.",
};

pub(super) fn model_dir(root: &Path) -> PathBuf {
    root.join("audiocpp/models").join(FAMILY)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Control {
    Score,
    Melody,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Options {
    reference_song_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_song_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reference_metadata: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target_metadata: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    control: Option<Control>,
}

fn recording(library: &library::Library, id: &str) -> Result<PathBuf> {
    let song = library
        .get_song(id)?
        .filter(|song| library::trashed_at(song) == 0)
        .context("Choose a singing recording from the library.")?;
    library
        .media_path_for_song(&song)
        .filter(|path| path.is_file())
        .context("The selected singing recording has no available audio.")
}

fn score(text: Option<&str>, label: &str) -> Result<String> {
    let text = text.with_context(|| format!("Provide the {label} metadata JSON."))?;
    ensure!(
        text.len() <= MAX_SCORE_BYTES,
        "The {label} metadata is larger than 256 KiB."
    );
    let segments: Vec<serde_json::Map<String, Value>> =
        serde_json::from_str(text).with_context(|| {
            format!("The {label} metadata must be a JSON array of aligned segments.")
        })?;
    ensure!(
        !segments.is_empty(),
        "The {label} metadata has no aligned segments."
    );
    Ok(text.into())
}

enum Input {
    Score {
        reference: String,
        target: String,
        control: Control,
    },
    Vocals(PathBuf),
}

pub(super) struct SingingTask {
    reference: PathBuf,
    input: Input,
    seed: i64,
}

pub(super) fn task(
    variant: Variant,
    request: &CreateMusicJobRequest,
    library: &library::Library,
) -> Result<SingingTask> {
    let options = request
        .soulx
        .as_ref()
        .context("Choose the SoulX-Singer inputs.")?;
    let reference = recording(library, &options.reference_song_id)?;
    let input = match variant {
        Variant::Synthesis => Input::Score {
            reference: score(options.reference_metadata.as_deref(), "reference")?,
            target: score(options.target_metadata.as_deref(), "target")?,
            control: options.control.clone().unwrap_or(Control::Score),
        },
        Variant::Conversion => Input::Vocals(recording(
            library,
            options
                .source_song_id
                .as_deref()
                .context("Choose the source vocals from the library.")?,
        )?),
    };
    Ok(SingingTask {
        reference,
        input,
        seed: request.seed.unwrap_or(2026),
    })
}

fn prepare_audio(source: &Path, destination: &Path) -> Result<()> {
    let audio = audio_pcm::decode_stereo(source)?.resampled(24_000)?;
    std::fs::write(destination, audio_pcm::wav24_bytes(&audio)?)?;
    Ok(())
}

impl SingingTask {
    pub(super) fn into_request(self) -> Result<(Value, tempfile::TempDir)> {
        let temp = tempfile::tempdir()?;
        let voice = temp.path().join("reference.wav");
        prepare_audio(&self.reference, &voice)
            .context("Preparing the SoulX-Singer voice reference")?;
        let mut body = json!({"voice_ref": voice, "seed": self.seed, "options": {"seed": self.seed.to_string()}});
        match self.input {
            Input::Score {
                reference,
                target,
                control,
            } => {
                let ref_file = temp.path().join("reference.json");
                let target_file = temp.path().join("target.json");
                std::fs::write(&ref_file, reference)?;
                std::fs::write(&target_file, target)?;
                body["options"]["reference_metadata_file"] = json!(ref_file);
                body["options"]["target_metadata_file"] = json!(target_file);
                body["options"]["control"] = json!(control);
            }
            Input::Vocals(source) => {
                let vocals = temp.path().join("vocals.wav");
                prepare_audio(&source, &vocals)
                    .context("Preparing the SoulX-Singer source vocals")?;
                body["audio"] = json!(vocals);
            }
        }
        Ok((body, temp))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_keep_distinct_install_paths_and_server_tasks() {
        let path = PathBuf::from("models/soulx_singer");
        let synthesis = Variant::Synthesis.entry(path.clone());
        let conversion = Variant::Conversion.entry(path);
        assert_eq!(synthesis.family, conversion.family);
        assert_ne!(synthesis.id, conversion.id);
        assert_ne!(synthesis.path, conversion.path);
        assert_eq!(synthesis.task, "gen");
        assert_eq!(conversion.task, "svc");
        let row = Variant::Conversion.catalog(None, true, false);
        assert!(
            row["missing"]
                .as_array()
                .unwrap()
                .contains(&json!("libsoxr runtime"))
        );
        assert_eq!(row["bytes"], SVC.bytes);
    }

    #[test]
    fn score_input_accepts_aligned_segments_and_rejects_empty_or_invalid_files() {
        let text = r#"[{"phoneme":"AA","duration":"0.2","note_pitch":"60","note_type":"1","time":[0,200]}]"#;
        assert_eq!(score(Some(text), "target").unwrap(), text);
        for invalid in [
            None,
            Some("[]"),
            Some("[null]"),
            Some("{}"),
            Some("bad JSON"),
        ] {
            assert!(score(invalid, "target").is_err());
        }
    }
}
