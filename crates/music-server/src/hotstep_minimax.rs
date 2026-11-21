//! MiniMax Music3 on the published HOT-Step Windows CPU runtime.
use super::*;
use crate::{hotstep_cover_assets as runtime, hotstep_runtime};
use anyhow::{Context, ensure};
use std::path::Path as FsPath;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub(super) const PROFILE: &str = "hotstep-minimax-music3";
const MODEL: &str = "MiniMax Music3";
const SCOPE: &str = PROFILE;
const ENGINE: &str = hotstep_cover::ENGINE;

macro_rules! weight {
    ($file:literal, $size:literal, $sha:literal) => {
        (downloads::Asset {
            id: $file, label: $file, kind: downloads::AssetKind::Model,
            url: concat!("https://huggingface.co/scragnog/MiniMax-Music3-GGUF/resolve/67b48c3028544e4ad9c26946b7c7c840f355ec2b/", $file),
            relative_path: concat!("models/minimax-music3/", $file),
            bytes: $size, unzip_into: None, marker: $file, pick: &[], vram_gb: None,
            note: "MiniMax Music3 compact split weights for HOT-Step",
        }, $sha)
    };
}

const WEIGHTS: [(downloads::Asset, &str); 5] = [
    weight!(
        "mm3-lm-Q3_K_M.gguf",
        4_589_971_808,
        "65a1ee35212547a95ed19067c5b3bc3492725d55e754729e693e4141396064a5"
    ),
    weight!(
        "mm3-depth-Q4_K_M.gguf",
        404_836_256,
        "77e729d6e0599ed7928a3c0f13a36e1d3ef5f7de9259eaf01a19445549fa958e"
    ),
    weight!(
        "mm3-cond-f16.gguf",
        50_341_120,
        "526a7682d5135ef46fa3e42de367ea3d411c8804c34796bc0c50822a3b4a2461"
    ),
    weight!(
        "mm3-dit-Q3_K_M.gguf",
        1_138_427_712,
        "f1a67119c34ede23e27240d3ebd630fa492247e1d0184263574482302ab48f3f"
    ),
    weight!(
        "mm3-voc-f16.gguf",
        216_638_016,
        "90e578bbbec4fb363b22af4c9203a246f09e18bc3261ca16805ebc2bebdbeb4c"
    ),
];

pub(super) fn sha256(url: &str) -> Option<&'static str> {
    WEIGHTS
        .iter()
        .find_map(|(asset, sha)| (asset.url == url).then_some(*sha))
}

fn missing(downloader: &downloads::Downloader) -> Vec<&'static downloads::Asset> {
    WEIGHTS
        .iter()
        .map(|(asset, _)| asset)
        .filter(|asset| {
            !std::fs::metadata(downloader.path_of(asset))
                .is_ok_and(|file| file.is_file() && file.len() == asset.bytes)
        })
        .chain(runtime::missing_runtime(downloader))
        .collect()
}

pub(super) async fn profile(state: &AppState, engine_installed: bool) -> Value {
    let downloader = &state.hotstep_cover;
    let missing = missing(downloader);
    let installing = state.hotstep_minimax_install.try_lock().is_err();
    serde_json::json!({
        "id": PROFILE, "label": "MiniMax Music3 · HOT-Step · Compact (experimental)",
        "status": "experimental", "ready": engine_installed && missing.is_empty() && !installing,
        "installable": runtime::server(downloader).is_some(),
        "installing": installing,
        "download": downloader.active_for(SCOPE).await,
        "missing": missing.iter().map(|asset| asset.label).collect::<Vec<_>>(),
        "missing_bytes": missing.iter().map(|asset| asset.bytes).sum::<u64>(),
        "license_url": "https://huggingface.co/scragnog/MiniMax-Music3-GGUF/blob/67b48c3028544e4ad9c26946b7c7c840f355ec2b/LICENSE",
    })
}

pub(super) async fn install(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let downloader = state.hotstep_cover.clone();
    if runtime::missing_runtime(&downloader).is_none()
        && !runtime::server(&downloader).is_some_and(|path| path.is_file())
    {
        return Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "The managed HOT-Step MiniMax profile is available on Windows.".into(),
        ));
    }
    let Ok(lease) = state.hotstep_minimax_install.clone().try_lock_owned() else {
        return Ok(Json(serde_json::json!({"installing": true})));
    };
    let missing = missing(&downloader);
    if missing.is_empty() {
        return Ok(Json(serde_json::json!({"ready": true})));
    }
    downloader.clear_finished(SCOPE).await;
    tokio::spawn(async move {
        let _lease = lease;
        let result = async {
            let assets = downloader.clone();
            let repair = missing.clone();
            tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                for asset in repair {
                    if let Some(sha) = sha256(asset.url) {
                        audio_cpp_music::repair_managed_file(&assets.path_of(asset), asset, sha)?;
                    }
                }
                Ok(())
            })
            .await??;
            downloader.install_all(SCOPE, &missing).await
        }
        .await;
        if let Err(error) = result {
            downloader.fail(SCOPE, format!("{error:#}")).await;
        }
    });
    Ok(Json(serde_json::json!({"installing": true})))
}

