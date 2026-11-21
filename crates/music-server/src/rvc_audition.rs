use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::{
    Json,
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    AppState, api_error, audio_pcm,
    rvc_apply::{self, ApplyRequest, Backend},
    rvc_voices, serve_audio_file,
};

const KEEP: Duration = Duration::from_secs(30 * 60);
const CAPACITY: usize = 6;
const MAX_SECONDS: f64 = 15.0;

#[derive(Default)]
pub(super) struct Cache(Mutex<VecDeque<(String, Instant, Arc<tempfile::TempDir>)>>);

#[derive(Deserialize)]
pub(super) struct PreviewRequest {
    #[serde(flatten)]
    apply: ApplyRequest,
    start_seconds: f64,
    end_seconds: f64,
}

#[derive(Clone, Copy)]
pub(super) struct Range {
    start: f64,
    end: f64,
}

pub(super) struct Clip {
    pub(super) input: std::path::PathBuf,
    source: audio_post::Stereo,
    offset: usize,
    start: usize,
    end: usize,
}

impl Range {
    fn new(start: f64, end: f64) -> Result<Self, rvc_voices::ApiFailure> {
        if !start.is_finite()
            || !end.is_finite()
            || start < 0.0
            || end - start < 0.5
            || end - start > MAX_SECONDS
        {
            return Err(rvc_voices::invalid(
                "Choose a vocal fragment between 0.5 and 15 seconds.",
            ));
        }
        Ok(Self { start, end })
    }

    pub(super) fn stage(
        self,
        source: &audio_post::Stereo,
        folder: &std::path::Path,
    ) -> anyhow::Result<Clip> {
        let rate = source.rate as f64;
        let start = (self.start * rate).round() as usize;
        let end = ((self.end * rate).round() as usize).min(source.frames());
        anyhow::ensure!(
            end.saturating_sub(start) >= (rate / 2.0) as usize,
            "The preview starts too close to the end of the vocal."
        );
        let pad = (rate / 2.0) as usize;
        let left = start.saturating_sub(pad);
        let right = end.saturating_add(pad).min(source.frames());
        let input = folder.join("preview-input.wav");
        audio_pcm::write_wav24(&input, &slice(source, left, right))?;
        Ok(Clip {
            input,
            source: slice(source, start, end),
            offset: start - left,
            start,
            end,
        })
    }
}

fn slice(audio: &audio_post::Stereo, start: usize, end: usize) -> audio_post::Stereo {
    audio_post::Stereo::new(
        audio.left[start..end].to_vec(),
        audio.right[start..end].to_vec(),
        audio.rate,
    )
}

impl Clip {
    pub(super) fn publish(
        self,
        cache: &Cache,
        converted: audio_post::Stereo,
        requested: Backend,
        effective: Backend,
        fallback: Option<String>,
    ) -> anyhow::Result<Value> {
        let mut converted = converted.resampled(self.source.rate)?;
        let end = self.offset + self.source.frames();
        anyhow::ensure!(
            end.saturating_sub(converted.frames()) <= self.source.rate as usize / 4,
            "The converted fragment does not align with the source vocal."
        );
        converted.left.resize(end, 0.0);
        converted.right.resize(end, 0.0);
        let after = slice(&converted, self.offset, end);
        anyhow::ensure!(
            after
                .left
                .iter()
                .chain(&after.right)
                .all(|sample| sample.is_finite()),
            "The converted fragment contains invalid audio."
        );
        let folder = tempfile::tempdir()?;
        for (side, audio) in [("before", &self.source), ("after", &after)] {
            std::fs::write(
                folder.path().join(format!("{side}.wav")),
                audio_pcm::wav_f32_bytes(audio)?,
            )?;
        }
        let id = uuid::Uuid::now_v7().to_string();
        let mut entries = cache.0.lock().unwrap();
        entries.retain(|(_, time, _)| time.elapsed() < KEEP);
        while entries.len() >= CAPACITY {
            entries.pop_front();
        }
        entries.push_back((id.clone(), Instant::now(), Arc::new(folder)));
        Ok(json!({
            "id": id,
            "before_url": format!("/v1/rvc/preview/{id}/before"),
            "after_url": format!("/v1/rvc/preview/{id}/after"),
            "start_seconds": self.start as f64 / self.source.rate as f64,
            "end_seconds": self.end as f64 / self.source.rate as f64,
            "frames": self.source.frames(),
            "sample_rate": self.source.rate,
            "requested_backend": requested, "effective_backend": effective, "fallback_reason": fallback
        }))
    }
}

pub(super) async fn preview(
    State(state): State<AppState>,
    Json(request): Json<PreviewRequest>,
) -> Result<(StatusCode, Json<Value>), rvc_voices::ApiFailure> {
    let range = Range::new(request.start_seconds, request.end_seconds)?;
    crate::card_free_of_training(&state, "preview vocals").await?;
    rvc_apply::process_request(state, request.apply, None, Some(range)).await
}

pub(super) async fn audio(
    State(state): State<AppState>,
    Path((id, side)): Path<(String, String)>,
    request: Request,
) -> Result<Response, rvc_voices::ApiFailure> {
    let missing = || {
        api_error(
            StatusCode::NOT_FOUND,
            "This RVC preview has expired.".into(),
        )
    };
    if !matches!(side.as_str(), "before" | "after") {
        return Err(missing());
    }
    let folder = {
        let mut entries = state.rvc_auditions.0.lock().unwrap();
        entries.retain(|(_, time, _)| time.elapsed() < KEEP);
        entries
            .iter()
            .find(|(key, _, _)| key == &id)
            .map(|(_, _, folder)| folder.clone())
    }
    .ok_or_else(missing)?;
    Ok(serve_audio_file(&folder.path().join(format!("{side}.wav")), request).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_the_same_sample_range_on_both_sides() {
        let temp = tempfile::tempdir().unwrap();
        let audio = audio_post::Stereo::new(vec![0.25; 2000], vec![0.0; 2000], 1000);
        let clip = Range::new(0.7, 2.1)
            .unwrap()
            .stage(&audio, temp.path())
            .unwrap();
        let cache = Cache::default();
        let converted = audio_post::Stereo::new(vec![0.1; 1775], vec![0.0; 1775], 1000);
        let result = clip
            .publish(&cache, converted, Backend::Auto, Backend::AudioCpp, None)
            .unwrap();
        assert_eq!(result["start_seconds"], 0.7);
        assert_eq!(result["end_seconds"], 2.0);
        assert_eq!(result["frames"], 1300);
        let state = cache.0.lock().unwrap();
        for side in ["before", "after"] {
            let wave =
                audio_pcm::decode_stereo(&state[0].2.path().join(format!("{side}.wav"))).unwrap();
            assert_eq!(wave.frames(), 1300);
        }
    }

    #[test]
    fn rejects_invalid_or_unbounded_ranges() {
        for (start, end) in [(-1.0, 2.0), (3.0, 3.0), (0.0, 15.01), (0.0, f64::INFINITY)] {
            assert!(Range::new(start, end).is_err(), "{start} to {end}");
        }
    }
}
