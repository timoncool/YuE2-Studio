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
    submit(state, request, profile, attempt, None).await
}

struct Replay {
    body: Value,
    derived: Value,
    request: ReplayMusicJobRequest,
}

pub(super) async fn replay(
    state: AppState,
    song: library::Song,
    mut request: ReplayMusicJobRequest,
    attempt: u32,
) -> Result<(StatusCode, Json<MusicJob>), (StatusCode, Json<ApiError>)> {
    if attempt == 0 {
        refuse_trashed(&song)?;
    }
    request.song_id = Some(song.id.clone());
    let (profile, render, replay) = replay_inputs(&song, request)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(submit(state, render, profile, attempt, Some(replay)).await)
}

fn replay_inputs(
    song: &library::Song,
    request: ReplayMusicJobRequest,
) -> anyhow::Result<(String, CreateMusicJobRequest, Replay)> {
    anyhow::ensure!(
        song.engine_id == native_minimax::ENGINE_ID,
        "This song is not a native MiniMax take"
    );
    anyhow::ensure!(
        request.solver.is_none() && request.extend_seconds.is_none(),
        "MiniMax replay does not support a custom solver or extending the saved audio codes"
    );
    anyhow::ensure!(
        request.synth_batch_size.is_none_or(|size| size == 1),
        "MiniMax replay currently renders one take"
    );
    let saved = song
        .replay_request
        .as_ref()
        .filter(|value| value.is_object())
        .context("The MiniMax song has no stored replay request")?;
    let profile = native_minimax::saved_profile(song.profile_id.as_deref(), saved)?.to_owned();
    let codes = saved["audio_codes"]
        .as_str()
        .or_else(|| song.audio_codes.as_ref().and_then(Value::as_str))
        .filter(|codes| !codes.trim().is_empty())
        .context("The MiniMax song has no stored audio codes")?;
    let unsigned = |key| {
        saved[key]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
    };
    let steps = request.steps.or_else(|| unsigned("steps")).unwrap_or(30);
    anyhow::ensure!(steps >= 2, "MiniMax needs at least two inference steps");
    // Never draw a fresh seed implicitly: repeat the original acoustic take.
    let seed = request
        .seed
        .or_else(|| saved["seed"].as_i64().filter(|seed| *seed != -1))
        .or_else(|| {
            song.generation_settings["seed"]
                .as_i64()
                .filter(|seed| *seed != -1)
        })
        .context("The MiniMax song has no stored acoustic seed")?;
    let text = |key, fallback: &str| {
        saved[key]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(fallback)
            .to_owned()
    };
    let caption = text("caption", &song.caption);
    let lyrics = text("lyrics", &song.lyrics);
    anyhow::ensure!(
        !caption.trim().is_empty() && !lyrics.trim().is_empty(),
        "The MiniMax replay needs a style and lyrics"
    );
    let duration = saved["duration"]
        .as_f64()
        .or_else(|| song.generation_settings["duration_seconds"].as_f64());
    let mut body = serde_json::json!({"caption":caption,"lyrics":lyrics,"steps":steps,"seed":seed,"audio_codes":codes});
    if let Some(duration) = duration.filter(|value| *value > 0.0) {
        body["duration"] = Value::from(duration);
    }
    for field in ["lm_seed", "dit_cfg", "flow_shift", "peak_clip", "adapters"] {
        if let Some(value) = saved.get(field) {
            body[field] = value.clone();
        }
    }
    let render = CreateMusicJobRequest {
        music_source: Some(MusicSource::Native {
            engine: native_minimax::ENGINE_ID.into(),
            model: profile.clone(),
        }),
        client_ref: request.client_ref.clone(),
        title: request
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(str::to_owned)
            .or_else(|| Some(song.title.clone())),
        cover_prompt: song.metadata["cover_prompt"].as_str().map(str::to_owned),
        style: caption,
        lyrics,
        steps: Some(steps),
        seed: Some(seed),
        lm_seed: saved["lm_seed"].as_i64(),
        duration_seconds: duration,
        output_format: request
            .output_format
            .clone()
            .or_else(|| saved["output_format"].as_str().map(str::to_owned)),
        mp3_bitrate: request.mp3_bitrate.or_else(|| unsigned("mp3_bitrate")),
        fade_in: saved["fade_in"].as_f64().map(|value| value as f32),
        fade_out: saved["fade_out"].as_f64().map(|value| value as f32),
        ..Default::default()
    };
    let derived = derivation(
        song,
        "replay",
        serde_json::json!({
            "steps":request.steps, "seed":request.seed,"output_format":request.output_format,"mp3_bitrate":request.mp3_bitrate,
        }),
    );
    Ok((
        profile,
        render,
        Replay {
            body,
            derived,
            request,
        },
    ))
}

