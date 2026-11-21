//! HOT-Step's native MuLaCover job, conditioned by the selected song's lead sheet.
use super::*;
use crate::{hotstep_cover_assets as assets, hotstep_cover_rolls as rolls};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub(super) const ENGINE: &str = "hotstep";
pub(super) const PROFILE: &str = "heartmula-mulacover";
const MODEL: &str = "MuLaCover";

fn downloader(state: &AppState) -> Arc<downloads::Downloader> {
    state.hotstep_cover.clone()
}

pub(super) async fn profiles(State(state): State<AppState>) -> Json<Value> {
    let downloader = downloader(&state);
    let server = assets::server(&downloader).is_some_and(|path| path.is_file());
    let mut missing: Vec<_> = assets::missing(&downloader)
        .iter()
        .map(|asset| asset.label)
        .collect();
    if !server {
        missing.push("HOT-Step server");
    }
    Json(serde_json::json!({
        "engine": ENGINE, "engine_installed": server,
        "profiles": [{"id": PROFILE, "label": "MuLaCover · HOT-Step · BF16", "ready": missing.is_empty(), "missing": missing}, crate::hotstep_sa3::profile(&downloader, server)]
    }))
}

pub(super) async fn install(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let downloader = downloader(&state);
    let missing = assets::missing(&downloader);
    downloader
        .install_all("hotstep-mulacover", &missing)
        .await
        .map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, format!("{error:#}")))?;
    Ok(Json(
        serde_json::json!({"ready": assets::ready(&downloader)}),
    ))
}

fn payload(request: &CreateMusicJobRequest, abc: &str) -> anyhow::Result<Value> {
    let duration = request.duration_seconds.unwrap_or(20.0);
    let steps = request.steps.unwrap_or(10);
    let seed = request.seed.unwrap_or(1234);
    let cfg = request.cfg_scale.unwrap_or(1.5);
    anyhow::ensure!(
        (1.0..=300.0).contains(&duration),
        "HOT-Step cover duration must be between 1 and 300 seconds."
    );
    anyhow::ensure!(seed >= 0, "The HOT-Step seed must be nonnegative.");
    anyhow::ensure!(steps > 0, "The HOT-Step codec needs at least one step.");
    anyhow::ensure!(
        cfg.is_finite() && cfg > 0.0,
        "The HOT-Step guidance scale must be positive."
    );
    let rolls = rolls::from_abc(abc)?;
    Ok(serde_json::json!({
        "variant": MODEL, "lyrics": request.lyrics, "tags": request.style, "seed": seed,
        "duration_s": duration.round(), "cfg_scale": cfg, "codec_steps": steps, "rolls": rolls,
    }))
}

