use super::*;
use serde_json::{json, Map};
use tokio::sync::{mpsc, oneshot};

type CancelReply = oneshot::Sender<Result<(), String>>;
pub(super) type CancelSender = mpsc::Sender<CancelReply>;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct AceOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub think: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_song_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_song_id: Option<String>,
    #[serde(flatten)]
    pub fields: Map<String, Value>,
}

pub(super) async fn submit(
    state: AppState,
    request: CreateMusicJobRequest,
    profile: String,
    attempt: u32,
) -> (StatusCode, Json<MusicJob>) {
    submit_inner(state, request, profile, attempt, None).await
}

struct ReplayOrigin {
    song: library::Song,
    request: ReplayMusicJobRequest,
}

async fn submit_inner(
    state: AppState,
    request: CreateMusicJobRequest,
    profile: String,
    attempt: u32,
    replay: Option<ReplayOrigin>,
) -> (StatusCode, Json<MusicJob>) {
    let started = std::time::Instant::now();
    let reject = |status, reason: String| {
        (
            status,
            Json(failed_request_job(
                request.clone(),
                ace::ENGINE_ID.into(),
                reason,
            )),
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
    let mut options = request.ace.clone().unwrap_or_default();
    if options.source_song_id.is_none() {
        options.source_song_id = request.cover_of.clone();
    }
    let input = match engine_request(&request, &options, replay.is_some()) {
        Ok(input) => input,
        Err(error) => return reject(StatusCode::BAD_REQUEST, format!("{error:#}")),
    };
    let Some(root) = studio_data_root() else {
        return reject(
            StatusCode::SERVICE_UNAVAILABLE,
            "Studio data folder is unavailable.".into(),
        );
    };
    let saved_runtime = replay
        .as_ref()
        .map(|replay| &replay.song.generation_settings["native_runtime"]);
    let (catalogue, runtime) = match ace::resolve_installation(
        &root,
        saved_runtime,
        engine_catalogue::install::Probe::Sha256,
    )
    .await
    {
        Ok(resolved) => resolved,
        Err(error) => return reject(StatusCode::SERVICE_UNAVAILABLE, format!("{error:#}")),
    };
    let resolved = if replay.is_some() {
        catalogue.launch_replay(&profile, &input)
    } else {
        catalogue.launch(&profile)
    };
    let launch = match resolved {
        Ok(launch) => launch,
        Err(error) => return reject(StatusCode::SERVICE_UNAVAILABLE, format!("{error:#}")),
    };
    if ace::adapter_names(&input).next().is_some()
        && native_music_runtime::requires_adapter_update(&root, ace::ENGINE_ID, &launch.executable)
    {
        return reject(
            StatusCode::SERVICE_UNAVAILABLE,
            "Update the ACE-Step runtime in Create before using LoRA adapters on Windows.".into(),
        );
    }
    for key in ["synth_model", "lm_model", "vae"] {
        if let Some(name) = input.get(key).and_then(Value::as_str) {
            let filename = std::path::Path::new(name);
            if filename.file_name().and_then(|name| name.to_str()) != Some(name)
                || !launch.models_root.join(filename).is_file()
            {
                return reject(
                    StatusCode::BAD_REQUEST,
                    format!("{name} is not part of the selected ACE-Step profile."),
                );
            }
        }
    }
    state.minimax.stop().await;
    evict_kept_models(&state, "before ACE-Step generation").await;
    let client = match state.ace_runtime.start(launch).await {
        Ok(client) => client,
        Err(error) => return reject(StatusCode::SERVICE_UNAVAILABLE, format!("{error:#}")),
    };
    let job = MusicJob {
        derived: replay.as_ref().map(|replay| derivation(&replay.song, "replay",
            json!({"steps":replay.request.steps, "seed":replay.request.seed,
                "solver":replay.request.solver, "synth_batch_size":replay.request.synth_batch_size,
                "output_format":replay.request.output_format, "mp3_bitrate":replay.request.mp3_bitrate}))),
        id: format!("ace-{}", uuid_suffix()),
        client_ref: request.client_ref.clone(),
        submitted_at: unix_millis(),
        engine_id: ace::ENGINE_ID.into(),
        cover_prompt: request.cover_prompt.clone(),
        title: Some(titled(&request, &state.adapters)),
        status: MusicJobStatus::Queued,
        dispatch: MusicJobDispatch::Local,
        phase: MusicJobPhase::Queued,
        style: request.style.clone(),
        lyrics: request.lyrics.clone(),
        duration_seconds: input["duration"].as_f64().unwrap_or_default(),
        generation_settings: json!({
            "music_source": request.music_source,
            "native_runtime": runtime,
            "ace": input,
            "fade_in": request.fade_in,
            "fade_out": request.fade_out,
        }),
        song: None,
        songs: Vec::new(),
        message: "Submitted to ACE-Step.".into(),
        code: None,
        params: None,
        playlist_id: request.playlist_id.clone(),
        laid: None,
        card_memory_short: false,
        tracks_asked: None,
    };
    let saved_request = match replay {
        Some(replay) => json!({"replay": replay.request, "music_source": request.music_source}),
        None => json!(request),
    };
    if let Err(error) = state
        .library
        .save_music_job(&stored_job(&job, saved_request, attempt))
    {
        return reject(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
    }
    let (tx, rx) = mpsc::channel(1);
    state
        .ace_cancellations
        .write()
        .await
        .insert(job.id.clone(), tx);
    state.jobs.write().await.insert(job.id.clone(), job.clone());
    let accepted = job.clone();
    tokio::spawn(async move {
        let _lease = lease;
        let outcome = watch(&state, &job, &profile, &client, input, options, rx).await;
        let mut jobs = state.jobs.write().await;
        let report = if let Some(active) = jobs.get_mut(&job.id) {
            match outcome {
                Ok(Some(songs)) => {
                    active.song = songs.first().cloned();
                    if let Some(song) = &active.song {
                        active.duration_seconds = song.song.metadata["duration_seconds"]
                            .as_f64()
                            .unwrap_or(active.duration_seconds);
                    }
                    active.songs = songs;
                    active.status = MusicJobStatus::Completed;
                    active.phase = MusicJobPhase::Completed;
                    active.message = "ACE-Step music was saved to the library.".into();
                }
                Ok(None) => stop_on_request(active),
                Err(error) => active.fail(format!("ACE-Step generation failed: {error:#}").into()),
            }
            if let Err(error) = state.library.set_music_job_status(
                &job.id,
                job_status_name(&active.status),
                &active.message,
            ) {
                studio_log!("[ERROR] saving ACE-Step job state: {error:#}");
            }
            Some(native_music_jobs::SongReport::from_job(active))
        } else {
            None
        };
        let elapsed = started.elapsed();
        drop(jobs);
        drop(_lease);
        state.ace_cancellations.write().await.remove(&job.id);
        if let Some(report) = report {
            report
                .record(&state, ace::ENGINE_ID, "native.ace_step", elapsed)
                .await;
        }
    });
    (StatusCode::ACCEPTED, Json(accepted))
}

fn engine_request(
    request: &CreateMusicJobRequest,
    options: &AceOptions,
    replay: bool,
) -> anyhow::Result<Value> {
    let mut fields = options.fields.clone();
    for (key, value) in [
        ("caption", json!(request.style)),
        ("lyrics", json!(request.lyrics)),
        ("duration", json!(request.duration_seconds)),
        ("seed", json!(request.seed)),
        ("lm_seed", json!(request.lm_seed)),
        ("inference_steps", json!(request.steps)),
        ("guidance_scale", json!(request.cfg_scale)),
        ("lm_batch_size", json!(request.lm_batch_size)),
        ("synth_batch_size", json!(request.synth_batch_size)),
        ("output_format", json!(request.output_format)),
        ("mp3_bitrate", json!(request.mp3_bitrate)),
    ] {
        if !value.is_null() {
            fields.entry(key.to_owned()).or_insert(value);
        }
    }
    if !fields.contains_key("vocal_language") {
        if let Some(language) = fields
            .get("lyrics")
            .and_then(Value::as_str)
            .and_then(ace::script_language)
        {
            fields.insert("vocal_language".into(), json!(language));
        }
    }
    if let Some(format) = fields.get("output_format").and_then(Value::as_str) {
        validate_output_format(format).map_err(anyhow::Error::msg)?;
    }
    if let Some(rate) = fields.get("mp3_bitrate") {
        let rate = rate
            .as_u64()
            .and_then(|rate| u32::try_from(rate).ok())
            .context("ACE-Step mp3_bitrate must be a whole number")?;
        validate_mp3_bitrate(rate).map_err(anyhow::Error::msg)?;
    }
    let input = ace::prepare(&Value::Object(fields))?;
    let task = input["task_type"]
        .as_str()
        .context("ACE-Step task has no type")?;
    anyhow::ensure!(
        !ace::needs_source(task) || options.source_song_id.is_some(),
        "ACE-Step task {task} needs a library source song"
    );
    anyhow::ensure!(
        !ace::base_model_only(task)
            || (replay
                && input["synth_model"]
                    .as_str()
                    .is_some_and(|name| name.contains("-base-"))),
        "ACE-Step task {task} needs the original base DiT"
    );
    Ok(input)
}

pub(super) async fn replay(
    state: AppState,
    song: library::Song,
    mut request: ReplayMusicJobRequest,
    attempt: u32,
) -> Result<(StatusCode, Json<MusicJob>), (StatusCode, Json<ApiError>)> {
    let (profile, create) = replay_request(&state.library, &song, &request)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, format!("{error:#}")))?;
    request.song_id = Some(song.id.clone());
    request.replay_request = None;
    Ok(submit_inner(
        state,
        create,
        profile,
        attempt,
        Some(ReplayOrigin { song, request }),
    )
    .await)
}

fn replay_request(
    library: &library::Library,
    song: &library::Song,
    overrides: &ReplayMusicJobRequest,
) -> anyhow::Result<(String, CreateMusicJobRequest)> {
    anyhow::ensure!(
        song.engine_id == ace::ENGINE_ID,
        "the source track was not made by ACE-Step"
    );
    anyhow::ensure!(
        overrides.extend_seconds.is_none(),
        "ACE-Step replay does not support extend_seconds"
    );
    if let Some(steps) = overrides.steps {
        anyhow::ensure!(steps > 0, "steps must be at least 1");
    }
    if let Some(takes) = overrides.synth_batch_size {
        anyhow::ensure!(
            (1..=ace::MAX_TAKES as u32).contains(&takes),
            "synth_batch_size must be between 1 and {}",
            ace::MAX_TAKES
        );
    }

    let mut fields = song
        .replay_request
        .as_ref()
        .and_then(Value::as_object)
        .cloned()
        .context("the track has no ACE-Step replay record")?;
    let take_link =
        |fields: &mut Map<String, Value>, key: &str| -> anyhow::Result<Option<String>> {
            let value = fields
                .remove(key)
                .or_else(|| song.generation_settings.get(key).cloned());
            let Some(id) = value
                .as_ref()
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
            else {
                return Ok(None);
            };
            Ok(Some(
                library
                    .resolve_legacy_song_reference(&song.id, id)?
                    .unwrap_or_else(|| id.into()),
            ))
        };
    let source = take_link(&mut fields, "source_song_id")?;
    let reference = take_link(&mut fields, "reference_song_id")?;
    let fade_in = fields
        .remove("fade_in")
        .or_else(|| song.generation_settings.get("fade_in").cloned());
    let fade_out = fields
        .remove("fade_out")
        .or_else(|| song.generation_settings.get("fade_out").cloned());
    if fields
        .get("audio_codes")
        .and_then(Value::as_str)
        .is_none_or(|codes| codes.trim().is_empty())
    {
        if let Some(codes) = song
            .audio_codes
            .as_ref()
            .and_then(Value::as_str)
            .filter(|codes| !codes.trim().is_empty())
        {
            fields.insert("audio_codes".into(), json!(codes));
        }
    }
    if fields
        .get("duration")
        .and_then(Value::as_f64)
        .is_none_or(|seconds| seconds <= 0.0)
    {
        if let Some(duration) = song
            .metadata
            .get("duration_seconds")
            .filter(|value| value.as_f64().is_some_and(|seconds| seconds > 0.0))
        {
            fields.insert("duration".into(), duration.clone());
        }
    }
    fields.insert("lm_batch_size".into(), json!(1));
    for (key, value) in [
        ("inference_steps", json!(overrides.steps)),
        ("seed", json!(overrides.seed)),
        ("solver", json!(overrides.solver)),
        ("synth_batch_size", json!(overrides.synth_batch_size)),
        ("output_format", json!(overrides.output_format)),
        ("mp3_bitrate", json!(overrides.mp3_bitrate)),
    ] {
        if !value.is_null() {
            fields.insert(key.into(), value);
        }
    }
    if fields
        .get("task_type")
        .and_then(Value::as_str)
        .unwrap_or("text2music")
        == "text2music"
        && fields
            .get("audio_codes")
            .and_then(Value::as_str)
            .is_none_or(|codes| codes.trim().is_empty())
    {
        anyhow::ensure!(
            fields
                .get("duration")
                .and_then(Value::as_f64)
                .is_some_and(|seconds| seconds > 0.0),
            "ACE-Step replay without audio codes needs the original duration"
        );
    }

    let profile = song
        .profile_id
        .clone()
        .unwrap_or_else(|| "legacy-replay".into());
    let options = AceOptions {
        think: Some(false),
        source_song_id: source,
        reference_song_id: reference,
        fields,
    };
    let create: CreateMusicJobRequest = serde_json::from_value(json!({
        "music_source":{"kind":"native","engine":ace::ENGINE_ID,"model":profile},
        "ace":options,
        "client_ref":overrides.client_ref,
        "style":song.caption,
        "lyrics":song.lyrics,
        "title":overrides.title.as_deref().filter(|title| !title.trim().is_empty()).unwrap_or(&song.title),
        "fade_in":fade_in,
        "fade_out":fade_out,
    }))?;
    engine_request(&create, &options, true)?;
    Ok((profile, create))
}

async fn watch(
    state: &AppState,
    job: &MusicJob,
    profile: &str,
    client: &ace::AceClient,
    input: Value,
    options: AceOptions,
    mut rx: mpsc::Receiver<CancelReply>,
) -> anyhow::Result<Option<Vec<CompletedSong>>> {
    let library = state.library.clone();
    let source = options.source_song_id.clone();
    let reference = options.reference_song_id.clone();
    let sources = tokio::task::spawn_blocking(move || {
        for id in [source.as_deref(), reference.as_deref()]
            .into_iter()
            .flatten()
        {
            let song = library
                .get_song(id)?
                .with_context(|| format!("ACE-Step input song {id} is missing"))?;
            anyhow::ensure!(
                !library::is_trashed(&song),
                "ACE-Step input song {id} is in the bin"
            );
        }
        ace::Sources::from_library(&library, source.as_deref(), reference.as_deref())
    })
    .await
    .context("read ACE-Step sources")??;
    if cancel_between_steps(&mut rx) {
        return Ok(None);
    }

    let is_song = input["task_type"] == "text2music";
    let has_codes = input["audio_codes"]
        .as_str()
        .is_some_and(|codes| !codes.is_empty());
    let wants_plan = options.think.unwrap_or(true);
    let mut plan = vec![input.clone()];
    if is_song && !has_codes && (wants_plan || !has_duration(&input)) {
        running(state, &job.id, "ACE-Step is planning the song.").await;
        let mut lm = input.clone();
        lm["lm_mode"] = json!(if wants_plan { "generate" } else { "format" });
        let remote = client.submit_lm(&[lm]).await?;
        if !wait_for_job(client, &remote, &mut rx).await? {
            return Ok(None);
        }
        plan = client.lm_result(&remote).await?;
        if !wants_plan {
            let mut filled = input.clone();
            let first = plan.first().context("ACE-Step returned no song plan")?;
            for key in ["duration", "bpm", "keyscale", "timesignature"] {
                let current = &filled[key];
                if (current.is_null()
                    || current.as_f64().is_some_and(|value| value <= 0.0)
                    || current
                        .as_str()
                        .is_some_and(|value| value.trim().is_empty()))
                    && !first[key].is_null()
                {
                    filled[key] = first[key].clone();
                }
            }
            anyhow::ensure!(
                has_duration(&filled),
                "ACE-Step could not infer the song length"
            );
            plan = vec![filled];
        }
    }
    if cancel_between_steps(&mut rx) {
        return Ok(None);
    }
    let mut requests = ace::synth_requests(plan);
    if job.derived.is_some() {
        if let Some(clip) = input.get("peak_clip") {
            for request in &mut requests {
                request["peak_clip"] = clip.clone();
            }
        }
    }
    running(state, &job.id, "ACE-Step is rendering the song.").await;
    let remote = client.submit_synth(&requests, sources).await?;
    if !wait_for_job(client, &remote, &mut rx).await? {
        return Ok(None);
    }
    if cancel_between_steps(&mut rx) {
        return Ok(None);
    }
    let (content_type, body) = client.result(&remote).await?;
    let takes = ace::takes(&requests, &content_type, &body)?;
    anyhow::ensure!(!takes.is_empty(), "ACE-Step returned no takes");
    if cancel_between_steps(&mut rx) {
        return Ok(None);
    }
    rx.close();
    for reply in std::iter::from_fn(|| rx.try_recv().ok()) {
        let _ = reply.send(Err("ACE-Step already finished rendering.".into()));
    }
    let mut songs = Vec::with_capacity(takes.len());
    for take in takes {
        let library = state.library.clone();
        let job = job.clone();
        let profile = profile.to_owned();
        let options = options.clone();
        let input = input.clone();
        songs.push(
            tokio::task::spawn_blocking(move || {
                import_take(&library, &job, &profile, &options, &input, take)
            })
            .await
            .context("import ACE-Step audio and latent")??,
        );
    }
    if let Some(playlist) = &job.playlist_id {
        if let Err(error) = add_to_playlist(
            &state.library,
            playlist,
            songs.iter().map(|song| song.id.clone()),
        ) {
            studio_log!("[ERROR] adding ACE-Step songs to playlist: {error:#}");
        }
    }
    for song in &songs {
        tag_stored_song(state, &song.id).await;
        after_import(state, &song.id);
    }
    Ok(Some(songs))
}

fn has_duration(input: &Value) -> bool {
    input["duration"].as_f64().is_some_and(|value| value > 0.0)
}

fn cancel_between_steps(rx: &mut mpsc::Receiver<CancelReply>) -> bool {
    if let Ok(reply) = rx.try_recv() {
        let _ = reply.send(Ok(()));
        return true;
    }
    false
}

async fn running(state: &AppState, id: &str, message: &str) {
    if let Some(job) = state.jobs.write().await.get_mut(id) {
        job.status = MusicJobStatus::Running;
        job.phase = MusicJobPhase::Running;
        job.message = message.into();
    }
}

async fn wait_for_job(
    client: &ace::AceClient,
    remote: &str,
    rx: &mut mpsc::Receiver<CancelReply>,
) -> anyhow::Result<bool> {
    let mut cancelled = false;
    let mut pending = Vec::<CancelReply>::new();
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(350));
    loop {
        tokio::select! {
            biased;
            Some(reply) = rx.recv() => {
                let response = if cancelled { Ok("cancelled".into()) } else { client.cancel_status(remote).await };
                match response {
                    Ok(status) if status == "cancelled" => {
                        cancelled = true;
                        pending.push(reply);
                    }
                    Ok(status) => { let _ = reply.send(Err(format!("ACE-Step job reported {status}."))); }
                    Err(error) => { let _ = reply.send(Err(error.to_string())); }
                }
            }
            _ = interval.tick() => {
                match client.status(remote).await?.as_str() {
                    "done" if !cancelled => return Ok(true),
                    "cancelled" | "done" => {
                        for reply in pending { let _ = reply.send(Ok(())); }
                        return Ok(false);
                    }
                    "running" | "queued" => {}
                    status => {
                        let error = client.result(remote).await.err().map(|error| error.to_string());
                        anyhow::bail!("{}", error.unwrap_or_else(|| format!("ACE-Step job {remote} reported {status}")));
                    }
                }
            }
        }
    }
}

