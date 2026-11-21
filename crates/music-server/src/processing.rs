//! Processing a finished track: noise reduction, the Spectral Lifter, vocal
//! naturalising, the user's own VST3 plugins, mastering to a reference and
//! peak normalisation, in that order. A song comes out of the engine as the
//! model made it; anything that changes its level happens here, by choice.
//!
//! A run never touches the track. It leaves a preview beside the library, to be
//! heard against the original and then kept as a version or thrown away; a kept
//! version plays in place of the original, which stays on disk and can be
//! chosen again at any time.

use std::ops::Range;
use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Context, Result};
use audio_post::{denoise, lifter, mastering, naturalize, Stereo};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What to do to a track. A stage left out is skipped.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProcessRequest {
    #[serde(default)]
    pub denoise: Option<denoise::DenoiseSettings>,
    #[serde(default)]
    pub lifter: Option<lifter::LifterSettings>,
    #[serde(default)]
    pub naturalize: Option<naturalize::NaturalizeSettings>,
    /// VST3 plugins, run in order before mastering.
    #[serde(default)]
    pub vst: Option<Vec<crate::vst::VstSlot>>,
    #[serde(default)]
    pub master: Option<MasterSource>,
    #[serde(default)]
    pub normalize: Option<NormalizeSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<ProcessRegion>,
}

/// A half-open interval in the source recording, in seconds.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ProcessRegion {
    pub start_seconds: f64,
    pub end_seconds: f64,
}

impl ProcessRegion {
    pub fn validate(self) -> Result<()> {
        ensure!(
            self.start_seconds.is_finite() && self.end_seconds.is_finite()
                && self.start_seconds >= 0.0 && self.end_seconds > self.start_seconds,
            "Choose a non-empty processing range with a nonnegative start and an end after it."
        );
        Ok(())
    }

    fn frames(self, rate: u32, count: usize) -> Result<Range<usize>> {
        self.validate()?;
        let start = (self.start_seconds * f64::from(rate)).round() as usize;
        let end = (self.end_seconds * f64::from(rate)).round() as usize;
        ensure!(start < end && end <= count, "The selected range must fit within the source audio.");
        Ok(start..end)
    }
}

/// Peak normalisation: the level all but `peak_clip` samples per million stay
/// under becomes full scale, and those few are clipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizeSettings {
    #[serde(default = "default_peak_clip")]
    pub peak_clip: u32,
}

fn default_peak_clip() -> u32 {
    10
}

/// The reference a track is mastered to: another song of the library, or a
/// file uploaded for the purpose.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MasterSource {
    Song { song_id: String },
    Upload { upload_id: String },
}

impl ProcessRequest {
    pub fn stages(&self) -> Vec<&'static str> {
        let mut stages = Vec::new();
        if self.denoise.is_some() {
            stages.push("denoise");
        }
        if self.lifter.is_some() {
            stages.push("lifter");
        }
        if self.naturalize.is_some() {
            stages.push("naturalize");
        }
        if self.vst.as_ref().is_some_and(|chain| chain.iter().any(|slot| slot.enabled)) {
            stages.push("vst");
        }
        if self.master.is_some() {
            stages.push("master");
        }
        if self.normalize.is_some() {
            stages.push("normalize");
        }
        stages
    }
}

/// The run in progress or the last one, as the interface polls it.
#[derive(Debug, Clone, Serialize)]
pub struct ProcessRun {
    /// Tells a finishing worker whether its run is still the current one.
    pub id: String,
    pub song_id: String,
    pub stages: Vec<&'static str>,
    /// The stage working now, once started.
    pub stage: Option<&'static str>,
    pub done: bool,
    pub error: Option<String>,
    /// The preview's file name inside the processing folder, when ready.
    #[serde(skip)]
    pub preview: Option<String>,
    pub preview_ready: bool,
    pub request: ProcessRequest,
}

impl ProcessRun {
    /// The files only this run owns: its preview and an uploaded reference.
    pub fn leftovers(&self, media: &Path) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = self.preview.iter().filter_map(|name| workspace_file(media, name)).collect();
        if let Some(MasterSource::Upload { upload_id }) = &self.request.master {
            files.extend(workspace_file(media, upload_id));
        }
        files
    }
}