pub(super) async fn submit(
    state: AppState,
    request: CreateMusicJobRequest,
    model: String,
    attempt: u32,
) -> (StatusCode, Json<MusicJob>) {
    let reject = |status, reason: String| {
        (
            status,
            Json(failed_request_job(request.clone(), ENGINE.into(), reason)),
        )
    };
    if model != PROFILE {
        return reject(
            StatusCode::BAD_REQUEST,
            "Choose the HOT-Step MuLaCover profile.".into(),
        );
    }
    let Some(source) = request.cover_of.as_deref() else {
        return reject(
            StatusCode::BAD_REQUEST,
            "Choose the original song from the library.".into(),
        );
    };
    let original = match cover_source(&state.library, source, attempt > 0) {
        Ok(song) => song,
        Err((status, reason)) => return reject(status, reason),
    };
    if !state
        .library
        .media_path_for_song(&original)
        .is_some_and(|path| path.is_file())
    {
        return reject(
            StatusCode::BAD_REQUEST,
            "The original song has no audio.".into(),
        );
    }
    let abc = request
        .abc
        .as_deref()
        .or_else(|| original.generation_settings["abc"].as_str())
        .unwrap_or_default();
    let body = match payload(&request, abc) {
        Ok(body) => body,
        Err(error) => return reject(StatusCode::BAD_REQUEST, format!("{error:#}")),
    };
    let format = request.output_format.as_deref().unwrap_or("flac");
    if let Err(error) = validate_output_format(format) {
        return reject(StatusCode::BAD_REQUEST, error);
    }
    let downloader = downloader(&state);
    if !assets::ready(&downloader) {
        return reject(
            StatusCode::SERVICE_UNAVAILABLE,
            "Install the HOT-Step runtime and MuLaCover weights in Create.".into(),
        );
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
    evict_kept_models(&state, "before HOT-Step cover generation").await;
    let job = MusicJob {
        derived: Some(derivation(
            &original,
            "cover",
            serde_json::json!({"model": MODEL}),
        )),
        id: format!("hotstep-{}", uuid_suffix()),
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
        duration_seconds: body["duration_s"].as_f64().unwrap_or_default(),
        generation_settings: serde_json::json!({
            "music_source":request.music_source, "family":"mulacover", "profile_id": PROFILE,
            "quantization":"LM BF16 / Flow Q8_0", "runtime_revision": assets::managed_revision(),
            "cover_of":source, "abc":abc, "roll_version":1, "roll_length":body["rolls"]["length"],
            "duration_seconds":body["duration_s"], "steps":body["codec_steps"], "seed":body["seed"],
            "guidance_scale":body["cfg_scale"], "output_format":format,
        }),
        song: None,
        songs: vec![],
        message: "MuLaCover is generating through HOT-Step.".into(),
        code: None,
        params: None,
        playlist_id: request.playlist_id.clone(),
        laid: None,
        card_memory_short: false,
        tracks_asked: Some(1),
    };
    let mut stored = serde_json::json!(request);
    stored["abc"] = Value::String(abc.into());
    if let Err(error) = state
        .library
        .save_music_job(&stored_job(&job, stored, attempt))
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
    let exe = assets::server(&downloader).expect("readiness checks the executable");
    let directory = assets::model_folder(&downloader);
    let format = format.to_owned();
    tokio::spawn(async move {
        let _lease = lease;
        let outcome = match render(&exe, &directory, body, &stop).await {
            Ok(wav) => tokio::task::spawn_blocking(move || {
                native_music_jobs::shape_output(wav, &format, None, (0.0, 0.0))
            })
            .await
            .context("HOT-Step output worker stopped")
            .and_then(|result| result),
            Err(error) => Err(error),
        };
        audio_cpp_music::complete_local(&state, &job.id, MODEL, PROFILE, outcome).await;
        state.audio_cpp_music_stops.write().await.remove(&job.id);
    });
    (StatusCode::ACCEPTED, Json(accepted))
}

use crate::hotstep_runtime::json_response;

async fn render(
    exe: &std::path::Path,
    models: &std::path::Path,
    request: Value,
    stop: &AtomicBool,
) -> anyhow::Result<Vec<u8>> {
    let (_child, http, url, props) =
        crate::hotstep_runtime::start(exe, models, "/heartmula/props", stop).await?;
    anyhow::ensure!(
        props["cover_ready"] == true,
        "HOT-Step cannot render covers: {}",
        props["cover_error"]
    );
    let accepted = json_response(
        http.post(format!("{url}/heartmula/cover?format=wav16"))
            .json(&request)
            .send()
            .await?,
    )
    .await?;
    let id = accepted["id"]
        .as_str()
        .filter(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .context("HOT-Step returned no valid job ID.")?;
    loop {
        anyhow::ensure!(!stop.load(Ordering::SeqCst), "Stopped.");
        let job = json_response(
            http.get(format!("{url}/heartmula/job"))
                .query(&[("id", id)])
                .send()
                .await?,
        )
        .await?;
        match job["status"].as_str() {
            Some("done") => break,
            Some("queued" | "running" | "pending") => {}
            _ => anyhow::bail!("HOT-Step cover failed: {job}"),
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let audio = http
        .get(format!("{url}/job"))
        .query(&[("id", id), ("result", "1")])
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    anyhow::ensure!(
        audio.starts_with(b"RIFF"),
        "HOT-Step returned no WAV audio."
    );
    Ok(audio.to_vec())
}
