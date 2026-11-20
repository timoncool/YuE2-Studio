//! MiniMax Music3's offline audio.cpp task through the shared server.
use super::*;
use acpp_server::asset::Build;
use acpp_server::{Mode, ModelEntry};
use base64::Engine as _;
use std::io::Read;
use std::path::{Path as FsPath, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) const ENGINE: &str = "audio.cpp";
const FAMILY: &str = "minimax_music3";
const PROFILE: &str = "quality-q8";
const QUANTIZATION: &str = "Q8_0";
const WEIGHTS: [(&str, &str); 3] = [
    (
        "minimax_music3.language_model_gguf",
        "language_model_q8_0.gguf",
    ),
    (
        "minimax_music3.rvq_depth_decoder_gguf",
        "rvq_depth_decoder_q8_0.gguf",
    ),
    (
        "minimax_music3.flow_transformer_gguf",
        "transformer_q8_0.gguf",
    ),
];
const SHARED_WEIGHTS: [&str; 2] = ["condition_encoder.gguf", "vocoder.gguf"];
const CONFIGS: [&str; 8] = [
    "config.json",
    "config/language_model.json",
    "config/rvq_depth_decoder.json",
    "config/condition_encoder.json",
    "config/transformer.json",
    "config/vocoder.json",
    "tokenizer/tokenizer.json",
    "tokenizer/tokenizer_config.json",
];

fn model_dir(root: &FsPath) -> PathBuf {
    root.join("audiocpp/models").join(FAMILY)
}

pub(super) fn missing_files(path: &FsPath) -> Vec<&'static str> {
    let configs = CONFIGS.into_iter().filter(|file| {
        !std::fs::read(path.join(file)).ok().is_some_and(|body| {
            serde_json::from_slice::<Value>(&body).is_ok_and(|value| value.is_object())
        })
    });
    let weights = WEIGHTS
        .into_iter()
        .map(|(_, file)| file)
        .chain(SHARED_WEIGHTS)
        .filter(|file| {
            let mut magic = [0; 4];
            !std::fs::File::open(path.join(file)).is_ok_and(|mut file| {
                file.metadata().is_ok_and(|info| info.len() > 24)
                    && file.read_exact(&mut magic).is_ok()
                    && &magic == b"GGUF"
            })
        });
    configs.chain(weights).collect()
}

fn weights() -> serde_json::Map<String, Value> {
    WEIGHTS
        .into_iter()
        .map(|(key, file)| (key.into(), Value::from(file)))
        .collect()
}

fn entry(path: PathBuf) -> ModelEntry {
    ModelEntry {
        id: FAMILY.into(),
        family: FAMILY.into(),
        task: "gen".into(),
        mode: Mode::Offline,
        path,
        load_options: Default::default(),
        session_options: weights(),
        default_request_options: Default::default(),
        busy_timeout_ms: None,
        output_rate: None,
    }
}

async fn build(state: &AppState) -> anyhow::Result<Build> {
    use music_engine::yue_server::ComputeBackend;
    match state.engine_options.read().await.backend {
        ComputeBackend::Auto => acpp::build_for("auto"),
        ComputeBackend::Cpu => Ok(Build::Cpu),
        ComputeBackend::Vulkan => Ok(Build::Vulkan),
        ComputeBackend::Cuda => match acpp::build_for("cuda")? {
            build @ (Build::Cuda12 | Build::Cuda13) => Ok(build),
            _ => anyhow::bail!("The CUDA audio.cpp runtime is unavailable."),
        },
    }
}

