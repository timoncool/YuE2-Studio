//! Stable Audio 3 Medium text-to-audio using the HOT-Step native server.
use super::*;
use anyhow::{Context, ensure};
use std::path::Path as FsPath;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::{hotstep_cover_assets, hotstep_runtime, hotstep_sa3_assets, hotstep_sa3_tokenizer};

pub(super) const PROFILE: &str = "stable-audio-3-medium";
const MODEL: &str = "Stable Audio 3 Medium";
const INSTALL_SCOPE: &str = "hotstep-stable-audio-3";

pub(super) async fn profile(state: &AppState, engine_installed: bool) -> Value {
    let downloader = &state.hotstep_cover;
    let weights = hotstep_sa3_assets::missing(downloader);
    let models = hotstep_cover_assets::model_folder(downloader);
    let mut adapters: Vec<_> = std::fs::read_dir(models.join("sa3-adapters"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_stem()?.to_str()?;
            (path.extension()?.to_str()? == "gguf" && valid_adapter_name(name) && path.is_file())
                .then(|| name.to_owned())
        })
        .collect();
    adapters.sort_unstable();
    let mut missing: Vec<_> = weights.iter().map(|asset| asset.label).collect();
    if !engine_installed {
        missing.push("HOT-Step server");
    }
    let runtime = hotstep_cover_assets::missing_runtime(downloader);
    let installing = state.hotstep_sa3_install.try_lock().is_err();
    serde_json::json!({
        "id": PROFILE, "label": "Stable Audio 3 Medium · HOT-Step · BF16",
        "ready": missing.is_empty() && !installing, "missing": missing, "installing": installing,
        "download": downloader.active_for(INSTALL_SCOPE).await,
        "missing_bytes": weights.iter().map(|asset| asset.bytes).sum::<u64>() + runtime.map_or(0, |asset| asset.bytes),
        "installable": engine_installed || runtime.is_some(), "adapters": adapters,
    })
}

pub(super) async fn install(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let downloader = &state.hotstep_cover;
    let Ok(lease) = state.hotstep_sa3_install.try_lock_owned() else {
        return Ok(Json(
            serde_json::json!({"started": false, "installing": true}),
        ));
    };
    let runtime = hotstep_cover_assets::missing_runtime(downloader);
    if runtime.is_none()
        && !hotstep_cover_assets::server(downloader).is_some_and(|path| path.is_file())
    {
        return Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Configure a HOT-Step server before downloading the weights.".into(),
        ));
    }
    let mut missing: Vec<_> = runtime.into_iter().collect();
    let weights = hotstep_sa3_assets::missing(downloader);
    missing.extend(weights.iter().copied());
    if missing.is_empty() {
        return Ok(Json(
            serde_json::json!({"started": false, "installing": false, "ready": true}),
        ));
    }
    let downloader = downloader.clone();
    downloader.clear_finished(INSTALL_SCOPE).await;
    tokio::spawn(async move {
        let _lease = lease;
        let result = async {
            let assets = downloader.clone();
            tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                for asset in weights {
                    let sha = hotstep_sa3_assets::sha256(asset.url)
                        .context("Missing Stable Audio 3 checksum")?;
                    audio_cpp_music::repair_managed_file(&assets.path_of(asset), asset, sha)?;
                }
                Ok(())
            })
            .await??;
            downloader.install_all(INSTALL_SCOPE, &missing).await?;
            ensure!(
                hotstep_sa3_assets::missing(&downloader).is_empty(),
                "HOT-Step Stable Audio 3 installation is incomplete."
            );
            Ok::<(), anyhow::Error>(())
        }
        .await;
        if let Err(error) = result {
            downloader.fail(INSTALL_SCOPE, format!("{error:#}")).await;
        }
    });
    Ok(Json(
        serde_json::json!({"started": true, "installing": true}),
    ))
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct Options {
    sampler: Option<Sampler>,
    adapter: Option<String>,
    adapter_scale: Option<f32>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Sampler {
    #[default]
    Pingpong,
    Euler,
}

impl Sampler {
    fn native(self) -> &'static str {
        match self {
            Self::Pingpong => "pingpong",
            Self::Euler => "euler",
        }
    }
}

struct Inputs {
    duration: f64,
    steps: u32,
    seed: u32,
    guidance: f64,
    sampler: Sampler,
    adapter: Option<(String, f32)>,
}