/// Empties the workspace: previews and references live for one sitting.
pub fn clear_workspace(media: &Path) {
    let Ok(entries) = std::fs::read_dir(workspace(media)) else { return };
    for entry in entries.flatten() {
        if entry.path().is_file() {
            if let Err(error) = std::fs::remove_file(entry.path()) {
                eprintln!("[ERROR] processing: remove {}: {error}", entry.path().display());
            }
        }
    }
}

/// Where previews and uploaded references wait, inside the media folder.
pub fn workspace(media: &Path) -> PathBuf {
    media.join("processing")
}

/// A name that is a plain file of the workspace, never a path out of it.
pub fn workspace_file(media: &Path, name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.contains(['/', '\\', ':']) || name.starts_with('.') {
        return None;
    }
    let path = workspace(media).join(name);
    path.is_file().then_some(path)
}

/// Renders a complete preview while processing either a selection or the whole track.
pub fn render_preview(
    source: &Path,
    preview: &Path,
    reference: Option<&Path>,
    request: &ProcessRequest,
    vst: Option<&crate::vst::VstHost>,
    on_stage: impl Fn(&'static str),
) -> Result<()> {
    if let Some(region) = request.region {
        let (mut planes, rate) = crate::audio_pcm::decode_channels(source)?;
        ensure!((1..=2).contains(&planes.len()), "Range processing requires a mono or stereo recording.");
        let frames = region.frames(rate, planes[0].len())?;
        ensure!(planes.iter().all(|channel| channel.len() == planes[0].len()), "Source channels have different lengths.");
        let left = planes[0][frames.clone()].to_vec();
        let right = planes.get(1).map_or_else(|| left.clone(), |plane| plane[frames.clone()].to_vec());
        let edited = run_stages(Stereo::new(left, right, rate), source, reference, request, vst, on_stage)?;
        ensure!(
            edited.rate == rate && edited.frames() == frames.len() && edited.right.len() == frames.len(),
            "An effect changed the sample rate or duration of the selected audio."
        );
        if planes.len() == 1 {
            let mono: Vec<_> = edited.left.iter().zip(&edited.right)
                .map(|(left, right)| 0.5 * left + 0.5 * right).collect();
            splice(&mut planes[0], &mono, frames, rate);
        } else {
            splice(&mut planes[0], &edited.left, frames.clone(), rate);
            splice(&mut planes[1], &edited.right, frames, rate);
        }
        crate::audio_pcm::write_wav_planes_f32(preview, rate, &planes)?;
    } else {
        let audio = crate::audio_pcm::decode_stereo(source)?;
        let audio = run_stages(audio, source, reference, request, vst, on_stage)?;
        std::fs::write(preview, audio_post::encode::flac(&audio)?)
            .with_context(|| format!("write {}", preview.display()))?;
    }
    Ok(())
}

fn splice(plane: &mut [f32], edited: &[f32], range: Range<usize>, rate: u32) {
    // The fades are inside the edit. The preceding and following samples are never written.
    let fade = (rate as usize / 200).min((range.len() - 1) / 2);
    let last = range.len() - 1;
    let fade_in = range.start > 0;
    let fade_out = range.end < plane.len();
    for (index, (source, &sample)) in plane[range].iter_mut().zip(edited).enumerate() {
        let distance = if fade_in && index < fade {
            index
        } else if fade_out && last - index < fade {
            last - index
        } else {
            fade
        };
        if distance == fade {
            *source = sample;
        } else if distance > 0 {
            let wet = distance as f32 / fade as f32;
            *source = *source * (1.0 - wet) + sample * wet;
        }
    }
}

fn run_stages(
    mut audio: Stereo,
    source: &Path,
    reference: Option<&Path>,
    request: &ProcessRequest,
    vst: Option<&crate::vst::VstHost>,
    on_stage: impl Fn(&'static str),
) -> Result<Stereo> {
    if request.stages().is_empty() {
        bail!("choose at least one kind of processing");
    }
    if let Some(settings) = &request.denoise {
        on_stage("denoise");
        audio = denoise::denoise(&audio, settings);
    }
    if let Some(settings) = &request.lifter {
        on_stage("lifter");
        audio = lifter::lift(&audio, settings);
    }
    if let Some(settings) = &request.naturalize {
        on_stage("naturalize");
        audio = naturalize::naturalize(&audio, settings);
    }
    if let Some(chain) = request.vst.as_ref().filter(|chain| chain.iter().any(|slot| slot.enabled)) {
        on_stage("vst");
        let host = vst.context("the VST host is not installed")?;
        let work = source.parent().context("the track has no folder")?.join("processing");
        audio = host.process(&audio, chain, &work)?;
    }
    if request.master.is_some() {
        on_stage("master");
        let reference = reference.context("mastering needs a reference track")?;
        let reference = crate::audio_pcm::decode_stereo(reference)?;
        audio = mastering::master(&audio, &reference, &mastering::MasteringConfig::default())?;
    }
    if let Some(settings) = &request.normalize {
        on_stage("normalize");
        audio_post::encode::normalize_peak(&mut audio, settings.peak_clip);
    }
    Ok(audio)
}

/// The processing settings a kept version records, for showing and repeating.
pub fn settings_record(request: &ProcessRequest, reference_title: Option<&str>) -> Value {
    let mut value = serde_json::to_value(request).unwrap_or(Value::Null);
    if let (Some(title), Some(object)) = (reference_title, value.as_object_mut()) {
        object.insert("reference_title".into(), Value::String(title.to_string()));
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_preview_preserves_decoded_samples_outside_and_keeps_channel_count() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.wav");
        let preview = root.path().join("preview.wav");
        let rate = 8_000;
        for channel_count in [1, 2] {
            let mut planes = vec![vec![0.125; rate as usize]];
            if channel_count == 2 {
                planes.push(vec![-0.25; rate as usize]);
            }
            crate::audio_pcm::write_wav_planes_f32(&source, rate, &planes).unwrap();
            let request = ProcessRequest {
                normalize: Some(NormalizeSettings { peak_clip: 0 }),
                region: Some(ProcessRegion { start_seconds: 0.25, end_seconds: 0.75 }),
                ..Default::default()
            };
            render_preview(&source, &preview, None, &request, None, |_| {}).unwrap();
            let (actual, actual_rate) = crate::audio_pcm::decode_channels(&preview).unwrap();
            assert_eq!(actual_rate, rate);
            assert_eq!(actual.len(), channel_count);
            for (original, edited) in planes.iter().zip(&actual) {
                assert_eq!(edited.len(), original.len());
                assert_eq!(&edited[..2_000], &original[..2_000]);
                assert_eq!(&edited[6_000..], &original[6_000..]);
                assert_eq!(edited[2_000], original[2_000]);
                assert_eq!(edited[5_999], original[5_999]);
                assert_ne!(edited[4_000], original[4_000]);
            }
        }
        let bad = ProcessRegion { start_seconds: 0.75, end_seconds: 1.25 };
        assert!(bad.frames(rate, rate as usize).is_err());
    }

    #[test]
    fn stages_follow_the_request_in_processing_order() {
        let request = ProcessRequest {
            master: Some(MasterSource::Song { song_id: "x".into() }),
            denoise: Some(Default::default()),
            ..Default::default()
        };
        assert_eq!(request.stages(), vec!["denoise", "master"]);
        let parsed: ProcessRequest =
            serde_json::from_value(serde_json::json!({"lifter": {"shimmer_reduction_db": 3.0}, "master": {"type": "upload", "upload_id": "u"}})).unwrap();
        assert_eq!(parsed.stages(), vec!["lifter", "master"]);
        assert_eq!(parsed.lifter.unwrap().shimmer_reduction_db, 3.0);
    }

    #[test]
    fn a_vst_chain_runs_before_mastering_only_with_a_plugin_on() {
        let chain = |enabled| serde_json::json!([{ "path": "C:/x.vst3", "name": "X", "enabled": enabled }]);
        let on: ProcessRequest = serde_json::from_value(serde_json::json!({ "vst": chain(true), "master": { "type": "upload", "upload_id": "u" } })).unwrap();
        assert_eq!(on.stages(), vec!["vst", "master"]);
        let level: ProcessRequest = serde_json::from_value(serde_json::json!({ "denoise": {}, "normalize": {} })).unwrap();
        assert_eq!(level.stages(), vec!["denoise", "normalize"]);
        assert_eq!(level.normalize, Some(NormalizeSettings { peak_clip: 10 }));
        let off: ProcessRequest = serde_json::from_value(serde_json::json!({ "vst": chain(false) })).unwrap();
        assert!(off.stages().is_empty());
    }

    #[test]
    fn workspace_names_cannot_leave_the_folder() {
        let media = std::env::temp_dir();
        assert!(workspace_file(&media, "../library.sqlite").is_none());
        assert!(workspace_file(&media, "a\\b.wav").is_none());
        assert!(workspace_file(&media, "").is_none());
    }
}
