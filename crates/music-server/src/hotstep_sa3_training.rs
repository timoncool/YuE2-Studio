use super::*;
use anyhow::{Context, ensure};
use music_engine::yue_train::TrainingStage;
use std::{ffi::OsString, io::Read, path::Path as FsPath};

use crate::{
    hotstep_cover_assets, hotstep_sa3_tokenizer::Sa3Tokenizer, hotstep_sa3_train_assets as assets,
};

const SCOPE: &str = "hotstep-sa3-training";
pub(super) const STAGE: &str = "sa3-train";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Model {
    Sa3,
    Heartmula,
}

impl Model {
    fn scope(self) -> &'static str {
        match self {
            Self::Sa3 => SCOPE,
            Self::Heartmula => "hotstep-heartmula-training",
        }
    }

    pub(super) fn profile(self) -> &'static str {
        match self {
            Self::Sa3 => hotstep_sa3::PROFILE,
            Self::Heartmula => hotstep_heartmula::PROFILE,
        }
    }

    fn missing(self, downloader: &downloads::Downloader) -> Vec<&'static downloads::Asset> {
        match self {
            Self::Sa3 => assets::missing(downloader),
            Self::Heartmula => {
                let mut missing = hotstep_heartmula_assets::missing(downloader, true);
                missing.extend(assets::missing_runtime(downloader));
                missing
            }
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct Start {
    dataset_id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    settings: Settings,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub(super) steps: u32,
    pub(super) rank: u32,
    pub(super) alpha: f64,
    pub(super) learning_rate: f64,
    pub(super) duration_seconds: f64,
    pub(super) seed: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            steps: 200,
            rank: 8,
            alpha: 8.0,
            learning_rate: 0.0001,
            duration_seconds: 30.0,
            seed: 42,
        }
    }
}

impl Settings {
    fn check(&self) -> anyhow::Result<()> {
        ensure!(
            (1..=100_000).contains(&self.steps),
            "Choose between 1 and 100,000 training steps."
        );
        ensure!(
            (1..=128).contains(&self.rank),
            "Choose an adapter rank between 1 and 128."
        );
        ensure!(
            (0.01..=512.0).contains(&self.alpha),
            "Choose adapter alpha between 0.01 and 512."
        );
        ensure!(
            (0.000000001..=0.1).contains(&self.learning_rate),
            "Invalid Stable Audio 3 learning rate."
        );
        ensure!(
            (1.0..=384.0).contains(&self.duration_seconds),
            "Choose a crop duration between 1 and 384 seconds."
        );
        Ok(())
    }
}

fn card_available() -> bool {
    hardware::hardware().card_cuda.is_some()
}

pub(super) async fn status(State(state): State<AppState>) -> Json<Value> {
    training_status(state, Model::Sa3).await
}
pub(super) async fn heartmula_status(State(state): State<AppState>) -> Json<Value> {
    training_status(state, Model::Heartmula).await
}
async fn training_status(state: AppState, model: Model) -> Json<Value> {
    let downloader = &state.hotstep_cover;
    let can_install = assets::trainer(downloader).is_some();
    let missing = model.missing(downloader);
    let trainer = assets::trainer(downloader).is_some_and(|path| path.is_file());
    let installing = state.hotstep_sa3_train_install.try_lock().is_err();
    Json(serde_json::json!({
        "model":model.profile(), "cuda":card_available(), "installable":can_install,
        "ready":trainer && missing.is_empty() && !installing, "installing":installing,
        "missing":missing.iter().map(|item| item.label).collect::<Vec<_>>(),
        "missing_bytes":missing.iter().map(|item| item.bytes).sum::<u64>(),
        "download":downloader.active_for(model.scope()).await,
    }))
}

pub(super) async fn install(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    install_model(state, Model::Sa3).await
}

pub(super) async fn heartmula_install(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    install_model(state, Model::Heartmula).await
}