pub(super) async fn models(State(state): State<AppState>) -> Json<Value> {
    let mut missing = studio_data_root().map_or_else(
        || vec!["Studio data folder"],
        |root| missing_files(&model_dir(&root)),
    );
    let build = build(&state).await;
    let runtime_ready = build
        .as_ref()
        .is_ok_and(|build| state.acpp.exe(*build).is_file() && state.acpp.installed(*build));
    if !runtime_ready {
        missing.push("audio.cpp runtime");
    }
    Json(serde_json::json!({
        "engine": ENGINE,
        "models": [{"id": FAMILY, "name": "MiniMax Music3", "profile_id": PROFILE,
            "quantization": QUANTIZATION, "status": "experimental", "ready": missing.is_empty(), "missing": missing,
            "requires": ["style", "lyrics"], "output_formats": ["flac", "mp3", "wav32"]}]
    }))
}

fn task(request: &CreateMusicJobRequest) -> anyhow::Result<Value> {
    anyhow::ensure!(
        !request.style.trim().is_empty() && !request.lyrics.trim().is_empty(),
        "MiniMax Music3 needs a style and lyrics."
    );
    anyhow::ensure!(
        request.cover_of.is_none()
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
            && request.synth_batch_size.is_none_or(|n| n == 1)
            && request.lm_batch_size.is_none_or(|n| n == 1),
        "This audio.cpp model accepts one text-and-lyrics generation with a single seed."
    );
    let duration = request.duration_seconds.unwrap_or(20.0);
    let steps = request.steps.unwrap_or(30);
    let seed = request.seed.unwrap_or(0);
    anyhow::ensure!(
        duration.is_finite() && duration >= 0.1,
        "Duration must be at least 0.1 seconds."
    );
    anyhow::ensure!(steps > 0, "Inference needs at least one step.");
    anyhow::ensure!(seed >= 0, "The seed must be nonnegative.");
    let mut body = serde_json::json!({
        "text": request.style, "lyrics": request.lyrics, "seed": seed,
        "duration_seconds": duration, "num_inference_steps": steps, "options": {}
    });
    if let Some(cfg) = request.cfg_scale {
        anyhow::ensure!(
            cfg.is_finite() && cfg > 0.0,
            "Guidance scale must be positive."
        );
        body["guidance_scale"] = Value::from(cfg);
    }
    Ok(body)
}

