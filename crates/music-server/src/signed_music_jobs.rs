//! Dispatches a single verified music-http-v1 generation while holding the
//! same device lease as the bundled engines.
use super::*;
use crate::signed_music_protocol as protocol;
use base64::Engine as _;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, Instant};

type CancelReply = oneshot::Sender<Result<(), String>>;
pub(super) type CancelSender = mpsc::Sender<CancelReply>;
type Failure = (StatusCode, Json<ApiError>);
type JobResponse = (StatusCode, Json<MusicJob>);

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Identity {
    engine: String,
    model: String,
    generation: String,
    version: String,
    model_generation: String,
    model_version: String,
    protocol: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct SavedReplay {
    signed_engine: Identity,
    payload: Value,
}

struct Replay {
    saved: SavedReplay,
    overrides: ReplayMusicJobRequest,
    derived: Value,
}

pub(super) fn is_job(job: &MusicJob) -> bool {
    job.id.starts_with("signed-") && job.generation_settings["music_source"]["kind"] == "signed"
}

pub(super) async fn submit(
    state: AppState,
    request: CreateMusicJobRequest,
    engine: String,
    model: String,
    generation: Option<String>,
    model_generation: Option<String>,
    attempt: u32,
) -> JobResponse {
    run(
        state,
        request,
        engine,
        model,
        generation,
        model_generation,
        attempt,
        None,
    )
    .await
}

pub(super) async fn replay(
    state: AppState,
    song: library::Song,
    overrides: ReplayMusicJobRequest,
    attempt: u32,
) -> Result<JobResponse, Failure> {
    let saved: SavedReplay =
        serde_json::from_value(song.replay_request.clone().unwrap_or_default()).map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "The signed-engine replay information is incomplete.".into(),
            )
        })?;
    let identity = &saved.signed_engine;
    if identity.engine != song.engine_id
        || Some(identity.model.as_str()) != song.profile_id.as_deref()
        || identity.protocol != protocol::NAME
        || !saved.payload.is_object()
    {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "The signed-engine replay does not match the stored track.".into(),
        ));
    }
    if overrides.extend_seconds.is_some()
        || overrides.synth_batch_size.is_some_and(|size| size != 1)
    {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "This adapter protocol re-renders one stored take at a time.".into(),
        ));
    }
    let request = CreateMusicJobRequest {
        title: overrides
            .title
            .clone()
            .filter(|title| !title.trim().is_empty())
            .or(Some(song.title.clone())),
        style: song.caption.clone(),
        lyrics: song.lyrics.clone(),
        client_ref: overrides.client_ref.clone(),
        duration_seconds: song.generation_settings["duration_seconds"].as_f64(),
        output_format: overrides.output_format.clone().or_else(|| {
            song.generation_settings["output_format"]
                .as_str()
                .map(str::to_owned)
        }),
        mp3_bitrate: overrides.mp3_bitrate.or_else(|| {
            song.generation_settings["mp3_bitrate"]
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
        }),
        fade_in: song.generation_settings["fade_in"]
            .as_f64()
            .map(|value| value as f32),
        fade_out: song.generation_settings["fade_out"]
            .as_f64()
            .map(|value| value as f32),
        ..Default::default()
    };
    let engine = identity.engine.clone();
    let model = identity.model.clone();
    let generation = Some(identity.generation.clone());
    let model_generation = Some(identity.model_generation.clone());
    let derived = derivation(
        &song,
        "replay",
        serde_json::json!({
            "seed": overrides.seed, "steps": overrides.steps, "solver": overrides.solver,
        }),
    );
    let (status, Json(job)) = run(
        state,
        request,
        engine,
        model,
        generation,
        model_generation,
        attempt,
        Some(Replay {
            saved,
            overrides,
            derived,
        }),
    )
    .await;
    if status == StatusCode::ACCEPTED {
        Ok((status, Json(job)))
    } else {
        Err(api_error(status, job.message))
    }
}