async fn install_model(
    state: AppState,
    model: Model,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let scope = model.scope();
    let downloader = state.hotstep_cover.clone();
    if !card_available() || assets::trainer(&downloader).is_none() {
        return Err(api_error(StatusCode::SERVICE_UNAVAILABLE, "HOT-Step training requires a CUDA trainer and an NVIDIA CUDA card.".into()));
    }
    let Ok(lease) = state.hotstep_sa3_train_install.try_lock_owned() else {
        return Ok(Json(serde_json::json!({"started":false,"installing":true})));
    };
    let missing = model.missing(&downloader);
    install_assets(downloader, lease, scope, missing).await
}

pub(super) async fn install_assets(
    downloader: Arc<downloads::Downloader>,
    lease: tokio::sync::OwnedMutexGuard<()>,
    scope: &'static str,
    missing: Vec<&'static downloads::Asset>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    if missing.is_empty() {
        return Ok(Json(
            serde_json::json!({"started":false,"installing":false,"ready":true}),
        ));
    }
    downloader.clear_finished(scope).await;
    tokio::spawn(async move {
        let _lease = lease;
        let outcome = async {
            let repair = downloader.clone();
            let repair_missing = missing.clone();
            tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                for item in repair_missing
                    .into_iter()
                    .filter(|item| item.kind == downloads::AssetKind::Model)
                {
                    let sha = assets::sha256(item.url)
                        .or_else(|| hotstep_sa3_assets::sha256(item.url))
                        .or_else(|| hotstep_heartmula_assets::sha256(item.url))
                        .or_else(|| hotstep_cover_assets::sha256(item.url))
                        .context("No pinned HOT-Step training checksum")?;
                    audio_cpp_music::repair_managed_file(&repair.path_of(item), item, sha)?;
                }
                Ok(())
            })
            .await??;
            downloader.install_all(scope, &missing).await?;
            Ok::<(), anyhow::Error>(())
        }
        .await;
        if let Err(error) = outcome {
            downloader.fail(scope, format!("{error:#}")).await;
        }
    });
    Ok(Json(serde_json::json!({"started":true,"installing":true})))
}

pub(super) async fn start(
    State(state): State<AppState>,
    Json(input): Json<Start>,
) -> Result<Json<training::Run>, (StatusCode, Json<ApiError>)> {
    start_model(state, input, Model::Sa3).await
}

pub(super) async fn heartmula_start(
    State(state): State<AppState>,
    Json(input): Json<Start>,
) -> Result<Json<training::Run>, (StatusCode, Json<ApiError>)> {
    start_model(state, input, Model::Heartmula).await
}

async fn start_model(
    state: AppState,
    input: Start,
    model: Model,
) -> Result<Json<training::Run>, (StatusCode, Json<ApiError>)> {
    input
        .settings
        .check()
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    let outcome = async {
        ensure!(
            card_available(),
            "HOT-Step training needs an NVIDIA CUDA card."
        );
        let trainer = assets::trainer(&state.hotstep_cover)
            .filter(|path| path.is_file())
            .context("Install the HOT-Step training pack.")?;
        ensure!(
            model.missing(&state.hotstep_cover).is_empty(),
            "Finish installing the selected HOT-Step training assets."
        );
        let _exclusive = state
            .engine_use
            .try_write()
            .context("The local music engine is busy.")?;
        no_song_rendering(&state)
            .await
            .map_err(anyhow::Error::msg)?;
        ensure!(
            !state
                .prepare
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_ref()
                .is_some_and(|job| !job.finished),
            "The training dataset is still being prepared."
        );
        ensure!(
            state.training.active_run().await.is_none(),
            "A training run is already going."
        );
        state.ace_runtime.stop().await?;
        state.minimax.stop().await;
        evict_kept_models(&state, "before HOT-Step training").await;
        state
            .training
            .start_hotstep(
                model,
                trainer,
                hotstep_cover_assets::model_folder(&state.hotstep_cover),
                &input.dataset_id,
                &input.name,
                input.settings,
                card_hooks(&state).await,
            )
            .await
    }
    .await;
    outcome
        .map(Json)
        .map_err(|error| api_error(StatusCode::CONFLICT, format!("{error:#}")))
}