fn import_take(
    library: &library::Library,
    job: &MusicJob,
    profile: &str,
    options: &AceOptions,
    input: &Value,
    take: ace::Take,
) -> anyhow::Result<CompletedSong> {
    let mut replay = take.replay;
    // The adapter requests float WAV from the engine. If the user selected an
    // encoded format, store that choice with the replay.
    let mut extension = engine_result::audio_extension(&take.audio_type)?;
    let mut bytes = take.audio;
    let duration = library::audio_duration_seconds(&bytes, extension, None)
        .context("ACE-Step produced an incomplete audio file")?;
    anyhow::ensure!(
        !take.latent.is_empty()
            && take.latent.len() % 256 == 0
            && take.latent.len() <= 15_000 * 256,
        "ACE-Step returned an invalid f32 latent"
    );
    let wanted = output_format(input);
    let bitrate = input["mp3_bitrate"]
        .as_u64()
        .and_then(|rate| u32::try_from(rate).ok())
        .unwrap_or(DEFAULT_MP3_KBPS);
    let fade_in = job.generation_settings["fade_in"]
        .as_f64()
        .unwrap_or_default() as f32;
    let fade_out = job.generation_settings["fade_out"]
        .as_f64()
        .unwrap_or_default() as f32;
    if extension == "wav" && (wanted != "wav32" || fade_in > 0.0 || fade_out > 0.0) {
        let mut stereo = audio_pcm::decode_stereo_bytes(bytes, extension)?;
        audio_post::encode::fade(&mut stereo, fade_in, fade_out);
        (bytes, extension) = match wanted {
            "mp3" => (audio_post::encode::mp3(&stereo, bitrate)?, "mp3"),
            "wav32" => (audio_pcm::wav_f32_bytes(&stereo)?, "wav"),
            _ => (audio_post::encode::flac(&stereo)?, "flac"),
        };
    }
    replay["output_format"] = json!(if extension == "wav" {
        "wav32"
    } else {
        extension
    });
    if extension == "mp3" {
        replay["mp3_bitrate"] = json!(audio_post::encode::mp3_bitrate(bitrate));
    } else if let Some(fields) = replay.as_object_mut() {
        fields.remove("mp3_bitrate");
    }
    if fade_in > 0.0 || fade_out > 0.0 {
        replay["fade_in"] = json!(fade_in);
        replay["fade_out"] = json!(fade_out);
    }
    let audio_codes = replay
        .get("audio_codes")
        .filter(|value| value.as_str().is_some_and(|codes| !codes.is_empty()))
        .cloned();
    let mut legacy_replay = replay.clone();
    if let Some(id) = &options.source_song_id {
        legacy_replay["source_song_id"] = json!(id);
    }
    if let Some(id) = &options.reference_song_id {
        legacy_replay["reference_song_id"] = json!(id);
    }
    let mut settings = legacy_replay.clone();
    settings
        .as_object_mut()
        .context("ACE-Step replay is not an object")?
        .remove("audio_codes");
    settings["music_source"] = job.generation_settings["music_source"].clone();
    if let Some(runtime) = job
        .generation_settings
        .get("native_runtime")
        .filter(|value| !value.is_null())
    {
        settings["native_runtime"] = runtime.clone();
    }
    let imported = library.import_generated_song_with_latent(library::GeneratedSongInput {
        title: job.title.clone(),
        metadata: json!({"duration_seconds": duration, "job_id": job.id, "cover_prompt": job.cover_prompt, "derived": job.derived}),
        caption: replay["caption"].as_str().unwrap_or(&job.style).into(),
        lyrics: replay["lyrics"].as_str().unwrap_or(&job.lyrics).into(),
        generation_settings: settings,
        replay_request: Some(legacy_replay),
        audio_codes,
        engine_id: ace::ENGINE_ID.into(),
        profile_id: Some(profile.into()),
        source: "local_generation".into(),
        audio_extension: extension,
        audio: bytes,
    }, Some(&take.latent))?;
    let id = imported.song.id.clone();
    Ok(CompletedSong {
        audio_url: format!("/v1/library/media/{id}"),
        id,
        song: imported.song,
    })
}