impl Inputs {
    fn from_request(request: &CreateMusicJobRequest, models: &FsPath) -> anyhow::Result<Self> {
        ensure!(
            !request.style.trim().is_empty(),
            "Describe the instrumental you want to generate."
        );
        ensure!(
            request.lyrics.trim().is_empty(),
            "HOT-Step Stable Audio 3 accepts a description and generates instrumental audio."
        );
        let duration = request.duration_seconds.unwrap_or(30.0);
        let steps = request.steps.unwrap_or(8);
        let guidance = request.cfg_scale.unwrap_or(1.0);
        ensure!(
            (1.0..=600.0).contains(&duration),
            "Stable Audio 3 duration must be between 1 and 600 seconds."
        );
        ensure!(
            (1..=64).contains(&steps),
            "Stable Audio 3 sampling steps must be between 1 and 64."
        );
        ensure!(
            (1.0..=20.0).contains(&guidance),
            "Stable Audio 3 guidance must be between 1 and 20."
        );
        let seed = match request.seed {
            None | Some(-1) => ace::random_seed() as u32,
            Some(seed) => u32::try_from(seed).context("Invalid Stable Audio 3 seed")?,
        };
        let options = request.hotstep_sa3.clone().unwrap_or_default();
        let adapter = match options.adapter {
            Some(name) => {
                ensure!(
                    valid_adapter_name(&name),
                    "Invalid Stable Audio 3 adapter name."
                );
                ensure!(
                    models
                        .join("sa3-adapters")
                        .join(format!("{name}.gguf"))
                        .is_file(),
                    "The selected Stable Audio 3 adapter is missing."
                );
                let scale = options.adapter_scale.unwrap_or(1.0);
                ensure!(
                    (0.0..=2.0).contains(&scale),
                    "Stable Audio 3 adapter scale must be between 0 and 2."
                );
                Some((name, scale))
            }
            None => None,
        };
        Ok(Self {
            duration,
            steps,
            seed,
            guidance,
            sampler: options.sampler.unwrap_or_default(),
            adapter,
        })
    }

    fn query(&self, tokens: String, count: usize) -> Vec<(&'static str, String)> {
        let mut query = vec![
            ("tokens", tokens),
            ("n_tokens", count.to_string()),
            ("duration", self.duration.to_string()),
            ("steps", self.steps.to_string()),
            ("seed", self.seed.to_string()),
            ("sampler", self.sampler.native().into()),
            ("guidance_scale", self.guidance.to_string()),
            ("out_fmt", "f32".into()),
        ];
        if let Some((name, scale)) = &self.adapter {
            query.push(("adapters", format!("{name}:{scale}")));
        }
        query
    }
}

fn valid_adapter_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub(super) async fn submit(
    state: AppState,
    request: CreateMusicJobRequest,
    attempt: u32,
) -> (StatusCode, Json<MusicJob>) {
    submit_with_source(state, request, attempt, None).await
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
            "Stable Audio 3 replay supports seed, steps and output overrides.".into(),
        ));
    }
    let mut request: CreateMusicJobRequest =
        serde_json::from_value(song.generation_settings.clone()).map_err(|error| {
            api_error(
                StatusCode::BAD_REQUEST,
                format!("Invalid saved Stable Audio 3 settings: {error}"),
            )
        })?;
    request.style = song.caption.clone();
    request.title = options.title.or_else(|| Some(song.title.clone()));
    request.client_ref = options.client_ref;
    request.steps = options.steps.or(request.steps);
    request.seed = options.seed.or(request.seed);
    request.output_format = options.output_format.or(request.output_format);
    request.mp3_bitrate = options.mp3_bitrate.or(request.mp3_bitrate);
    Ok(submit_with_source(state, request, attempt, Some(&song)).await)
}

