//! Serialise preparation requests independently of the songs already in flight.
use super::*;
use std::time::{Duration, Instant};

static INSTANCE_ID: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| uuid::Uuid::now_v7().to_string());

pub(super) fn instance_id() -> &'static str {
    &INSTANCE_ID
}

#[derive(Clone, Copy, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Stage {
    #[default]
    Idle,
    WaitingForGeneration,
    CheckingFiles,
    NeedsInstall,
    Unloading,
    StartingEngine,
    LoadingModel,
    Ready,
    Error,
}

impl Stage {
    fn in_progress(self) -> bool {
        matches!(
            self,
            Self::WaitingForGeneration
                | Self::CheckingFiles
                | Self::Unloading
                | Self::StartingEngine
                | Self::LoadingModel
        )
    }
}

#[derive(Clone, Default, Serialize)]
pub(super) struct Status {
    instance_id: String,
    revision: u64,
    requested: Option<MusicSource>,
    active: Option<MusicSource>,
    stage: Stage,
    elapsed_ms: u64,
    waiting_job_id: Option<String>,
    missing_bytes: Option<u64>,
    error: Option<String>,
    // Some processes resolve their weights per request. A ready response for
    // those routes confirms admission prerequisites, while work still starts on Create.
    load_on_create: bool,
}

struct Current {
    status: Status,
    started: Instant,
    // Sequence numbers belong to a window. Keep histories for recently seen windows.
    clients: std::collections::VecDeque<(String, u64)>,
}

impl Default for Current {
    fn default() -> Self {
        Self {
            status: Status::default(),
            started: Instant::now(),
            clients: Default::default(),
        }
    }
}

impl Current {
    fn snapshot(&self) -> Status {
        let mut status = self.status.clone();
        status.instance_id = instance_id().into();
        if status.stage.in_progress() {
            status.elapsed_ms = self
                .started
                .elapsed()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX);
        }
        status
    }

    fn select(&mut self, request: Select) -> Option<u64> {
        if let (Some(client), Some(sequence)) = (request.client_id, request.sequence) {
            if let Some(position) = self.clients.iter().position(|(known, _)| *known == client) {
                if self.clients[position].1 >= sequence {
                    return None;
                }
                self.clients.remove(position);
            }
            self.clients.push_back((client, sequence));
            if self.clients.len() > 64 {
                self.clients.pop_front();
            }
        }
        if self.status.requested.as_ref() == Some(&request.music_source)
            && (self.status.stage.in_progress()
                || self.status.stage == Stage::Ready
                    && self.status.active.as_ref() == Some(&request.music_source))
        {
            return None;
        }
        self.started = Instant::now();
        let revision = self.status.revision + 1;
        self.status = Status {
            revision,
            requested: Some(request.music_source),
            active: self.status.active.clone(),
            stage: Stage::CheckingFiles,
            ..Status::default()
        };
        Some(revision)
    }
}

#[derive(Default)]
pub(super) struct Lifecycle {
    current: std::sync::Mutex<Current>,
    selected: tokio::sync::Notify,
}

impl Lifecycle {
    fn read(&self) -> std::sync::MutexGuard<'_, Current> {
        self.current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn update(&self, revision: u64, update: impl FnOnce(&mut Status)) -> bool {
        let mut current = self.read();
        if current.status.revision != revision {
            return false;
        }
        current.status.elapsed_ms = current
            .started
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX);
        update(&mut current.status);
        true
    }

    async fn superseded(&self, revision: u64) {
        loop {
            let selected = self.selected.notified();
            if self.read().status.revision != revision {
                return;
            }
            selected.await;
        }
    }

    pub(super) fn note_generation(&self, source: MusicSource) {
        self.read().status.active = Some(source);
    }
}

#[derive(Deserialize)]
pub(super) struct Select {
    music_source: MusicSource,
    client_id: Option<String>,
    sequence: Option<u64>,
}

pub(super) async fn status(State(state): State<AppState>) -> Json<Status> {
    Json(state.music_pair.read().snapshot())
}

pub(super) async fn select(
    State(state): State<AppState>,
    Json(request): Json<Select>,
) -> Result<Json<Status>, (StatusCode, Json<ApiError>)> {
    if request
        .client_id
        .as_ref()
        .is_some_and(|id| id.is_empty() || id.len() > 128)
        || request.client_id.is_some() != request.sequence.is_some()
    {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "Supply a client ID and sequence together.".into(),
        ));
    }
    let source = request.music_source.clone();
    let (revision, snapshot) = {
        let mut current = state.music_pair.read();
        (current.select(request), current.snapshot())
    };
    if let Some(revision) = revision {
        state.music_pair.selected.notify_waiters();
        tokio::spawn(prepare(state, revision, source));
    }
    Ok(Json(snapshot))
}

