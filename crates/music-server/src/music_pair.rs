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
    error_code: Option<&'static str>,
    // Ready confirms the pair can accept a request. These engines can defer
    // loading their generation weights until Create.
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
        Ok(Preparation::Missing(bytes, code, reason)) => {
            pair.update(revision, |status| {
                status.stage = Stage::NeedsInstall;
                status.missing_bytes = bytes;
                status.error = Some(reason);
                status.error_code = Some(code);
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
            let text = format!("{error:#}");
            status.stage = Stage::Error;
            status.error_code = Some(error_code(&text));
            status.error = Some(text);
        }
    });
    drop(lease);
}

/// The window's code for a pair that could not be prepared: the OpenRouter choices checked here, else what the
/// engine's startup failed on.
fn error_code(text: &str) -> &'static str {
    if text.contains("Select an OpenRouter music model") {
        "openrouter_model_missing"
    } else if text.contains("Configure the OpenRouter API key") {
        "openrouter_key_missing"
    } else if text.starts_with("Unknown ") {
        "unknown_model"
    } else {
        crate::engine_error_code(text)
    }
}

enum Preparation {
    Missing(Option<u64>, &'static str, String),
    Lazy,
    YuE2,
    Ace(ace::Launch),
    MiniMax(String),
}

async fn check_files(state: &AppState, source: &MusicSource) -> anyhow::Result<Preparation> {
    let missing = |bytes, code, why: String| Ok(Preparation::Missing(bytes, code, why));
    let root = update_data_root(state);
    match source {
        MusicSource::Local => {
            let target = effective_install_target(state).await;
            let status = state.model_manager.status(target).await;
            if !status.ready {
                return missing(
                    Some(status.download_pending),
                    "yue2_model_set_missing",
                    "Install the selected YuE2 model set.".into(),
                );
            }
            let options = state.engine_options.read().await.clone();
            if options.backend == music_engine::yue_server::ComputeBackend::Hip {
                let status = state.engine_hip.status().await;
                if let Some(reason) = status.reason {
                    anyhow::bail!("{reason}");
                }
                if !status.installed {
                    return missing(
                        Some(status.missing_bytes),
                        "yue2_hip_missing",
                        "Install the YuE2 HIP component in Engine Settings.".into(),
                    );
                }
                return Ok(Preparation::YuE2);
            }
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
                        "yue2_runtime_missing",
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
            let install = ace::resolve_installation(root);
            let profile = install
                .profiles()
                .into_iter()
                .find(|profile| profile.id == model)
                .context("Unknown ACE-Step profile")?;
            if !install.engine_installed() || !profile.ready {
                return missing(
                    None,
                    "ace_files_missing",
                    format!(
                        "Install the ACE-Step engine and the missing profile files: {}",
                        profile.missing.join(", ")
                    ),
                );
            }
            return Ok(Preparation::Ace(install.launch(model)?));
        }
        MusicSource::Native { engine, model } if engine == native_minimax::ENGINE_ID => {
            let install = native_minimax::Runtime::resolve_installation(root);
            let profile = install
                .profiles()
                .into_iter()
                .find(|profile| profile.profile.id == model)
                .context("Unknown MiniMax profile")?;
            if !install.engine_installed() || !profile.ready {
                return missing(
                    None,
                    "minimax_files_missing",
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
        MusicSource::Signed { .. } => {
            return missing(None, "signed_package_gone", crate::SIGNED_PACKAGES_GONE.into());
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
        "model_files_missing",
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
                        return Ok(true);
                    }
                }
            }
            if state.ace_runtime.health().await {
                if !stage(Stage::Unloading) {
                    return Ok(true);
                }
                state.ace_runtime.stop().await?;
            }
            if state.minimax.healthy().await {
                if !stage(Stage::Unloading) {
                    return Ok(true);
                }
                state.minimax.stop().await;
            }
            start_engine_again_with(state, Some(&stage))
                .await
                .map(|()| true)
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
        Preparation::Missing(..) => unreachable!("Missing files are handled before startup"),
    }
}

#[cfg(test)]
mod tests {
    /// Every code the window can be sent: from a pair that could not be prepared or an engine that did not start.
    const CODES: [&str; 21] = [
        "engine_no_answer", "cuda_backend_missing", "runtime_libraries_failed", "device_missing", "cuda_unsupported",
        "engine_port_busy", "engine_runtime_missing", "model_set_not_selected", "chosen_card_missing", "engine_exited",
        "engine_failed", "yue2_model_set_missing", "yue2_hip_missing", "yue2_runtime_missing", "ace_files_missing",
        "minimax_files_missing", "model_files_missing", "signed_package_gone", "openrouter_model_missing",
        "openrouter_key_missing", "unknown_model",
    ];

    #[test]
    fn every_code_has_a_translation_in_every_language() {
        let dictionary = include_str!("../../../app/i18n/engineErrors.ts");
        let missing: Vec<_> = CODES
            .iter()
            .filter_map(|code| {
                let found = dictionary.matches(&format!("\n  {code}:")).count();
                (found != 6).then(|| format!("{code}: {found} of 6"))
            })
            .collect();
        assert!(missing.is_empty(), "without a translation: {missing:?}");
    }

    #[test]
    fn startup_failures_and_pair_checks_get_codes_the_window_knows() {
        for (_, _, code) in crate::ENGINE_FAILURES {
            assert!(CODES.contains(&code), "{code}");
        }
        let cases = [
            ("the local engine runtime was not found: yue-server.exe was not found below D:\\x", "engine_runtime_missing"),
            ("the local engine runtime was not found: the engine's CUDA backend C:\\x\\ggml-cuda.dll is missing", "cuda_backend_missing"),
            ("yue-server exited during startup with status exit code: 3", "engine_exited"),
            ("Select an OpenRouter music model.", "openrouter_model_missing"),
            ("Configure the OpenRouter API key.", "openrouter_key_missing"),
            ("Unknown ACE-Step profile", "unknown_model"),
            ("something nobody foresaw", "engine_failed"),
        ];
        for (text, code) in cases {
            assert_eq!(super::error_code(text), code, "{text}");
        }
    }
}
