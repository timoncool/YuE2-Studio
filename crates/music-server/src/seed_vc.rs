//! Convert an isolated vocal using a short target-voice sample.
use super::*;
use serde_json::json;
use std::path::Path as FsPath;

const FILE: &str = "seed-vc-mlx-q8_0.gguf";
const BYTES: u64 = 3_120_809_440;
const DIRECTORY: &str = "audiocpp/models/seed_vc";
const MODEL_URL: &str = "https://huggingface.co/audio-cpp/audio.cpp-gguf/blob/32c1ce1fea57a513eb9386aa62408d47b1cf7ab2/SeedVC-MLX-GGUF/seed-vc-mlx-q8_0.gguf";
const PROFILE: &str = "v1_svc";

fn model(state: &AppState) -> PathBuf {
    update_data_root(state).join(DIRECTORY).join(FILE)
}

fn ready(model: &FsPath) -> bool {
    audio_cpp_music::matches_gguf_package(model, BYTES, 6025, 16)
}

async fn build(state: &AppState) -> anyhow::Result<acpp_server::asset::Build> {
    separation_build(state.separation_config.read().await.runtime)
}

pub(super) async fn status(State(state): State<AppState>) -> Json<Value> {
    let model = model(&state);
    let model_ready = ready(&model);
    let runtime_ready = build(&state)
        .await
        .is_ok_and(|build| state.acpp.installed(build));
    Json(json!({
        "ready": model_ready && runtime_ready,
        "runtime_ready": runtime_ready,
        "model_ready": model_ready,
        "model_path": plain_path(&model),
        "model_url": MODEL_URL,
        "model_source_url": "https://huggingface.co/Plachta/Seed-VC",
        "bytes": BYTES
    }))
}

#[derive(Deserialize)]
pub(super) struct Apply {
    song_id: String,
    upload_id: String,
}

pub(super) async fn apply(
    State(state): State<AppState>,
    Json(input): Json<Apply>,
) -> Result<(StatusCode, Json<Value>), rvc_voices::ApiFailure> {
    let original = state
        .library
        .get_song(&input.song_id)
        .map_err(rvc_voices::internal)?
        .ok_or_else(|| {
            api_error(
                StatusCode::NOT_FOUND,
                "The source song was not found.".into(),
            )
        })?;
    refuse_trashed(&original)?;
    let source = state
        .library
        .media_path_for_song(&original)
        .filter(|path| path.is_file())
        .ok_or_else(|| {
            api_error(
                StatusCode::CONFLICT,
                "The source audio is unavailable.".into(),
            )
        })?;
    let vocal = stem_path(&state, &original.id, "vocals");
    let instrumental = stem_path(&state, &original.id, "instrumental");
    let stems = if instrumental.is_file() {
        vec![instrumental]
    } else {
        separation::STEMS
            .iter()
            .filter(|stem| **stem != "vocals")
            .map(|stem| stem_path(&state, &original.id, stem))
            .collect()
    };
    if !vocal.is_file() || !stems.iter().all(|path| path.is_file()) {
        return Err(api_error(
            StatusCode::CONFLICT,
            "Separate the song into vocals and accompaniment first.".into(),
        ));
    }
    let reference = input
        .upload_id
        .starts_with("reference-")
        .then(|| processing::workspace_file(state.library.media_dir(), &input.upload_id))
        .flatten()
        .ok_or_else(|| rvc_voices::invalid("Upload the reference voice audio first."))?;
    let model = model(&state);
    let build = build(&state).await.map_err(rvc_voices::internal)?;
    if !ready(&model) || !state.acpp.installed(build) {
        return Err(api_error(
            StatusCode::CONFLICT,
            "Install the Seed-VC model and audio.cpp runtime first.".into(),
        ));
    }
    let lease = state.engine_use.clone().try_write_owned().map_err(|_| {
        api_error(
            StatusCode::CONFLICT,
            "The local music engine is busy.".into(),
        )
    })?;
    no_song_rendering(&state)
        .await
        .map_err(|error| api_error(StatusCode::CONFLICT, error))?;
    let result = tokio::task::spawn_blocking(move || {
        let _lease = lease;
        let input_audio = audio_pcm::decode_stereo(&vocal)?;
        let source_audio = audio_pcm::decode_stereo(&source)?;
        let (frames, rate) = (source_audio.frames(), source_audio.rate);
        drop(source_audio);
        let reference_audio = audio_pcm::decode_stereo(&reference)?;
        let seconds = reference_audio.frames() as f64 / f64::from(reference_audio.rate);
        if !(1.0..=25.0).contains(&seconds) {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Choose a voice sample from 1 to 25 seconds."
                })),
            ));
        }
        let stage = tempfile::tempdir()?;
        let reference_clip = stage.path().join("reference.wav");
        std::fs::write(&reference_clip, audio_pcm::wav_f32_bytes(&reference_audio)?)?;
        let entry = acpp_server::ModelEntry {
            id: "seed_vc".into(),
            family: "seed_vc".into(),
            task: "svc".into(),
            mode: acpp_server::Mode::Offline,
            path: model,
            load_options: Default::default(),
            session_options: Default::default(),
            default_request_options: Default::default(),
            busy_timeout_ms: None,
            output_rate: None,
        };
        let reply = acpp::run(
            &state.acpp,
            build,
            entry,
            json!({
                "audio": vocal, "voice_ref": reference_clip, "num_inference_steps": 30, "seed": 42,
                "options": {"route": PROFILE, "duration_scale": "1.0"}
            }),
            &|| false,
        )?;
        let artifact = acpp_server::task_audio(&reply)?
            .into_iter()
            .next()
            .context("Seed-VC returned no audio.")?;
        let channels = artifact.channels as usize;
        anyhow::ensure!(
            (1..=2).contains(&channels)
                && artifact.sample_rate > 0
                && !artifact.samples.is_empty()
                && artifact.samples.len() % channels == 0
                && artifact.samples.iter().all(|sample| sample.is_finite()),
            "Seed-VC returned invalid audio."
        );
        let left = artifact.samples.iter().step_by(channels).copied().collect();
        let right = artifact
            .samples
            .iter()
            .skip(channels - 1)
            .step_by(channels)
            .copied()
            .collect();
        let converted =
            audio_post::Stereo::new(left, right, artifact.sample_rate).resampled(rate)?;
        let vocals = input_audio.resampled(rate)?;
        let mut backing = audio_post::Stereo::new(vec![0.0; frames], vec![0.0; frames], rate);
        for stem in stems {
            rvc_apply::add_aligned(
                &mut backing,
                &audio_pcm::decode_stereo(&stem)?.resampled(rate)?,
            )?;
        }
        let protected = mix_protecting_pauses(&mut backing, &vocals, &converted)?;
        let song = state.library.import_audio_song(library::AudioImportInput {
            title: format!("{} · Seed-VC", original.title),
            caption: original.caption.clone(),
            lyrics: original.lyrics.clone(),
            metadata: json!({
                "derived": derivation(&original, "seed_vc", json!({
                    "route": PROFILE, "reference_seconds": seconds, "protected_frames": protected
                })),
                "duration_seconds": frames as f64 / f64::from(rate)
            }),
            generation_settings: Value::Null,
            engine_id: "seed_vc".into(),
            profile_id: Some(PROFILE.into()),
            source: "seed_vc".into(),
            audio_extension: "wav".into(),
            audio: audio_pcm::wav_f32_bytes(&backing)?,
        })?.song;
        if let Err(error) = cover_like(&state, &original, &song.id) {
            studio_log!("[Seed-VC] Could not copy the cover: {error:#}");
        }
        Ok::<_, anyhow::Error>((
            StatusCode::CREATED,
            Json(json!({
                "song_id": song.id, "audio_url": format!("/v1/library/media/{}", song.id)
            })),
        ))
    })
    .await
    .map_err(rvc_voices::internal)?;
    result.map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("{error:#}")))
}