async fn blocking_job(state: &AppState) -> Option<String> {
    state
        .jobs
        .read()
        .await
        .values()
        .filter(|job| {
            matches!(job.status, MusicJobStatus::Queued | MusicJobStatus::Running)
                && !matches!(job.dispatch, MusicJobDispatch::NotConfigured)
        })
        .min_by_key(|job| job.submitted_at)
        .map(|job| job.id.clone())
}

async fn prepare(state: AppState, revision: u64, source: MusicSource) {
    let pair = &state.music_pair;
    let lease = loop {
        if pair.read().status.revision != revision {
            return;
        }
        let job = blocking_job(&state).await;
        if job.is_none()
            && let Ok(lease) = state.engine_use.clone().try_write_owned()
        {
            // Dispatch can acquire the lease or insert an external job between samples.
            if blocking_job(&state).await.is_none() && state.training.active_run().await.is_none() {
                break lease;
            }
        }
        if !pair.update(revision, |status| {
            status.stage = Stage::WaitingForGeneration;
            status.waiting_job_id = job;
        }) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    if !pair.update(revision, |status| {
        status.stage = Stage::CheckingFiles;
        status.waiting_job_id = None;
    }) {
        return;
    }
    let outcome = check_files(&state, &source).await;
    if pair.read().status.revision != revision {
        return;
    }
    let result = match outcome {
        Ok(Preparation::Missing(bytes, reason)) => {
            pair.update(revision, |status| {
                status.stage = Stage::NeedsInstall;
                status.missing_bytes = bytes;
                status.error = Some(reason);
            });
            return;
        }
        Ok(preparation) => start(&state, revision, preparation).await,
        Err(error) => Err(error),
    };
    pair.update(revision, |status| match result {
        Ok(load_on_create) => {
            status.stage = Stage::Ready;
            status.active = Some(source);
            status.load_on_create = load_on_create;
        }
        Err(error) => {
            status.stage = Stage::Error;
            status.error = Some(format!("{error:#}"));
        }
    });
    drop(lease);
}

enum Preparation {
    Missing(Option<u64>, String),
    Lazy,
    YuE2,
    Ace(ace::Launch),
    MiniMax(String),
}

async fn check_files(state: &AppState, source: &MusicSource) -> anyhow::Result<Preparation> {
    let missing = |bytes, why: String| Ok(Preparation::Missing(bytes, why));
    let root = update_data_root(state);
    match source {
        MusicSource::Local => {
            let target = effective_install_target(state).await;
            let status = state.model_manager.status(target).await;
            if !status.ready {
                return missing(
                    Some(status.download_pending),
                    "Install the selected YuE2 model set.".into(),
                );
            }
            let options = state.engine_options.read().await.clone();
            let chain = options.device_chain(&state.failed_devices.read().await);
            if let Some(backend) = chain.first().copied() {
                let cuda = EngineOptions { backend, ..options }.cuda_build();
                let assets = state.engine_runtime.missing(cuda);
                if !assets.is_empty() {
                    return missing(
                        Some(
                            assets
                                .iter()
                                .map(|asset| {
                                    asset.bytes.saturating_sub(
                                        state.engine_runtime.downloader().partial_bytes(asset),
                                    )
                                })
                                .sum(),
                        ),
                        "Install the missing YuE2 runtime libraries.".into(),
                    );
                }
            }
            return Ok(Preparation::YuE2);
        }
        MusicSource::OpenRouter { model } => {
            anyhow::ensure!(
                !model.trim().is_empty(),
                "Select an OpenRouter music model."
            );
            anyhow::ensure!(
                credentials::openrouter_api_key().is_some(),
                "Configure the OpenRouter API key."
            );
        }
        MusicSource::Native { engine, model } if engine == ace::ENGINE_ID => {
            let install = ace::installation(root);
            let profile = install
                .profiles()
                .into_iter()
                .find(|profile| profile.id == model)
                .context("Unknown ACE-Step profile")?;
            if !install.engine_installed() || !profile.ready {
                return missing(
                    None,
                    format!(
                        "Install the ACE-Step engine and the missing profile files: {}",
                        profile.missing.join(", ")
                    ),
                );
            }
            return Ok(Preparation::Ace(install.launch(model)?));
        }
        MusicSource::Native { engine, model } if engine == native_minimax::ENGINE_ID => {
            let install = native_minimax::Runtime::installation(root);
            let profile = install
                .profiles()
                .into_iter()
                .find(|profile| profile.profile.id == model)
                .context("Unknown MiniMax profile")?;
            if !install.engine_installed() || !profile.ready {
                return missing(
                    None,
                    format!(
                        "Install the MiniMax engine and the missing profile files: {}",
                        profile.missing.join(", ")
                    ),
                );
            }
            return Ok(Preparation::MiniMax(model.clone()));
        }
        MusicSource::Native { engine, model } if engine == hotstep_cover::ENGINE => {
            let Json(catalogue) = hotstep_cover::profiles(State(state.clone())).await;
            return catalogue_result(&catalogue["profiles"], model);
        }
        MusicSource::AudioCpp { model } => {
            let Json(catalogue) = audio_cpp_music::models(State(state.clone())).await;
            return catalogue_result(&catalogue["models"], model);
        }
        MusicSource::Signed {
            engine,
            model,
            generation,
            model_generation,
        } => {
            if let Err(error) = engine_catalogue::install::require_pair_presence(
                root,
                engine,
                model,
                generation.as_deref(),
                model_generation.as_deref(),
            )
            .await
            {
                return missing(None, error.to_string());
            }
        }
        MusicSource::Native { engine, .. } => {
            anyhow::bail!("Unknown native music engine {engine}.")
        }
    }
    Ok(Preparation::Lazy)
}

fn check_catalogue<'a>(catalogue: &'a Value, model: &str) -> anyhow::Result<&'a Value> {
    catalogue
        .as_array()
        .and_then(|entries| entries.iter().find(|row| row["id"] == model))
        .context("Unknown music model")
}