async fn submit(
    state: AppState,
    request: CreateMusicJobRequest,
    profile: String,
    attempt: u32,
    replay: Option<Replay>,
) -> (StatusCode, Json<MusicJob>) {
    let engine = native_minimax::ENGINE_ID;
    let reject = |status, reason: String| {
        (
            status,
            Json(failed_request_job(request.clone(), engine.into(), reason)),
        )
    };
    let format = request.output_format.as_deref().unwrap_or("flac");
    if let Err(error) = validate_output_format(format) {
        return reject(StatusCode::BAD_REQUEST, error);
    }
    if let Some(bitrate) = request.mp3_bitrate {
        if let Err(error) = validate_mp3_bitrate(bitrate) {
            return reject(StatusCode::BAD_REQUEST, error);
        }
    }
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
    if let Err(error) = state.ace_runtime.stop().await {
        return reject(StatusCode::CONFLICT, format!("ACE-Step is busy: {error:#}"));
    }
    evict_kept_models(&state, "before MiniMax generation").await;
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
    let submitted = match &replay {
        Some(replay) => client.submit_replay(&profile, replay.body.clone()).await,
        None => client.submit(&profile, &input).await,
    };
    let remote = match submitted {
        Ok(id) => id,
        Err(error) => return reject(StatusCode::BAD_REQUEST, format!("{error:#}")),
    };
    let mut settings = serde_json::json!({"music_source": request.music_source, "steps": request.steps.unwrap_or(30),
        "duration_seconds":request.duration_seconds.unwrap_or(60.0), "seed":request.seed, "lm_seed":request.lm_seed,
        "output_format":format, "fade_in":request.fade_in.unwrap_or(0.0).max(0.0),
        "fade_out":request.fade_out.unwrap_or(0.0).max(0.0)});
    if format == "mp3" {
        settings["mp3_bitrate"] = Value::from(audio_post::encode::mp3_bitrate(
            request.mp3_bitrate.unwrap_or(DEFAULT_MP3_KBPS),
        ));
    }
    let job = MusicJob {
        derived: replay.as_ref().map(|replay| replay.derived.clone()),
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
        message: if replay.is_some() {
            "Submitted MiniMax replay with saved audio codes."
        } else {
            "Submitted to MiniMax."
        }
        .into(),
        code: None,
        params: None,
        playlist_id: request.playlist_id.clone(),
        laid: None,
        card_memory_short: false,
        tracks_asked: None,
    };
    let stored_request = match replay {
        Some(replay) => serde_json::json!({"replay":replay.request}),
        None => serde_json::json!(request),
    };
    if let Err(error) = state
        .library
        .save_music_job(&stored_job(&job, stored_request, attempt))
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
        let completed = matches!(outcome, Ok(Some(_)));
        if !completed {
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
                active.generation_settings = song.song.generation_settings.clone();
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
        drop(_lease);
        drop(jobs);
        state.minimax_cancellations.write().await.remove(&job.id);
        // Only this watcher completes an import. The Hub writes its local
        // state synchronously, so do it after releasing the jobs and engine locks.
        if completed && let Some(hub) = state.hub.clone() {
            if let Err(error) = tokio::task::spawn_blocking(move || {
                hub.count("songs", 1);
                hub.count("song_done", 1);
            })
            .await
            {
                studio_log!("[ERROR] recording MiniMax completion: {error}");
            }
        }
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
    let format = output_format(&job.generation_settings).to_owned();
    let bitrate = job.generation_settings["mp3_bitrate"]
        .as_u64()
        .map(|rate| rate as u32);
    let fade = |key| job.generation_settings[key].as_f64().unwrap_or(0.0) as f32;
    let fades = (fade("fade_in"), fade("fade_out"));
    let target = format.clone();
    let output =
        tokio::task::spawn_blocking(move || shape_output(track.audio, &target, bitrate, fades))
            .await
            .context("the MiniMax audio encoder stopped")??;
    let mut replay = track.replay_request;
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
        if !matches!(
            key.as_str(),
            "audio_codes"
                | "music_source"
                | "output_format"
                | "mp3_bitrate"
                | "fade_in"
                | "fade_out"
        ) {
            settings[key] = value.clone();
        }
    }
    for record in [&mut settings, &mut replay] {
        record["output_format"] = Value::from(format.as_str());
        record["fade_in"] = Value::from(output.fades.0);
        record["fade_out"] = Value::from(output.fades.1);
        if let Some(bitrate) = bitrate {
            record["mp3_bitrate"] = Value::from(bitrate);
        } else {
            record
                .as_object_mut()
                .expect("validated settings record")
                .remove("mp3_bitrate");
        }
    }
    let imported = state.library.import_generated_song(library::GeneratedSongInput {
        title: job.title.clone(), metadata: serde_json::json!({"duration_seconds":output.duration,"job_id":job.id,"cover_prompt":job.cover_prompt,"output_format":format,"derived":job.derived}),
        caption: replay["caption"].as_str().unwrap_or(&job.style).to_owned(),
        lyrics: replay["lyrics"].as_str().unwrap_or(&job.lyrics).to_owned(),
        generation_settings: settings, replay_request: Some(replay), audio_codes: Some(codes),
        engine_id: job.engine_id.clone(), profile_id: Some(profile.into()), source: "local_generation".into(),
        audio_extension: output.extension, audio: output.audio,
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

struct ShapedOutput {
    audio: Vec<u8>,
    extension: &'static str,
    duration: f64,
    fades: (f32, f32),
}

fn shape_output(
    audio: Vec<u8>,
    format: &str,
    bitrate: Option<u32>,
    fades: (f32, f32),
) -> anyhow::Result<ShapedOutput> {
    let duration = openrouter_stream::wav_duration(&audio).context("MiniMax WAV is incomplete")?;
    if format == "wav32"
        && fades == (0.0, 0.0)
        && hound::WavReader::new(audio.as_slice())?
            .spec()
            .sample_format
            == hound::SampleFormat::Float
    {
        let audio = std::sync::Arc::new(audio);
        if let Some(problem) = audio_pcm::output_problem(audio.clone(), "wav")? {
            anyhow::bail!("MiniMax returned {problem} instead of a song");
        }
        let audio = std::sync::Arc::try_unwrap(audio)
            .map_err(|_| anyhow::anyhow!("MiniMax audio is still held by its validation"))?;
        return Ok(ShapedOutput {
            audio,
            extension: "wav",
            duration,
            fades,
        });
    }

    let mut stereo = audio_pcm::decode_stereo_bytes(audio, "wav")?;
    if let Some(problem) = audio_pcm::stereo_problem(&stereo) {
        anyhow::bail!("MiniMax returned {problem} instead of a song");
    }
    audio_post::encode::fade(&mut stereo, fades.0, fades.1);
    let (extension, audio) = match format {
        "wav32" => ("wav", audio_pcm::wav_f32_bytes(&stereo)?),
        "mp3" => (
            "mp3",
            audio_post::encode::mp3(&stereo, bitrate.unwrap_or(DEFAULT_MP3_KBPS))?,
        ),
        _ => ("flac", audio_post::encode::flac(&stereo)?),
    };
    let duration = library::audio_duration_seconds(&audio, extension, bitrate).unwrap_or(duration);
    let applied = |seconds: f32| {
        let rate = stereo.rate as f32;
        ((seconds * rate) as usize).min(stereo.frames()) as f32 / rate
    };
    let fades = (applied(fades.0), applied(fades.1));
    Ok(ShapedOutput {
        audio,
        extension,
        duration,
        fades,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimax_replays_the_original_acoustic_seed_or_overrides_it_without_changing_codes() {
        let codes = "1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16";
        let song: library::Song = serde_json::from_value(serde_json::json!({
            "id":"original", "title":"Original", "audio_path":"original.wav",
            "caption":"folk", "lyrics":"[verse]\nEarly morning", "source":"local_generation",
            "engine_id":native_minimax::ENGINE_ID,"profile_id":"minimal",
            "metadata":{},"generation_settings":{"music_source":{"kind":"native","engine":native_minimax::ENGINE_ID,"model":"minimal"}},
            "created_at":"0","updated_at":"0","audio_codes":codes,
            "replay_request":{"audio_codes":codes,"seed":42,"lm_seed":17,"steps":4,"duration":3.2,"dit_cfg":2.0,
                "output_format":"flac","fade_out":0.25}
        })).unwrap();
        let request: ReplayMusicJobRequest =
            serde_json::from_value(serde_json::json!({"song_id":"original"})).unwrap();
        let (profile, render, original) = replay_inputs(&song, request.clone()).unwrap();
        let (_, rerender, changed) = replay_inputs(
            &song,
            ReplayMusicJobRequest {
                seed: Some(43),
                ..request
            },
        )
        .unwrap();

        assert_eq!(profile, "minimal");
        assert_eq!(render.seed, Some(42));
        assert_eq!(rerender.seed, Some(43));
        assert_eq!(original.body["seed"], 42);
        assert_eq!(changed.body["seed"], 43);
        assert_eq!(original.body["audio_codes"], codes);
        assert_eq!(changed.body["audio_codes"], codes);
        assert_eq!(changed.body["dit_cfg"], 2.0);
        assert_eq!(changed.body["lm_seed"], 17);
        assert_eq!(changed.derived["from"], "original");
        assert_eq!(changed.derived["tool"], "replay");
        assert_eq!(changed.derived["settings"]["seed"], 43);
        assert_eq!(
            serde_json::to_value(rerender.music_source).unwrap(),
            song.generation_settings["music_source"]
        );
        assert_eq!(rerender.output_format.as_deref(), Some("flac"));
        assert_eq!(rerender.fade_out, Some(0.25));
        assert_eq!(song.replay_request.as_ref().unwrap()["seed"], 42);
    }
}