pub(super) async fn submit(
    state: AppState,
    request: CreateMusicJobRequest,
    model: String,
    attempt: u32,
) -> (StatusCode, Json<MusicJob>) {
    let reject = |status, reason| {
        (
            status,
            Json(failed_request_job(request.clone(), ENGINE.into(), reason)),
        )
    };
    if model != FAMILY {
        return reject(
            StatusCode::BAD_REQUEST,
            format!("Unknown audio.cpp music model {model}."),
        );
    }
    let body = match task(&request) {
        Ok(body) => body,
        Err(error) => return reject(StatusCode::BAD_REQUEST, error.to_string()),
    };
    let format = request.output_format.as_deref().unwrap_or("flac");
    if let Err(error) = validate_output_format(format) {
        return reject(StatusCode::BAD_REQUEST, error);
    }
    if let Some(bitrate) = request.mp3_bitrate
        && let Err(error) = validate_mp3_bitrate(bitrate)
    {
        return reject(StatusCode::BAD_REQUEST, error);
    }
    let bitrate = (format == "mp3")
        .then(|| audio_post::encode::mp3_bitrate(request.mp3_bitrate.unwrap_or(DEFAULT_MP3_KBPS)));
    let Some(root) = studio_data_root() else {
        return reject(
            StatusCode::SERVICE_UNAVAILABLE,
            "Studio data folder is unavailable.".into(),
        );
    };
    let path = model_dir(&root);
    let missing = missing_files(&path);
    if !missing.is_empty() {
        return reject(
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "MiniMax Music3 weights in {} are incomplete: {}.",
                path.display(),
                missing.join(", ")
            ),
        );
    }
    let build = match build(&state).await {
        Ok(build) if state.acpp.exe(build).is_file() && state.acpp.installed(build) => build,
        Ok(_) => {
            return reject(
                StatusCode::SERVICE_UNAVAILABLE,
                "Install the selected audio.cpp runtime in Settings - Models.".into(),
            );
        }
        Err(error) => return reject(StatusCode::SERVICE_UNAVAILABLE, error.to_string()),
    };
    let Ok(lease) = state.engine_use.clone().try_write_owned() else {
        return reject(
            StatusCode::CONFLICT,
            "The local music engine is busy.".into(),
        );
    };
    if state.jobs.read().await.values().any(song_in_engine)
        || state.training.active_run().await.is_some()
    {
        return reject(
            StatusCode::CONFLICT,
            "Wait for the active generation or training run to finish.".into(),
        );
    }
    if let Err(error) = state.ace_runtime.stop().await {
        return reject(StatusCode::CONFLICT, format!("ACE-Step is busy: {error:#}"));
    }
    state.minimax.stop().await;
    evict_kept_models(&state, "before audio.cpp music generation").await;
    let job = MusicJob {
        derived: None,
        id: format!("audio-cpp-{}", uuid_suffix()),
        client_ref: request.client_ref.clone(),
        submitted_at: unix_millis(),
        engine_id: ENGINE.into(),
        cover_prompt: request.cover_prompt.clone(),
        title: Some(titled(&request, &state.adapters)),
        status: MusicJobStatus::Running,
        dispatch: MusicJobDispatch::Local,
        phase: MusicJobPhase::Running,
        style: request.style.clone(),
        lyrics: request.lyrics.clone(),
        duration_seconds: body["duration_seconds"].as_f64().unwrap_or_default(),
        generation_settings: serde_json::json!({
            "music_source": request.music_source, "family": FAMILY, "profile_id": PROFILE,
            "quantization": QUANTIZATION, "weight_files": weights(), "shared_weight_files": SHARED_WEIGHTS,
            "backend": build.backend(), "duration_seconds": body["duration_seconds"], "steps": body["num_inference_steps"],
            "seed": body["seed"], "guidance_scale": body.get("guidance_scale"), "output_format": format,
            "mp3_bitrate": bitrate, "fade_in": request.fade_in.unwrap_or_default().max(0.0),
            "fade_out": request.fade_out.unwrap_or_default().max(0.0)
        }),
        song: None,
        songs: vec![],
        message: "MiniMax Music3 is generating through audio.cpp.".into(),
        code: None,
        params: None,
        playlist_id: request.playlist_id.clone(),
        laid: None,
        card_memory_short: false,
        tracks_asked: Some(1),
    };
    if let Err(error) =
        state
            .library
            .save_music_job(&stored_job(&job, serde_json::json!(request), attempt))
    {
        return reject(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
    }
    let stop = Arc::new(AtomicBool::new(false));
    state
        .audio_cpp_music_stops
        .write()
        .await
        .insert(job.id.clone(), stop.clone());
    state.jobs.write().await.insert(job.id.clone(), job.clone());
    let accepted = job.clone();
    tokio::spawn(async move {
        let _lease = lease;
        let acpp = state.acpp.clone();
        let settings = job.generation_settings.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            anyhow::ensure!(!stop.load(Ordering::SeqCst), "stopped");
            acpp.with_server(
                build,
                entry(path),
                &|| stop.load(Ordering::SeqCst),
                |server| {
                    anyhow::ensure!(!stop.load(Ordering::SeqCst), "stopped");
                    // A disconnected JSON client still leaves the shared server running.
                    // Hold its lease until the answer so tools cannot replace it mid-task.
                    let reply = server.run_task(FAMILY, body, &AtomicBool::new(false))?;
                    anyhow::ensure!(!stop.load(Ordering::SeqCst), "stopped");
                    shape(&reply, &settings)
                },
            )
        })
        .await
        .context("the audio.cpp worker stopped")
        .and_then(|result| result);
        complete(&state, &job.id, outcome).await;
        state.audio_cpp_music_stops.write().await.remove(&job.id);
    });
    (StatusCode::ACCEPTED, Json(accepted))
}