fn catalogue_result(catalogue: &Value, model: &str) -> anyhow::Result<Preparation> {
    let item = check_catalogue(catalogue, model)?;
    if item["ready"] == true {
        return Ok(Preparation::Lazy);
    }
    Ok(Preparation::Missing(
        item["missing_bytes"].as_u64(),
        "Install the engine and model files for this choice.".into(),
    ))
}

async fn start(state: &AppState, revision: u64, preparation: Preparation) -> anyhow::Result<bool> {
    let pair = &state.music_pair;
    let stage = |next| {
        pair.update(revision, |status| {
            status.stage = next;
            if next == Stage::Unloading {
                status.active = None;
            }
        })
    };
    match preparation {
        Preparation::Lazy => Ok(true),
        Preparation::YuE2 => {
            anyhow::ensure!(!engine_is_paused(state), ENGINE_PAUSED_MESSAGE);
            let selected = state
                .engine
                .lock()
                .await
                .as_ref()
                .map(|engine| engine.config().models.clone());
            if state.music_server.health().await {
                if let Some(files) = selected.as_ref() {
                    if selection_matches_files(state, files).await.unwrap_or(false) {
                        return Ok(false);
                    }
                }
            }
            if state.ace_runtime.health().await {
                if !stage(Stage::Unloading) {
                    return Ok(false);
                }
                state.ace_runtime.stop().await?;
            }
            if state.minimax.healthy().await {
                if !stage(Stage::Unloading) {
                    return Ok(false);
                }
                state.minimax.stop().await;
            }
            start_engine_again_with(state, Some(&stage))
                .await
                .map(|()| false)
                .map_err(anyhow::Error::msg)
        }
        Preparation::Ace(launch) => {
            if state.minimax.healthy().await {
                if !stage(Stage::Unloading) {
                    return Ok(true);
                }
                state.minimax.stop().await;
            }
            evict_kept_models(state, "before preparing an ACE-Step pair").await;
            if stage(Stage::StartingEngine) {
                tokio::select! {
                    biased;
                    _ = pair.superseded(revision) => return Ok(true),
                    result = state.ace_runtime.start(launch) => { result?; }
                }
            }
            Ok(true)
        }
        Preparation::MiniMax(profile) => {
            if state.ace_runtime.health().await {
                if !stage(Stage::Unloading) {
                    return Ok(true);
                }
                state.ace_runtime.stop().await?;
            }
            evict_kept_models(state, "before preparing a MiniMax pair").await;
            if stage(Stage::StartingEngine) {
                tokio::select! {
                    biased;
                    _ = pair.superseded(revision) => return Ok(true),
                    result = state.minimax.start(&profile, update_data_root(state)) => { result?; }
                }
            }
            Ok(true)
        }
        Preparation::Missing(_, _) => unreachable!("Missing files are handled before startup"),
    }
}
