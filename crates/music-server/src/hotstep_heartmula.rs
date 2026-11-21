use super::*;
use crate::{hotstep_cover_assets as runtime, hotstep_heartmula_assets as assets};
use anyhow::{Context, ensure};
use std::sync::atomic::AtomicBool;

pub(super) const PROFILE: &str = "heartmula-3b-q8_0";
const MODEL: &str = "HeartMuLa 3B";
const SCOPE: &str = "hotstep-heartmula";
pub(super) const ADAPTERS: &str = "heartmula-adapters";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct Options {
    adapter: Option<String>,
    adapter_scale: Option<f32>,
}

pub(super) async fn profile(state: &AppState, installed: bool) -> Value {
    let manager = &state.hotstep_cover;
    let files = assets::missing(manager, false);
    let runtime_missing = runtime::missing_runtime(manager);
    let mut adapters: Vec<_> = std::fs::read_dir(runtime::model_folder(manager).join(ADAPTERS))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|item| {
            let path = item.path();
            let name = path.file_stem()?.to_str()?;
            (path.extension()?.to_str()? == "gguf" && path.is_file() && valid_adapter(name))
                .then(|| name.to_owned())
        })
        .collect();
    adapters.sort_unstable();
    let installing = state.hotstep_sa3_install.try_lock().is_err();
    let mut missing = files.iter().map(|item| item.label).collect::<Vec<_>>();
    if !installed {
        missing.push("HOT-Step server");
    }
    serde_json::json!({
        "id":PROFILE,"label":"HeartMuLa 3B · HOT-Step · Q8_0","ready":installed && files.is_empty() && !installing,
        "installing":installing,"installable":installed || runtime_missing.is_some(),"missing":missing,
        "missing_bytes":files.iter().map(|item| item.bytes).sum::<u64>() + runtime_missing.map_or(0, |item| item.bytes),
        "download":manager.active_for(SCOPE).await,"adapters":adapters,
    })
}

pub(super) async fn install(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let manager = state.hotstep_cover.clone();
    let Ok(lease) = state.hotstep_sa3_install.try_lock_owned() else {
        return Ok(Json(serde_json::json!({"started":false,"installing":true})));
    };
    let runtime_missing = runtime::missing_runtime(&manager);
    if runtime_missing.is_none() && !runtime::server(&manager).is_some_and(|exe| exe.is_file()) {
        return Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Configure a HOT-Step server before installing HeartMuLa.".into(),
        ));
    }
    let mut missing = assets::missing(&manager, false);
    missing.extend(runtime_missing);
    hotstep_sa3_training::install_assets(manager, lease, SCOPE, missing).await
}

