use super::*;
use anyhow::{Context, ensure};
use music_engine::yue_train::TrainingStage;
use std::{ffi::OsString, io::Read, path::Path as FsPath};

use crate::{
    hotstep_cover_assets, hotstep_sa3_tokenizer::Sa3Tokenizer, hotstep_sa3_train_assets as assets,
};

const SCOPE: &str = "hotstep-sa3-training";
pub(super) const STAGE: &str = "sa3-train";

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
    let downloader = &state.hotstep_cover;
    let can_install = assets::trainer(downloader).is_some();
    let missing = assets::missing(downloader);
    let trainer = assets::trainer(downloader).is_some_and(|path| path.is_file());
    let installing = state.hotstep_sa3_train_install.try_lock().is_err();
    Json(serde_json::json!({
        "model":hotstep_sa3::PROFILE, "cuda":card_available(), "installable":can_install,
        "ready":trainer && missing.is_empty() && !installing, "installing":installing,
        "missing":missing.iter().map(|item| item.label).collect::<Vec<_>>(),
        "missing_bytes":missing.iter().map(|item| item.bytes).sum::<u64>(),
        "download":downloader.active_for(SCOPE).await,
    }))
}

pub(super) async fn install(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let downloader = state.hotstep_cover.clone();
    if !card_available() || assets::trainer(&downloader).is_none() {
        return Err(api_error(StatusCode::SERVICE_UNAVAILABLE, "Stable Audio 3 training requires an installed HOT-Step CUDA trainer and an NVIDIA CUDA card.".into()));
    }
    let Ok(lease) = state.hotstep_sa3_train_install.try_lock_owned() else {
        return Ok(Json(serde_json::json!({"started":false,"installing":true})));
    };
    let missing = assets::missing(&downloader);
    if missing.is_empty() {
        return Ok(Json(
            serde_json::json!({"started":false,"installing":false,"ready":true}),
        ));
    }
    downloader.clear_finished(SCOPE).await;
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
                        .context("No pinned Stable Audio 3 checksum")?;
                    audio_cpp_music::repair_managed_file(&repair.path_of(item), item, sha)?;
                }
                Ok(())
            })
            .await??;
            downloader.install_all(SCOPE, &missing).await?;
            Ok::<(), anyhow::Error>(())
        }
        .await;
        if let Err(error) = outcome {
            downloader.fail(SCOPE, format!("{error:#}")).await;
        }
    });
    Ok(Json(serde_json::json!({"started":true,"installing":true})))
}

pub(super) async fn start(
    State(state): State<AppState>,
    Json(input): Json<Start>,
) -> Result<Json<training::Run>, (StatusCode, Json<ApiError>)> {
    input
        .settings
        .check()
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    let outcome = async {
        ensure!(
            card_available(),
            "Stable Audio 3 training needs an NVIDIA CUDA card."
        );
        let trainer = assets::trainer(&state.hotstep_cover)
            .filter(|path| path.is_file())
            .context("Install the HOT-Step Stable Audio 3 training pack.")?;
        ensure!(
            assets::missing(&state.hotstep_cover).is_empty(),
            "Finish installing Stable Audio 3 training assets."
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
        evict_kept_models(&state, "before HOT-Step Stable Audio 3 training").await;
        state
            .training
            .start_hotstep_sa3(
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
    let path = run.join("sa3-output/adapter.gguf");
    let mut source = std::fs::File::open(path)
        .context("The Stable Audio 3 trainer did not export an adapter.")?;
    let mut magic = [0; 4];
    source.read_exact(&mut magic)?;
    ensure!(
        &magic == b"GGUF",
        "The Stable Audio 3 trainer exported an invalid GGUF adapter."
    );
    let name = format!("sa3-{id}");
    let adapters = models.join("sa3-adapters");
    std::fs::create_dir_all(&adapters)?;
    let mut target = tempfile::NamedTempFile::new_in(&adapters)?;
    std::io::Write::write_all(&mut target, &magic)?;
    std::io::copy(&mut source, &mut target)?;
    target.as_file().sync_all()?;
    target.persist_noclobber(adapters.join(format!("{name}.gguf")))?;
    Ok(name)
}