async fn run(
    state: AppState,
    mut request: CreateMusicJobRequest,
    engine: String,
    model: String,
    generation: Option<String>,
    model_generation: Option<String>,
    attempt: u32,
    replay: Option<Replay>,
) -> JobResponse {
    let reject = |request: CreateMusicJobRequest, status, reason: String| {
        (
            status,
            Json(failed_request_job(request, engine.clone(), reason)),
        )
    };
    if matches!(
        engine.as_str(),
        PRIMARY_MUSIC_ENGINE_ID
            | ace::ENGINE_ID
            | native_minimax::ENGINE_ID
            | audio_cpp_music::ENGINE
            | hotstep_cover::ENGINE
            | "openrouter"
    ) {
        return reject(
            request,
            StatusCode::BAD_REQUEST,
            "The adapter ID is reserved for a bundled engine.".into(),
        );
    }
    let format = request
        .output_format
        .clone()
        .unwrap_or_else(|| "flac".into());
    if let Err(reason) = validate_output_format(&format) {
        return reject(request, StatusCode::BAD_REQUEST, reason);
    }
    if let Some(rate) = request.mp3_bitrate
        && let Err(reason) = validate_mp3_bitrate(rate)
    {
        return reject(request, StatusCode::BAD_REQUEST, reason);
    }
    if model.trim().is_empty()
        || model.len() > 160
        || model.chars().any(char::is_control)
        || request.vocals_only
        || request.cover_of.is_some()
        || request
            .duration_seconds
            .is_some_and(|seconds| !seconds.is_finite() || seconds <= 0.0)
        || request.synth_batch_size.is_some_and(|size| size != 1)
        || request.lm_batch_size.is_some_and(|size| size != 1)
    {
        return reject(request, StatusCode::BAD_REQUEST, "Choose one signed-engine model and one take. Covers and vocal extraction are unavailable in this protocol.".into());
    }
    let Ok(lease) = state.engine_use.clone().try_write_owned() else {
        return reject(
            request,
            StatusCode::CONFLICT,
            "The local music engine is busy.".into(),
        );
    };
    if engine_is_paused(&state)
        || state.jobs.read().await.values().any(song_in_engine)
        || state.training.active_run().await.is_some()
    {
        return reject(
            request,
            StatusCode::CONFLICT,
            "Resume the engine or wait for generation or training to finish.".into(),
        );
    }
    let root = update_data_root(&state);
    let package =
        match engine_catalogue::install::resolve_generation(root, &engine, generation.as_deref())
            .await
        {
            Ok(package) => package,
            Err(error) => {
                return reject(
                    request,
                    StatusCode::SERVICE_UNAVAILABLE,
                    format!("Could not verify the music engine: {error:#}"),
                );
            }
        };
    let model_package = match engine_catalogue::install::resolve_model_generation(
        root,
        &engine,
        &model,
        model_generation.as_deref(),
    )
    .await
    {
        Ok(package) => package,
        Err(error) => {
            return reject(
                request,
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Could not verify the model for {engine}: {error:#}"),
            );
        }
    };
    if package.protocol != protocol::NAME {
        return reject(
            request,
            StatusCode::BAD_REQUEST,
            "The installed music engine uses an unsupported protocol.".into(),
        );
    }
    if replay.as_ref().is_some_and(|replay| {
        replay.saved.signed_engine.version != package.version
            || replay.saved.signed_engine.model_version != model_package.version
    }) {
        return reject(
            request,
            StatusCode::CONFLICT,
            "The verified engine or model version differs from the stored replay.".into(),
        );
    }
    if let Err(error) = state.ace_runtime.stop().await {
        return reject(
            request,
            StatusCode::CONFLICT,
            format!("ACE-Step is busy: {error:#}"),
        );
    }
    state.minimax.stop().await;
    evict_kept_models(&state, "before signed music generation").await;
    let identity = Identity {
        engine: engine.clone(),
        model,
        generation: package.generation,
        version: package.version,
        model_generation: model_package.generation,
        model_version: model_package.version,
        protocol: package.protocol,
    };
    let source = MusicSource::Signed {
        engine: identity.engine.clone(),
        model: identity.model.clone(),
        generation: Some(identity.generation.clone()),
        model_generation: Some(identity.model_generation.clone()),
    };
    request.music_source = Some(source.clone());
    let submission = submission(&request, &identity, replay.as_ref());
    let process = match protocol::Process::launch(
        &package.executable,
        &model_package.files,
        &engine,
        &identity.version,
    )
    .await
    {
        Ok(process) => process,
        Err(error) => {
            return reject(request, StatusCode::SERVICE_UNAVAILABLE, error.to_string());
        }
    };
    let started = Instant::now();
    let remote = match process.submit(&submission).await {
        Ok(id) => id,
        Err(error) => {
            process.stop().await;
            return reject(request, StatusCode::BAD_REQUEST, error.to_string());
        }
    };
    let bitrate = (format == "mp3")
        .then(|| audio_post::encode::mp3_bitrate(request.mp3_bitrate.unwrap_or(DEFAULT_MP3_KBPS)));
    let settings = serde_json::json!({
        "music_source": source, "signed_engine": identity, "output_format": format, "mp3_bitrate": bitrate,
        "fade_in": request.fade_in.unwrap_or_default().max(0.0), "fade_out": request.fade_out.unwrap_or_default().max(0.0),
    });
    let job = MusicJob {
        id: format!("signed-{}", uuid_suffix()),
        engine_id: engine.clone(),
        submitted_at: unix_millis(),
        style: request.style.clone(),
        lyrics: request.lyrics.clone(),
        duration_seconds: request.duration_seconds.unwrap_or_default(),
        title: Some(titled(&request, &state.adapters)),
        status: MusicJobStatus::Queued,
        phase: MusicJobPhase::Queued,
        dispatch: MusicJobDispatch::Local,
        client_ref: request.client_ref.clone(),
        message: "Submitted to the signed music engine.".into(),
        generation_settings: settings,
        playlist_id: request.playlist_id.clone(),
        cover_prompt: request.cover_prompt.clone(),
        derived: replay.as_ref().map(|replay| replay.derived.clone()),
        song: None,
        songs: vec![],
        params: None,
        code: None,
        laid: None,
        card_memory_short: false,
        tracks_asked: None,
    };
    let stored_request = replay.as_ref().map_or_else(
        || serde_json::json!(request),
        |replay| serde_json::json!({"replay": replay.overrides}),
    );
    if let Err(error) = state
        .library
        .save_music_job(&stored_job(&job, stored_request, attempt))
    {
        let _ = process.cancel(&remote).await;
        process.stop().await;
        return reject(
            request,
            StatusCode::INTERNAL_SERVER_ERROR,
            error.to_string(),
        );
    }
    let (sender, receiver) = mpsc::channel(1);
    state
        .signed_music_cancellations
        .write()
        .await
        .insert(job.id.clone(), sender);
    state.jobs.write().await.insert(job.id.clone(), job.clone());
    let accepted = job.clone();
    tokio::spawn(async move {
        let mut cancel_ack = None;
        let outcome = watch(&state, &job, &process, &remote, receiver, &mut cancel_ack).await;
        let elapsed = started.elapsed();
        let backend = process.compute_backend.report_name();
        process.stop().await;
        let outcome = match outcome {
            Ok(Some(track)) => import(&state, &job, identity, track).await.map(Some),
            Ok(None) => Ok(None),
            Err(error) => Err(error),
        };
        let report = finish(&state, &job.id, outcome).await;
        state
            .signed_music_cancellations
            .write()
            .await
            .remove(&job.id);
        drop(lease);
        if let Some(reply) = cancel_ack {
            let _ = reply.send(Ok(()));
        }
        if let Some(report) = report {
            report
                .record_on(
                    &state,
                    "signed_music",
                    "native.signed_music",
                    elapsed,
                    Some(backend),
                )
                .await;
        }
    });
    (StatusCode::ACCEPTED, Json(accepted))
}

fn submission(
    request: &CreateMusicJobRequest,
    identity: &Identity,
    replay: Option<&Replay>,
) -> protocol::Submission {
    let model = identity.model.clone();
    if let Some(replay) = replay {
        let overrides = &replay.overrides;
        return protocol::Submission {
            model,
            mode: protocol::Mode::Replay,
            payload: serde_json::json!({
                "saved": replay.saved.payload,
                "overrides": { "seed": overrides.seed, "steps": overrides.steps, "solver": overrides.solver },
            }),
        };
    }
    let mut payload = serde_json::json!({
        "style": request.style, "lyrics": request.lyrics, "duration_seconds": request.duration_seconds,
        "seed": request.seed, "lm_seed": request.lm_seed, "steps": request.steps, "cfg_scale": request.cfg_scale,
        "abc": request.abc, "solver": request.solver, "cot": request.cot,
    });
    payload
        .as_object_mut()
        .expect("object")
        .retain(|_, value| !value.is_null());
    protocol::Submission {
        model,
        mode: protocol::Mode::Create,
        payload,
    }
}

async fn watch(
    state: &AppState,
    job: &MusicJob,
    process: &protocol::Process,
    remote: &str,
    mut receiver: mpsc::Receiver<CancelReply>,
    cancel_ack: &mut Option<CancelReply>,
) -> anyhow::Result<Option<protocol::Track>> {
    use protocol::Status;
    let deadline = Instant::now() + Duration::from_secs(60 * 60);
    let mut poll = tokio::time::interval(Duration::from_millis(350));
    loop {
        tokio::select! {
            biased;
            Some(reply) = receiver.recv() => {
                match process.cancel(remote).await {
                    Ok(progress) if progress.status == Status::Cancelled => {
                        *cancel_ack = Some(reply);
                        return Ok(None);
                    }
                    Ok(_) => { let _ = reply.send(Err("The music adapter is already finishing this take.".into())); }
                    Err(error) => { let _ = reply.send(Err(error.to_string())); }
                }
            }
            _ = tokio::time::sleep_until(deadline) => anyhow::bail!("Music adapter did not finish within one hour"),
            _ = poll.tick() => {
                let progress = process.status(remote).await?;
                match progress.status {
                    Status::Completed => return process.result(remote).await.map(Some),
                    Status::Cancelled => return Ok(None),
                    Status::Failed => anyhow::bail!("{}", progress.message.unwrap_or_else(|| "The music adapter failed".into())),
                    Status::Running => {
                        if let Some(job) = state.jobs.write().await.get_mut(&job.id) {
                            job.status = MusicJobStatus::Running;
                            job.phase = MusicJobPhase::Running;
                            job.message = progress.message.unwrap_or_else(|| "Generating music.".into());
                        }
                    }
                    Status::Queued => {}
                }
            }
        }
    }
}

async fn import(
    state: &AppState,
    job: &MusicJob,
    identity: Identity,
    track: protocol::Track,
) -> anyhow::Result<CompletedSong> {
    anyhow::ensure!(
        track.replay.is_object(),
        "The music adapter returned no replay information"
    );
    let mut settings = job.generation_settings.clone();
    let format = settings["output_format"]
        .as_str()
        .unwrap_or("flac")
        .to_owned();
    let bitrate = settings["mp3_bitrate"].as_u64().map(|value| value as u32);
    let fade = |key| settings[key].as_f64().unwrap_or_default() as f32;
    let fades = (fade("fade_in"), fade("fade_out"));
    let output = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let audio = base64::engine::general_purpose::STANDARD
            .decode(track.audio_base64)
            .context("invalid music adapter WAV encoding")?;
        native_music_jobs::shape_output(audio, &format, bitrate, fades)
    })
    .await??;
    settings["duration_seconds"] = serde_json::json!(output.duration);
    settings["fade_in"] = serde_json::json!(output.fades.0);
    settings["fade_out"] = serde_json::json!(output.fades.1);
    settings["adapter_settings"] = track.settings;
    let model = identity.model.clone();
    let metadata = serde_json::json!({
        "duration_seconds": output.duration, "job_id": job.id, "cover_prompt": job.cover_prompt,
        "output_format": settings["output_format"], "derived": job.derived, "signed_engine": identity,
    });
    let saved = serde_json::to_value(SavedReplay {
        signed_engine: identity,
        payload: track.replay,
    })?;
    let imported = state
        .library
        .import_generated_song(library::GeneratedSongInput {
            title: job.title.clone(),
            metadata,
            caption: job.style.clone(),
            lyrics: job.lyrics.clone(),
            generation_settings: settings,
            replay_request: Some(saved),
            audio_codes: None,
            engine_id: job.engine_id.clone(),
            profile_id: Some(model),
            source: "local_generation".into(),
            audio_extension: output.extension,
            audio: output.audio,
        })?;
    let id = imported.song.id.clone();
    if let Some(playlist) = &job.playlist_id
        && let Err(error) = add_to_playlist(&state.library, playlist, std::iter::once(id.clone()))
    {
        studio_log!("[ERROR] adding the signed engine's song to the playlist: {error:#}");
    }
    tag_stored_song(state, &id).await;
    after_import(state, &id);
    Ok(CompletedSong {
        audio_url: format!("/v1/library/media/{id}"),
        id,
        song: imported.song,
    })
}