pub(super) fn dataset_manifest(
    models: &FsPath,
    run_dir: &FsPath,
    input_audio: &FsPath,
    dataset: &training::Dataset,
    stop: &std::sync::atomic::AtomicBool,
) -> anyhow::Result<()> {
    let tokenizer = Sa3Tokenizer::load(models)?;
    let clips = run_dir.join("clips");
    std::fs::create_dir_all(&clips)?;
    let mut samples = Vec::with_capacity(dataset.items.len());
    for (index, item) in dataset.items.iter().enumerate() {
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }
        let file = FsPath::new(&item.file);
        ensure!(
            file.components().count() == 1
                && matches!(
                    file.components().next(),
                    Some(std::path::Component::Normal(_))
                ),
            "Invalid Stable Audio 3 sample filename"
        );
        let audio = audio_pcm::decode_stereo_44k(&input_audio.join(file))
            .with_context(|| format!("Decoding {}", item.title))?;
        let target = clips.join(format!("{index}.wav"));
        audio_pcm::write_wav24_interleaved(&target, 44_100, 2, &audio)?;
        let prompt = item.style.trim();
        let (ids, count) = tokenizer.encode(prompt)?;
        samples.push(serde_json::json!({"audio_path":target,"token_ids":ids,"n_tokens":count,"prompt":prompt}));
    }
    std::fs::write(
        run_dir.join("sa3-samples.json"),
        serde_json::to_vec(&serde_json::json!({"samples":samples}))?,
    )?;
    Ok(())
}

pub(super) fn stage(models: &FsPath, run: &FsPath, settings: &Settings) -> TrainingStage {
    let args = vec![
        STAGE.into(),
        "--models".into(),
        models.as_os_str().to_owned(),
        "--base-dit".into(),
        models.join(assets::BASE).into_os_string(),
        "--inference-dit".into(),
        models.join("sa3-dit-BF16.gguf").into_os_string(),
        "--manifest".into(),
        run.join("sa3-samples.json").into_os_string(),
        "--out".into(),
        run.join("sa3-output").into_os_string(),
        "--steps".into(),
        OsString::from(settings.steps.to_string()),
        "--rank".into(),
        OsString::from(settings.rank.to_string()),
        "--alpha".into(),
        OsString::from(settings.alpha.to_string()),
        "--lr".into(),
        OsString::from(settings.learning_rate.to_string()),
        "--duration".into(),
        OsString::from(settings.duration_seconds.to_string()),
        "--seed".into(),
        OsString::from(settings.seed.to_string()),
        "--bwd".into(),
        "mm".into(),
        "--jsonl".into(),
    ];
    TrainingStage { id: STAGE, args }
}

pub(super) fn progress(line: &str) -> Option<training::TrainingStepRecord> {
    if !line.starts_with('{') {
        return None;
    }
    let data: Value = serde_json::from_str(line).ok()?;
    (data["type"] == "step" && data.get("lossEma").is_some()).then_some(())?;
    Some(training::TrainingStepRecord {
        step: u32::try_from(data["step"].as_u64()?).ok()?,
        loss: data["loss"].as_f64()?,
        ar_kl: None,
        step_ms: data["ms"].as_f64(),
    })
}

pub(super) fn publish(models: &FsPath, run: &FsPath, id: &str) -> anyhow::Result<String> {
    publish_file(
        run.join("sa3-output/adapter.gguf"),
        models.join("sa3-adapters"),
        format!("sa3-{id}"),
    )
}

fn publish_file(path: PathBuf, adapters: PathBuf, name: String) -> anyhow::Result<String> {
    let mut source =
        std::fs::File::open(path).context("The HOT-Step trainer did not export an adapter.")?;
    let mut magic = [0; 4];
    source.read_exact(&mut magic)?;
    ensure!(
        &magic == b"GGUF",
        "The HOT-Step trainer exported an invalid GGUF adapter."
    );
    std::fs::create_dir_all(&adapters)?;
    let mut target = tempfile::NamedTempFile::new_in(&adapters)?;
    std::io::Write::write_all(&mut target, &magic)?;
    std::io::copy(&mut source, &mut target)?;
    target.as_file().sync_all()?;
    target.persist_noclobber(adapters.join(format!("{name}.gguf")))?;
    Ok(name)
}

