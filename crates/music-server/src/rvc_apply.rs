use super::*;
use serde_json::json;
use std::path::Path as FsPath;

const MODEL_URL: &str = "https://huggingface.co/audio-cpp/audio.cpp-gguf/resolve/32c1ce1fea57a513eb9386aa62408d47b1cf7ab2/RVC-GGUF/rvc-f16.gguf";
const MODEL: downloads::Asset = downloads::Asset {
    id: "rvc-f16",
    label: "RVC F16 GGUF",
    kind: downloads::AssetKind::Model,
    url: MODEL_URL,
    relative_path: "models/rvc/rvc-f16.gguf",
    bytes: 1_260_187_648,
    unzip_into: None,
    marker: "",
    pick: &[],
    vram_gb: None,
    note: "Native RVC voice conversion support weights for audio.cpp.",
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Backend {
    #[default]
    Auto,
    VocalFastRs,
    #[serde(rename = "audio.cpp")]
    AudioCpp,
}

pub(super) async fn setup(
    State(state): State<AppState>,
) -> Result<Json<Value>, rvc_voices::ApiFailure> {
    Ok(Json(setup_status(&state).await))
}

pub(super) async fn install(
    State(state): State<AppState>,
) -> Result<Json<Value>, rvc_voices::ApiFailure> {
    let build = selected_build(&state).await?;
    install_acpp(&state, build, "rvc-apply")
        .await
        .map_err(bad_gateway)?;
    state
        .separator
        .downloader()
        .install_all("rvc-apply", &[&MODEL])
        .await
        .map_err(bad_gateway)?;
    Ok(Json(setup_status(&state).await))
}

async fn setup_status(state: &AppState) -> Value {
    let native = fast_status(update_data_root(state)).await;
    let native_ready = native.as_ref().is_ok_and(|ready| *ready);
    let build = selected_build(state).await.ok();
    let model_installed = model_ready(state);
    let audio_ready = build.is_some_and(|build| model_installed && state.acpp.installed(build));
    let missing_bytes = build.map(|build| {
        state
            .acpp
            .missing(build)
            .iter()
            .map(|asset| asset.bytes)
            .sum::<u64>()
            + if model_installed { 0 } else { MODEL.bytes }
    });
    let effective = effective_backend(Backend::Auto, native_ready, audio_ready);
    json!({
        "effective_backend": effective,
        "backends": {
            "vocal-fast-rs": { "ready": native_ready, "can_start": native.is_ok(), "reason": native.err() },
            "audio.cpp": { "ready": audio_ready, "missing_bytes": missing_bytes },
        },
    })
}

fn effective_backend(requested: Backend, native_ready: bool, audio_ready: bool) -> Option<Backend> {
    match requested {
        Backend::Auto if native_ready => Some(Backend::VocalFastRs),
        Backend::Auto if audio_ready => Some(Backend::AudioCpp),
        Backend::VocalFastRs if native_ready => Some(Backend::VocalFastRs),
        Backend::AudioCpp if audio_ready => Some(Backend::AudioCpp),
        _ => None,
    }
}

async fn fast_status(data_root: &FsPath) -> Result<bool, String> {
    let data_root = data_root.to_path_buf();
    tokio::task::spawn_blocking(move || rvc_fast::status(&data_root))
        .await
        .map_err(|error| format!("The vocal-fast-rs status check stopped: {error}"))?
}

async fn fast_availability(data_root: &FsPath) -> Result<(), String> {
    let data_root = data_root.to_path_buf();
    tokio::task::spawn_blocking(move || rvc_fast::availability(&data_root))
        .await
        .map_err(|error| format!("The vocal-fast-rs readiness check stopped: {error}"))?
}

async fn selected_build(
    state: &AppState,
) -> Result<acpp_server::asset::Build, rvc_voices::ApiFailure> {
    separation_build(state.separation_config.read().await.runtime)
        .map_err(|error| api_error(StatusCode::CONFLICT, error.to_string()))
}

fn model_ready(state: &AppState) -> bool {
    state.separator.downloader().is_installed(&MODEL)
}

fn bad_gateway(error: impl std::fmt::Display) -> rvc_voices::ApiFailure {
    api_error(StatusCode::BAD_GATEWAY, error.to_string())
}

#[derive(Deserialize)]
pub(super) struct ApplyRequest {
    pub(super) song_id: String,
    pub(super) voice_id: String,
    #[serde(default)]
    pub(super) backend: Backend,
    #[serde(default)]
    pub(super) semitone_shift: i32,
    #[serde(default)]
    pub(super) retrieval_blend: f64,
}

pub(super) async fn apply(
    State(state): State<AppState>,
    Json(request): Json<ApplyRequest>,
) -> Result<(StatusCode, Json<Value>), rvc_voices::ApiFailure> {
    apply_with(state, request, None).await
}

pub(super) async fn apply_with(
    state: AppState,
    request: ApplyRequest,
    quick: Option<std::sync::Arc<crate::rvc_quick::Run>>,
) -> Result<(StatusCode, Json<Value>), rvc_voices::ApiFailure> {
    if !(-24..=24).contains(&request.semitone_shift)
        || !(0.0..=1.0).contains(&request.retrieval_blend)
    {
        return Err(rvc_voices::invalid(
            "RVC pitch shift must be between -24 and 24 semitones, and index strength between 0 and 1.",
        ));
    }
    let original = state
        .library
        .get_song(&request.song_id)
        .map_err(rvc_voices::internal)?
        .ok_or_else(|| {
            api_error(
                StatusCode::NOT_FOUND,
                "The source song was not found.".into(),
            )
        })?;
    refuse_trashed(&original)?;
    let source = original
        .audio_path
        .as_deref()
        .and_then(|path| state.library.resolve_media(path))
        .filter(|path| path.is_file())
        .ok_or_else(|| {
            api_error(
                StatusCode::CONFLICT,
                "The source audio is unavailable.".into(),
            )
        })?;
    let vocal = stem_path(&state, &original.id, "vocals");
    let instrumental = stem_path(&state, &original.id, "instrumental");
    let accompaniment = if instrumental.is_file() {
        vec![instrumental]
    } else {
        separation::STEMS
            .iter()
            .filter(|stem| **stem != "vocals")
            .map(|stem| stem_path(&state, &original.id, stem))
            .collect()
    };
    if !vocal.is_file() || !accompaniment.iter().all(|stem| stem.is_file()) {
        return Err(api_error(
            StatusCode::CONFLICT,
            "Separate the song into vocals and accompaniment first.".into(),
        ));
    }
    let voice_root = update_data_root(&state).join("rvc/voices");
    if request.voice_id.len() != 64
        || !request
            .voice_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(rvc_voices::invalid("Use an installed RVC voice ID."));
    }
    let voice = rvc_voices::read_voice(&voice_root, &request.voice_id).ok_or_else(|| {
        api_error(
            StatusCode::NOT_FOUND,
            "The installed RVC voice was not found.".into(),
        )
    })?;
    let native = if request.backend == Backend::AudioCpp {
        Err("audio.cpp was explicitly requested.".into())
    } else {
        fast_availability(update_data_root(&state)).await
    };
    let audio_build = selected_build(&state)
        .await
        .ok()
        .filter(|build| model_ready(&state) && state.acpp.installed(*build));
    let effective = effective_backend(request.backend, native.is_ok(), audio_build.is_some())
        .ok_or_else(|| {
            api_error(
                StatusCode::CONFLICT,
                if request.backend == Backend::VocalFastRs {
                    native
                        .as_ref()
                        .err()
                        .cloned()
                        .unwrap_or_else(|| "vocal-fast-rs is unavailable.".into())
                } else {
                    "Install the RVC conversion components first.".into()
                },
            )
        })?;
    let model = state.separator.downloader().path_of(&MODEL);
    let result = tokio::task::spawn_blocking(move || {
        let stage = tempfile::tempdir_in(&voice_root)?;
        let folder = voice_root.join(&voice.id);
        let copy = |file: &str| -> anyhow::Result<PathBuf> {
            let source = folder.join(file);
            let destination = stage.path().join(file);
            if fs::hard_link(&source, &destination).is_err() {
                fs::copy(source, &destination)?;
            }
            Ok(destination)
        };
        let weights = copy("voice.pth")?;
        let hash = install_record::sha256_of(&weights)?;
        let index = voice.has_index().then(|| copy("retrieval.index")).transpose()?;
        let blend = if index.is_some() { request.retrieval_blend } else { 0.0 };
        // Validate the source audio and accompaniment before dispatching a backend.
        let (frames, rate) = {
            let source_audio = audio_pcm::decode_stereo(&source)?;
            (source_audio.frames(), source_audio.rate)
        };
        {
            let vocal_audio = audio_pcm::decode_stereo(&vocal)?;
            let duration_delta = (frames as f64 / rate as f64
                - vocal_audio.frames() as f64 / vocal_audio.rate as f64).abs();
            let valid_audio = vocal_audio.left.iter()
                .chain(&vocal_audio.right)
                .all(|sample| sample.is_finite());
            anyhow::ensure!(
                duration_delta <= 0.25 && valid_audio,
                "The vocal stem does not align with the source song or contains invalid audio."
            );
        }
        let mut backing = audio_post::Stereo::new(vec![0.0; frames], vec![0.0; frames], rate);
        for path in accompaniment {
            let stem = audio_pcm::decode_stereo(&path)?.resampled(rate)?;
            add_aligned(&mut backing, &stem)?;
        }
        let cancelled = || quick.as_ref().is_some_and(|run| run.cancelled());
        let audio_cpp = || convert(&state.acpp, audio_build.context("audio.cpp runtime is unavailable")?,
            model, &vocal, &weights, index.as_deref(), request.semitone_shift, blend, &cancelled);
        let native_convert = || rvc_fast::convert(update_data_root(&state), &rvc_fast::Request {
            vocal: &vocal, voice: &weights, index: index.as_deref(),
            semitone_shift: request.semitone_shift, retrieval_blend: blend,
        });
        let (conversion, effective, fallback_reason) = match effective {
            Backend::VocalFastRs => match native_convert() {
                Ok(output) => (output, Backend::VocalFastRs, None),
                Err(error) if !cancelled() && request.backend == Backend::Auto && audio_build.is_some()
                    && error.is::<rvc_fast::BackendFailure>() => {
                    (audio_cpp()?, Backend::AudioCpp, Some(format!("{error:#}")))
                }
                Err(error) => return Err(error),
            },
            Backend::AudioCpp => (audio_cpp()?, Backend::AudioCpp,
                if request.backend == Backend::Auto { native.err() } else { None }),
            Backend::Auto => unreachable!("Auto resolves to a concrete RVC backend"),
        };
        add_aligned(&mut backing, &conversion.resampled(rate)?)?;
        let peak = backing.left.iter().chain(&backing.right).map(|sample| sample.abs()).fold(0.0f32, f32::max);
        let gain = if peak > 1.0 { 0.98 / peak } else { 1.0 };
        if gain < 1.0 {
            for sample in backing.left.iter_mut().chain(&mut backing.right) {
                *sample *= gain;
            }
        }
        let options = json!({
            "voice_id": voice.id, "voice_name": voice.title(), "voice_sha256": hash,
            "index_sha256": index.as_deref().map(install_record::sha256_of).transpose()?,
            "semitone_shift": request.semitone_shift, "retrieval_blend": blend, "mix_gain": gain,
            "requested_backend": request.backend, "effective_backend": effective, "fallback_reason": fallback_reason, "quick": quick.is_some()
        });
        let mut commit = quick.as_ref().map(|run| run.commit()).transpose()?;
        let song = state.library.import_audio_song(library::AudioImportInput {
            title: format!("{} · RVC · {}", original.title, voice.title()),
            caption: original.caption.clone(),
            lyrics: original.lyrics.clone(),
            metadata: json!({ "derived": derivation(&original, "rvc", options), "duration_seconds": backing.frames() as f64 / backing.rate as f64 }),
            generation_settings: Value::Null,
            engine_id: "rvc".into(),
            profile_id: None,
            source: "rvc".into(),
            audio_extension: "wav".into(),
            audio: audio_pcm::wav_f32_bytes(&backing)?,
        })?.song;
        if let Err(error) = cover_like(&state, &original, &song.id) {
            studio_log!("[RVC] Could not copy source cover: {error:#}");
        }
        let take = json!({ "song_id": song.id, "audio_url": format!("/v1/library/media/{}", song.id), "requested_backend": request.backend, "effective_backend": effective, "fallback_reason": fallback_reason });
        if let Some(progress) = commit.as_mut() {
            progress.complete(take.clone());
        }
        Ok::<_, anyhow::Error>(take)
    }).await.map_err(rvc_voices::internal)?.map_err(bad_gateway)?;
    Ok((StatusCode::CREATED, Json(result)))
}