pub(super) async fn cancel(
    state: &AppState,
    job: MusicJob,
) -> Result<Json<MusicJob>, (StatusCode, Json<ApiError>)> {
    let sender = state.ace_cancellations.read().await.get(&job.id).cloned();
    if let Some(sender) = sender {
        let (reply, result) = oneshot::channel();
        if sender.send(reply).await.is_ok() {
            if let Ok(answer) = result.await {
                answer.map_err(|reason| api_error(StatusCode::CONFLICT, reason))?;
                if let Some(active) = state.jobs.write().await.get_mut(&job.id) {
                    stop_on_request(active);
                }
            }
        }
    }
    Ok(Json(
        state.jobs.read().await.get(&job.id).cloned().unwrap_or(job),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[tokio::test]
    async fn cancellation_waits_for_the_ace_worker_to_stop() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let polled_after_cancel = Arc::new(AtomicBool::new(false));
        let state_read = cancelled.clone();
        let state_write = cancelled.clone();
        let polled = polled_after_cancel.clone();
        let app = axum::Router::new().route(
            "/job",
            get(move || {
                let state = state_read.clone();
                let polled = polled.clone();
                async move {
                    let stopped = state.load(Ordering::SeqCst);
                    if stopped {
                        polled.store(true, Ordering::SeqCst);
                    }
                    Json(json!({"status": if stopped { "cancelled" } else { "running" }}))
                }
            })
            .post(move || {
                let state = state_write.clone();
                async move {
                    state.store(true, Ordering::SeqCst);
                    Json(json!({"status":"cancelled"}))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = ace::AceClient::new(&format!("http://{}", listener.local_addr().unwrap()));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (tx, mut rx) = mpsc::channel(1);
        let (reply, result) = oneshot::channel();
        tx.send(reply).await.unwrap();
        assert!(!wait_for_job(&client, "fixture", &mut rx).await.unwrap());
        assert_eq!(result.await.unwrap(), Ok(()));
        assert!(polled_after_cancel.load(Ordering::SeqCst));
        server.abort();
    }

    #[test]
    fn legacy_ace_replays_preserve_model_codes_and_remapped_links() {
        let root = std::env::temp_dir().join(format!("ace-replay-{}", uuid::Uuid::now_v7()));
        let library =
            library::Library::open_at(root.join("library.sqlite"), root.join("media")).unwrap();
        let song = library
            .create_song(
                serde_json::from_value(json!({
                    "title":"original", "caption":"style", "lyrics":"lyrics",
                    "engine_id":"acestep-cpp", "metadata":{"duration_seconds":10},
                    "audio_codes":"7,8,9",
                    "replay_request":{
                        "task_type":"repaint", "synth_model":"acestep-v15-xl-sft-Q4_K_M.gguf",
                        "lm_model":"acestep-5Hz-lm-4B-BF16.gguf", "vae":"vae-BF16.gguf",
                        "source_song_id":"old-source", "reference_song_id":"old-reference",
                        "repainting_start":2, "repainting_end":4, "lm_seed":9,
                        "seed":1, "fade_in":0.2, "inference_steps":10, "peak_clip":10
                    }
                }))
                .unwrap(),
            )
            .unwrap();
        library.legacy_transaction(|tx| {
            tx.execute("INSERT INTO legacy_ids VALUES ('F:/portable','song','old-original',?1,1)", [&song.id])?;
            tx.execute("INSERT INTO legacy_ids VALUES ('F:/portable','song','old-source','current-source',1)", [])?;
            tx.execute("INSERT INTO legacy_ids VALUES ('F:/portable','song','old-reference','current-reference',1)", [])?;
            Ok(())
        }).unwrap();
        let options = serde_json::from_value(json!({
            "steps":6, "seed":123, "output_format":"wav32", "synth_batch_size":2, "solver":"euler", "title":"override"
        })).unwrap();
        let (profile, request) = replay_request(&library, &song, &options).unwrap();
        assert_eq!(profile, "legacy-replay");
        assert_eq!(request.title.as_deref(), Some("override"));
        assert_eq!(request.fade_in, Some(0.2));
        let ace = request.ace.unwrap();
        assert_eq!(ace.think, Some(false));
        assert_eq!(ace.source_song_id.as_deref(), Some("current-source"));
        assert_eq!(ace.reference_song_id.as_deref(), Some("current-reference"));
        assert_eq!(ace.fields["audio_codes"], "7,8,9");
        assert_eq!(ace.fields["synth_model"], "acestep-v15-xl-sft-Q4_K_M.gguf");
        assert_eq!(ace.fields["repainting_start"], 2);
        assert_eq!(ace.fields["lm_seed"], 9);
        assert_eq!(ace.fields["inference_steps"], 6);
        assert_eq!(ace.fields["seed"], 123);
        assert_eq!(ace.fields["output_format"], "wav32");
        assert_eq!(ace.fields["synth_batch_size"], 2);
        assert_eq!(ace.fields["peak_clip"], 10);
        drop(library);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_ace_take_enters_the_library_with_audio_latent_and_replay() {
        let root = std::env::temp_dir().join(format!("ace-take-{}", uuid::Uuid::now_v7()));
        let library =
            library::Library::open_at(root.join("library.sqlite"), root.join("media")).unwrap();
        let request: CreateMusicJobRequest =
            serde_json::from_value(json!({"style":"fixture","lyrics":"fixture"})).unwrap();
        let mut job = failed_request_job(request, ace::ENGINE_ID.into(), "fixture".to_owned());
        job.generation_settings =
            json!({"music_source":{"kind":"native","engine":"acestep-cpp","model":"minimal"}});
        let stereo = audio_post::Stereo::new(vec![0.1; 480], vec![0.2; 480], 48_000);
        let wav = audio_pcm::wav_f32_bytes(&stereo).unwrap();
        let latent = vec![0x5a; 256];
        let take = ace::Take {
            audio_type: "audio/wav".into(),
            audio: wav.clone(),
            latent: latent.clone(),
            replay: json!({"caption":"fixture","seed":12,"audio_codes":"1,2","task_type":"cover"}),
        };
        let options = AceOptions {
            source_song_id: Some("old-song".into()),
            ..AceOptions::default()
        };
        let song = import_take(
            &library,
            &job,
            "minimal",
            &options,
            &json!({"output_format":"wav32"}),
            take,
        )
        .unwrap();
        let recorded = library.get_song(&song.id).unwrap().unwrap();
        assert_eq!(recorded.profile_id.as_deref(), Some("minimal"));
        assert_eq!(recorded.audio_codes, Some(json!("1,2")));
        let replay = recorded.replay_request.unwrap();
        assert_eq!(replay["source_song_id"], "old-song");
        assert_eq!(replay["seed"], 12);
        assert_eq!(
            std::fs::read(library.media_file(&format!("{}.wav", song.id)).unwrap()).unwrap(),
            wav
        );
        assert_eq!(
            std::fs::read(library.media_file(&format!("{}.latent", song.id)).unwrap()).unwrap(),
            latent
        );
        drop(library);
        std::fs::remove_dir_all(root).unwrap();
    }
}