async fn finish(
    state: &AppState,
    id: &str,
    result: anyhow::Result<Option<CompletedSong>>,
) -> Option<native_music_jobs::SongReport> {
    let mut jobs = state.jobs.write().await;
    let job = jobs.get_mut(id)?;
    if matches!(
        job.status,
        MusicJobStatus::Completed | MusicJobStatus::Failed | MusicJobStatus::Cancelled
    ) {
        return None;
    }
    match result {
        Ok(Some(song)) => {
            job.duration_seconds = song.song.metadata["duration_seconds"]
                .as_f64()
                .unwrap_or(job.duration_seconds);
            job.generation_settings = song.song.generation_settings.clone();
            job.song = Some(song.clone());
            job.songs = vec![song];
            job.status = MusicJobStatus::Completed;
            job.phase = MusicJobPhase::Completed;
            job.message = "Music was saved to the library.".into();
        }
        Ok(None) => {
            job.status = MusicJobStatus::Cancelled;
            job.phase = MusicJobPhase::Cancelled;
            job.dispatch = MusicJobDispatch::Cancelled;
            job.message = "Music generation was cancelled.".into();
        }
        Err(error) => {
            job.status = MusicJobStatus::Failed;
            job.phase = MusicJobPhase::Failed;
            job.message = format!("Music adapter failed: {error:#}");
        }
    }
    if let Err(error) =
        state
            .library
            .set_music_job_status(id, job_status_name(&job.status), &job.message)
    {
        studio_log!("[ERROR] saving signed music job state: {error:#}");
    }
    Some(native_music_jobs::SongReport::from_job(job))
}

pub(super) async fn cancel(
    state: &AppState,
    existing: MusicJob,
) -> Result<Json<MusicJob>, Failure> {
    let sender = state
        .signed_music_cancellations
        .read()
        .await
        .get(&existing.id)
        .cloned();
    if let Some(sender) = sender {
        let (reply, result) = oneshot::channel();
        if sender.send(reply).await.is_ok()
            && let Ok(Err(reason)) = result.await
        {
            return Err(api_error(StatusCode::CONFLICT, reason));
        }
    }
    let live = state
        .jobs
        .read()
        .await
        .get(&existing.id)
        .cloned()
        .unwrap_or(existing);
    if matches!(
        live.status,
        MusicJobStatus::Running | MusicJobStatus::Queued
    ) {
        return Err(api_error(
            StatusCode::CONFLICT,
            "The music adapter is finishing this take.".into(),
        ));
    }
    Ok(Json(live))
}
