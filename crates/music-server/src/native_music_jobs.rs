use super::*;
use tokio::sync::{mpsc, oneshot};

type CancelReply = oneshot::Sender<Result<(), String>>;
pub(super) type CancelSender = mpsc::Sender<CancelReply>;

pub(super) async fn submit_minimax(
    state: AppState,
    request: CreateMusicJobRequest,
    profile: String,
    attempt: u32,
) -> (StatusCode, Json<MusicJob>) {
    let engine = native_minimax::ENGINE_ID;
    let reject = |status, reason: String| {
        (
            status,
            Json(failed_request_job(request.clone(), engine.into(), reason)),
        )
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
    let Some(root) = studio_data_root() else {
        return reject(
            StatusCode::SERVICE_UNAVAILABLE,
            "Studio data folder is unavailable.".into(),
        );
    };
    let client = match state.minimax.start(&profile, &root).await {
        Ok(client) => client,
        Err(error) => return reject(StatusCode::SERVICE_UNAVAILABLE, format!("{error:#}")),
    };
    let input = native_minimax::SongRequest {
        caption: &request.style,
        lyrics: &request.lyrics,
        duration_seconds: request.duration_seconds,
        steps: request.steps,
        seed: request.seed,
        lm_seed: request.lm_seed,
    };
    let remote = match client.submit(&profile, &input).await {
        Ok(id) => id,
        Err(error) => return reject(StatusCode::BAD_REQUEST, format!("{error:#}")),
    };
    let settings = serde_json::json!({"music_source": request.music_source, "steps": request.steps.unwrap_or(30),
        "duration_seconds":request.duration_seconds.unwrap_or(60.0), "seed":request.seed, "lm_seed":request.lm_seed});
    let job = MusicJob {
        derived: None,
        id: format!("minimax-{}", uuid_suffix()),
        client_ref: request.client_ref.clone(),
        submitted_at: unix_millis(),
        engine_id: engine.into(),
        cover_prompt: request.cover_prompt.clone(),
        title: Some(titled(&request, &state.adapters)),
        status: MusicJobStatus::Queued,
        dispatch: MusicJobDispatch::Local,
        phase: MusicJobPhase::Queued,
        style: request.style.clone(),
        lyrics: request.lyrics.clone(),
        duration_seconds: request.duration_seconds.unwrap_or_default(),
        generation_settings: settings,
        song: None,
        songs: vec![],
        message: "Submitted to MiniMax.".into(),
        code: None,
        params: None,
        playlist_id: request.playlist_id.clone(),
        laid: None,
        card_memory_short: false,
        tracks_asked: None,
    };
    if let Err(error) =
        state
            .library
            .save_music_job(&stored_job(&job, serde_json::json!(request), attempt))
    {
        let _ = client.cancel(&remote).await;
        return reject(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
    }
    let (tx, rx) = mpsc::channel(1);
    state
        .minimax_cancellations
        .write()
        .await
        .insert(job.id.clone(), tx);
    state.jobs.write().await.insert(job.id.clone(), job.clone());
    let accepted = job.clone();
    tokio::spawn(async move {
        let _lease = lease;
        let outcome = watch(&state, &job, &profile, &client, &remote, rx).await;
        if !matches!(outcome, Ok(Some(_))) {
            state.minimax.stop().await;
        }
        let mut jobs = state.jobs.write().await;
        let Some(active) = jobs.get_mut(&job.id) else {
            drop(jobs);
            state.minimax_cancellations.write().await.remove(&job.id);
            return;
        };
        match outcome {
            Ok(Some(song)) => {
                active.duration_seconds = song.song.metadata["duration_seconds"]
                    .as_f64()
                    .unwrap_or(active.duration_seconds);
                active.song = Some(song.clone());
                active.songs = vec![song];
                active.status = MusicJobStatus::Completed;
                active.phase = MusicJobPhase::Completed;
                active.message = "MiniMax music was saved to the library.".into();
            }
            Ok(None) => stop_on_request(active),
            Err(error) => {
                active.status = MusicJobStatus::Failed;
                active.phase = MusicJobPhase::Failed;
                active.message = format!("MiniMax generation failed: {error:#}");
            }
        }
        if let Err(error) = state.library.set_music_job_status(
            &job.id,
            job_status_name(&active.status),
            &active.message,
        ) {
            studio_log!("[ERROR] saving MiniMax job state: {error:#}");
        }
        drop(jobs);
        state.minimax_cancellations.write().await.remove(&job.id);
    });
    (StatusCode::ACCEPTED, Json(accepted))
}

async fn watch(
    state: &AppState,
    job: &MusicJob,
    profile: &str,
    client: &native_minimax::Client,
    remote: &str,
    mut rx: mpsc::Receiver<CancelReply>,
) -> anyhow::Result<Option<CompletedSong>> {
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(400));
    loop {
        tokio::select! {
            biased;
            Some(reply) = rx.recv() => {
                match client.cancel(remote).await {
                    Ok(status) if status == "done" => { let _ = reply.send(Err("MiniMax already finished rendering.".into())); }
                    Ok(_) => {
                        if let Some(active) = state.jobs.write().await.get_mut(&job.id) { stop_on_request(active); }
                        let _ = reply.send(Ok(()));
                        return Ok(None);
                    }
                    Err(error) => { let _ = reply.send(Err(error.to_string())); }
                }
            }
            _ = interval.tick() => {
                match client.status(remote).await?.as_str() {
                    "done" => break,
                    "cancelled" => return Ok(None),
                    "queued" => {},
                    "running" => {
                        if let Some(active) = state.jobs.write().await.get_mut(&job.id) {
                            active.status = MusicJobStatus::Running;
                            active.phase = MusicJobPhase::Running;
                            active.message = "MiniMax is generating music.".into();
                        }
                    }
                    other => anyhow::bail!("MiniMax job {remote} reported {other}"),
                }
            }
        }
    }
    let mut tracks = client.result(remote).await?;
    anyhow::ensure!(
        tracks.len() == 1,
        "MiniMax returned {} takes for a single take request",
        tracks.len()
    );
    let track = tracks.pop().unwrap();
    anyhow::ensure!(
        engine_result::audio_extension(&track.audio_content_type)? == "wav",
        "MiniMax returned an unexpected audio format"
    );
    let duration =
        openrouter_stream::wav_duration(&track.audio).context("MiniMax WAV is incomplete")?;
    let replay = track.replay_request;
    let codes = replay
        .get("audio_codes")
        .filter(|v| v.as_str().is_some_and(|s| !s.is_empty()))
        .context("MiniMax result has no audio codes")?
        .clone();
    let mut settings = job.generation_settings.clone();
    for (key, value) in replay
        .as_object()
        .context("MiniMax replay settings are missing")?
    {
        if key != "audio_codes" && key != "music_source" {
            settings[key] = value.clone();
        }
    }
    let imported = state.library.import_generated_song(library::GeneratedSongInput {
        title: job.title.clone(), metadata: serde_json::json!({"duration_seconds":duration,"job_id":job.id,"cover_prompt":job.cover_prompt}),
        caption: replay["caption"].as_str().unwrap_or(&job.style).to_owned(),
        lyrics: replay["lyrics"].as_str().unwrap_or(&job.lyrics).to_owned(),
        generation_settings: settings, replay_request: Some(replay), audio_codes: Some(codes),
        engine_id: job.engine_id.clone(), profile_id: Some(profile.into()), source: "local_generation".into(),
        audio_extension: "wav", audio: track.audio,
    })?;
    let id = imported.song.id.clone();
    if let Some(playlist) = &job.playlist_id {
        if let Err(error) = add_to_playlist(&state.library, playlist, std::iter::once(id.clone())) {
            studio_log!("[ERROR] adding MiniMax song to playlist: {error:#}");
        }
    }
    tag_stored_song(state, &id).await;
    after_import(state, &id);
    Ok(Some(CompletedSong {
        audio_url: format!("/v1/library/media/{id}"),
        id,
        song: imported.song,
    }))
}

pub(super) async fn cancel_minimax(
    state: &AppState,
    job: MusicJob,
) -> Result<Json<MusicJob>, (StatusCode, Json<ApiError>)> {
    let sender = state
        .minimax_cancellations
        .read()
        .await
        .get(&job.id)
        .cloned();
    if let Some(sender) = sender {
        let (reply, result) = oneshot::channel();
        if sender.send(reply).await.is_ok() {
            if let Ok(Err(reason)) = result.await {
                return Err(api_error(StatusCode::CONFLICT, reason));
            }
        }
    }
    let current = state.jobs.read().await.get(&job.id).cloned().unwrap_or(job);
    if matches!(
        current.status,
        MusicJobStatus::Queued | MusicJobStatus::Running
    ) {
        return Err(api_error(
            StatusCode::CONFLICT,
            "MiniMax is finishing the take.".into(),
        ));
    }
    Ok(Json(current))
}