fn payload(request: &mut CreateMusicJobRequest) -> anyhow::Result<Value> {
    ensure!(
        !request.style.trim().is_empty() || !request.lyrics.trim().is_empty(),
        "Enter a style or lyrics."
    );
    ensure!(
        request.adapters.is_empty()
            && request
                .abc
                .as_deref()
                .is_none_or(|abc| abc.trim().is_empty())
            && request.cover_of.is_none(),
        "This HOT-Step MiniMax profile accepts style and lyrics for generation."
    );
    let duration = request.duration_seconds.unwrap_or(30.0);
    let steps = request.steps.unwrap_or(15);
    let seed = request.seed.unwrap_or(424242);
    let cfg = request.cfg_scale.unwrap_or(1.7);
    ensure!(
        (1.0..=300.0).contains(&duration),
        "MiniMax Music3 duration must be between 1 and 300 seconds."
    );
    ensure!(
        (1..=200).contains(&steps),
        "MiniMax Music3 needs between 1 and 200 steps."
    );
    ensure!(
        seed >= 0 && cfg.is_finite() && cfg > 0.0,
        "Enter a nonnegative seed and positive guidance."
    );
    request.duration_seconds = Some(duration);
    request.steps = Some(steps);
    request.seed = Some(seed);
    request.cfg_scale = Some(cfg);
    Ok(
        serde_json::json!({"caption": request.style, "lyrics": request.lyrics, "duration": duration,
        "steps": steps, "seed": seed, "ar_seed": seed, "cfg_flow": cfg, "get_wav_bits": 16}),
    )
}

pub(super) async fn submit(
    state: AppState,
    request: CreateMusicJobRequest,
    attempt: u32,
) -> (StatusCode, Json<MusicJob>) {
    submit_from(state, request, attempt, None).await
}

pub(super) async fn replay(
    state: AppState,
    song: library::Song,
    options: ReplayMusicJobRequest,
    attempt: u32,
) -> Result<(StatusCode, Json<MusicJob>), (StatusCode, Json<ApiError>)> {
    if options.extend_seconds.is_some()
        || options.solver.is_some()
        || options.synth_batch_size.is_some()
    {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "The HOT-Step MiniMax profile replays one whole song with its saved request.".into(),
        ));
    }
    let mut request: CreateMusicJobRequest =
        serde_json::from_value(song.generation_settings.clone()).map_err(|error| {
            api_error(
                StatusCode::BAD_REQUEST,
                format!("Invalid saved HOT-Step MiniMax settings: {error}"),
            )
        })?;
    request.style = song.caption.clone();
    request.lyrics = song.lyrics.clone();
    request.title = options.title.or_else(|| Some(song.title.clone()));
    request.client_ref = options.client_ref;
    request.steps = options.steps.or(request.steps);
    request.seed = options.seed.or(request.seed);
    request.output_format = options.output_format.or(request.output_format);
    request.mp3_bitrate = options.mp3_bitrate.or(request.mp3_bitrate);
    Ok(submit_from(state, request, attempt, Some(&song)).await)
}