#[allow(clippy::too_many_arguments)]
fn convert(
    engine: &acpp::Acpp,
    build: acpp_server::asset::Build,
    model: PathBuf,
    vocal: &FsPath,
    weights: &FsPath,
    index: Option<&FsPath>,
    shift: i32,
    blend: f64,
    cancelled: &impl Fn() -> bool,
) -> anyhow::Result<audio_post::Stereo> {
    let mut options =
        json!({ "voice_model_path": weights, "semitone_shift": shift, "retrieval_blend": blend });
    if let Some(index) = index {
        options["retrieval_index_file"] = json!(index);
    }
    let entry = acpp_server::ModelEntry {
        id: "rvc".into(),
        family: "rvc".into(),
        path: model,
        task: "vc".into(),
        mode: acpp_server::Mode::Offline,
        load_options: Default::default(),
        session_options: Default::default(),
        default_request_options: Default::default(),
        busy_timeout_ms: None,
        output_rate: None,
    };
    let reply = acpp::run(
        engine,
        build,
        entry,
        json!({ "audio": vocal, "options": options }),
        cancelled,
    )?;
    let audio = acpp_server::task_audio(&reply)?
        .into_iter()
        .next()
        .context("RVC returned no audio")?;
    anyhow::ensure!(
        matches!(audio.channels, 1 | 2)
            && audio.sample_rate > 0
            && !audio.samples.is_empty()
            && audio.samples.len() % audio.channels as usize == 0,
        "RVC returned an invalid audio format."
    );
    let left = audio
        .samples
        .iter()
        .step_by(audio.channels as usize)
        .copied()
        .collect::<Vec<_>>();
    let right = if audio.channels == 1 {
        left.clone()
    } else {
        audio.samples.iter().skip(1).step_by(2).copied().collect()
    };
    Ok(audio_post::Stereo::new(left, right, audio.sample_rate))
}