async fn submit_with_source(
    state: AppState,
    mut request: CreateMusicJobRequest,
    attempt: u32,
    source: Option<&library::Song>,
) -> (StatusCode, Json<MusicJob>) {
    let engine = hotstep_cover::ENGINE;
    let reject = |status, reason: String| {
        (
            status,
            Json(failed_request_job(request.clone(), engine.into(), reason)),
        )
    };
    let downloader = &state.hotstep_cover;
    let models = hotstep_cover_assets::model_folder(downloader);
    let exe = hotstep_cover_assets::server(downloader).filter(|path| path.is_file());
    let Some(exe) = exe.filter(|_| hotstep_sa3_assets::missing(downloader).is_empty()) else {
        return reject(
            StatusCode::SERVICE_UNAVAILABLE,
            "Install the HOT-Step runtime and Stable Audio 3 Medium in Create.".into(),
        );
    };
    let inputs = match Inputs::from_request(&request, &models) {
        Ok(inputs) => inputs,
        Err(err) => return reject(StatusCode::BAD_REQUEST, format!("{err:#}")),
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
    evict_kept_models(&state, "before HOT-Step Stable Audio 3 generation").await;

    let options = Options {
        sampler: Some(inputs.sampler),
        adapter: inputs.adapter.as_ref().map(|(name, _)| name.clone()),
        adapter_scale: inputs.adapter.as_ref().map(|(_, scale)| *scale),
    };
    let generation_settings = serde_json::json!({
        "music_source": request.music_source, "family": "stable_audio", "profile_id": PROFILE,
        "model": MODEL, "quantization": "BF16", "runtime_revision": hotstep_cover_assets::managed_revision(),
        "hotstep_sa3": options, "duration_seconds": inputs.duration, "steps": inputs.steps,
        "seed": inputs.seed, "cfg_scale": inputs.guidance, "output_format": format,
        "fade_in": request.fade_in, "fade_out": request.fade_out, "mp3_bitrate": request.mp3_bitrate,
    });
    let job = MusicJob {
        derived: source.map(|song| {
            derivation(
                song,
                "replay",
                serde_json::json!({"seed": inputs.seed, "steps": inputs.steps}),
            )
        }),
        id: format!("hotstep-sa3-{}", uuid_suffix()),
        client_ref: request.client_ref.clone(),
        submitted_at: unix_millis(),
        engine_id: engine.into(),
        cover_prompt: request.cover_prompt.clone(),
        title: Some(titled(&request, &state.adapters)),
        status: MusicJobStatus::Running,
        dispatch: MusicJobDispatch::Local,
        phase: MusicJobPhase::Running,
        style: request.style.clone(),
        lyrics: String::new(),
        duration_seconds: inputs.duration,
        generation_settings,
        song: None,
        songs: vec![],
        message: "Stable Audio 3 is generating through HOT-Step.".into(),
        code: None,
        params: None,
        playlist_id: request.playlist_id.clone(),
        laid: None,
        card_memory_short: false,
        tracks_asked: Some(1),
    };
    request.duration_seconds = Some(inputs.duration);
    request.steps = Some(inputs.steps);
    request.seed = Some(i64::from(inputs.seed));
    request.cfg_scale = Some(inputs.guidance);
    request.hotstep_sa3 = Some(options);
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
        let result = async {
            let weights = models.clone();
            let prompt = request.style;
            let (tokens, count) = tokio::task::spawn_blocking(move || hotstep_sa3_tokenizer::encode(&weights, &prompt)).await??;
            let (_child, http, url, props) = hotstep_runtime::start(&exe, &models, "/sa3/props", &stop).await?;
            ensure!(props["synth_ready"] == true, "HOT-Step cannot load the Stable Audio 3 generation pack.");
            let synthesis = async {
                let reply = http.post(format!("{url}/sa3-synth"))
                    .query(&inputs.query(tokens, count)).timeout(Duration::from_secs(900)).send().await?;
                if !reply.status().is_success() {
                    hotstep_runtime::json_response(reply).await?;
                    anyhow::bail!("HOT-Step Stable Audio 3 generation failed.");
                }
                let audio = reply.bytes().await?;
                ensure!(audio.starts_with(b"RIFF"), "HOT-Step returned no WAV audio.");
                Ok(audio.to_vec())
            };
            let wav = tokio::select! {
                result = synthesis => result?,
                _ = async {
                    while !stop.load(Ordering::SeqCst) { tokio::time::sleep(Duration::from_millis(200)).await; }
                } => anyhow::bail!("Stopped."),
            };
            let fades = (request.fade_in.unwrap_or_default(), request.fade_out.unwrap_or_default());
            tokio::task::spawn_blocking(move || native_music_jobs::shape_output(wav, &format, request.mp3_bitrate, fades)).await?
        }.await;
        audio_cpp_music::complete_local(&state, &job.id, MODEL, PROFILE, result).await;
        state.audio_cpp_music_stops.write().await.remove(&job.id);
    });
    (StatusCode::ACCEPTED, Json(accepted))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_the_stock_request_and_keeps_the_selected_adapter() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("sa3-adapters")).unwrap();
        std::fs::write(root.path().join("sa3-adapters/reference.gguf"), b"GGUF").unwrap();
        let request: CreateMusicJobRequest = serde_json::from_value(serde_json::json!({
            "style": "Instrumental drums", "seed": 42, "duration_seconds": 4.5, "steps": 16,
            "cfg_scale": 2, "hotstep_sa3": {"sampler":"euler", "adapter":"reference", "adapter_scale":0.5},
        })).unwrap();
        let inputs = Inputs::from_request(&request, root.path()).unwrap();
        let query = inputs.query("1,2,3".to_owned(), 3);
        assert!(query.contains(&("seed", "42".to_owned())));
        assert!(query.contains(&("duration", "4.5".to_owned())));
        assert!(query.contains(&("sampler", "euler".to_owned())));
        assert!(query.contains(&("adapters", "reference:0.5".to_owned())));
        assert!(!valid_adapter_name("../reference"));
    }
}