async fn submit_from(
    state: AppState,
    mut request: CreateMusicJobRequest,
    attempt: u32,
    source: Option<&library::Song>,
) -> (StatusCode, Json<MusicJob>) {
    let submitted = Instant::now();
    let reject = |request: &CreateMusicJobRequest, status, reason| {
        (
            status,
            Json(failed_request_job(request.clone(), ENGINE.into(), reason)),
        )
    };
    let downloader = &state.hotstep_cover;
    let Some(exe) =
        runtime::server(downloader).filter(|path| path.is_file() && missing(downloader).is_empty())
    else {
        return reject(
            &request,
            StatusCode::SERVICE_UNAVAILABLE,
            "Install the HOT-Step MiniMax Music3 profile in Create.".into(),
        );
    };
    let models = runtime::model_folder(downloader).join("minimax-music3");
    let body = match payload(&mut request) {
        Ok(value) => value,
        Err(error) => return reject(&request, StatusCode::BAD_REQUEST, error.to_string()),
    };
    let format = request
        .output_format
        .as_deref()
        .unwrap_or("flac")
        .to_owned();
    if let Err(error) = validate_output_format(&format) {
        return reject(&request, StatusCode::BAD_REQUEST, error);
    }
    let Ok(lease) = state.engine_use.clone().try_write_owned() else {
        return reject(
            &request,
            StatusCode::CONFLICT,
            "The local music engine is busy.".into(),
        );
    };
    if state.jobs.read().await.values().any(song_in_engine)
        || state.training.active_run().await.is_some()
    {
        return reject(
            &request,
            StatusCode::CONFLICT,
            "Wait for active generation or training to finish.".into(),
        );
    }
    if let Err(error) = state.ace_runtime.stop().await {
        return reject(
            &request,
            StatusCode::CONFLICT,
            format!("ACE-Step is busy: {error:#}"),
        );
    }
    state.minimax.stop().await;
    evict_kept_models(&state, "before HOT-Step MiniMax generation").await;
    let job = MusicJob {
        id: format!("hotstep-minimax-{}", uuid_suffix()),
        client_ref: request.client_ref.clone(),
        submitted_at: unix_millis(),
        engine_id: ENGINE.into(),
        title: Some(titled(&request, &state.adapters)),
        cover_prompt: request.cover_prompt.clone(),
        status: MusicJobStatus::Running,
        dispatch: MusicJobDispatch::Local,
        phase: MusicJobPhase::Running,
        style: request.style.clone(),
        lyrics: request.lyrics.clone(),
        duration_seconds: request.duration_seconds.unwrap_or_default(),
        generation_settings: serde_json::json!({"music_source": request.music_source,
            "family":"minimax_music3", "profile_id":PROFILE, "model":MODEL, "output_format":format,
            "runtime_revision":runtime::managed_revision(), "native_request":body,
            "duration_seconds":request.duration_seconds, "seed":request.seed, "steps":request.steps,
            "cfg_scale":request.cfg_scale, "fade_in":request.fade_in, "fade_out":request.fade_out,
            "mp3_bitrate":request.mp3_bitrate}),
        song: None,
        songs: vec![],
        message: "MiniMax Music3 is generating through HOT-Step.".into(),
        code: None,
        params: None,
        playlist_id: request.playlist_id.clone(),
        laid: None,
        card_memory_short: false,
        tracks_asked: Some(1),
        derived: source.map(|song| {
            derivation(
                song,
                "replay",
                serde_json::json!({"seed":request.seed,"steps":request.steps}),
            )
        }),
    };
    if let Err(error) =
        state
            .library
            .save_music_job(&stored_job(&job, serde_json::json!(request), attempt))
    {
        return reject(
            &request,
            StatusCode::INTERNAL_SERVER_ERROR,
            error.to_string(),
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
            let audio = render(&exe, &models, body, &stop).await?;
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
            "hotstep.minimax",
            submitted,
            outcome,
        )
        .await;
        state.audio_cpp_music_stops.write().await.remove(&job.id);
    });
    (StatusCode::ACCEPTED, Json(accepted))
}

async fn render(
    exe: &FsPath,
    models: &FsPath,
    body: Value,
    stop: &AtomicBool,
) -> anyhow::Result<Vec<u8>> {
    let (_child, http, url, props) =
        hotstep_runtime::start(exe, models, "/mm3/props", stop).await?;
    ensure!(
        props["synth_ready"] == true,
        "HOT-Step MiniMax Music3 weights are incompatible."
    );
    let accepted = hotstep_runtime::json_response(
        http.post(format!("{url}/mm3/synth"))
            .json(&body)
            .send()
            .await?,
    )
    .await?;
    let id = accepted["job_id"]
        .as_str()
        .context("HOT-Step did not return a MiniMax job ID")?;
    let deadline = Instant::now() + Duration::from_secs(1800);
    loop {
        ensure!(!stop.load(Ordering::SeqCst), "Stopped.");
        ensure!(
            Instant::now() < deadline,
            "HOT-Step MiniMax Music3 timed out."
        );
        let state = hotstep_runtime::json_response(
            http.get(format!("{url}/mm3/job"))
                .query(&[("id", id)])
                .send()
                .await?,
        )
        .await?;
        match state["status"].as_str() {
            Some("done" | "completed") => break,
            Some("failed" | "error" | "cancelled") => {
                anyhow::bail!("HOT-Step MiniMax Music3 failed: {state}")
            }
            _ => tokio::time::sleep(Duration::from_millis(300)).await,
        }
    }
    ensure!(!stop.load(Ordering::SeqCst), "Stopped.");
    let response = http
        .get(format!("{url}/mm3/take"))
        .query(&[("id", id), ("take", "0")])
        .send()
        .await?
        .error_for_status()?;
    Ok(response.bytes().await?.to_vec())
}