fn valid_adapter(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn native_payload(
    request: &CreateMusicJobRequest,
    models: &std::path::Path,
) -> anyhow::Result<(Value, Options)> {
    ensure!(
        !request.style.trim().is_empty() || !request.lyrics.trim().is_empty(),
        "Enter HeartMuLa style or lyrics."
    );
    let duration = request.duration_seconds.unwrap_or(30.0);
    let steps = request.steps.unwrap_or(10);
    let guidance = request.cfg_scale.unwrap_or(1.5);
    ensure!(
        (1.0..=240.0).contains(&duration),
        "HeartMuLa duration must be between 1 and 240 seconds."
    );
    ensure!(
        (1..=64).contains(&steps),
        "HeartMuLa codec steps must be between 1 and 64."
    );
    ensure!(
        (1.0..=20.0).contains(&guidance),
        "HeartMuLa guidance must be between 1 and 20."
    );
    let seed = match request.seed {
        None | Some(-1) => ace::random_seed(),
        Some(seed) => i64::from(u32::try_from(seed).context("Invalid HeartMuLa seed")?),
    };
    let mut opts = request.hotstep_heartmula.clone().unwrap_or_default();
    let mut body = serde_json::json!({
        "variant":"3B", "global_type":"Q8_0", "local_type":"Q8_0", "flow_type":"Q8_0", "scalar_decoder_type":"F16",
        "tags":request.style, "lyrics":request.lyrics, "duration_s":duration, "seed":seed,
        "codec_steps":steps, "cfg_scale":guidance,
    });
    if let Some(name) = &opts.adapter {
        ensure!(valid_adapter(name), "Invalid HeartMuLa adapter name.");
        let path = models.join(ADAPTERS).join(format!("{name}.gguf"));
        ensure!(path.is_file(), "The selected HeartMuLa adapter is missing.");
        let scale = opts.adapter_scale.unwrap_or(1.0);
        ensure!(
            (0.0..=2.0).contains(&scale),
            "HeartMuLa adapter strength must be between 0 and 2."
        );
        opts.adapter_scale = Some(scale);
        body["adapter"] = serde_json::json!(path);
        body["adapter_scale"] = serde_json::json!(scale);
    } else {
        opts.adapter_scale = None;
    }
    Ok((body, opts))
}

pub(super) async fn submit(
    state: AppState,
    mut request: CreateMusicJobRequest,
    attempt: u32,
) -> (StatusCode, Json<MusicJob>) {
    let engine = hotstep_cover::ENGINE;
    let submitted = std::time::Instant::now();
    let reject = |status, reason: String| {
        (
            status,
            Json(failed_request_job(request.clone(), engine.into(), reason)),
        )
    };
    let manager = &state.hotstep_cover;
    let models = runtime::model_folder(manager);
    let Some(exe) = runtime::server(manager)
        .filter(|exe| exe.is_file() && assets::missing(manager, false).is_empty())
    else {
        return reject(
            StatusCode::SERVICE_UNAVAILABLE,
            "Install the HOT-Step HeartMuLa profile in Create.".into(),
        );
    };
    let (body, options) = match native_payload(&request, &models) {
        Ok(data) => data,
        Err(error) => return reject(StatusCode::BAD_REQUEST, format!("{error:#}")),
    };
    let format = request
        .output_format
        .as_deref()
        .unwrap_or("flac")
        .to_owned();
    if let Err(error) = validate_output_format(&format) {
        return reject(StatusCode::BAD_REQUEST, error);
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
            "Wait for active generation or training to finish.".into(),
        );
    }
    if let Err(error) = state.ace_runtime.stop().await {
        return reject(StatusCode::CONFLICT, format!("ACE-Step is busy: {error:#}"));
    }
    state.minimax.stop().await;
    evict_kept_models(&state, "before HOT-Step HeartMuLa generation").await;

    request.duration_seconds = body["duration_s"].as_f64();
    request.steps = body["codec_steps"].as_u64().map(|steps| steps as u32);
    request.seed = body["seed"].as_i64();
    request.cfg_scale = body["cfg_scale"].as_f64();
    request.hotstep_heartmula = Some(options);
    let generation_settings = serde_json::json!({
        "music_source":request.music_source,"family":"heartmula","profile_id":PROFILE,"model":MODEL,
        "quantization":"Q8_0","runtime_revision":runtime::managed_revision(),
        "hotstep_heartmula":request.hotstep_heartmula,"duration_seconds":request.duration_seconds,
        "steps":request.steps,"seed":request.seed,"cfg_scale":request.cfg_scale,"output_format":format,
        "fade_in":request.fade_in,"fade_out":request.fade_out,"mp3_bitrate":request.mp3_bitrate,
    });
    let job = MusicJob {
        id: format!("hotstep-heartmula-{}", uuid_suffix()),
        client_ref: request.client_ref.clone(),
        submitted_at: unix_millis(),
        engine_id: engine.into(),
        title: Some(titled(&request, &state.adapters)),
        cover_prompt: request.cover_prompt.clone(),
        status: MusicJobStatus::Running,
        dispatch: MusicJobDispatch::Local,
        phase: MusicJobPhase::Running,
        style: request.style.clone(),
        lyrics: request.lyrics.clone(),
        duration_seconds: request.duration_seconds.unwrap_or_default(),
        generation_settings,
        song: None,
        songs: vec![],
        message: "HeartMuLa is generating through HOT-Step.".into(),
        code: None,
        params: None,
        playlist_id: request.playlist_id.clone(),
        laid: None,
        card_memory_short: false,
        tracks_asked: Some(1),
        derived: None,
    };
    if let Err(error) =
        state
            .library
            .save_music_job(&stored_job(&job, serde_json::json!(request), attempt))
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(failed_request_job(
                request,
                engine.into(),
                error.to_string(),
            )),
        );
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
        let outcome = async {
            let audio = hotstep_cover::render(&exe, &models, body, &stop, true).await?;
            let fades = (
                request.fade_in.unwrap_or_default(),
                request.fade_out.unwrap_or_default(),
            );
            tokio::task::spawn_blocking(move || {
                native_music_jobs::shape_output(audio, &format, request.mp3_bitrate, fades)
            })
            .await?
        }
        .await;
        audio_cpp_music::complete_local(
            &state,
            &job.id,
            MODEL,
            PROFILE,
            "hotstep.heartmula",
            submitted,
            outcome,
        )
        .await;
        state.audio_cpp_music_stops.write().await.remove(&job.id);
    });
    (StatusCode::ACCEPTED, Json(accepted))
}
