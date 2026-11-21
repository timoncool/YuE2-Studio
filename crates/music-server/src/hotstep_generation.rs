//! Shared generation jobs for the published HOT-Step Windows CPU runtime.
use super::*;
use crate::{hotstep_cover_assets as runtime, hotstep_runtime};
use anyhow::{Context, ensure};
use std::path::Path as FsPath;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub(super) const PROFILE: &str = "hotstep-minimax-music3";
pub(super) const ACE_PROFILE: &str = "hotstep-ace-step-1.5";

#[derive(Clone, Copy)]
pub(super) enum Family {
    MiniMax,
    Ace,
}

impl Family {
    fn id(self) -> &'static str {
        match self {
            Self::MiniMax => PROFILE,
            Self::Ace => ACE_PROFILE,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::MiniMax => "MiniMax Music3",
            Self::Ace => "ACE-Step 1.5",
        }
    }
    fn family(self) -> &'static str {
        match self {
            Self::MiniMax => "minimax_music3",
            Self::Ace => "ace_step",
        }
    }
    fn folder(self) -> &'static str {
        match self {
            Self::MiniMax => "minimax-music3",
            Self::Ace => "ace-step-1.5",
        }
    }
    fn telemetry(self) -> &'static str {
        match self {
            Self::MiniMax => "hotstep.minimax",
            Self::Ace => "hotstep.ace",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::MiniMax => "MiniMax Music3 · HOT-Step · Compact (experimental)",
            Self::Ace => "ACE-Step 1.5 Turbo · HOT-Step · Q8_0 (experimental)",
        }
    }
    fn license(self) -> &'static str {
        match self {
            Self::MiniMax => {
                "https://huggingface.co/scragnog/MiniMax-Music3-GGUF/blob/67b48c3028544e4ad9c26946b7c7c840f355ec2b/LICENSE"
            }
            Self::Ace => {
                "https://huggingface.co/Serveurperso/ACE-Step-1.5-GGUF/tree/666ac70204440867d8c01ba4b119cc79c95b370a"
            }
        }
    }
    fn assets(self) -> &'static [(downloads::Asset, &'static str)] {
        match self {
            Self::MiniMax => &WEIGHTS,
            Self::Ace => &crate::hotstep_ace_assets::ASSETS,
        }
    }
    fn lock(self, state: &AppState) -> Arc<tokio::sync::Mutex<()>> {
        match self {
            Self::MiniMax => state.hotstep_minimax_install.clone(),
            Self::Ace => state.hotstep_ace_install.clone(),
        }
    }
    fn supported(self, downloader: &downloads::Downloader) -> bool {
        match self {
            Self::MiniMax => runtime::server(downloader).is_some(),
            Self::Ace => runtime::managed_revision().is_some(),
        }
    }
}
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
        .chain(crate::hotstep_ace_assets::ASSETS.iter())
        .find_map(|(asset, sha)| (asset.url == url).then_some(*sha))
}

fn missing(downloader: &downloads::Downloader, family: Family) -> Vec<&'static downloads::Asset> {
    runtime::missing_runtime(downloader)
        .into_iter()
        .chain(
            family
                .assets()
                .iter()
                .filter(|(asset, sha)| {
                    let path = downloader.path_of(asset);
                    !std::fs::metadata(&path)
                        .is_ok_and(|file| file.is_file() && file.len() == asset.bytes)
                        || (asset.marker.ends_with(".lua")
                            && !crate::install_record::sha256_of(&path)
                                .is_ok_and(|actual| actual == *sha))
                })
                .map(|(asset, _)| asset),
        )
        .collect()
}

pub(super) async fn profile(state: &AppState, engine_installed: bool, family: Family) -> Value {
    let downloader = &state.hotstep_cover;
    let missing = missing(downloader, family);
    let installing = family.lock(state).try_lock().is_err();
    serde_json::json!({
        "id": family.id(), "label": family.label(),
        "status": "experimental", "ready": family.supported(downloader) && engine_installed && missing.is_empty() && !installing,
        "installable": family.supported(downloader),
        "installing": installing,
        "download": downloader.active_for(family.id()).await,
        "missing": missing.iter().map(|asset| asset.label).collect::<Vec<_>>(),
        "missing_bytes": missing.iter().map(|asset| asset.bytes).sum::<u64>(),
        "license_url": family.license(),
    })
}