pub(super) const HEART_PREPARE: &str = "heartmula-prepare";
pub(super) const HEART_STAGE: &str = "heartmula-train";

pub(super) fn heart_manifest(
    run: &FsPath,
    input_audio: &FsPath,
    dataset: &training::Dataset,
) -> anyhow::Result<()> {
    let mut samples = Vec::with_capacity(dataset.items.len());
    for item in &dataset.items {
        let file = FsPath::new(&item.file);
        ensure!(
            file.components().count() == 1
                && matches!(
                    file.components().next(),
                    Some(std::path::Component::Normal(_))
                ),
            "Invalid HeartMuLa sample filename"
        );
        let lyrics = if item.instrumental {
            ""
        } else {
            item.lyrics.trim()
        };
        let audio = input_audio.join(file);
        ensure!(audio.is_file(), "Missing HeartMuLa sample: {}", item.title);
        samples.push(
            serde_json::json!({"id":item.id,"audio":audio,"tags":item.style,"lyrics":lyrics}),
        );
    }
    std::fs::write(
        run.join("heartmula-samples.json"),
        serde_json::to_vec(&serde_json::json!({"samples":samples}))?,
    )?;
    Ok(())
}

pub(super) fn heart_stages(
    models: &FsPath,
    run: &FsPath,
    settings: &Settings,
) -> Vec<TrainingStage> {
    let prepared = run.join("heartmula-prepared").into_os_string();
    vec![
        TrainingStage {
            id: HEART_PREPARE,
            args: vec![
                HEART_PREPARE.into(),
                "--dataset".into(),
                run.join("heartmula-samples.json").into_os_string(),
                "--out".into(),
                prepared.clone(),
                "--models".into(),
                models.as_os_str().to_owned(),
                "--encoder".into(),
                models.as_os_str().to_owned(),
            ],
        },
        TrainingStage {
            id: HEART_STAGE,
            args: vec![
                HEART_STAGE.into(),
                "--data".into(),
                prepared,
                "--out".into(),
                run.join("heartmula-output").into_os_string(),
                "--models".into(),
                models.as_os_str().to_owned(),
                "--variant".into(),
                "3B".into(),
                "--type".into(),
                "Q8_0".into(),
                "--steps".into(),
                settings.steps.to_string().into(),
                "--rank".into(),
                settings.rank.to_string().into(),
                "--alpha".into(),
                settings.alpha.to_string().into(),
                "--lr".into(),
                settings.learning_rate.to_string().into(),
                "--grad-accum".into(),
                "1".into(),
                "--seed".into(),
                settings.seed.to_string().into(),
                "--save-every".into(),
                settings.steps.to_string().into(),
                "--crop-frames".into(),
                ((settings.duration_seconds * 12.5).round() as u32)
                    .to_string()
                    .into(),
            ],
        },
    ]
}

pub(super) fn heart_progress(line: &str) -> Option<training::TrainingStepRecord> {
    let data: Value = serde_json::from_str(line.strip_prefix("HMTRAIN ")?).ok()?;
    (data["event"] == "step").then_some(())?;
    Some(training::TrainingStepRecord {
        step: u32::try_from(data["step"].as_u64()?).ok()?,
        loss: data["loss"].as_f64()?,
        ar_kl: None,
        step_ms: data["ms_step"].as_f64(),
    })
}

pub(super) fn publish_heart(
    models: &FsPath,
    run: &FsPath,
    id: &str,
    step: u32,
) -> anyhow::Result<String> {
    publish_file(
        run.join(format!("heartmula-output/adapters/step-{step:06}.gguf")),
        models.join(hotstep_heartmula::ADAPTERS),
        format!("heartmula-{id}"),
    )
}