/// Use the original vocal's activity on its original timeline. Keep note tails
/// and fade into long pauses while preserving the backing at its existing gain.
fn mix_protecting_pauses(
    backing: &mut audio_post::Stereo,
    vocal: &audio_post::Stereo,
    converted: &audio_post::Stereo,
) -> anyhow::Result<usize> {
    let frames = backing.frames();
    let rate = backing.rate as usize;
    anyhow::ensure!(
        rate >= 100
            && backing.rate == vocal.rate
            && backing.rate == converted.rate
            && frames.abs_diff(vocal.frames()) <= rate / 4
            && frames.abs_diff(converted.frames()) <= rate / 4,
        "The converted vocal does not align with the source song."
    );
    let window = rate / 100;
    let active: Vec<bool> = vocal
        .left
        .chunks(window)
        .zip(vocal.right.chunks(window))
        .map(|(left, right)| {
            left.iter()
                .chain(right)
                .any(|sample| sample.abs() >= 0.0018)
        })
        .collect();
    let mut gains = vec![0.0f32; frames.div_ceil(window) + 1];
    for (index, _) in active
        .into_iter()
        .enumerate()
        .filter(|(_, has_voice)| *has_voice)
    {
        for offset in index.saturating_sub(7)..(index + 8).min(gains.len()) {
            let gain = (8 - index.abs_diff(offset)).min(3) as f32 / 3.0;
            gains[offset] = gains[offset].max(gain);
        }
    }
    let mut protected = 0;
    for index in 0..frames {
        let block = index / window;
        let fraction = (index % window) as f32 / window as f32;
        let gain = gains[block] * (1.0 - fraction) + gains[block + 1] * fraction;
        if gain == 0.0 {
            protected += 1;
            continue;
        }
        if let (Some(&left), Some(&right)) = (converted.left.get(index), converted.right.get(index))
        {
            for (mix, wet) in [
                (&mut backing.left[index], left),
                (&mut backing.right[index], right),
            ] {
                *mix += (wet * gain).clamp((-1.0 - *mix).min(0.0), (1.0 - *mix).max(0.0));
            }
        }
    }
    Ok(protected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_pause_keeps_the_instrumental_samples_and_the_source_length() {
        let rate = 1000;
        let mut vocal = vec![0.0; 1000];
        vocal[200..400].fill(0.2);
        vocal[700..900].fill(0.2);
        let vocal = audio_post::Stereo::new(vocal.clone(), vocal, rate);
        let converted = audio_post::Stereo::new(vec![0.4; 990], vec![0.4; 990], rate);
        let mut backing = audio_post::Stereo::new(vec![0.25; 1000], vec![-0.2; 1000], rate);
        assert!(mix_protecting_pauses(&mut backing, &vocal, &converted).unwrap() > 200);
        assert_eq!(backing.frames(), 1000);
        assert_eq!(&backing.left[500..600], &[0.25; 100]);
        assert_eq!(&backing.right[500..600], &[-0.2; 100]);
        assert!(backing.left[300] > 0.5);
    }
}