fn shape(reply: &Value, settings: &Value) -> anyhow::Result<native_music_jobs::ShapedOutput> {
    let encoded = reply["audio"]
        .as_str()
        .context("audio.cpp returned no song audio")?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded)?;
    let format = output_format(settings);
    let bitrate = settings["mp3_bitrate"].as_u64().map(|value| value as u32);
    let fade = |key| settings[key].as_f64().unwrap_or_default() as f32;
    native_music_jobs::shape_output(bytes, format, bitrate, (fade("fade_in"), fade("fade_out")))
}

fn save(
    library: &library::Library,
    job: &MusicJob,
    output: native_music_jobs::ShapedOutput,
) -> anyhow::Result<library::ImportedSong> {
    let mut settings = job.generation_settings.clone();
    settings["fade_in"] = Value::from(output.fades.0);
    settings["fade_out"] = Value::from(output.fades.1);
    library.import_generated_song(library::GeneratedSongInput {
        title: job.title.clone(),
        caption: job.style.clone(),
        lyrics: job.lyrics.clone(),
        metadata: serde_json::json!({"job_id": job.id, "duration_seconds": output.duration,
            "output_format": settings["output_format"], "cover_prompt": job.cover_prompt}),
        generation_settings: settings,
        replay_request: None,
        audio_codes: None,
        engine_id: ENGINE.into(),
        profile_id: Some(PROFILE.into()),
        source: "local_generation".into(),
        audio_extension: output.extension,
        audio: output.audio,
    })
}

async fn complete(
    state: &AppState,
    id: &str,
    outcome: anyhow::Result<native_music_jobs::ShapedOutput>,
) {
    let mut jobs = state.jobs.write().await;
    let Some(job) = jobs.get_mut(id) else { return };
    if !matches!(job.status, MusicJobStatus::Running) {
        return;
    }
    let song_id = match outcome.and_then(|output| save(&state.library, job, output)) {
        Ok(imported) => {
            let id = imported.song.id.clone();
            if let Some(playlist) = &job.playlist_id
                && let Err(error) =
                    add_to_playlist(&state.library, playlist, std::iter::once(id.clone()))
            {
                studio_log!("[ERROR] adding audio.cpp song to playlist: {error:#}");
            }
            job.duration_seconds = imported.song.metadata["duration_seconds"]
                .as_f64()
                .unwrap_or(job.duration_seconds);
            job.generation_settings = imported.song.generation_settings.clone();
            let song = CompletedSong {
                audio_url: format!("/v1/library/media/{id}"),
                id: id.clone(),
                song: imported.song,
            };
            job.song = Some(song.clone());
            job.songs = vec![song];
            job.status = MusicJobStatus::Completed;
            job.phase = MusicJobPhase::Completed;
            job.message = "MiniMax Music3 song saved to the library.".into();
            Some(id)
        }
        Err(error) => {
            job.status = MusicJobStatus::Failed;
            job.phase = MusicJobPhase::Failed;
            job.message = format!("audio.cpp music generation failed: {error:#}");
            None
        }
    };
    persist(state, job);
    drop(jobs);
    if let Some(id) = song_id {
        tag_stored_song(state, &id).await;
        after_import(state, &id);
    }
}

fn persist(state: &AppState, job: &MusicJob) {
    if let Err(error) =
        state
            .library
            .set_music_job_status(&job.id, job_status_name(&job.status), &job.message)
    {
        studio_log!("[ERROR] saving audio.cpp music job status: {error:#}");
    }
}

pub(super) async fn cancel(
    state: &AppState,
    id: &str,
) -> Result<Json<MusicJob>, (StatusCode, Json<ApiError>)> {
    let mut jobs = state.jobs.write().await;
    let job = jobs
        .get_mut(id)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Music job was not found.".into()))?;
    if matches!(job.status, MusicJobStatus::Running) {
        if let Some(stop) = state.audio_cpp_music_stops.read().await.get(id) {
            stop.store(true, Ordering::SeqCst);
        }
        stop_on_request(job);
        persist(state, job);
    }
    Ok(Json(job.clone()))
}