pub(super) async fn install(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    install_family(state, Family::MiniMax).await
}
pub(super) async fn install_ace(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    install_family(state, Family::Ace).await
}
async fn install_family(
    state: AppState,
    family: Family,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let scope = family.id();
    let downloader = state.hotstep_cover.clone();
    if !family.supported(&downloader)
        || (runtime::missing_runtime(&downloader).is_none()
            && !runtime::server(&downloader).is_some_and(|path| path.is_file()))
    {
        return Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "The managed HOT-Step profile is available on Windows.".into(),
        ));
    }
    let Ok(lease) = family.lock(&state).try_lock_owned() else {
        return Ok(Json(serde_json::json!({"installing": true})));
    };
    let missing = missing(&downloader, family);
    if missing.is_empty() {
        return Ok(Json(serde_json::json!({"ready": true})));
    }
    downloader.clear_finished(scope).await;
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
            downloader.install_all(scope, &missing).await
        }
        .await;
        if let Err(error) = result {
            downloader.fail(scope, format!("{error:#}")).await;
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

fn ace_payload(request: &mut CreateMusicJobRequest) -> anyhow::Result<Value> {
    ensure!(
        !request.style.trim().is_empty() || !request.lyrics.trim().is_empty(),
        "Enter a style or lyrics."
    );
    ensure!(
        request.ace.is_none()
            && request.adapters.is_empty()
            && request.cover_of.is_none()
            && request
                .abc
                .as_deref()
                .is_none_or(|abc| abc.trim().is_empty()),
        "HOT-Step ACE-Step Turbo accepts style and lyrics for text-to-music."
    );
    let duration = request.duration_seconds.unwrap_or(30.0);
    let seed = request.seed.unwrap_or(42);
    let steps = request.steps.unwrap_or(8);
    let cfg = request.cfg_scale.unwrap_or(1.0);
    ensure!(
        (1.0..=300.0).contains(&duration),
        "HOT-Step ACE-Step duration must be between 1 and 300 seconds."
    );
    ensure!(
        (1..=100).contains(&steps) && seed >= 0 && cfg > 0.0 && cfg.is_finite(),
        "Enter 1–100 steps, a nonnegative seed and positive guidance."
    );
    request.duration_seconds = Some(duration);
    request.seed = Some(seed);
    request.steps = Some(steps);
    request.cfg_scale = Some(cfg);
    let lyrics = if request.lyrics.trim().is_empty() {
        "[Instrumental]"
    } else {
        &request.lyrics
    };
    Ok(
        serde_json::json!({"task_type":"text2music","caption":request.style,"lyrics":lyrics,
        "duration":duration,"seed":seed,"inference_steps":steps,"guidance_scale":cfg,
        "shift":3,"solver":"euler","output_format":"wav16"}),
    )
}

pub(super) async fn submit(
    state: AppState,
    request: CreateMusicJobRequest,
    attempt: u32,
) -> (StatusCode, Json<MusicJob>) {
    submit_from(state, request, attempt, None, Family::MiniMax).await
}

pub(super) async fn submit_ace(
    state: AppState,
    request: CreateMusicJobRequest,
    attempt: u32,
) -> (StatusCode, Json<MusicJob>) {
    submit_from(state, request, attempt, None, Family::Ace).await
}

pub(super) async fn replay(
    state: AppState,
    song: library::Song,
    options: ReplayMusicJobRequest,
    attempt: u32,
    family: Family,
) -> Result<(StatusCode, Json<MusicJob>), (StatusCode, Json<ApiError>)> {
    if options.extend_seconds.is_some()
        || options.solver.is_some()
        || options.synth_batch_size.is_some()
    {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "This HOT-Step profile replays one whole song with its saved request.".into(),
        ));
    }
    let mut request: CreateMusicJobRequest =
        serde_json::from_value(song.generation_settings.clone()).map_err(|error| {
            api_error(
                StatusCode::BAD_REQUEST,
                format!("Invalid saved HOT-Step settings: {error}"),
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
    Ok(submit_from(state, request, attempt, Some(&song), family).await)
}

async fn submit_from(
    state: AppState,
    mut request: CreateMusicJobRequest,
    attempt: u32,
    source: Option<&library::Song>,
    family: Family,
) -> (StatusCode, Json<MusicJob>) {
    let submitted = Instant::now();
    let reject = |request: &CreateMusicJobRequest, status, reason| {
        (
            status,
            Json(failed_request_job(request.clone(), ENGINE.into(), reason)),
        )
    };
    let downloader = &state.hotstep_cover;
    let Some(exe) = runtime::server(downloader).filter(|path| {
        family.supported(downloader) && path.is_file() && missing(downloader, family).is_empty()
    }) else {
        return reject(
            &request,
            StatusCode::SERVICE_UNAVAILABLE,
            format!("Install the HOT-Step {} profile in Create.", family.name()),
        );
    };
    let models = runtime::model_folder(downloader).join(family.folder());
    let native = match family {
        Family::MiniMax => payload(&mut request),
        Family::Ace => ace_payload(&mut request),
    };
    let body = match native {
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
    evict_kept_models(&state, "before HOT-Step generation").await;
    let job = MusicJob {
        id: format!("{}-{}", family.id(), uuid_suffix()),
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
            "family":family.family(), "profile_id":family.id(), "model":family.name(), "output_format":format,
            "runtime_revision":runtime::managed_revision(), "native_request":body,
            "duration_seconds":request.duration_seconds, "seed":request.seed, "steps":request.steps,
            "cfg_scale":request.cfg_scale, "fade_in":request.fade_in, "fade_out":request.fade_out,
            "mp3_bitrate":request.mp3_bitrate}),
        song: None,
        songs: vec![],
        message: format!("{} is generating through HOT-Step.", family.name()),
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
            let audio = render(&exe, &models, body, &stop, family).await?;
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
            family.name(),
            family.id(),
            family.telemetry(),
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
    family: Family,
) -> anyhow::Result<Vec<u8>> {
    let prefix = match family {
        Family::MiniMax => "/mm3",
        Family::Ace => "",
    };
    let (_child, http, url, props) =
        hotstep_runtime::start(exe, models, &format!("{prefix}/props"), stop).await?;
    if matches!(family, Family::MiniMax) {
        ensure!(
            props["synth_ready"] == true,
            "HOT-Step MiniMax Music3 weights are incompatible."
        );
    }
    let accepted = hotstep_runtime::json_response(
        http.post(format!("{url}{prefix}/synth"))
            .json(&body)
            .send()
            .await?,
    )
    .await?;
    let id = accepted["id"]
        .as_str()
        .or_else(|| accepted["job_id"].as_str())
        .filter(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .context("HOT-Step did not return a valid job ID")?;
    let deadline = Instant::now() + Duration::from_secs(1800);
    loop {
        ensure!(!stop.load(Ordering::SeqCst), "Stopped.");
        ensure!(Instant::now() < deadline, "HOT-Step generation timed out.");
        let state = hotstep_runtime::json_response(
            http.get(format!("{url}{prefix}/job"))
                .query(&[("id", id)])
                .send()
                .await?,
        )
        .await?;
        match state["status"].as_str() {
            Some("done" | "completed") => break,
            Some("failed" | "error" | "cancelled") => {
                anyhow::bail!("HOT-Step generation failed: {state}")
            }
            _ => tokio::time::sleep(Duration::from_millis(300)).await,
        }
    }
    ensure!(!stop.load(Ordering::SeqCst), "Stopped.");
    let (result, query) = match family {
        Family::MiniMax => ("/mm3/take", [("id", id), ("take", "0")]),
        Family::Ace => ("/job", [("id", id), ("result", "1")]),
    };
    let response = http
        .get(format!("{url}{result}"))
        .query(&query)
        .send()
        .await?
        .error_for_status()?;
    Ok(response.bytes().await?.to_vec())
}