fn add_aligned(mix: &mut audio_post::Stereo, stem: &audio_post::Stereo) -> anyhow::Result<()> {
    // The native model rounds to its analysis hop. Preserve the original song's frame count.
    anyhow::ensure!(
        mix.rate == stem.rate && mix.frames().abs_diff(stem.frames()) <= mix.rate as usize / 4,
        "A stem does not align with the source song's duration."
    );
    for (output, input) in [(&mut mix.left, &stem.left), (&mut mix.right, &stem.right)] {
        for (destination, sample) in output.iter_mut().zip(input) {
            let mixed = *destination + sample;
            anyhow::ensure!(mixed.is_finite(), "A stem contains invalid audio samples.");
            *destination = mixed;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(
        any(target_os = "windows", target_os = "linux"),
        target_arch = "x86_64"
    ))]
    #[tokio::test]
    async fn catalog_setup_ignores_unverified_worker_without_starting_it() {
        let root = tempfile::tempdir().unwrap();
        let runtime = root.path().join("rvc/native/v1");
        fs::create_dir_all(&runtime).unwrap();
        let executable = format!("vocal-fast{}", std::env::consts::EXE_SUFFIX);
        for name in [&executable, "hubert.vfrm", "rmvpe.vfrm"] {
            fs::write(runtime.join(name), b"fixture, cannot launch").unwrap();
        }
        assert!(fast_status(root.path()).await.is_err());
        assert!(!root.path().join("rvc/native/cache-v1").exists());
    }

    #[test]
    fn auto_prefers_vocal_fast_without_overriding_an_explicit_audio_cpp_request() {
        assert!(effective_backend(Backend::Auto, true, true) == Some(Backend::VocalFastRs));
        assert!(effective_backend(Backend::Auto, false, true) == Some(Backend::AudioCpp));
        assert!(effective_backend(Backend::AudioCpp, true, true) == Some(Backend::AudioCpp));
        assert!(effective_backend(Backend::VocalFastRs, false, true).is_none());
    }

    #[test]
    fn remix_preserves_stereo_backing_and_pads_a_short_analysis_hop() {
        let mut backing = audio_post::Stereo::new(vec![0.1; 11], vec![0.25; 11], 100);
        let voice = audio_post::Stereo::new(vec![0.5; 10], vec![0.5; 10], 100);
        add_aligned(&mut backing, &voice).unwrap();
        assert_eq!(backing.left[..10], [0.6; 10]);
        assert_eq!(backing.right[..10], [0.75; 10]);
        assert_eq!(backing.left[10], 0.1);
        assert_eq!(backing.right[10], 0.25);
        let wrong = audio_post::Stereo::new(vec![0.0; 100], vec![0.0; 100], 100);
        assert!(add_aligned(&mut backing, &wrong).is_err());
    }
}
