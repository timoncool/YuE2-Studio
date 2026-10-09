mod adapters;
mod processing;
mod vst;
mod training;
mod auto_title;
mod tagging;
mod cover_art;
mod cover_prompt;
mod providers;
mod assistant;
mod song_tokenizer;
mod assistant_runtime;
mod audio_facts;
mod beat_dbn;
mod listen;
mod prepare;
mod audio_pcm;
mod legacy_text;
mod downloads;
mod engine_runtime;
mod lyrics_db;
mod mcp;
mod lyrics_sync;
mod comfy_export;
mod remote;
pub use remote::{set_asset_source, AssetSource};
mod credentials;
mod model_manager;
mod hardware;
mod request_log;
mod resources;
mod chunked;
mod separation;
mod midi;
mod midi_edit;
mod score;
mod sizes;
pub mod net;
mod saving;
mod skins;
pub use saving::{set_save_dialog, SaveDialog};
mod skill;
mod library;
mod engine_result;
mod progress;
mod engine_log;
mod harmony;

use std::{collections::HashMap, env, fs, net::SocketAddr, path::PathBuf, sync::Arc};
use anyhow::Context;
use futures_util::StreamExt;

use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, Query, Request, State},
    body::Body,
    http::{header, StatusCode},
    response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
    Json, Router,
};
use music_core::{Capability, EngineDescriptor, ExecutionMode, StudioConfiguration};
use model_manager::{InstallRequest, ModelManager};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;
use tower::ServiceExt;
use tower_http::{cors::CorsLayer, services::ServeFile, set_header::SetResponseHeaderLayer, trace::TraceLayer};

const PRIMARY_MUSIC_ENGINE_ID: &str = model_manager::ENGINE_ID;

#[derive(Clone)]
struct AppState {
    configuration: Arc<RwLock<StudioConfiguration>>,
    jobs: Arc<RwLock<HashMap<String, MusicJob>>>,
    music_server: EngineClient,
    /// The engine's log, followed for the service's life: its recent lines and
    /// the running job's progress.
    engine_log: engine_log::EngineLog,
    model_manager: ModelManager,
    selected_profile_id: Arc<RwLock<Option<String>>>,
    selected_component_ids: Arc<RwLock<Option<Vec<String>>>>,
    settings_path: PathBuf,
    openrouter_catalog: Arc<RwLock<OpenRouterCatalogState>>,
    library: library::Library,
    /// Owned local engine process, when this service started one.
    engine: Arc<tokio::sync::Mutex<Option<music_engine::yue_server::YueServerSupervisor>>>,
    engine_options: Arc<RwLock<EngineOptions>>,
    /// Devices that failed under Auto in this session - the engine did not
    /// start on them, or died with a CUDA or Vulkan error - and the one the
    /// running engine computes on. Auto walks CUDA, Vulkan, the processor and
    /// skips the failed; a restart of the studio tries them all again.
    failed_devices: Arc<RwLock<Vec<music_engine::yue_server::ComputeBackend>>>,
    /// Why the engine last failed to come up with a complete set on disk, for the setup page.
    engine_start_error: Arc<RwLock<Option<String>>>,
    active_device: Arc<RwLock<Option<music_engine::yue_server::ComputeBackend>>>,
    /// The CUDA libraries the engine binary imports. They are downloaded, not
    /// installed, so the engine cannot start until they are on disk.
    engine_runtime: Arc<engine_runtime::EngineRuntime>,
    assistant: Arc<RwLock<AssistantConfig>>,
    assistant_runtime: Arc<assistant_runtime::AssistantRuntime>,
    lyrics_sync: Arc<lyrics_sync::LyricsSync>,
    lyrics_sync_config: Arc<RwLock<lyrics_sync::LyricsSyncConfig>>,
    /// Saved cover looks, filled in from whichever track a cover is for.
    cover_templates: Arc<RwLock<Vec<cover_prompt::CoverTemplate>>>,
    /// The look a new cover starts from, chosen in Settings.
    cover_template_default: Arc<RwLock<Option<String>>>,
    separator: Arc<separation::Separator>,
    separation_config: Arc<RwLock<separation::SeparationConfig>>,
    /// Draw a cover as soon as a track is finished.
    cover_auto: Arc<RwLock<bool>>,
    /// How a track without a cover of its own looks, and whether that look is
    /// written into the track.
    cover_look: Arc<RwLock<cover_art::CoverLook>>,
    /// Why the last photo for a track could not be fetched, until one is.
    cover_problem: Arc<RwLock<Option<String>>>,
    /// One pass over the library at a time writes placeholders into tracks.
    cover_pinning: Arc<tokio::sync::Mutex<()>>,
    /// What is being done to finished tracks right now - covers, karaoke - so
    /// the interface can say it instead of leaving the user guessing.
    activity: Arc<RwLock<Vec<Activity>>>,
    /// The separation run in progress, if any. One at a time: the model wants
    /// the whole machine for a minute, and two runs would only make both slow.
    separation_run: Arc<RwLock<Option<SeparationRun>>>,
    /// Audio to MIDI: the transcriber and its weights, the run at work or the
    /// last one, the notes it has heard so far, and the stop signal.
    midi: Arc<midi::Transcriber>,
    midi_run: Arc<RwLock<Option<midi::Run>>>,
    midi_notes: Arc<RwLock<Vec<midi::Note>>>,
    midi_stop: Arc<(std::sync::atomic::AtomicBool, tokio::sync::Notify)>,
    /// LoRA adapters for the local engine, and the example catalogue.
    adapters: Arc<adapters::AdapterLibrary>,
    /// The processing run in progress or the last one, with its preview.
    processing_run: Arc<RwLock<Option<processing::ProcessRun>>>,
    /// Adapter training: its optional weights, datasets and runs.
    training: Arc<training::Training>,
    /// The dataset preparation in progress or the last one.
    prepare: prepare::Shared,
    prepare_cancel: Arc<std::sync::atomic::AtomicBool>,
    prepare_train: prepare::TrainSlot,
}

#[derive(Clone)]
struct EngineClient {
    base_url: String,
    http: reqwest::Client,
    /// The last health answer and when it was taken. Every status poll asks,
    /// and on Windows a refused loopback connect takes about two seconds, so
    /// with the engine down the polls queued up behind each other.
    health_cache: Arc<std::sync::Mutex<Option<(std::time::Instant, bool)>>>,
}

/// One autoregressive stage's sampling preset. Every knob is optional: an
/// absent one is the checkpoint value the engine applies.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
struct SamplingPreset {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    repetition_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    penalty_window: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

impl SamplingPreset {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The protocol bounds yue-server enforces, checked here so the user is
    /// told which knob is wrong instead of reading a bare 400.
    fn validate(&self, label: &str) -> Result<(), String> {
        if self.temperature.is_some_and(|value| !(0.0..=5.0).contains(&value)) {
            return Err(format!("{label}: temperature must be between 0 and 5"));
        }
        if self.top_p.is_some_and(|value| !(value > 0.0 && value <= 1.0)) {
            return Err(format!("{label}: top_p must be above 0 and at most 1"));
        }
        if self.top_k.is_some_and(|value| value < 1) {
            return Err(format!("{label}: top_k must be at least 1"));
        }
        if self.repetition_penalty.is_some_and(|value| !(value > 0.0 && value.is_finite())) {
            return Err(format!("{label}: repetition_penalty must be positive"));
        }
        if self.penalty_window.is_some_and(|value| !(1..=100).contains(&value)) {
            return Err(format!("{label}: penalty_window must be between 1 and 100"));
        }
        if self.max_tokens.is_some_and(|value| value < 1) {
            return Err(format!("{label}: max_tokens must be at least 1"));
        }
        if let (Some(min), Some(max)) = (self.min_tokens, self.max_tokens) {
            if min > max {
                return Err(format!("{label}: min_tokens cannot exceed max_tokens"));
            }
        }
        Ok(())
    }
}

/// A YuE2 generation request, in the engine's own vocabulary. Fields left
/// out are the engine's protocol defaults.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct CreateMusicJobRequest {
    /// The window's own mark for this request, handed back on the job so the
    /// window knows the job as its own before the response reaches it.
    client_ref: Option<String>,
    /// Comma-separated style tags, verbatim under `[Tags]`.
    #[serde(default)]
    style: String,
    /// Lyrics with their structural tags, verbatim under `[Lyrics]`.
    #[serde(default)]
    lyrics: String,
    /// ABC score to realise; empty lets the model write one.
    abc: Option<String>,
    /// Move a supplied score before singing; zero keeps its pitches.
    transpose: Option<i32>,
    /// Reveal each section's words as a supplied score reaches it; on unless false.
    #[serde(default)]
    lyric_timing: Option<bool>,
    #[serde(default)]
    vocals_only: bool,
    /// The playlist the made songs are added to.
    #[serde(default)]
    playlist_id: Option<String>,
    /// Chain-of-thought mode: `full`, `melody` or `off`.
    cot: Option<String>,
    /// Target length in seconds; the model may end the song earlier.
    duration_seconds: Option<f64>,
    lm_seed: Option<i64>,
    seed: Option<i64>,
    steps: Option<u32>,
    lm_batch_size: Option<u32>,
    synth_batch_size: Option<u32>,
    cfg_scale: Option<f64>,
    /// Strength of the engine's realaudio decoder companion; 0 decodes with the checkpoint alone.
    companion_scale: Option<f64>,
    /// Comma-separated semantic codes; present means the AR stage is skipped.
    semantic_tokens: Option<String>,
    abc_sampling: Option<SamplingPreset>,
    semantic_sampling: Option<SamplingPreset>,
    /// Chord variety and section order of the score the model plans.
    #[serde(default)]
    harmony: Option<harmony::Harmony>,
    output_format: Option<String>,
    mp3_bitrate: Option<u32>,
    /// Library title only, never sent to the engine.
    title: Option<String>,
    /// What the cover should show, when the assistant described it.
    cover_prompt: Option<String>,
    /// Installed adapters to merge for this song, in order.
    #[serde(default)]
    adapters: Vec<AdapterUse>,
    /// The library song whose melody this song sings (a cover); the new song
    /// names it as the track it was made from.
    #[serde(default)]
    cover_of: Option<String>,
}

/// One adapter of a request: its folder and a strength per engine slot. A slot
/// left out is not changed.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct AdapterUse {
    id: String,
    #[serde(default)]
    scales: std::collections::BTreeMap<String, f64>,
}

/// The name this request goes into the library under: the user's, or one
/// taken from the song when they left the field empty.
fn titled(request: &CreateMusicJobRequest, library: &adapters::AdapterLibrary) -> String {
    request
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            // the words that switch the song's LoRA on say nothing about it
            let triggers: Vec<String> = request.adapters.iter().filter_map(|adapter| library.trigger_of(&adapter.id)).collect();
            auto_title::auto_title(&auto_title::without_triggers(&request.style, &triggers), &request.lyrics, request.lyrics.trim().is_empty())
        })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum MusicJobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum MusicJobDispatch {
    NotConfigured,
    Local,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum MusicJobPhase {
    Queued,
    Running,
    ExtractingVocals,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
struct MusicJob {
    /// Where a track made from another comes from (a re-render).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    derived: Option<Value>,
    id: String,
    /// The mark the submitting window gave the request; absent for an agent's.
    #[serde(skip_serializing_if = "Option::is_none")]
    client_ref: Option<String>,
    /// When the request came in, in Unix milliseconds: a window opened later
    /// places the job among the ones it made itself.
    submitted_at: u64,
    engine_id: String,
    /// What the assistant said this track's cover should show, if anything.
    #[serde(skip_serializing_if = "Option::is_none")]
    cover_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    status: MusicJobStatus,
    dispatch: MusicJobDispatch,
    phase: MusicJobPhase,
    style: String,
    lyrics: String,
    duration_seconds: f64,
    generation_settings: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    song: Option<CompletedSong>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    songs: Vec<CompletedSong>,
    message: String,
    /// The playlist the made songs go into, a project the user works in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    playlist_id: Option<String>,
    /// How the lyrics were laid along a score that came without sections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    laid: Option<Value>,
}

fn job_status_name(status: &MusicJobStatus) -> &'static str {
    match status {
        MusicJobStatus::Queued => "queued",
        MusicJobStatus::Running => "running",
        MusicJobStatus::Completed => "completed",
        MusicJobStatus::Failed => "failed",
        MusicJobStatus::Cancelled => "cancelled",
    }
}

fn stored_job(job: &MusicJob, request: Value, attempt: u32) -> library::StoredJob {
    library::StoredJob {
        id: job.id.clone(),
        submitted_at: job.submitted_at,
        engine_id: job.engine_id.clone(),
        title: job.title.clone(),
        style: job.style.clone(),
        lyrics: job.lyrics.clone(),
        duration_seconds: job.duration_seconds,
        playlist_id: job.playlist_id.clone(),
        generation_settings: job.generation_settings.clone(),
        request,
        status: job_status_name(&job.status).into(),
        message: job.message.clone(),
        attempt,
        resumed_as: None,
    }
}

/// A job as the library kept it, for a job that ended without a result.
fn job_from_stored(stored: library::StoredJob, status: MusicJobStatus, message: String) -> MusicJob {
    let (dispatch, phase) = match status {
        MusicJobStatus::Failed => (MusicJobDispatch::Local, MusicJobPhase::Failed),
        _ => (MusicJobDispatch::Cancelled, MusicJobPhase::Cancelled),
    };
    MusicJob {
        derived: None,
        id: stored.id,
        client_ref: None,
        submitted_at: stored.submitted_at,
        engine_id: stored.engine_id,
        cover_prompt: None,
        title: stored.title,
        status,
        dispatch,
        phase,
        style: stored.style,
        lyrics: stored.lyrics,
        duration_seconds: stored.duration_seconds,
        generation_settings: stored.generation_settings,
        song: None,
        songs: vec![],
        message,
        playlist_id: stored.playlist_id,
        laid: None,
    }
}

/// How many times a song is started again after the studio closed on it; a
/// request that takes the engine down every time must not start for ever.
const MAX_RESUMES: u32 = 3;

/// Songs the studio was closed on are started again, oldest first. The old
/// job stays as `cancelled`, its message naming the job that took its place.
async fn resume_unfinished_jobs(state: AppState) {
    // what ended without a result stays asked about, as it was left
    match state.library.ended_music_jobs(200) {
        Ok(ended) => {
            let mut jobs = state.jobs.write().await;
            for stored in ended {
                let status = if stored.status == "failed" { MusicJobStatus::Failed } else { MusicJobStatus::Cancelled };
                let message = stored.message.clone();
                jobs.entry(stored.id.clone()).or_insert_with(|| job_from_stored(stored, status, message));
            }
        }
        Err(error) => eprintln!("[ERROR] the songs that ended without a result could not be read: {error:#}"),
    }
    let cut_off = match state.library.unfinished_music_jobs() {
        Ok(jobs) if !jobs.is_empty() => jobs,
        Ok(_) => return,
        Err(error) => {
            eprintln!("[ERROR] the songs cut off by the last run could not be read: {error:#}");
            return;
        }
    };
    // the engine is started with the service and takes a while to answer; if
    // it never does, the songs stay as they are for the next start
    let mut ready = false;
    for _ in 0..90 {
        if state.music_server.props().await.is_ok() {
            ready = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    if !ready {
        eprintln!("[ERROR] the engine did not answer; {} cut-off song(s) were not started again", cut_off.len());
        return;
    }
    for old in cut_off {
        let (resumed_as, message) = if old.attempt >= MAX_RESUMES {
            (None, format!("Cut off when the studio closed, and already started again {MAX_RESUMES} times; make it again by hand."))
        } else if let Some(replay) = old.request.get("replay") {
            match serde_json::from_value::<ReplayMusicJobRequest>(replay.clone()) {
                Err(error) => (None, format!("Cut off when the studio closed; its request could not be read to start it again: {error}")),
                Ok(mut request) => {
                    request.client_ref = None;
                    match submit_replay_job(state.clone(), request, old.attempt + 1).await {
                        Ok((_, Json(job))) => (Some(job.id.clone()), format!("Cut off when the studio closed; started again as {}.", job.id)),
                        Err((_, Json(error))) => (None, format!("Cut off when the studio closed; it could not be started again: {}", error.error)),
                    }
                }
            }
        } else {
            match serde_json::from_value::<CreateMusicJobRequest>(old.request.clone()) {
                Err(error) => (None, format!("Cut off when the studio closed; its request could not be read to start it again: {error}")),
                Ok(mut request) => {
                    request.client_ref = None;
                    let (status, Json(job)) = submit_music_job(state.clone(), request, old.attempt + 1).await;
                    if status == StatusCode::ACCEPTED {
                        (Some(job.id.clone()), format!("Cut off when the studio closed; started again as {}.", job.id))
                    } else {
                        (None, format!("Cut off when the studio closed; it could not be started again: {}", job.message))
                    }
                }
            }
        };
        if let Err(error) = state.library.cancel_cut_off_music_job(&old.id, resumed_as.as_deref(), &message) {
            eprintln!("[ERROR] a cut-off song could not be marked: {error:#}");
        }
        state.jobs.write().await.insert(old.id.clone(), job_from_stored(old, MusicJobStatus::Cancelled, message));
    }
}

fn unix_millis() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_millis() as u64)
}

#[derive(Debug, Clone, Serialize)]
struct CompletedSong {
    id: String,
    song: library::Song,
    audio_url: String,
}

#[derive(Debug, Serialize)]
struct LocalMusicModelCatalog {
    engine_id: String,
    catalog: Value,
}

#[derive(Debug, Serialize)]
struct CapabilitiesResponse {
    engines: Vec<EngineDescriptor>,
}

#[derive(Debug, Serialize)]
struct ApiError {
    error: String,
}

#[derive(Debug, Deserialize)]
struct EngineSubmitResponse {
    id: String,
}

#[derive(Debug, Deserialize)]
struct EngineJobResponse {
    status: String,
}

struct EngineResultResponse {
    content_type: String,
    body: Vec<u8>,
}

#[derive(Debug, Deserialize)]
struct SetupSelectRequest {
    #[serde(default)]
    profile_id: Option<String>,
    #[serde(default)]
    component_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct SetupDownloadRequest {
    // The panel calls this field `component_ids`. Reading only `ids` meant a
    // download request arrived empty, and an empty request quietly fell back to
    // the default set - which is how pressing "download" on the 11.9 GB set
    // started fetching the 26.6 GB one.
    #[serde(default, alias = "component_ids")]
    ids: Vec<String>,
    profile_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ReplayMusicJobRequest {
    song_id: Option<String>,
    replay_request: Option<Value>,
    steps: Option<u32>,
    seed: Option<i64>,
    synth_batch_size: Option<u32>,
    output_format: Option<String>,
    mp3_bitrate: Option<u32>,
    /// A title for the re-render; the source track's own name otherwise.
    title: Option<String>,
    /// Seconds to compose on after the track's last frame; its start stays as it is.
    extend_seconds: Option<f64>,
    /// The window's own mark, as on a new song.
    client_ref: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedStudioSettings {
    #[serde(default)]
    engine_options: EngineOptions,
    #[serde(default)]
    assistant: AssistantConfig,
    lyrics_sync: lyrics_sync::LyricsSyncConfig,
    configuration: StudioConfiguration,
    selected_profile_id: Option<String>,
    #[serde(default)]
    selected_component_ids: Option<Vec<String>>,
    #[serde(default)]
    cover_templates: Option<Vec<cover_prompt::CoverTemplate>>,
    #[serde(default)]
    cover_template_default: Option<String>,
    #[serde(default)]
    separation: Option<separation::SeparationConfig>,
    /// Whether a finished track gets its cover drawn without being asked.
    #[serde(default)]
    cover_auto: Option<bool>,
    #[serde(default)]
    cover_look: Option<cover_art::CoverLook>,
    #[serde(default)]
    proxy: Option<net::ProxySettings>,
    /// Access from other computers, off unless turned on.
    #[serde(default)]
    network: Option<remote::NetworkAccess>,
}

#[derive(Default)]
struct OpenRouterCatalogState {
    catalog: Option<providers::openrouter::CapabilityCatalog>,
    refreshed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterTranscriptionRequest {
    model_id: String,
    audio_base64: String,
    audio_format: String,
    language: Option<String>,
}

/// Launch flags for the local engine process. They belong to the running
/// engine, so changing one restarts it.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
struct EngineOptions {
    backend: music_engine::yue_server::ComputeBackend,
    keep_loaded: bool,
    max_batch: Option<u32>,
    max_seq: Option<u32>,
    vae_core: Option<u32>,
    vae_halo: Option<u32>,
    disable_flash_attention: bool,
    clamp_fp16: bool,
    /// The NVIDIA card, by its nvidia-smi index, that every CUDA process of
    /// the studio computes on. None: the first. Taken at the studio's start.
    gpu: Option<u32>,
}

impl EngineOptions {
    /// Songs one request may draw, and the `--max-batch` the engine starts
    /// with. Each song reserves a KV set, so nothing is reserved unasked.
    /// The CUDA build the engine will compute on, and so the cuBLAS it needs:
    /// chosen outright, or left to ggml on an NVIDIA card one of the builds
    /// runs on. None on Vulkan and the processor.
    fn cuda_build(&self) -> Option<hardware::CudaBuild> {
        use music_engine::yue_server::ComputeBackend;
        match self.backend {
            ComputeBackend::Cuda | ComputeBackend::Auto => hardware::hardware().cuda,
            ComputeBackend::Vulkan | ComputeBackend::Cpu => None,
        }
    }

    /// Whether the engine will compute on Vulkan: chosen outright, or left to
    /// ggml on a machine whose card is not NVIDIA.
    fn uses_vulkan(&self) -> bool {
        use music_engine::yue_server::ComputeBackend;
        match self.backend {
            ComputeBackend::Vulkan => true,
            ComputeBackend::Auto => {
                if cfg!(target_os = "macos") {
                    // The macOS engine build has no Vulkan backend; Auto there
                    // means the engine's own best device, which is Metal.
                    return false;
                }
                let hardware = hardware::hardware();
                hardware.cuda.is_none() && hardware.gpu_name.is_some()
            }
            ComputeBackend::Cuda | ComputeBackend::Cpu => false,
        }
    }

    fn effective_max_batch(&self) -> u32 {
        self.max_batch.unwrap_or(1).max(1)
    }

    /// The devices a start tries, in order. A device chosen in Settings is
    /// the only one, whatever happens to it. Auto goes CUDA, Vulkan, the
    /// processor: CUDA when one of its builds runs this card and driver,
    /// Vulkan when there is a card, and the processor always, last.
    fn device_chain(&self, failed: &[music_engine::yue_server::ComputeBackend]) -> Vec<music_engine::yue_server::ComputeBackend> {
        use music_engine::yue_server::ComputeBackend;
        if self.backend != ComputeBackend::Auto {
            return vec![self.backend];
        }
        // Off Windows the engine is a single native build whose ggml loads its
        // own best device - Metal on Apple Silicon - so Auto asks the engine
        // to choose (no GGML_BACKEND is set) and only falls back to the
        // processor if that start fails.
        #[cfg(not(windows))]
        {
            let mut chain = vec![ComputeBackend::Auto];
            chain.retain(|device| !failed.contains(device));
            chain.push(ComputeBackend::Cpu);
            return chain;
        }
        #[cfg(windows)]
        {
            let hardware = hardware::hardware();
            let mut chain = Vec::new();
            if hardware.cuda.is_some() {
                chain.push(ComputeBackend::Cuda);
            }
            if hardware.gpu_name.is_some() {
                chain.push(ComputeBackend::Vulkan);
            }
            chain.retain(|device| !failed.contains(device));
            chain.push(ComputeBackend::Cpu);
            chain
        }
    }

    fn to_engine(self) -> music_engine::yue_server::YueServerOptions {
        music_engine::yue_server::YueServerOptions {
            backend: self.backend,
            keep_loaded: self.keep_loaded,
            max_batch: Some(self.effective_max_batch()),
            max_seq: self.max_seq,
            vae_core: self.vae_core,
            vae_halo: self.vae_halo,
            disable_flash_attention: self.disable_flash_attention,
            // On an AMD Radeon through Vulkan the hidden states leave the FP16
            // range and the song comes out as pure silence, which the engine's
            // MP3 path then crashes on; the engine's own clamp fixes it.
            clamp_fp16: self.clamp_fp16 || self.uses_vulkan() || (self.cuda_build().is_some() && hardware::accumulates_in_fp16()),
            cuda_folder: self.cuda_build().map(hardware::CudaBuild::folder),
        }
    }
}

/// Where the optional writing assistant runs.
///
/// `None` is the default and a first-class state: the manual form is the
/// primary way to use this model, and on a modest card nobody wants a language
/// model competing for VRAM. Nothing is downloaded or started unless the user
/// picks a provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
struct AssistantConfig {
    provider: AssistantProvider,
    /// Base URL of an OpenAI-compatible server (llama.cpp, LM Studio, Ollama).
    local_base_url: Option<String>,
    local_model: Option<String>,
    openrouter_model: Option<String>,
    /// Id of a model downloaded through the assistant runtime, run as a
    /// sidecar by Studio itself.
    managed_model: Option<String>,
    /// A GGUF already on this machine, run by the same sidecar. Machines that
    /// already keep a Gemma around for another tool do not need a second copy.
    managed_path: Option<String>,
    /// How hard a reasoning model should think, in OpenRouter's own terms:
    /// minimal, low, medium, high, xhigh, max - or none.
    reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AssistantProvider {
    #[default]
    None,
    Local,
    OpenRouter,
    /// A model Studio downloaded and runs itself with llama.cpp.
    Managed,
    /// The agent connected over MCP: the studio asks it what it would ask its
    /// own model, with the same instructions and answer schema.
    Agent,
}

impl AssistantConfig {
    pub(crate) fn available(&self) -> bool {
        match self.provider {
            AssistantProvider::None => false,
            AssistantProvider::Agent => true,
            AssistantProvider::Local => {
                self.local_base_url.as_deref().is_some_and(|url| !url.trim().is_empty())
                    && self.local_model.as_deref().is_some_and(|model| !model.trim().is_empty())
            }
            AssistantProvider::OpenRouter => {
                self.openrouter_model.as_deref().is_some_and(|model| !model.trim().is_empty())
                    && credentials::openrouter_source().is_some()
            }
            // Availability is confirmed against the disk in `assistant_status`;
            // a model id alone only says one was chosen.
            AssistantProvider::Managed => {
                self.managed_model.as_deref().is_some_and(|model| !model.trim().is_empty())
                    || self.managed_path.as_deref().is_some_and(|path| !path.trim().is_empty())
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct ProxyImageRequest {
    url: String,
}

#[derive(Debug, Deserialize)]
struct OpenRouterSettingsRequest {
    /// `None` or an empty string clears the locally stored credential.
    api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterCompletionRequest {
    model_id: String,
    prompt: String,
}

#[derive(Debug, Deserialize)]
struct OpenRouterCoverRequest {
    model_id: String,
    prompt: String,
}

#[derive(Debug, Serialize)]
struct OpenRouterResponse {
    body: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_id: Option<String>,
}

/// Runs the studio service until the process is asked to stop.
///
/// This is a library entry point on purpose: the desktop application hosts it
/// in-process, so a release is a single executable rather than a launcher that
/// has to start a second binary and keep it alive.
pub async fn serve() -> anyhow::Result<()> {
    let settings_path = studio_settings_path();
    let persisted = load_studio_settings(&settings_path);
    let proxy = persisted.as_ref().and_then(|settings| settings.proxy.clone()).unwrap_or_default();
    if let Err(error) = proxy.clone().validated() {
        eprintln!("[ERROR] the saved proxy cannot be used, requests go straight out until it is fixed in Settings: {error:#}");
    }
    net::set(proxy);
    let bind_to = remote::start(persisted.as_ref().and_then(|settings| settings.network.clone()));
    let model_manager = ModelManager::from_environment()?;
    let persisted_components = persisted
        .as_ref()
        .and_then(|settings| settings.selected_component_ids.clone())
        .filter(|ids| model_manager.picked_components_installed(ids));
    let (selected_profile_id, selected_component_ids) = match persisted_components {
        Some(ids) => match model_manager::profile_matching(&ids) {
            Some(profile) => (Some(profile.to_owned()), None),
            None => (None, Some(ids)),
        },
        None => (
            persisted
                .as_ref()
                .and_then(|settings| settings.selected_profile_id.clone())
                .or_else(|| Some(hardware::recommended_local_profile().into())),
            None,
        ),
    };
    let music_server = EngineClient::from_environment();
    let engine_log = engine_log::EngineLog::follow(music_server.http.clone(), music_server.url("/logs"));
    let state = AppState {
        configuration: Arc::new(RwLock::new(sanitize_persisted_configuration(
            persisted.as_ref().map(|settings| settings.configuration.clone()).unwrap_or_else(initial_configuration),
        ))),
        jobs: Arc::new(RwLock::new(HashMap::new())),
        music_server,
        engine_log,
        model_manager,
        cover_templates: Arc::new(RwLock::new(
            persisted
                .as_ref()
                .and_then(|settings| settings.cover_templates.clone())
                .filter(|templates| !templates.is_empty())
                .unwrap_or_else(cover_prompt::default_templates),
        )),
        cover_template_default: Arc::new(RwLock::new(
            persisted.as_ref().and_then(|settings| settings.cover_template_default.clone()),
        )),
        activity: Arc::new(RwLock::new(Vec::new())),
        cover_auto: Arc::new(RwLock::new(
            persisted.as_ref().and_then(|settings| settings.cover_auto).unwrap_or(false),
        )),
        cover_look: Arc::new(RwLock::new(
            persisted.as_ref().and_then(|settings| settings.cover_look.clone()).unwrap_or_default().checked(),
        )),
        cover_problem: Arc::new(RwLock::new(None)),
        cover_pinning: Arc::new(tokio::sync::Mutex::new(())),
        separation_config: Arc::new(RwLock::new(
            persisted.as_ref().and_then(|settings| settings.separation.clone()).unwrap_or_default(),
        )),
        separator: Arc::new(separation::Separator::new(
            &studio_data_root().unwrap_or_else(|| std::path::PathBuf::from(".")),
        )),
        separation_run: Arc::new(RwLock::new(None)),
        midi: Arc::new(midi::Transcriber::new(&studio_data_root().unwrap_or_else(|| std::path::PathBuf::from(".")))),
        midi_run: Arc::new(RwLock::new(None)),
        midi_notes: Arc::new(RwLock::new(Vec::new())),
        midi_stop: Arc::new((std::sync::atomic::AtomicBool::new(false), tokio::sync::Notify::new())),
        adapters: Arc::new(adapters::AdapterLibrary::new(
            &studio_data_root().unwrap_or_else(|| std::path::PathBuf::from(".")),
            PRIMARY_MUSIC_ENGINE_ID,
        )),
        processing_run: Arc::new(RwLock::new(None)),
        prepare: Arc::new(std::sync::Mutex::new(None)),
        prepare_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        prepare_train: Arc::new(std::sync::Mutex::new(None)),
        training: Arc::new(training::Training::new(
            &studio_data_root().unwrap_or_else(|| std::path::PathBuf::from(".")),
            PRIMARY_MUSIC_ENGINE_ID,
        )),
        selected_profile_id: Arc::new(RwLock::new(selected_profile_id)),
        selected_component_ids: Arc::new(RwLock::new(selected_component_ids)),
        settings_path,
        openrouter_catalog: Arc::new(RwLock::new(OpenRouterCatalogState::default())),
        library: library::Library::open_default()?,
        engine: Arc::new(tokio::sync::Mutex::new(None)),
        engine_options: Arc::new(RwLock::new(persisted.as_ref().map(|settings| settings.engine_options).unwrap_or_default())),
        engine_runtime: Arc::new(engine_runtime::EngineRuntime::new(&engine_bundle_root())),
        failed_devices: Arc::new(RwLock::new(Vec::new())),
        engine_start_error: Arc::new(RwLock::new(None)),
        active_device: Arc::new(RwLock::new(None)),
        assistant: Arc::new(RwLock::new(persisted.as_ref().map(|settings| settings.assistant.clone()).unwrap_or_default())),
        assistant_runtime: Arc::new(assistant_runtime::AssistantRuntime::new(
            &studio_data_root().unwrap_or_else(|| std::path::PathBuf::from(".")),
        )),
        lyrics_sync: Arc::new(lyrics_sync::LyricsSync::new(
            &studio_data_root().unwrap_or_else(|| std::path::PathBuf::from(".")),
        )),
        lyrics_sync_config: Arc::new(RwLock::new(
            persisted.as_ref().map(|settings| settings.lyrics_sync.clone()).unwrap_or_default(),
        )),
    };
    processing::clear_workspace(state.library.media_dir());
    state.training.recover();
    prepare::resume(&state);
    tokio::spawn(resume_unfinished_jobs(state.clone()));
    {
        let state = state.clone();
        // both rewrite the tags of stored tracks, so one after the other
        tokio::spawn(async move {
            tag_untagged_songs(state.clone()).await;
            pin_placeholders(state).await;
        });
    }

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/capabilities", get(capabilities))
        .route("/v1/configuration", get(configuration).put(update_configuration))
        .route("/engine/options", get(engine_options).put(update_engine_options))
        .route("/engine/restart", post(restart_local_engine))
        .route("/v1/engine/logs", get(engine_logs))
        .route("/v1/engine/progress", get(engine_progress))
        .route("/v1/system/resources", get(system_resources))
        .route("/v1/proxy/image", get(proxy_image))
        .route("/v1/openrouter/settings", get(openrouter_settings).put(update_openrouter_settings))
        .route("/v1/openrouter/logs", get(openrouter_logs))
        .route("/v1/files/save", post(saving::choose))
        .route("/v1/files/save/{id}", post(saving::write).layer(DefaultBodyLimit::disable()))
        .route("/v1/files/reveal", post(saving::reveal))
        .route("/v1/skins", get(skins::list).post(skins::add).layer(DefaultBodyLimit::max(skins::LIMIT)))
        .route("/v1/skins/file/{name}", get(skins::file))
        .route("/v1/network/proxy", get(read_proxy).put(update_proxy))
        .route("/v1/network/proxy/test", post(test_proxy))
        .route("/v1/assistant/status", get(assistant_status).put(update_assistant_settings))
        .route("/v1/assistant/local-models", get(assistant_local_models))
        .route("/v1/assistant/local-key", post(set_local_server_key))
        .route("/v1/assistant/write", post(assistant_write))
        .route("/v1/assistant/write/stream", post(assistant_write_stream))
        .route("/v1/assistant/sections", post(assistant_sections))
        .route("/v1/assistant/runtime", get(assistant_runtime_status))
        .route("/v1/assistant/runtime/install", post(assistant_runtime_install))
        .route("/v1/assistant/runtime/cancel", post(cancel_assistant_download))
        .route("/v1/assistant/runtime/remove", post(assistant_runtime_remove))
        .route("/v1/assistant/runtime/start", post(assistant_runtime_start))
        .route("/v1/assistant/runtime/stop", post(assistant_runtime_stop))
        .route("/v1/karaoke/status", get(karaoke_status).put(update_karaoke_settings))
        .route("/v1/karaoke/install", post(karaoke_install))
        .route("/v1/karaoke/cancel", post(cancel_karaoke_download))
        .route("/v1/karaoke/remove", post(karaoke_remove))
        .route("/v1/library/songs/{id}/karaoke", post(create_song_karaoke).delete(delete_song_karaoke))
        .route("/v1/openrouter/catalog", get(openrouter_catalog))
        .route("/v1/openrouter/catalog/refresh", post(refresh_openrouter_catalog))
        .route("/v1/openrouter/transcriptions", post(create_openrouter_transcription))
        .route("/v1/openrouter/covers", post(create_openrouter_cover))
        .route("/editor", get(|| async { axum::response::Redirect::permanent("/editor/index.html") }))
        .route("/editor/{*path}", get(editor_asset))
        .route("/v1/separation/runtime", get(separation_assets))
        .route("/v1/separation/runtime/install", post(install_separation_asset))
        .route("/v1/separation/runtime/cancel", post(cancel_separation_download))
        .route("/v1/separation/status", get(separation_status))
        .route("/v1/separation/settings", get(read_separation_settings).put(write_separation_settings))
        .route("/v1/separation/install", post(install_separation_model))
        .route("/v1/separation/remove", post(remove_separation_model))
        .route("/v1/adapters", get(list_adapters))
        .route("/v1/adapters/import", post(import_adapter))
        .route("/v1/adapters/cancel", post(cancel_adapter_download))
        .route("/v1/adapters/install", post(install_catalog_adapters))
        .route("/v1/adapters/hub", get(search_hub_adapters))
        .route("/v1/adapters/hub/files", get(list_hub_files))
        .route("/v1/adapters/hub/install", post(install_hub_adapters))
        .route("/v1/adapters/{id}", axum::routing::patch(update_adapter).delete(delete_adapter))
        .route("/v1/library/songs/{id}/process", post(start_processing))
        .route("/v1/library/songs/{id}/version", axum::routing::put(select_song_version))
        .route("/v1/library/songs/{id}/versions/{version}", axum::routing::delete(remove_song_version))
        .route("/v1/processing", get(read_processing))
        .route("/v1/processing/vst", get(read_vst))
        .route("/v1/processing/vst/scan", post(scan_vst))
        .route("/v1/processing/vst/editor", post(open_vst_editor))
        .route("/v1/processing/preview", get(processing_preview))
        .route("/v1/processing/keep", post(keep_processing))
        .route("/v1/processing/discard", post(discard_processing))
        .route("/v1/processing/reference", post(upload_processing_reference))
        .route("/v1/training", get(read_training))
        .route("/v1/training/pack/install", post(install_training_pack))
        .route("/v1/training/pack/cancel", post(cancel_training_pack))
        .route("/v1/training/datasets", post(create_training_dataset))
        // A dataset is gigabytes of lossless audio, far over the studio's usual body limit.
        .route("/v1/training/datasets/import", post(import_training_dataset).layer(DefaultBodyLimit::max(TRAINING_UPLOAD_LIMIT)))
        .route("/v1/training/datasets/{id}/reveal", post(reveal_training_dataset))
        .route("/v1/training/datasets/{id}", axum::routing::patch(update_training_dataset).delete(delete_training_dataset))
        .route("/v1/training/datasets/{id}/songs", post(add_training_songs))
        .route("/v1/training/datasets/{id}/files", post(upload_training_files).layer(DefaultBodyLimit::max(TRAINING_UPLOAD_LIMIT)))
        .route("/v1/training/datasets/{id}/items/{item}", axum::routing::patch(update_training_item).delete(delete_training_item))
        .route("/v1/training/datasets/{id}/items/{item}/audio", get(training_item_audio))
        .route("/v1/training/datasets/{id}/prepare", post(prepare::start))
        .route("/v1/lyrics/find", post(find_lyrics))
        // a clip of a long song is more than the router's 256 MB
        .route("/v1/videos", post(store_video).layer(DefaultBodyLimit::max(TRAINING_UPLOAD_LIMIT)))
        .route("/v1/writing/guide", get(writing_guide))
        .route("/v1/writing/examples", get(writing_examples))
        .route("/v1/library/songs/{id}/files", get(library_song_files))
        .route("/v1/midi", get(midi_status))
        .route("/v1/midi/runtime", get(midi_runtime))
        .route("/v1/training/pack/runtime", get(training_pack_runtime))
        .route("/v1/training/listen/runtime", get(listen_pack_runtime))
        .route("/v1/midi/notes", get(midi_live_notes))
        .route("/v1/midi/install", post(install_midi))
        .route("/v1/midi/remove", post(remove_midi_model))
        .route("/v1/midi/cancel", post(cancel_midi))
        .route("/v1/midi/transcribe", post(start_midi))
        .route("/v1/library/songs/{id}/midi", get(read_song_midi).put(write_song_midi).delete(delete_song_midi))
        .route("/v1/library/songs/{id}/midi/file", get(song_midi_file))
        .route("/v1/training/datasets/{id}/items/{item}/files", get(dataset_song_files))
        .route("/v1/training/prepare/cancel", post(prepare::cancel))
        .route("/v1/training/datasets/{id}/take-as-is", post(prepare::take_as_is))
        .route("/v1/library/songs/{id}/describe-style", post(prepare::describe_song_style))
        .route("/v1/system/gpus", get(system_gpus))
        .route("/v1/network", get(remote::status).put(remote::change))
        .route("/v1/adapters/{id}/comfyui", get(export_adapter_comfyui).post(save_adapter_comfyui))
        .route("/v1/score/midi", post(score::api::midi))
        .route("/v1/score/instrumental", post(score::api::instrumental))
        .route("/v1/score/vocal-octave", post(score::api::vocal_octave))
        .route("/v1/score/chord-bed", post(score::api::chord_bed))
        .route("/v1/score/sections", post(match_score_sections))
        .route("/v1/score/from-midi", post(score::api::from_midi))
        .route("/v1/song/tokenize", post(song_tokenizer::tokenize))
        .route("/v1/score/mark", post(score::api::mark))
        .route("/v1/training/prepare/train-after", post(prepare::set_train_after))
        .route("/v1/training/listen/install", post(install_listen_pack))
        .route("/v1/training/runs", post(start_training))
        .route("/v1/training/runs/{id}/cancel", post(cancel_training))
        .route("/v1/training/runs/{id}/continue", post(continue_training))
        .route("/v1/training/runs/{id}", axum::routing::delete(delete_training_run))
        .route("/v1/training/runs/{id}/checkpoints/{step}/install", post(install_training_checkpoint))
        .route("/v1/library/songs/{id}/stems", get(read_stems).post(start_separation))
        .route("/v1/library/songs/{id}/stems/{stem}", get(read_stem_audio))
        .route("/v1/library/songs/{id}/cover/auto", post(draw_cover_now))
        .route("/v1/activity", get(read_activity))
        .route("/v1/cover-templates", get(read_cover_templates).put(write_cover_templates))
        .route("/v1/cover-templates/render", post(render_cover_template))
        .route("/v1/openrouter/completions", post(create_openrouter_completion))
        .route("/v1/library/songs", get(library_songs).post(create_library_song))
        .route("/v1/library/import", post(import_library_audio))
        .route("/v1/library/songs/{id}", get(library_song).put(update_library_song).delete(delete_library_song))
        .route("/v1/library/songs/{id}/liked", axum::routing::put(set_library_song_liked))
        .route("/v1/library/songs/{id}/note", axum::routing::put(set_library_song_note))
        .route("/v1/library/liked", get(library_liked))
        .route("/v1/journal", get(read_journal).post(write_journal).delete(clear_journal))
        .route("/v1/journal/{id}", axum::routing::delete(remove_journal_entry))
        .route("/v1/library/media/{song_id}", get(library_media))
        .route("/v1/library/songs/{id}/cover", get(library_cover).put(store_library_cover))
        .route("/v1/covers/look", get(read_cover_look).put(write_cover_look))
        .route("/v1/library/songs/{id}/cover/placeholder", get(placeholder_cover))
        .route("/v1/media/scenes", get(media_scenes))
        .route("/v1/media/photos", get(media_photos))
        .route("/v1/media/videos", get(media_videos))
        .route("/v1/library/songs/{id}/cover/photo", post(choose_cover_photo))
        .route("/v1/library/songs/{id}/cover/pattern", post(choose_cover_pattern))
        .route("/v1/library/playlists", get(library_playlists).post(create_library_playlist))
        .route("/v1/library/playlists/{id}", get(library_playlist).put(update_library_playlist).delete(delete_library_playlist))
        .route("/setup/status", get(setup_status))
        .route("/setup/catalog", get(setup_catalog))
        .route("/setup/download", post(setup_download))
        .route("/setup/remove", post(setup_remove))
        .route("/setup/adopt", post(setup_adopt))
        .route("/v1/open-data-directory", post(open_data_directory))
        .route("/setup/select", post(setup_select))
        .route("/setup/cancel", post(setup_cancel))
        .route("/v1/local-models/music", get(local_music_model_catalog))
        .route("/v1/music/jobs", post(create_music_job).get(list_active_music_jobs))
        .route("/v1/music/replay", post(replay_music_job))
        .route("/v1/transcriptions", post(create_transcription))
        .route("/v1/transcriptions/{job_id}", get(score_job_status).post(cancel_score_job))
        .route("/v1/scores", post(compose_score))
        .route("/v1/scores/{job_id}", get(score_job_status).post(cancel_score_job))
        .route("/v1/music/jobs/ended", get(list_ended_music_jobs))
        .route(
            "/v1/music/jobs/{job_id}",
            get(music_job_status).post(cancel_music_job).delete(dismiss_music_job),
        )
        .with_state(state.clone())
        // Covers and imported audio are megabytes, not kilobytes. The default
        // two-megabyte cap rejected a generated cover by dropping the
        // connection, which reaches the interface as "Failed to fetch".
        .layer(DefaultBodyLimit::max(256 * 1024 * 1024));
    // the MCP tools call the same routes, inside the process
    mcp::install(app.clone());
    let app = app
        // POST only: a GET is answered 405, as the stateless revision asks
        .route("/mcp", post(mcp::handle))
        .route("/mcp/status", get(mcp::status))
        .route("/mcp/window", get(mcp::window_events))
        .route("/mcp/window/result", post(mcp::window_result))
        .route("/mcp/window/focus", post(mcp::window_focus))
        .fallback(remote::interface)
        .layer(axum::middleware::from_fn(remote::guard))
        // Everything here is live state or a local file: nothing is worth a
        // browser cache, and Chrome holds a second request for a URL until a
        // cacheable first one finishes - a song asked for twice waited 20 s.
        .layer(SetResponseHeaderLayer::if_not_present(header::CACHE_CONTROL, header::HeaderValue::from_static("no-store")))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    // The provider catalog is public and small; reading it once at startup
    // means the settings panel is right the first time it is opened, instead
    // of after the user presses a refresh button.
    // Start the engine as soon as a complete set is installed. It takes about
    // three seconds; making the user press a button for it - or worse, wait
    // without knowing what for - is the studio being lazy on their time.
    // The engine is supervised, not started once and forgotten. It used to be
    // launched a single time at startup, and only if a complete set of weights
    // was already on disk - so a first installation downloaded its models,
    // nothing started them, and the window waited on "loading the models into
    // memory" until the studio was restarted by hand. The same gap swallowed a
    // crashed engine. This watches instead: whenever a complete set is on disk
    // and nothing is answering on the engine port, it brings the engine up.
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut complained = false;
            // Whether the engine was up on the last look. Only a fall from up to
            // down is a crash worth a line; an engine that has never started is
            // a startup that is failing, and repeating "stopped answering" every
            // two seconds is what filled a whole log with one sentence.
            let mut was_running = false;
            loop {
                let ready = state.model_manager.status(effective_install_target(&state).await).await.ready;
                let running = state.music_server.health().await;
                // the card has one owner: a preparation or a training run
                // holding it is left alone, and the engine comes back after
                let preparing = state.prepare.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_ref().is_some_and(|job| !job.finished);
                let card_taken = preparing || state.training.active_run().await.is_some();
                if ready && !running && !card_taken {
                    if was_running {
                        // It was answering and now it is not: the one line that
                        // explains a log which suddenly starts again from
                        // "Listening on". Written once, not once a cycle.
                        music_engine::yue_server::note_in_log("the engine stopped answering; restarting it");
                        // Under Auto a device that died of its own fault is
                        // not tried again: the restart moves down the chain.
                        let auto = state.engine_options.read().await.backend == music_engine::yue_server::ComputeBackend::Auto;
                        let active = *state.active_device.read().await;
                        if let Some(device) = active.filter(|device| auto && *device != music_engine::yue_server::ComputeBackend::Cpu) {
                            let log = last_run_log().to_lowercase();
                            if describes_device_failure(&log) || died_silently(&log) {
                                music_engine::yue_server::note_in_log(&format!("{} failed on this machine; leaving it for this session", device_name(device)));
                                state.failed_devices.write().await.push(device);
                            }
                        }
                    }
                    match restart_engine(&state).await {
                        Ok(()) => {
                            complained = false;
                            *state.engine_start_error.write().await = None;
                        }
                        Err(error) => {
                            *state.engine_start_error.write().await = Some(error.clone());
                            // Say it once per failure, not once every few
                            // seconds: a card with too little memory would
                            // otherwise fill the log with the same line.
                            if !complained {
                                eprintln!("the local engine did not start: {error}");
                                complained = true;
                            }
                        }
                    }
                }
                was_running = running;
                tokio::time::sleep(std::time::Duration::from_secs(if running { 5 } else { 2 })).await;
            }
        });
    }

    let address = SocketAddr::from((bind_to, listen_port()));
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("music-server listening on http://{address}");
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn library_songs(State(state): State<AppState>) -> Result<Json<Vec<library::Song>>, (StatusCode, Json<ApiError>)> { state.library.list_songs().map(Json).map_err(|e|api_error(StatusCode::INTERNAL_SERVER_ERROR,e.to_string())) }

#[derive(Deserialize)]
struct LikeInput {
    liked: bool,
}

#[derive(Deserialize)]
struct NoteInput {
    note: String,
}

/// The person's own note on a song, kept with it.
async fn set_library_song_note(State(state): State<AppState>, Path(id): Path<String>, Json(input): Json<NoteInput>) -> Result<Json<library::Song>, (StatusCode, Json<ApiError>)> {
    state
        .library
        .set_song_note(&id, &input.note)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map(Json)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))
}

/// The thumbs-up: set or take back, kept with the song.
async fn set_library_song_liked(State(state): State<AppState>, Path(id): Path<String>, Json(input): Json<LikeInput>) -> Result<Json<library::Song>, (StatusCode, Json<ApiError>)> {
    state
        .library
        .set_song_liked(&id, input.liked)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map(Json)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))
}

/// The liked songs, the latest like first.
async fn library_liked(State(state): State<AppState>) -> Result<Json<Vec<library::Song>>, (StatusCode, Json<ApiError>)> {
    state.library.liked_songs().map(Json).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

/// The journal's latest lines, the newest first: what agents changed and what
/// the studio told the person.
async fn read_journal(State(state): State<AppState>) -> Result<Json<Vec<library::JournalEntry>>, (StatusCode, Json<ApiError>)> {
    state.library.journal(library::JOURNAL_KEPT).map(Json).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

/// A line for the journal: an agent's change as facts (the studio's MCP writes
/// those), or a message a window showed.
async fn write_journal(State(state): State<AppState>, Json(entry): Json<library::JournalEntry>) -> Result<Json<library::JournalEntry>, (StatusCode, Json<ApiError>)> {
    let complete = match entry.source.as_str() {
        "agent" => !entry.verb.is_empty() && !entry.kind.is_empty(),
        "studio" => !entry.text.trim().is_empty(),
        _ => false,
    };
    if !complete {
        return Err(api_error(StatusCode::BAD_REQUEST, "a journal line is an agent's change (verb and kind) or a studio message (text)".into()));
    }
    state.library.note_journal(&entry).map(Json).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

async fn clear_journal(State(state): State<AppState>) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    state.library.clear_journal().map(|()| StatusCode::NO_CONTENT).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

async fn remove_journal_entry(State(state): State<AppState>, Path(id): Path<i64>) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    match state.library.remove_journal_entry(id) {
        Ok(true) => Ok(StatusCode::NO_CONTENT),
        Ok(false) => Err(api_error(StatusCode::NOT_FOUND, "no such journal line".into())),
        Err(error) => Err(api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())),
    }
}
async fn library_song(State(state): State<AppState>,Path(id):Path<String>)->Result<Json<library::Song>,(StatusCode,Json<ApiError>)>{state.library.get_song(&id).map_err(|e|api_error(StatusCode::INTERNAL_SERVER_ERROR,e.to_string()))?.map(Json).ok_or_else(||api_error(StatusCode::NOT_FOUND,"Song not found".into()))}
async fn library_media(State(state): State<AppState>, Path(song_id): Path<String>, request: Request) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let song = state.library.get_song(&song_id).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?.ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))?;
    let path = state.library.media_path_for_song(&song).ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song audio is not available in the studio media library".into()))?;
    Ok(serve_audio_file(&path, request).await)
}

/// A dataset song's recording, to listen to while its style is checked.
async fn training_item_audio(State(state): State<AppState>, Path((id, item)): Path<(String, String)>, request: Request) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let path = state.training.item_audio(&id, &item).map_err(training_error)?;
    Ok(serve_audio_file(&path, request).await)
}

/// An audio file as `<audio>` needs it: byte ranges for seeking, streamed from
/// disk instead of read whole for every range.
async fn serve_audio_file(path: &std::path::Path, request: Request) -> axum::response::Response {
    ServeFile::new(path).oneshot(request).await.into_response()
}

#[derive(Debug, Deserialize)]
struct StoreCoverRequest {
    /// Raw base64 image bytes, without a data-URL prefix.
    image_base64: String,
    media_type: String,
}

/// Cover art is Studio-side metadata: a track keeps working without one, so a
/// missing cover is a 404 the UI answers with its generated placeholder art
/// rather than an error state.
#[derive(Debug, Deserialize)]
struct CoverTemplatesRequest {
    templates: Vec<cover_prompt::CoverTemplate>,
    /// Draw a cover as soon as a track finishes.
    #[serde(default)]
    auto: Option<bool>,
    /// Which of them a new cover starts from. `None` leaves it as it was.
    #[serde(default)]
    default_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RenderCoverPromptRequest {
    template: String,
    /// The track the prompt is for. Without it the placeholders have nothing
    /// to stand in for, which is only useful for previewing the wording.
    song_id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    lyrics: Option<String>,
}


/// The waveform editor, carried inside the binary.
///
/// It is a static web application; embedding it keeps the promise that the
/// studio is one executable, and serving it over the studio's own port means
/// the browser can open it with the track already loaded.
static EDITOR: include_dir::Dir<'_> = include_dir::include_dir!("$CARGO_MANIFEST_DIR/../../app/public/editor");

async fn editor_asset(Path(path): Path<String>) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let file = EDITOR
        .get_file(path.trim_start_matches('/'))
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, format!("the editor has no file {path}")))?;
    let media_type = match std::path::Path::new(&path).extension().and_then(|value| value.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("mp3") => "audio/mpeg",
        Some("mp4") => "video/mp4",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    };
    Ok(axum::response::Response::builder()
        .header(header::CONTENT_TYPE, media_type)
        .header(header::CONTENT_LENGTH, file.contents().len())
        .body(Body::from(file.contents().to_vec()))
        .expect("valid editor response"))
}

/// One separation in progress, as the interface sees it.
#[derive(Debug, Clone, Serialize)]
struct SeparationRun {
    song_id: String,
    /// Between 0 and 1.
    progress: f64,
    done: bool,
    error: Option<String>,
    stems: Vec<String>,
    /// Whether the graphics card did the work, once the run is over.
    used_gpu: Option<bool>,
    /// The library tracks the stems became.
    library_songs: Vec<String>,
}

/// What a track made by a tool says about where it came from: the track it
/// was made from, the tool, and the settings the tool ran with.
fn derivation(from: &library::Song, tool: &str, settings: Value) -> Value {
    serde_json::json!({ "from": from.id, "from_title": from.title, "tool": tool, "settings": settings })
}

/// The track a derived track was made from, and by which tool.
fn derived_from(song: &library::Song) -> Option<(&str, &str)> {
    let derived = song.metadata.get("derived")?;
    Some((derived.get("from")?.as_str()?, derived.get("tool")?.as_str()?))
}

/// Gives a track made by a tool the cover of the one it was made from: a
/// placeholder stays a placeholder, a photograph keeps where it came from.
fn cover_like(state: &AppState, from: &library::Song, to: &str) -> anyhow::Result<()> {
    if let Some((path, media_type)) = state.library.cover_path_for_song(from) {
        let image = std::fs::read(&path).with_context(|| format!("read the cover {}", path.display()))?;
        let source = from.metadata.get("cover_source").and_then(Value::as_str);
        match from.metadata.get("cover_placeholder").and_then(Value::as_str) {
            Some(look) => state.library.store_placeholder_cover(to, &image, &media_type, look, source)?,
            None => state.library.store_chosen_cover(to, &image, &media_type, source)?,
        };
    }
    Ok(())
}

/// Every stem of a song becomes a track of the library made from it: it
/// plays, goes to the tools, makes a clip or a cover like any track, and
/// wears the original's cover. Separating the song again replaces its stems
/// in the library instead of adding more: the new ones are in before the old
/// ones go, so a stem file held open by a player costs nothing but itself.
fn stems_into_library(state: &AppState, song_id: &str, stems: &[String], overlap: f64) -> anyhow::Result<(Vec<String>, Option<String>)> {
    let original = state.library.get_song(song_id)?.ok_or_else(|| anyhow::anyhow!("the song {song_id} is gone"))?;
    let replaced: Vec<library::Song> = state
        .library
        .list_songs()?
        .into_iter()
        .filter(|old| {
            let stem = old.metadata.pointer("/derived/settings/stem").and_then(Value::as_str).unwrap_or_default();
            derived_from(old) == Some((song_id, "stems")) && stems.iter().any(|name| name == stem)
        })
        .collect();
    let mut added = Vec::new();
    for stem in stems {
        let audio = std::fs::read(stem_path(state, song_id, stem)).with_context(|| format!("read the {stem} stem"))?;
        let duration = library::audio_duration_seconds(&audio, "wav", None);
        let song = state.library.import_audio_song(library::AudioImportInput {
            title: format!("{} · {stem}", original.title),
            caption: original.caption.clone(),
            // the words belong to the voice, not to the drums
            lyrics: if stem == "vocals" { original.lyrics.clone() } else { String::new() },
            metadata: serde_json::json!({
                "derived": derivation(&original, "stems", serde_json::json!({ "stem": stem, "model": "HT-Demucs", "overlap": overlap })),
                "duration_seconds": duration,
            }),
            generation_settings: Value::Null,
            engine_id: "stems".into(),
            profile_id: None,
            source: "stems".into(),
            audio_extension: "wav".into(),
            audio,
        })?.song;
        cover_like(state, &original, &song.id)?;
        added.push(song.id);
    }
    let mut kept = Vec::new();
    for old in replaced {
        let files = song_files(state, &old);
        state.library.delete_song(&old.id)?;
        for path in files {
            if let Err(error) = std::fs::remove_file(&path) {
                kept.push(format!("{}: {error}", path.display()));
            }
        }
    }
    let problem = (!kept.is_empty()).then(|| format!("the new stems are in the library, but files of the old ones could not be removed: {}", kept.join("; ")));
    Ok((added, problem))
}

/// Where a song's stems live: beside the track, named after it.
fn stem_path(state: &AppState, song_id: &str, stem: &str) -> PathBuf {
    state.library.media_dir().join(format!("{song_id}-{stem}.wav"))
}

fn stems_on_disk(state: &AppState, song_id: &str) -> Vec<String> {
    separation::STEMS
        .iter()
        .filter(|stem| stem_path(state, song_id, stem).is_file())
        .map(|stem| (*stem).to_string())
        .collect()
}

/// The separator as an optional module: its files, and whichever download is
/// running. The same envelope the assistant and karaoke use, because the models
/// page lists all three the same way.
async fn separation_assets(State(state): State<AppState>) -> Json<Value> {
    let runtime_installed = state.lyrics_sync.onnxruntime_library().is_some();
    let assets = serde_json::json!([
        {
            "id": separation::MODEL.id,
            "label": separation::MODEL.label,
            "bytes": separation::MODEL.bytes,
            "note": separation::MODEL.note,
            "installed": state.separator.is_installed(),
        },
        {
            "id": "onnxruntime-cuda",
            "label": "ONNX Runtime 1.30.0 · CUDA",
            "bytes": 379_723_801u64,
            "note": "The CUDA build of the runtime.",
            "installed": state.lyrics_sync.has_cuda_runtime(),
        },
        {
            "id": "cuda-cublas",
            "label": "NVIDIA cuBLAS 12.9",
            "bytes": 549_731_131u64,
            "note": "The linear algebra the CUDA provider is built on.",
            "installed": state.lyrics_sync.downloader().runtime_dir("onnx-cuda").join("cublasLt64_12.dll").is_file(),
        },
        {
            "id": "cuda-cudart",
            "label": "NVIDIA CUDA runtime 12.9",
            "bytes": 3_521_238u64,
            "note": "The CUDA runtime itself.",
            "installed": state.lyrics_sync.downloader().runtime_dir("onnx-cuda").join("cudart64_12.dll").is_file(),
        },
        {
            "id": "cuda-cudnn",
            "label": "NVIDIA cuDNN 9.25",
            "bytes": 1_904_452_100u64,
            "note": "The convolution kernels the separator spends its time in.",
            "installed": state.lyrics_sync.downloader().runtime_dir("onnx-cuda").join("cudnn64_9.dll").is_file(),
        },
        {
            "id": "onnxruntime",
            "label": "ONNX Runtime 1.30.0",
            "bytes": 82_645_522,
            "note": "Runs the separator and the karaoke recogniser; shared between them.",
            "installed": runtime_installed,
        }
    ]);
    let config = state.separation_config.read().await.clone();
    let mut set: Vec<&'static lyrics_sync::Asset> = Vec::new();
    if let Some(asset) = lyrics_sync::asset("onnxruntime") { set.push(asset); }
    set.extend(separation_card_assets(config.runtime).iter().filter_map(|id| lyrics_sync::asset(id)));
    let runtime_progress = set_progress(state.lyrics_sync.downloader(), &set);
    let model_installed = state.separator.is_installed();
    let bytes = runtime_progress["bytes"].as_u64().unwrap_or(0) + separation::MODEL.bytes;
    let installed_bytes = runtime_progress["installed_bytes"].as_u64().unwrap_or(0)
        + if model_installed { separation::MODEL.bytes } else { 0 };
    Json(serde_json::json!({
        "assets": assets,
        "settings": { "runtime": config.runtime },
        "set": {
            "bytes": bytes,
            "installed_bytes": installed_bytes,
            "ready": installed_bytes == bytes,
            "files": set.len() + 1,
        },
        // Only this panel's own download. The recogniser shares this
        // downloader, and its gigabytes are not the separator's business.
        "active_download": state.separator.downloader().active_for("separation").await.or(state.lyrics_sync.downloader().active_for("separation").await),
        // Not an error and not a stall: the file server is asking us to wait.
        "waiting_for_server": crate::chunked::waiting_for_server(),
    }))
}

#[derive(Debug, Deserialize)]
struct InstallSeparationAssetRequest {
    asset_id: String,
}

/// Everything the card path needs, in the order it is used. `karaoke_set`
/// builds a recogniser out of these plus its own model files, so this is the
/// one place the CUDA provider's parts are named.
const CUDA_ASSETS: [&str; 5] = ["onnxruntime-cuda", "cuda-cudart", "cuda-cublas", "cuda-cufft", "cuda-cudnn"];

/// The runtime parts work set to `runtime` needs on this machine's card:
/// CUDA's on an NVIDIA card that runs it, DirectML's on any other card,
/// none on the processor.
fn card_assets(runtime: lyrics_sync::OnnxFlavour) -> &'static [&'static str] {
    match runtime.card() {
        Some(lyrics_sync::OnnxCard::Cuda) => &CUDA_ASSETS,
        Some(lyrics_sync::OnnxCard::DirectMl) => &lyrics_sync::DIRECTML_ASSETS,
        None => &[],
    }
}

/// Whether separation runs on this card: CUDA only. HT-Demucs's ONNX export
/// does not run through DirectML - out of memory on a 2 GB card, over twenty
/// gigabytes and minutes for 30 seconds on a 24 GB one - so every other card
/// separates on the processor.
fn separates_on(card: lyrics_sync::OnnxCard) -> bool {
    card == lyrics_sync::OnnxCard::Cuda
}

/// The runtime parts separation needs on this machine's card.
fn separation_card_assets(runtime: lyrics_sync::OnnxFlavour) -> &'static [&'static str] {
    if runtime.card().is_some_and(separates_on) { &CUDA_ASSETS } else { &[] }
}

/// Stops a download. What arrived stays on disk: pressing this again later
/// carries on from the last finished piece rather than starting the file over.
///
/// Two panels can be downloading at once, and until this existed the only way
/// to stop one of them was to close the studio.
async fn cancel_separation_download(State(state): State<AppState>) -> Json<Value> {
    state.separator.downloader().cancel();
    state.lyrics_sync.downloader().cancel();
    Json(serde_json::json!({ "cancelled": true }))
}

async fn cancel_karaoke_download(State(state): State<AppState>) -> Json<Value> {
    state.lyrics_sync.downloader().cancel();
    Json(serde_json::json!({ "cancelled": true }))
}

/// Frees the disk an assistant takes: the model, the runtime and any half of
/// either. Every other capability could be removed from its panel; this one
/// could only be added.
async fn assistant_runtime_remove(
    State(state): State<AppState>,
    Json(request): Json<AssistantAssetRequest>,
) -> Result<Json<assistant_runtime::RuntimeStatus>, (StatusCode, Json<ApiError>)> {
    // "managed" from the panel means the whole thing: whichever llama.cpp build
    // is on disk, and the model that was chosen with it.
    let ids: Vec<String> = if request.asset_id == "managed" || request.asset_id == "cuda" || request.asset_id == "cpu" {
        let chosen = state.assistant.read().await.managed_model.clone();
        ["llama-cuda", "llama-cuda-runtime", "llama-cuda12", "llama-cuda12-runtime", "llama-vulkan", "llama-cpu"].iter().map(|id| id.to_string()).chain(chosen).collect()
    } else {
        vec![request.asset_id.clone()]
    };
    for id in &ids {
        state
            .assistant_runtime
            .remove(id)
            .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    }
    {
        let mut assistant = state.assistant.write().await;
        assistant.managed_model = None;
    }
    let _ = persist_studio_settings(&state).await;
    Ok(Json(state.assistant_runtime.status().await))
}

async fn cancel_assistant_download(State(state): State<AppState>) -> Json<Value> {
    state.assistant_runtime.cancel();
    Json(serde_json::json!({ "cancelled": true }))
}

async fn install_separation_asset(
    State(state): State<AppState>,
    Json(request): Json<InstallSeparationAssetRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    // "card" means whatever is still missing for the graphics card, one after
    // another: asking someone to press four buttons in the right order is not a
    // setup, it is a quiz.
    // The separator as one thing: its model, the runtime that loads it, and -
    // for the card - the CUDA provider. Six rows of file names asked the user
    // to work out which of them belong together.
    if matches!(request.asset_id.as_str(), "auto" | "cuda" | "cpu") {
        let wanted = if request.asset_id == "cpu" { lyrics_sync::OnnxFlavour::Cpu } else { lyrics_sync::OnnxFlavour::Auto };
        let separator = state.separator.clone();
        let sync = state.lyrics_sync.clone();
        let mut runtime: Vec<&'static lyrics_sync::Asset> = Vec::new();
        if let Some(asset) = lyrics_sync::asset("onnxruntime") { runtime.push(asset); }
        runtime.extend(separation_card_assets(wanted).iter().filter_map(|id| lyrics_sync::asset(id)));
        tokio::spawn(async move {
            if let Err(error) = separator.downloader().install_all("separation", &[&separation::MODEL]).await {
                eprintln!("the separator model could not be installed: {error}");
                return;
            }
            if let Err(error) = sync.downloader().install_all("separation", &runtime).await {
                eprintln!("the separator runtime could not be installed: {error}");
            }
        });
        return Ok(Json(serde_json::json!({ "started": true })));
    }
    if request.asset_id == "card" {
        let sync = state.lyrics_sync.clone();
        let card: Vec<&'static lyrics_sync::Asset> = separation_card_assets(lyrics_sync::OnnxFlavour::Auto).iter().filter_map(|id| lyrics_sync::asset(id)).collect();
        tokio::spawn(async move {
            if let Err(error) = sync.downloader().install_all("separation", &card).await {
                eprintln!("the card path could not be installed: {error}");
            }
        });
        return Ok(Json(serde_json::json!({ "started": true })));
    }
    // Anything in the catalogue may be installed by name; listing the ids here
    // by hand is how cuFFT ended up silently rejected.
    // `install` starts a background task and returns immediately; its Result
    // says whether the download was accepted at all. Discarding it inside a
    // spawn - which is what this did - meant a refusal ("another download is
    // already running") was thrown away while the endpoint answered
    // "started: true". The button then did nothing, twice, silently, with a
    // successful reply, and no amount of error handling in the interface could
    // have shown it.
    if request.asset_id == separation::MODEL.id {
        state
            .separator
            .downloader()
            .install(&separation::MODEL)
            .await
            .map_err(|error| api_error(StatusCode::CONFLICT, error.to_string()))?;
    } else if let Some(asset) = lyrics_sync::asset(&request.asset_id) {
        state
            .lyrics_sync
            .downloader()
            .install(asset)
            .await
            .map_err(|error| api_error(StatusCode::CONFLICT, error.to_string()))?;
    } else {
        return Err(api_error(StatusCode::BAD_REQUEST, format!("unknown asset {}", request.asset_id)));
    }
    Ok(Json(serde_json::json!({ "started": true })))
}

async fn read_separation_settings(State(state): State<AppState>) -> Json<separation::SeparationConfig> {
    Json(state.separation_config.read().await.clone())
}

async fn write_separation_settings(
    State(state): State<AppState>,
    Json(config): Json<separation::SeparationConfig>,
) -> Result<Json<separation::SeparationConfig>, (StatusCode, Json<ApiError>)> {
    // A run that writes nothing is a run nobody wanted; an empty choice means
    // everything, which is also what the studio starts with.
    let stems: Vec<String> = if config.stems.is_empty() {
        separation::STEMS.iter().map(|stem| (*stem).to_string()).collect()
    } else {
        config.stems.iter().filter(|stem| separation::STEMS.contains(&stem.as_str())).cloned().collect()
    };
    let stored = separation::SeparationConfig { runtime: config.runtime, stems, overlap: config.sane_overlap() };
    *state.separation_config.write().await = stored.clone();
    persist_studio_settings(&state)
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(stored))
}

async fn separation_status(State(state): State<AppState>) -> Json<Value> {
    let runtime = state.lyrics_sync.onnxruntime_library();
    let card = lyrics_sync::OnnxFlavour::Auto.card().filter(|card| separates_on(*card));
    Json(serde_json::json!({
        "model": {
            "id": separation::MODEL.id,
            "label": separation::MODEL.label,
            "bytes": separation::MODEL.bytes,
            "note": separation::MODEL.note,
            "installed": state.separator.is_installed(),
        },
        "runtime_installed": runtime.is_some(),
        // the card separation takes on this machine, none on any card but
        // CUDA's, and whether every library of it is installed
        "card": card,
        "card_runtime_installed": card.is_some_and(|card| state.lyrics_sync.has_card_libraries(card)),
        "card_missing_bytes": separation_card_assets(lyrics_sync::OnnxFlavour::Auto)
            .iter()
            .filter_map(|id| lyrics_sync::asset(id))
            .filter(|asset| !state.lyrics_sync.downloader().is_installed(asset))
            .map(|asset| asset.bytes)
            .sum::<u64>(),
        "ready": state.separator.ready(runtime.as_deref()),
        "stems": separation::STEMS,
        // Either downloader may be the busy one: the model has its own, the
        // card libraries come through karaoke's. Reporting only the first is
        // what made a running download look like a dead button.
        "download": match state.separator.downloader().active().await {
            Some(active) if !active.done => Some(active),
            other => match state.lyrics_sync.downloader().active().await {
                Some(active) if !active.done => Some(active),
                fallback => fallback.or(other),
            },
        },
        "settings": state.separation_config.read().await.clone(),
        "run": state.separation_run.read().await.clone(),
    }))
}

/// Fetches the separation model. Nothing here downloads on its own; this is the
/// button, and it also brings the runtime if karaoke has not already.
async fn install_separation_model(State(state): State<AppState>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let separator = state.separator.clone();
    let sync = state.lyrics_sync.clone();
    tokio::spawn(async move {
        if sync.onnxruntime_library().is_none() {
            if let Some(runtime) = lyrics_sync::asset("onnxruntime") {
                let _ = sync.downloader().install(runtime).await;
            }
        }
        let _ = separator.downloader().install(&separation::MODEL).await;
    });
    Ok(Json(serde_json::json!({ "started": true })))
}

/// The adapter page: the parts of the model an adapter can change, the
/// installed adapters with what the engine found in each, the catalogue, and
/// the download in progress. The engine is asked only when it is already up;
/// a stopped engine leaves the slots the studio remembered.
async fn list_adapters(State(state): State<AppState>) -> Json<Value> {
    let slot_ids: Vec<&str> = music_engine::yue_server::ADAPTER_SLOTS.iter().map(|slot| slot.id).collect();
    let views = match tokio::time::timeout(std::time::Duration::from_secs(3), state.music_server.props()).await {
        Ok(Ok(props)) => Some(adapters::engine_views(&props, &slot_ids)),
        _ => None,
    };
    Json(serde_json::json!({
        "slots": music_engine::yue_server::ADAPTER_SLOTS,
        "installed": state.adapters.installed(views.as_ref()),
        "catalog": state.adapters.offered(),
        // one size of the model: nothing for an adapter to be the wrong size for
        "model": Value::Null,
        "engine_checked": views.is_some(),
        "download": state.adapters.downloader().active_for(adapters::SCOPE).await,
        "installing": state.adapters.installing(),
    }))
}

#[derive(Debug, Deserialize)]
struct InstallAdaptersRequest {
    /// Catalogue entries to fetch as one download.
    ids: Vec<String>,
}

async fn install_catalog_adapters(
    State(state): State<AppState>,
    Json(input): Json<InstallAdaptersRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    state.adapters.begin_install(&input.ids).map_err(|error| api_error(StatusCode::CONFLICT, error.to_string()))?;
    Ok(Json(serde_json::json!({ "started": true })))
}

#[derive(Debug, Deserialize)]
struct HubQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    repo: String,
}

async fn search_hub_adapters(State(state): State<AppState>, Query(query): Query<HubQuery>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let found = state.adapters.hub_search(&net::client(), &query.q).await.map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("{error:#}")))?;
    Ok(Json(serde_json::json!({ "repos": found })))
}

/// The weight files of a repository; `repo` may be an id or any link into it,
/// and a link to one file comes back with that file named.
async fn list_hub_files(State(state): State<AppState>, Query(query): Query<HubQuery>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let (repo, file) = adapters::hub_reference(&query.repo)
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "that is not a Hugging Face repository or file link".into()))?;
    let listing = state.adapters.hub_files(&net::client(), &repo).await.map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("{error:#}")))?;
    Ok(Json(serde_json::json!({ "listing": listing, "file": file })))
}

#[derive(Debug, Deserialize)]
struct InstallHubRequest {
    repo: String,
    paths: Vec<String>,
}

async fn install_hub_adapters(State(state): State<AppState>, Json(input): Json<InstallHubRequest>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    state
        .adapters
        .begin_hub_install(&net::client(), &input.repo, &input.paths)
        .await
        .map_err(|error| api_error(StatusCode::CONFLICT, format!("{error:#}")))?;
    Ok(Json(serde_json::json!({ "started": true })))
}

async fn cancel_adapter_download(State(state): State<AppState>) -> Json<Value> {
    state.adapters.downloader().cancel();
    Json(serde_json::json!({ "cancelled": true }))
}

/// Stores uploaded adapter files: one or more `.safetensors`, and the
/// `adapter_config.json` or `lora.json` that came with them.
async fn import_adapter(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<adapters::AdapterMeta>), (StatusCode, Json<ApiError>)> {
    let mut name = String::new();
    let mut files = Vec::new();
    while let Some(field) = multipart.next_field().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read adapter form: {e}")))? {
        match (field.name().unwrap_or_default().to_owned(), field.file_name().map(str::to_owned)) {
            (key, Some(file)) if key == "files" => {
                let bytes = field.bytes().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read {file}: {e}")))?;
                files.push((file, bytes.to_vec()));
            }
            (key, _) if key == "name" => {
                name = field.text().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read adapter name: {e}")))?;
            }
            _ => {}
        }
    }
    if name.trim().is_empty() {
        name = files
            .iter()
            .find(|(file, _)| file.ends_with(".safetensors"))
            .and_then(|(file, _)| std::path::Path::new(file).file_stem().and_then(|stem| stem.to_str()).map(str::to_owned))
            .unwrap_or_else(|| "Adapter".into());
    }
    let meta = state
        .adapters
        .import(&name, files, adapters::Origin::Imported)
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok((StatusCode::CREATED, Json(meta)))
}

async fn update_adapter(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(patch): Json<adapters::Patch>,
) -> Result<Json<adapters::AdapterMeta>, (StatusCode, Json<ApiError>)> {
    state.adapters.update(&id, patch).map(Json).map_err(|e| api_error(StatusCode::NOT_FOUND, e.to_string()))
}

async fn delete_adapter(State(state): State<AppState>, Path(id): Path<String>) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    state.adapters.remove(&id).map_err(|e| api_error(StatusCode::NOT_FOUND, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Starts processing a track into a preview. One run at a time: the stages are
/// quick, and a second request would only race the first for the preview.
async fn start_processing(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<processing::ProcessRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    if request.stages().is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "choose at least one kind of processing".into()));
    }
    let song = state
        .library
        .get_song(&id)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))?;
    let source = state
        .library
        .media_path_for_song(&song)
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "this track has no stored audio".into()))?;
    let media = state.library.media_dir().to_path_buf();
    let reference = match &request.master {
        None => None,
        Some(processing::MasterSource::Song { song_id }) => {
            let reference_song = state
                .library
                .get_song(song_id)
                .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
                .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "the reference track is not in the library".into()))?;
            Some(
                state
                    .library
                    .media_path_for_song(&reference_song)
                    .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "the reference track has no stored audio".into()))?,
            )
        }
        Some(processing::MasterSource::Upload { upload_id }) => Some(
            processing::workspace_file(&media, upload_id)
                .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "the uploaded reference is gone; upload it again".into()))?,
        ),
    };

    let run_id = uuid::Uuid::now_v7().simple().to_string();
    {
        // checked and claimed under one lock, so two requests cannot both start
        let mut current = state.processing_run.write().await;
        if current.as_ref().is_some_and(|run| !run.done) {
            return Err(api_error(StatusCode::CONFLICT, "a track is already being processed".into()));
        }
        // a new run replaces the last preview nobody kept; its reference stays
        // when this run masters to the same upload
        if let Some(previous) = current.take() {
            let keep = reference.clone();
            for file in previous.leftovers(&media).into_iter().filter(|file| Some(file) != keep.as_ref()) {
                let _ = std::fs::remove_file(file);
            }
        }
        *current = Some(processing::ProcessRun {
            id: run_id.clone(),
            song_id: id.clone(),
            stages: request.stages(),
            stage: None,
            done: false,
            error: None,
            preview: None,
            preview_ready: false,
            request: request.clone(),
        });
    }

    let background = state.clone();
    tokio::task::spawn_blocking(move || {
        let handle = tokio::runtime::Handle::current();
        let current = |run: &Option<processing::ProcessRun>| run.as_ref().is_some_and(|run| run.id == run_id);
        let outcome = (|| -> anyhow::Result<std::path::PathBuf> {
            let vst = studio_data_root().and_then(|root| vst::VstHost::locate(&root));
            let audio = processing::run(&source, reference.as_deref(), &request, vst.as_ref(), |stage| {
                let state = background.clone();
                let run_id = run_id.clone();
                handle.spawn(async move {
                    if let Some(run) = state.processing_run.write().await.as_mut().filter(|run| run.id == run_id) {
                        run.stage = Some(stage);
                    }
                });
            })?;
            let folder = processing::workspace(&media);
            std::fs::create_dir_all(&folder)?;
            let path = folder.join(format!("{id}-{}.flac", &run_id[run_id.len() - 8..]));
            std::fs::write(&path, audio_post::encode::flac(&audio)?).with_context(|| format!("write {}", path.display()))?;
            Ok(path)
        })();
        handle.block_on(async {
            let mut guard = background.processing_run.write().await;
            if !current(&guard) {
                // discarded while it worked: nothing will ever ask for the preview
                if let Ok(path) = &outcome {
                    let _ = std::fs::remove_file(path);
                }
                return;
            }
            let run = guard.as_mut().expect("checked above");
            run.done = true;
            match outcome {
                Ok(path) => {
                    run.preview = path.file_name().and_then(|name| name.to_str()).map(str::to_owned);
                    run.preview_ready = run.preview.is_some();
                }
                Err(error) => run.error = Some(format!("{error:#}")),
            }
        });
    });
    Ok(Json(serde_json::json!({ "started": true })))
}

/// The VST host and the plugins its last scan found; `available` is false
/// when the host did not ship with this build.
async fn read_vst() -> Json<Value> {
    let host = studio_data_root().and_then(|root| vst::VstHost::locate(&root));
    Json(serde_json::json!({ "available": host.is_some(), "plugins": host.and_then(|host| host.plugins()) }))
}

async fn scan_vst() -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let host = studio_data_root()
        .and_then(|root| vst::VstHost::locate(&root))
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "the VST host is not installed with this studio".into()))?;
    let plugins = tokio::task::spawn_blocking(move || host.scan())
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("{error:#}")))?;
    Ok(Json(serde_json::json!({ "available": true, "plugins": plugins })))
}

#[derive(Debug, Deserialize)]
struct VstEditorRequest {
    path: String,
    #[serde(default)]
    state_id: Option<String>,
}

/// Opens a plugin's window; the settings save into the slot's state when it closes.
async fn open_vst_editor(Json(input): Json<VstEditorRequest>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let host = studio_data_root()
        .and_then(|root| vst::VstHost::locate(&root))
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "the VST host is not installed with this studio".into()))?;
    let state_id = host.open_editor(&input.path, input.state_id).map_err(|error| api_error(StatusCode::BAD_REQUEST, format!("{error:#}")))?;
    Ok(Json(serde_json::json!({ "state_id": state_id })))
}

async fn read_processing(State(state): State<AppState>) -> Json<Value> {
    Json(serde_json::json!({ "run": state.processing_run.read().await.clone() }))
}

async fn processing_preview(State(state): State<AppState>, request: Request) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let name = state.processing_run.read().await.as_ref().and_then(|run| run.preview.clone());
    let path = name
        .and_then(|name| processing::workspace_file(state.library.media_dir(), &name))
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "there is no processed preview".into()))?;
    Ok(serve_audio_file(&path, request).await)
}

#[derive(Debug, Deserialize)]
struct KeepProcessingRequest {
    /// What the processing is called on the new track; its stages when left out.
    #[serde(default)]
    label: String,
}

/// Keeps the preview as a version of its track, playing from now on.
async fn keep_processing(
    State(state): State<AppState>,
    Json(input): Json<KeepProcessingRequest>,
) -> Result<Json<library::Song>, (StatusCode, Json<ApiError>)> {
    let run = state.processing_run.read().await.clone().filter(|run| run.preview_ready);
    let run = run.ok_or_else(|| api_error(StatusCode::NOT_FOUND, "there is no processed preview to keep".into()))?;
    let media = state.library.media_dir().to_path_buf();
    let preview = run
        .preview
        .as_deref()
        .and_then(|name| processing::workspace_file(&media, name))
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "the preview file is gone".into()))?;
    let extension = preview.extension().and_then(|extension| extension.to_str()).unwrap_or("flac").to_owned();
    let filename = format!("{}-v{}-{}.{extension}", run.song_id, &uuid::Uuid::now_v7().simple().to_string()[..8], run.stages.join("-"));
    let stored = media.join(&filename);
    std::fs::rename(&preview, &stored).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("store the version: {error}")))?;
    let reference_title = match &run.request.master {
        Some(processing::MasterSource::Song { song_id }) => state.library.get_song(song_id).ok().flatten().map(|song| song.title),
        _ => None,
    };
    let settings = processing::settings_record(&run.request, reference_title.as_deref());
    // the result is a track of its own, made from the one it processed
    let recorded = (|| -> anyhow::Result<library::Song> {
        let original = state.library.get_song(&run.song_id)?.ok_or_else(|| anyhow::anyhow!("Song not found"))?;
        let audio = std::fs::read(&stored).with_context(|| format!("read {}", stored.display()))?;
        let label = if input.label.trim().is_empty() { run.stages.join(" + ") } else { input.label.trim().to_string() };
        let mut settings = settings;
        settings["label"] = Value::from(label.clone());
        let song = state.library.create_song(library::SongInput {
            title: format!("{} · {label}", original.title),
            audio_path: Some(stored.display().to_string()),
            caption: original.caption.clone(),
            lyrics: original.lyrics.clone(),
            metadata: serde_json::json!({
                "derived": derivation(&original, "processing", settings),
                "duration_seconds": library::audio_duration_seconds(&audio, &extension, None),
            }),
            generation_settings: Value::Null,
            engine_id: "processing".into(),
            profile_id: None,
            replay_request: None,
            audio_codes: None,
            source: "processing".into(),
        })?;
        cover_like(&state, &original, &song.id)?;
        Ok(song)
    })()
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}")));
    let song = match recorded {
        Ok(song) => song,
        Err(problem) => {
            // the preview goes back where it was, so the run can still be kept or discarded
            if let Err(error) = std::fs::rename(&stored, &preview) {
                eprintln!("[ERROR] processing: return {} to the workspace: {error}", stored.display());
            }
            return Err(problem);
        }
    };
    let mut current = state.processing_run.write().await;
    if current.as_ref().is_some_and(|now| now.id == run.id) {
        for file in current.take().map(|run| run.leftovers(&media)).unwrap_or_default() {
            let _ = std::fs::remove_file(file);
        }
    }
    Ok(Json(song))
}

/// Forgets the run, finished or not. A worker still going sees it is no longer
/// current and removes its own output.
async fn discard_processing(State(state): State<AppState>) -> Json<Value> {
    if let Some(run) = state.processing_run.write().await.take() {
        for file in run.leftovers(state.library.media_dir()) {
            let _ = std::fs::remove_file(file);
        }
    }
    Json(serde_json::json!({ "discarded": true }))
}

/// Stores a reference recording for mastering. It waits in the processing
/// folder, not the library: a reference is a tool, not a song.
async fn upload_processing_reference(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    while let Some(field) = multipart.next_field().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read the upload: {e}")))? {
        if field.name() != Some("audio") {
            continue;
        }
        let original = field.file_name().unwrap_or("reference").to_owned();
        let extension = std::path::Path::new(&original)
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .filter(|value| matches!(value.as_str(), "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aiff" | "aif"))
            .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "the reference must be MP3, WAV, FLAC, OGG or M4A".into()))?;
        let bytes = field.bytes().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read the upload: {e}")))?;
        let folder = processing::workspace(state.library.media_dir());
        std::fs::create_dir_all(&folder).map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let upload_id = format!("reference-{}.{extension}", uuid::Uuid::now_v7().simple());
        std::fs::write(folder.join(&upload_id), &bytes).map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        return Ok(Json(serde_json::json!({ "upload_id": upload_id, "name": original })));
    }
    Err(api_error(StatusCode::BAD_REQUEST, "no audio part in the upload".into()))
}

#[derive(Debug, Deserialize)]
struct SelectVersionRequest {
    /// `original`, or the id of one of the track's versions.
    version: String,
}

async fn select_song_version(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<SelectVersionRequest>,
) -> Result<Json<library::Song>, (StatusCode, Json<ApiError>)> {
    let song = state
        .library
        .select_song_version(&id, &input.version)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))?;
    tag_stored_song(&state, &id).await;
    Ok(Json(song))
}

async fn remove_song_version(
    State(state): State<AppState>,
    Path((id, version)): Path<(String, String)>,
) -> Result<Json<library::Song>, (StatusCode, Json<ApiError>)> {
    let (song, file) = state
        .library
        .remove_song_version(&id, &version)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))?;
    if let Some(file) = file {
        if let Err(error) = std::fs::remove_file(&file) {
            eprintln!("[ERROR] remove version {version} of {id}: {error}");
        }
    }
    Ok(Json(song))
}

/// The studio's separator, handed to training for the vocals lyric timing
/// and lyric recognition are measured on.
struct StudioSeparator {
    model: PathBuf,
    overlap: f64,
    /// The runtime the separation settings chose. The card it reaches is
    /// settled when the first song is separated: that binds the process's
    /// ONNX Runtime, which a status read must never do.
    runtime: lyrics_sync::OnnxFlavour,
    sync: Arc<lyrics_sync::LyricsSync>,
    /// Loaded on the first song and kept for the rest: one separator is made
    /// per run of songs and dropped with it, taking the card's memory along.
    loaded: std::sync::Mutex<Option<separation::Loaded>>,
}

impl training::VocalSeparator for StudioSeparator {
    fn separate(&self, mix: &std::path::Path, out: &std::path::Path) -> anyhow::Result<()> {
        let audio = audio_pcm::decode_stereo_44k(mix)?;
        let mut loaded = self.loaded.lock().map_err(|_| anyhow::anyhow!("the separator failed on an earlier song"))?;
        if loaded.is_none() {
            let card = self.sync.onnx_card(self.runtime)?.filter(|card| separates_on(*card));
            *loaded = Some(separation::load(&self.model, card)?);
        }
        let model = loaded.as_mut().expect("loaded above");
        let separated = separation::separate_with(model, &audio, separation::STEMS.len(), self.overlap, |_| {})?;
        let vocals = separated.stems.into_iter().find(|stem| stem.name == "vocals").context("the separator returned no vocals")?;
        separation::write_wav_stereo(out, &vocals.samples)
    }
}

/// The separator when its model and a runtime are on disk. The training page
/// and the models page ask this on every read, so it only looks at the files:
/// a runtime bound here would be the processor build whenever that lands
/// before the card's libraries, and it stays bound until the studio restarts.
async fn vocal_separator(state: &AppState) -> Option<Arc<dyn training::VocalSeparator>> {
    if !state.separator.is_installed() || state.lyrics_sync.onnxruntime_library().is_none() {
        return None;
    }
    let config = state.separation_config.read().await.clone();
    Some(Arc::new(StudioSeparator {
        model: state.separator.model_path(),
        overlap: config.sane_overlap(),
        runtime: config.runtime,
        sync: state.lyrics_sync.clone(),
        loaded: std::sync::Mutex::new(None),
    }))
}

/// Everything the separator still lacks, through the same downloaders the
/// separator's own panel uses; the card path unless the processor was chosen.
async fn install_separator(state: &AppState) {
    let wanted = state.separation_config.read().await.runtime;
    let separator = state.separator.clone();
    let sync = state.lyrics_sync.clone();
    let mut runtime: Vec<&'static lyrics_sync::Asset> = Vec::new();
    if let Some(asset) = lyrics_sync::asset("onnxruntime") {
        runtime.push(asset);
    }
    runtime.extend(separation_card_assets(wanted).iter().filter_map(|id| lyrics_sync::asset(id)));
    runtime.retain(|asset| !sync.downloader().is_installed(asset));
    if !separator.is_installed() {
        if let Err(error) = separator.downloader().install_all("separation", &[&separation::MODEL]).await {
            eprintln!("[ERROR] the separator model could not be installed: {error:#}");
            return;
        }
    }
    if !runtime.is_empty() {
        if let Err(error) = sync.downloader().install_all("separation", &runtime).await {
            eprintln!("[ERROR] the separator runtime could not be installed: {error:#}");
        }
    }
}

/// What one upload of songs to a dataset may weigh.
const TRAINING_UPLOAD_LIMIT: usize = 16 * 1024 * 1024 * 1024;

fn training_error(error: anyhow::Error) -> (StatusCode, Json<ApiError>) {
    api_error(StatusCode::BAD_REQUEST, format!("{error:#}"))
}

/// The training page: what is installed, the datasets, the runs, and what the
/// run in progress is doing.
async fn read_training(State(state): State<AppState>) -> Json<Value> {
    let training = &state.training;
    let active = training.active_run().await;
    let runs: Vec<Value> = training
        .runs()
        .into_iter()
        .map(|run| {
            let checkpoints = training.checkpoints(&run.id);
            let mut value = serde_json::to_value(&run).unwrap_or(Value::Null);
            value["checkpoints"] = serde_json::json!(checkpoints.iter().map(|checkpoint| checkpoint.step).collect::<Vec<_>>());
            // where "train further" starts, or why it cannot; a run going now has neither
            if run.status != training::RunStatus::Running {
                match training.resume_point(&run.id) {
                    Ok((step, _)) => value["resume_step"] = serde_json::json!(step),
                    Err(reason) => value["resume_refused"] = serde_json::json!(reason),
                }
            }
            if active.as_deref() == Some(run.id.as_str()) || run.status == training::RunStatus::Failed {
                value["log"] = serde_json::json!(training.log_tail(&run.id, 12));
            }
            if active.as_deref() == Some(run.id.as_str()) {
                value["device"] = serde_json::json!(training.run_device(&run.id));
            }
            value
        })
        .collect();
    let (pack, separator_ready) = training_pack_files(&state).await;
    let listen_download = training.downloader().active_for(training::LISTEN_SCOPE).await.filter(|active| !active.done);
    Json(serde_json::json!({
        "pack": pack,
        "pack_ready": training.pack_ready() && !trainer_cublas_missing(&state),
        "separator_ready": separator_ready,
        "recipe_defaults": training::Recipe::default(),
        "recipe_fields": music_engine::yue_train::recipe_fields(),
        "min_vram_gb": music_engine::yue_train::MIN_VRAM_GB,
        "card_trains": trainer_card_refusal().is_none(),
        "card_needs": trainer_card_needs(),
        "trainer_driver": hardware::CUDA13_DRIVER,
        "item_style": "style",
        "download": training_pack_download(&state).await,
        "listen": {
            "pack": training.listen_status(),
            "ready": training.listen_ready() && state.lyrics_sync.onnxruntime_library().is_some(),
            "download": listen_download,
        },
        "prepare": state.prepare.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone(),
        "datasets": training.datasets(),
        "runs": runs,
        "active": active,
    }))
}


/// The training pack's files as the training page lists them: the trainer's
/// own, the cuBLAS its CUDA backend loads, and the vocal separator a recipe's
/// lyric timing needs. Says whether the separator is ready too.
async fn training_pack_files(state: &AppState) -> (Vec<Value>, bool) {
    let separator_ready = vocal_separator(state).await.is_some();
    let mut pack = state.training.pack_status();
    if trainer_card_refusal().is_none() {
        let cublas = engine_runtime::cublas_asset(hardware::CudaBuild::Cuda13);
        pack.push(serde_json::json!({
            "id": cublas.id,
            "label": cublas.label,
            "bytes": cublas.bytes,
            "installed": !trainer_cublas_missing(state),
        }));
    }
    pack.push(serde_json::json!({
        "id": "vocal-separator",
        "label": separation::MODEL.label,
        "bytes": separation::MODEL.bytes,
        "installed": separator_ready,
    }));
    (pack, separator_ready)
}

/// The training pack's download in progress: its own, the trainer's cuBLAS,
/// or the separator's, whose files come through their own downloaders.
async fn training_pack_download(state: &AppState) -> Option<downloads::DownloadProgress> {
    let separator_download = match state.separator.downloader().active_for("separation").await {
        Some(active) if !active.done => Some(active),
        other => state.lyrics_sync.downloader().active_for("separation").await.filter(|active| !active.done).or(other),
    };
    let cublas_download = state.engine_runtime.downloader().active_for("engine").await;
    match state.training.downloader().active_for(training::SCOPE).await {
        Some(active) if !active.done => Some(active),
        other => match cublas_download {
            Some(active) if !active.done || active.error.is_some() => Some(active),
            _ => separator_download.or(other),
        },
    }
}

/// A pack as the models page lists an optional part: every file a runtime
/// row, how much of the whole is on disk, and its download.
fn pack_runtime(mut files: Vec<Value>, ready: bool, active_download: Option<downloads::DownloadProgress>) -> Value {
    let bytes: u64 = files.iter().map(|file| file["bytes"].as_u64().unwrap_or(0)).sum();
    let installed_bytes: u64 = files.iter().filter(|file| file["installed"].as_bool() == Some(true)).map(|file| file["bytes"].as_u64().unwrap_or(0)).sum();
    for file in &mut files {
        file["kind"] = "runtime".into();
        file["note"] = "".into();
    }
    let count = files.len();
    serde_json::json!({
        "assets": files,
        "set": { "bytes": bytes, "installed_bytes": installed_bytes, "ready": ready, "files": count },
        "active_download": active_download,
    })
}

/// The training pack on the models page, with what the reference there says:
/// the video memory a run needs and whether this card trains at all.
async fn training_pack_runtime(State(state): State<AppState>) -> Json<Value> {
    let (pack, separator_ready) = training_pack_files(&state).await;
    let mut body = pack_runtime(pack, state.training.pack_ready() && separator_ready && !trainer_cublas_missing(&state), training_pack_download(&state).await);
    body["min_vram_gb"] = music_engine::yue_train::MIN_VRAM_GB.into();
    body["card_trains"] = trainer_card_refusal().is_none().into();
    body["card_needs"] = trainer_card_needs().into();
    Json(body)
}

/// The listening pack on the models page.
async fn listen_pack_runtime(State(state): State<AppState>) -> Json<Value> {
    let training = &state.training;
    let ready = training.listen_ready() && state.lyrics_sync.onnxruntime_library().is_some();
    Json(pack_runtime(training.listen_status(), ready, training.downloader().active_for(training::LISTEN_SCOPE).await))
}

/// Audio to MIDI on the models page: the transcriber, and its sizes as one
/// model's variants - one is chosen and downloaded.
async fn midi_runtime(State(state): State<AppState>) -> Json<Value> {
    let tool = midi::tool_asset();
    let tool_installed = state.midi.tool_installed();
    let mut assets = vec![serde_json::json!({ "id": tool.id, "label": tool.label, "bytes": tool.bytes, "note": "", "installed": tool_installed, "kind": "runtime" })];
    assets.extend(midi::SIZES.iter().map(|size| {
        serde_json::json!({
            "id": size.id,
            "label": format!("MuScriptor {} · {}", size.id, size.params),
            "bytes": size.bytes,
            "note": "",
            "installed": state.midi.model_installed(size),
            "kind": "model",
        })
    }));
    let installed_size = midi::SIZES.iter().find(|size| state.midi.model_installed(size));
    Json(serde_json::json!({
        "assets": assets,
        "set": { "bytes": tool.bytes, "installed_bytes": if tool_installed { tool.bytes } else { 0 }, "ready": tool_installed && installed_size.is_some(), "files": 1 },
        "active_download": state.midi.downloader().active().await,
        "chosen_model": installed_size.map_or(midi::DEFAULT_SIZE, |size| size.id),
    }))
}

async fn install_training_pack(State(state): State<AppState>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    // gigabytes of training files are no use to a machine that cannot train
    if let Some(refusal) = trainer_card_refusal() {
        return Err(api_error(StatusCode::CONFLICT, refusal));
    }
    let background = state.clone();
    tokio::spawn(async move {
        if let Err(error) = background.training.install_pack().await {
            eprintln!("[ERROR] training pack: {error:#}");
            return;
        }
        if let Err(error) = background.engine_runtime.install_missing(Some(hardware::CudaBuild::Cuda13)).await {
            eprintln!("[ERROR] cuBLAS for the trainer: {error:#}");
            return;
        }
        install_separator(&background).await;
    });
    Ok(Json(serde_json::json!({ "started": true })))
}

/// The optional listening pack, and the ONNX Runtime its tempo model runs
/// on when nothing else has brought it yet.
async fn install_listen_pack(State(state): State<AppState>) -> Json<Value> {
    let background = state.clone();
    tokio::spawn(async move {
        if let Err(error) = background.training.install_listen().await {
            eprintln!("[ERROR] listening pack: {error:#}");
            return;
        }
        if background.lyrics_sync.onnxruntime_library().is_none() {
            let runtime: Vec<&'static lyrics_sync::Asset> = lyrics_sync::asset("onnxruntime").into_iter().collect();
            if let Err(error) = background.lyrics_sync.downloader().install_all("listen", &runtime).await {
                eprintln!("[ERROR] ONNX Runtime for the listening pack: {error:#}");
            }
        }
    });
    Json(serde_json::json!({ "started": true }))
}

async fn cancel_training_pack(State(state): State<AppState>) -> Json<Value> {
    state.training.downloader().cancel();
    state.separator.downloader().cancel();
    state.lyrics_sync.downloader().cancel();
    Json(serde_json::json!({ "cancelled": true }))
}

#[derive(Debug, Deserialize)]
struct DatasetInput {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    trigger: Option<String>,
}

async fn create_training_dataset(State(state): State<AppState>, Json(input): Json<DatasetInput>) -> Result<Json<training::Dataset>, (StatusCode, Json<ApiError>)> {
    state.training.create_dataset(input.name.as_deref().unwrap_or_default(), input.trigger.as_deref().unwrap_or_default()).map(Json).map_err(training_error)
}

/// Takes a dataset folder uploaded from another studio: its dataset.json and
/// the audio beside it.
/// A folder of uploaded files, removed however the request ends.
struct UploadFolder(std::path::PathBuf);

impl UploadFolder {
    fn new() -> Result<Self, (StatusCode, Json<ApiError>)> {
        let folder = std::env::temp_dir().join(format!("training-upload-{}", uuid::Uuid::now_v7().simple()));
        std::fs::create_dir_all(&folder).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        Ok(Self(folder))
    }
}

impl Drop for UploadFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Writes one uploaded file to disk as it arrives, so a dataset of several
/// gigabytes never sits in memory.
async fn save_upload(mut field: axum::extract::multipart::Field<'_>, path: &std::path::Path) -> Result<(), (StatusCode, Json<ApiError>)> {
    use tokio::io::AsyncWriteExt;
    let name = field.file_name().unwrap_or_default().to_owned();
    let mut file = tokio::fs::File::create(path).await.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    while let Some(chunk) = field.chunk().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read {name}: {e}")))? {
        file.write_all(&chunk).await.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    }
    file.flush().await.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(())
}

async fn import_training_dataset(State(state): State<AppState>, mut multipart: Multipart) -> Result<Json<training::Dataset>, (StatusCode, Json<ApiError>)> {
    let folder = UploadFolder::new()?;
    let mut manifest = None;
    let mut files = Vec::new();
    while let Some(field) = multipart.next_field().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read the upload: {e}")))? {
        let Some(name) = field.file_name().map(str::to_owned) else { continue };
        let base = name.rsplit(['/', '\\']).next().unwrap_or_default().to_owned();
        if base == "dataset.json" {
            let bytes = field.bytes().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read {name}: {e}")))?;
            manifest = Some(bytes.to_vec());
        } else if base.to_ascii_lowercase().ends_with(".wav") {
            let path = folder.0.join(format!("{}.wav", files.len()));
            save_upload(field, &path).await?;
            files.push((base, path));
        }
    }
    let manifest = manifest.ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "the folder has no dataset.json".into()))?;
    let training = state.training.clone();
    let outcome = tokio::task::spawn_blocking(move || training.import_dataset(&manifest, &files)).await;
    drop(folder);
    outcome.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?.map(Json).map_err(training_error)
}

/// Opens a dataset's folder in the file manager, to copy it to another studio.
async fn reveal_training_dataset(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let folder = state.training.dataset_folder(&id).map_err(training_error)?;
    #[cfg(windows)]
    let opened = std::process::Command::new("explorer.exe").arg(&folder).spawn();
    #[cfg(target_os = "macos")]
    let opened = std::process::Command::new("open").arg(&folder).spawn();
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let opened = std::process::Command::new("xdg-open").arg(&folder).spawn();
    opened.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(serde_json::json!({ "opened": folder.display().to_string() })))
}

async fn update_training_dataset(State(state): State<AppState>, Path(id): Path<String>, Json(input): Json<DatasetInput>) -> Result<Json<training::Dataset>, (StatusCode, Json<ApiError>)> {
    state.training.update_dataset(&id, input.name, input.trigger).map(Json).map_err(training_error)
}

async fn delete_training_dataset(State(state): State<AppState>, Path(id): Path<String>) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    if state.training.dataset_in_use(&id).await {
        return Err(api_error(StatusCode::CONFLICT, "a LoRA is training on this dataset; change it once the run ends".into()));
    }
    state.training.remove_dataset(&id).map_err(training_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct DatasetSongs {
    song_ids: Vec<String>,
}

/// Adds library songs with their style and lyrics; decoding and resampling is
/// real work, so it runs off the request threads.
async fn add_training_songs(State(state): State<AppState>, Path(id): Path<String>, Json(input): Json<DatasetSongs>) -> Result<Json<training::Dataset>, (StatusCode, Json<ApiError>)> {
    let mut sources = Vec::new();
    for song_id in &input.song_ids {
        let song = state
            .library
            .get_song(song_id)
            .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
            .ok_or_else(|| api_error(StatusCode::NOT_FOUND, format!("song {song_id} is not in the library")))?;
        let audio = state.library.media_path_for_song(&song).ok_or_else(|| api_error(StatusCode::BAD_REQUEST, format!("{} has no stored audio", song.title)))?;
        sources.push((audio, song.title, song.caption, song.lyrics, song.id));
    }
    let training = state.training.clone();
    tokio::task::spawn_blocking(move || {
        let mut dataset = training.dataset(&id)?;
        for (audio, title, style, lyrics, song_id) in sources {
            dataset = training.add_item(&id, &audio, &title, "", &style, &lyrics, lyrics.trim().is_empty(), &format!("song:{song_id}"))?;
        }
        Ok::<_, anyhow::Error>(dataset)
    })
    .await
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map(Json)
    .map_err(training_error)
}

/// Adds audio files from the user's disk; a same-named `.txt` or `.lrc` part
/// is taken as that song's lyrics.
async fn upload_training_files(State(state): State<AppState>, Path(id): Path<String>, mut multipart: Multipart) -> Result<Json<training::Dataset>, (StatusCode, Json<ApiError>)> {
    let folder = UploadFolder::new()?;
    let mut audio = Vec::new();
    let mut texts = std::collections::HashMap::new();
    let mut cues: Vec<(String, Option<String>, Vec<training::CueTrack>)> = Vec::new();
    while let Some(field) = multipart.next_field().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read the upload: {e}")))? {
        // the page sends the path inside what was dropped; its folders name the artist
        let Some(relative) = field.file_name().map(|name| name.replace('\\', "/")) else { continue };
        let name = relative.rsplit('/').next().filter(|name| !name.is_empty()).unwrap_or("song").to_owned();
        let lower = name.to_lowercase();
        let stem = std::path::Path::new(&name).file_stem().and_then(|stem| stem.to_str()).unwrap_or(&name).to_owned();
        if lower.ends_with(".txt") || lower.ends_with(".lrc") {
            let bytes = field.bytes().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read {name}: {e}")))?;
            texts.insert(stem, legacy_text::decode(&bytes));
        } else if lower.ends_with(".cue") {
            let bytes = field.bytes().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read {name}: {e}")))?;
            let (file, tracks) = training::cue_sheet(&bytes);
            cues.push((stem, file, tracks));
        } else if [".wav", ".mp3", ".flac", ".ogg", ".m4a", ".aiff", ".aif"].iter().any(|extension| lower.ends_with(extension)) {
            let path = folder.0.join(format!("{}-{name}", audio.len()));
            save_upload(field, &path).await?;
            audio.push((path, stem, name, relative));
        }
    }
    if audio.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "no audio in the upload: WAV, MP3, FLAC, OGG or M4A".into()));
    }
    let training = state.training.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let mut dataset = training.dataset(&id)?;
        for (path, stem, name, relative) in audio {
            // an album in one file comes with the cue sheet that cuts it
            let sheet = cues.iter().find(|(cue_stem, file, tracks)| tracks.len() > 1 && (file.as_deref() == Some(name.as_str()) || *cue_stem == stem));
            if let Some((_, _, tracks)) = sheet {
                let album_artist = audio_pcm::tags(&path).artist;
                dataset = training.add_album(&id, &path, tracks, &album_artist, &format!("file:{relative}"))?;
                continue;
            }
            // a text beside the file, else the lyrics the file carries in its own tags
            let lyrics = texts
                .get(&stem)
                .map(|text| training::plain_lyrics(text))
                .filter(|text| !text.trim().is_empty())
                .unwrap_or_else(|| training::plain_lyrics(&audio_pcm::tags(&path).lyrics));
            let (artist, title) = training::identify(&path, &relative);
            dataset = training.add_item(&id, &path, &title, &artist, "", &lyrics, false, &format!("file:{relative}"))?;
        }
        Ok::<_, anyhow::Error>(dataset)
    })
    .await;
    drop(folder);
    outcome.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?.map(Json).map_err(training_error)
}

async fn update_training_item(
    State(state): State<AppState>,
    Path((id, item)): Path<(String, String)>,
    Json(patch): Json<training::ItemPatch>,
) -> Result<Json<training::Dataset>, (StatusCode, Json<ApiError>)> {
    state.training.edit_item(&id, &item, patch).map(Json).map_err(training_error)
}

async fn delete_training_item(State(state): State<AppState>, Path((id, item)): Path<(String, String)>) -> Result<Json<training::Dataset>, (StatusCode, Json<ApiError>)> {
    if state.training.dataset_in_use(&id).await {
        return Err(api_error(StatusCode::CONFLICT, "a LoRA is training on this dataset; change it once the run ends".into()));
    }
    state.training.remove_item(&id, &item).map(Json).map_err(training_error)
}

#[derive(Debug, Deserialize)]
struct StartTraining {
    dataset_id: String,
    #[serde(default)]
    name: String,
    recipe: training::Recipe,
}

/// Starts a run. It wants the whole card: refused while a song renders, and
/// the writing assistant is let go first.
async fn start_training(State(state): State<AppState>, Json(input): Json<StartTraining>) -> Result<Json<training::Run>, (StatusCode, Json<ApiError>)> {
    card_free_for_training(&state).await.map_err(|reason| api_error(StatusCode::CONFLICT, reason))?;
    start_training_run(&state, &input.dataset_id, &input.name, input.recipe)
        .await
        .map(Json)
        .map_err(|error| api_error(StatusCode::CONFLICT, error))
}

/// A run of `dataset`, refused while a song renders or where the trainer
/// could not compute on the card.
async fn start_training_run(state: &AppState, dataset: &str, name: &str, recipe: training::Recipe) -> Result<training::Run, String> {
    trainer_ready(state)?;
    no_song_rendering(state).await?;
    let models = selected_engine_models(state).await?;
    state
        .training
        .start(Some(engine_bundle_root()), models.backbone, models.companion, vocal_separator(state).await, dataset, name, recipe, card_hooks(state).await)
        .await
        .map_err(|error| format!("{error:#}"))
}

/// Frees the card for a run and gives it back after: the assistant is stopped,
/// and an engine kept loaded between songs is stopped and started again.
async fn card_hooks(state: &AppState) -> training::CardHooks {
    let keep_loaded = state.engine_options.read().await.keep_loaded;
    let (take_state, back_state) = (state.clone(), state.clone());
    training::CardHooks {
        take: Box::new(move || {
            Box::pin(async move {
                free_the_card_for_the_engine(&take_state).await;
                if keep_loaded {
                    if let Some(engine) = take_state.engine.lock().await.as_mut() {
                        if let Err(error) = tokio::task::block_in_place(|| engine.stop(std::time::Duration::from_secs(10))) {
                            eprintln!("[ERROR] stopping the engine for training: {error}");
                        }
                    }
                }
            })
        }),
        give_back: Box::new(move || {
            Box::pin(async move {
                if keep_loaded {
                    if let Err(error) = restart_engine(&back_state).await {
                        eprintln!("[ERROR] starting the engine after training: {error}");
                    }
                }
            })
        }),
    }
}

/// Refuses work that needs the graphics card while a LoRA trains on it.
/// A song holds the card while it renders; training waits for it.
async fn no_song_rendering(state: &AppState) -> Result<(), String> {
    let rendering = state.jobs.read().await.values().any(|job| matches!(job.status, MusicJobStatus::Queued | MusicJobStatus::Running));
    if rendering {
        return Err("a song is being made; train once it is done".into());
    }
    Ok(())
}

/// What keeps the user from starting a training run or training one further:
/// a machine the trainer does not run on, songs being prepared, or a song
/// being made.
async fn card_free_for_training(state: &AppState) -> Result<(), String> {
    trainer_ready(state)?;
    let preparing = state.prepare.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_ref().is_some_and(|job| !job.finished);
    if preparing {
        return Err("songs are being prepared; train once that is done".into());
    }
    no_song_rendering(state).await
}

/// Why the trainer does not run on this machine's card, none when it does. It
/// is a CUDA 13 build and computes on the card only: on the processor one run
/// would take days.
fn trainer_card_refusal() -> Option<String> {
    match hardware::hardware().cuda {
        Some(hardware::CudaBuild::Cuda13) => None,
        Some(hardware::CudaBuild::Cuda12) => Some(format!(
            "training needs NVIDIA driver {} or newer: the trainer is a CUDA 13 build, and this card or its driver runs CUDA 12 only",
            hardware::CUDA13_DRIVER
        )),
        None => Some(TRAINING_NEEDS_CUDA.into()),
    }
}

/// What the card lacks for the trainer, as the training page names it:
/// "nvidia" on a machine with no CUDA card, "driver" where the card or its
/// driver runs CUDA 12 only.
fn trainer_card_needs() -> Option<&'static str> {
    match hardware::hardware().cuda {
        Some(hardware::CudaBuild::Cuda13) => None,
        Some(hardware::CudaBuild::Cuda12) => Some("driver"),
        None => Some("nvidia"),
    }
}

/// Whether the cuBLAS the trainer's CUDA backend loads is missing. It comes
/// from beside the engine, which fetches it only for a CUDA 13 run of its own.
fn trainer_cublas_missing(state: &AppState) -> bool {
    !state.engine_runtime.missing(Some(hardware::CudaBuild::Cuda13)).is_empty()
}

/// Refuses a run the trainer could not compute on the card.
fn trainer_ready(state: &AppState) -> Result<(), String> {
    if let Some(refusal) = trainer_card_refusal() {
        return Err(refusal);
    }
    if trainer_cublas_missing(state) {
        return Err(TRAINER_CUBLAS_MISSING.into());
    }
    Ok(())
}

/// Why this machine does not train.
const TRAINING_NEEDS_CUDA: &str = "training runs on an NVIDIA card with CUDA only, and this machine has none";
/// Why a run cannot start before the training pack is complete.
const TRAINER_CUBLAS_MISSING: &str = "the training files are not downloaded yet: the trainer needs NVIDIA cuBLAS 13; download the training pack";

async fn card_free_of_training(state: &AppState, what: &str) -> Result<(), (StatusCode, Json<ApiError>)> {
    if state.training.active_run().await.is_some() {
        return Err(api_error(StatusCode::CONFLICT, format!("a LoRA is training on the card; {what} once it finishes")));
    }
    Ok(())
}

#[derive(Deserialize)]
struct ContinueTraining {
    /// The steps the run is to reach in all.
    steps: u32,
}

/// Trains a finished or stopped run further from its latest checkpoint.
async fn continue_training(State(state): State<AppState>, Path(id): Path<String>, Json(input): Json<ContinueTraining>) -> Result<Json<training::Run>, (StatusCode, Json<ApiError>)> {
    card_free_for_training(&state).await.map_err(|reason| api_error(StatusCode::CONFLICT, reason))?;
    let companion = selected_engine_models(&state).await.map_err(|reason| api_error(StatusCode::CONFLICT, reason))?.companion;
    state
        .training
        .continue_run(Some(engine_bundle_root()), companion, &id, input.steps, card_hooks(&state).await)
        .await
        .map(Json)
        .map_err(|error| api_error(StatusCode::CONFLICT, format!("{error:#}")))
}

async fn cancel_training(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    state.training.cancel(&id).await.map_err(training_error)?;
    Ok(Json(serde_json::json!({ "cancelled": true })))
}

async fn delete_training_run(State(state): State<AppState>, Path(id): Path<String>) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    state.training.remove_run(&id).await.map_err(training_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct InstallCheckpoint {
    #[serde(default)]
    name: Option<String>,
}

/// Adds a checkpoint to the adapter library, where the create page finds it.
async fn install_training_checkpoint(
    State(state): State<AppState>,
    Path((id, step)): Path<(String, u32)>,
    Json(input): Json<InstallCheckpoint>,
) -> Result<Json<adapters::AdapterMeta>, (StatusCode, Json<ApiError>)> {
    let run = state.training.run(&id).map_err(training_error)?;
    let checkpoint = state
        .training
        .checkpoints(&id)
        .into_iter()
        .find(|checkpoint| checkpoint.step == step)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, format!("the run has no checkpoint at step {step}")))?;
    let name = input.name.filter(|name| !name.trim().is_empty()).unwrap_or_else(|| format!("{} · {step}", run.name));
    let trigger = Some(run.trigger.clone());
    let meta = state
        .adapters
        .import_trained(&name, trigger, None, &checkpoint.files, adapters::Origin::Trained { run: id.clone(), step })
        .map_err(training_error)?;
    state.training.mark_installed(&id, step).map_err(training_error)?;
    Ok(Json(meta))
}

async fn read_stems(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let songs = state.library.list_songs().map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let in_library: Vec<Value> = songs
        .iter()
        .filter(|song| derived_from(song) == Some((id.as_str(), "stems")))
        .map(|song| serde_json::json!({ "stem": song.metadata.pointer("/derived/settings/stem"), "song_id": song.id, "title": song.title }))
        .collect();
    Ok(Json(serde_json::json!({
        "song_id": id,
        "stems": stems_on_disk(&state, &id),
        "library_songs": in_library,
        "run": state.separation_run.read().await.clone(),
    })))
}

async fn read_stem_audio(
    State(state): State<AppState>,
    Path((id, stem)): Path<(String, String)>,
    request: Request,
) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    if !separation::STEMS.contains(&stem.as_str()) {
        return Err(api_error(StatusCode::BAD_REQUEST, format!("unknown stem {stem}")));
    }
    let path = stem_path(&state, &id, &stem);
    let exists = tokio::fs::try_exists(&path)
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("read stem audio: {error}")))?;
    if !exists {
        return Err(api_error(StatusCode::NOT_FOUND, "this track has no such stem yet".into()));
    }
    Ok(serve_audio_file(&path, request).await)
}

/// Separates one track into stems, in the background, reporting progress.
async fn start_separation(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let song = state
        .library
        .get_song(&id)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))?;
    // a stem is a part of a song already: the song it came from is what is separated
    if song.metadata["derived"]["tool"].as_str() == Some("stems") {
        return Err(api_error(StatusCode::CONFLICT, "this track is a stem, a part of a song already; separate the song it came from".into()));
    }
    card_free_of_training(&state, "separate tracks").await?;
    if state.separation_run.read().await.as_ref().is_some_and(|run| !run.done) {
        return Err(api_error(StatusCode::CONFLICT, "a track is already being separated".into()));
    }
    let wanted_runtime = state.separation_config.read().await.runtime;
    let card = state
        .lyrics_sync
        .onnx_card(wanted_runtime)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, format!("{error:#}")))?
        .filter(|card| separates_on(*card));
    if !state.separator.is_installed() {
        return Err(api_error(StatusCode::BAD_REQUEST, "the separation model is not installed yet".into()));
    }
    let audio_path = state
        .library
        .media_path_for_song(&song)
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "this track has no stored audio".into()))?;

    *state.separation_run.write().await =
        Some(SeparationRun { song_id: id.clone(), progress: 0.0, done: false, error: None, stems: vec![], used_gpu: None, library_songs: vec![] });

    let model = state.separator.model_path();
    let config = state.separation_config.read().await.clone();
    let overlap = config.sane_overlap();
    let wanted = config.stems.clone();
    let background = state.clone();
    let song_id = id.clone();
    tokio::task::spawn_blocking(move || {
        let outcome = (|| -> anyhow::Result<(Vec<String>, bool)> {
            let audio = audio_pcm::decode_stereo_44k(&audio_path)?;
            let handle = tokio::runtime::Handle::current();
            let separated = separation::separate(&model, &audio, separation::STEMS.len(), overlap, card, |fraction| {
                let state = background.clone();
                handle.spawn(async move {
                    if let Some(run) = state.separation_run.write().await.as_mut() {
                        run.progress = fraction;
                    }
                });
            })?;
            let mut written = Vec::new();
            let ran_on_gpu = separated.used_gpu;
            for stem in separated.stems {
                if !wanted.iter().any(|name| name == stem.name) {
                    continue;
                }
                let path = stem_path(&background, &song_id, stem.name);
                separation::write_wav_stereo(&path, &stem.samples)?;
                written.push(stem.name.to_string());
            }
            Ok((written, ran_on_gpu))
        })();
        // still on this thread: the stems are hundreds of megabytes to copy
        let library = outcome.as_ref().ok().map(|(stems, _)| stems_into_library(&background, &song_id, stems, overlap));

        let handle = tokio::runtime::Handle::current();
        handle.spawn(async move {
            if let Some(run) = background.separation_run.write().await.as_mut() {
                run.done = true;
                match outcome {
                    Ok((stems, ran_on_gpu)) => {
                        run.progress = 1.0;
                        run.used_gpu = Some(ran_on_gpu);
                        match library {
                            Some(Ok((songs, problem))) => {
                                run.library_songs = songs;
                                run.error = problem;
                            }
                            Some(Err(error)) => run.error = Some(format!("the stems are separated but did not reach the library: {error:#}")),
                            None => {}
                        }
                        run.stems = stems;
                    }
                    Err(error) => run.error = Some(format!("{error:#}")),
                }
            }
        });
    });

    Ok(Json(serde_json::json!({ "started": true, "song_id": id })))
}


/// Draws a cover for a finished track, if the studio was told to.
///
/// The same pieces the cover window uses: the default template, filled in from
/// this track, and the image model chosen on the provider page. Nothing happens
/// without a key, without a model, or when the user turned this off - and a
/// failure is written to the log rather than shown as a broken track.
/// Draws the cover for one track now, and says what went wrong if it did not.
async fn draw_cover_now(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    match draw_cover(&state, &id).await {
        Ok(()) => Ok(Json(serde_json::json!({ "drawn": true }))),
        Err(error) => Err(api_error(StatusCode::BAD_GATEWAY, error.to_string())),
    }
}


/// Times the lyrics of a finished track, if karaoke is switched on.
///
/// The switch said "on" and nothing happened: the timings were only ever made
/// by the button in the track menu. A track arrives with its words already
/// known, so this is the moment to time them.

/// One background piece of work on a finished track.
#[derive(Debug, Clone, Serialize)]
struct Activity {
    song_id: String,
    title: String,
    /// "cover" or "karaoke".
    kind: &'static str,
    /// "running", "done" or "failed".
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

/// Notes what is happening, keeping only the recent past.
async fn note_activity(state: &AppState, song_id: &str, title: &str, kind: &'static str, phase: &'static str, detail: Option<String>) {
    let mut activity = state.activity.write().await;
    if let Some(existing) = activity.iter_mut().find(|entry| entry.song_id == song_id && entry.kind == kind) {
        existing.state = phase;
        existing.detail = detail;
        existing.title = title.to_string();
    } else {
        activity.push(Activity {
            song_id: song_id.to_string(),
            title: title.to_string(),
            kind,
            state: phase,
            detail,
        });
    }
    let overflow = activity.len().saturating_sub(20);
    if overflow > 0 {
        activity.drain(..overflow);
    }
}

async fn read_activity(State(state): State<AppState>) -> Json<Value> {
    Json(serde_json::json!({ "activity": state.activity.read().await.clone() }))
}

/// Downloads whatever the chosen local recogniser is missing, then waits for it.
///
/// Choosing Parakeet or Whisper in the settings is the instruction to use it;
/// making the user then find a download button for it is a second instruction
/// nobody asked for. The first track that needs timings fetches the model and
/// carries on.
async fn ensure_local_recogniser(state: &AppState, config: &lyrics_sync::LyricsSyncConfig, song_id: &str) -> bool {
    let ready = |state: &AppState| match config.provider {
        lyrics_sync::AsrProvider::Parakeet => state.lyrics_sync.parakeet_ready(config.whisper_model.as_deref()),
        lyrics_sync::AsrProvider::Whisper => {
            state.lyrics_sync.whisper_binary().is_some() && state.lyrics_sync.whisper_model_ready(config)
        }
        _ => true,
    };
    if ready(state) {
        return true;
    }

    let missing: Vec<&'static lyrics_sync::Asset> = match config.provider {
        lyrics_sync::AsrProvider::Parakeet => lyrics_sync::parakeet_variant(config.whisper_model.as_deref())
            .0
            .iter()
            .filter_map(|id| lyrics_sync::asset(id))
            .filter(|asset| !state.lyrics_sync.downloader().is_installed(asset))
            .collect(),
        // Whatever the chosen recogniser is made of, by the same reckoning the
        // panel uses. Naming the files here by hand is how this went on asking
        // for `whisper-cuda` after that asset had ceased to exist, and then
        // concluded that nothing was missing and nothing was ready.
        lyrics_sync::AsrProvider::Whisper => karaoke_set("whisper", config.runtime, config.whisper_model.as_deref())
            .into_iter()
            .filter(|asset| !state.lyrics_sync.downloader().is_installed(asset))
            .collect(),
        _ => Vec::new(),
    };
    if missing.is_empty() {
        return ready(state);
    }

    let title = state.library.get_song(song_id).ok().flatten().map(|song| song.title).unwrap_or_default();
    note_activity(state, song_id, &title, "karaoke", "running", Some("karaoke.downloading".into())).await;
    for asset in missing {
        if let Err(error) = state.lyrics_sync.downloader().install(asset).await {
            eprintln!("could not fetch the karaoke model {}: {error}", asset.id);
            note_activity(state, song_id, &title, "karaoke", "failed", Some(format!("{error:#}"))).await;
            return false;
        }
        // The downloader runs one file at a time in the background; the timings
        // wait for it rather than starting against half a model. The wait is
        // reported with real numbers: a spinner that says "downloading" for ten
        // minutes without moving is indistinguishable from one that is stuck.
        loop {
            let Some(progress) = state.lyrics_sync.downloader().active().await else { break };
            if progress.done {
                break;
            }
            let percent = if progress.total_bytes > 0 {
                (progress.downloaded_bytes * 100 / progress.total_bytes).min(100)
            } else {
                0
            };
            note_activity(state, song_id, &title, "karaoke", "running", Some(format!("karaoke.downloading {percent}%"))).await;
            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        }
    }
    ready(state)
}

async fn time_lyrics_for(state: AppState, song_id: String) {
    let config = state.lyrics_sync_config.read().await.clone();
    if !config.enabled || config.provider == lyrics_sync::AsrProvider::None {
        return;
    }
    // The cloud recogniser is the one case that cannot be fixed from here: a key
    // is the user's to add, and announcing a failure they cannot act on is
    // noise. A local recogniser is different - if its model is not on disk yet,
    // choosing it is the instruction to fetch it, so the first use downloads it
    // and then does the work.
    if config.provider == lyrics_sync::AsrProvider::OpenRouter && credentials::openrouter_api_key().is_none() {
        return;
    }
    if !ensure_local_recogniser(&state, &config, &song_id).await {
        return;
    }
    let Ok(Some(song)) = state.library.get_song(&song_id) else { return };
    // An instrumental has section markers and no words. Timing it means asking
    // the recogniser to find lyrics that were never sung.
    if !auto_title::has_sung_lines(&song.lyrics) {
        return;
    }
    let Some(audio) = state.library.media_path_for_song(&song) else { return };
    let audio = audio.display().to_string();

    note_activity(&state, &song_id, &song.title, "karaoke", "running", None).await;
    let words = match config.provider {
        lyrics_sync::AsrProvider::None => return,
        lyrics_sync::AsrProvider::Parakeet => {
            let sync = state.lyrics_sync.clone();
            let path = std::path::PathBuf::from(&audio);
            match tokio::task::spawn_blocking(move || sync.parakeet_words(config.runtime, config.whisper_model.as_deref(), &path)).await {
                Ok(result) => result,
                Err(error) => {
                    eprintln!("no karaoke for {song_id}: {error}");
                    return;
                }
            }
        }
        lyrics_sync::AsrProvider::Whisper => {
            let sync = state.lyrics_sync.clone();
            let config = config.clone();
            let path = std::path::PathBuf::from(&audio);
            let lyrics = song.lyrics.clone();
            match tokio::task::spawn_blocking(move || sync.whisper_words(&config, &path, None, &lyrics)).await {
                Ok(result) => result,
                Err(error) => {
                    eprintln!("no karaoke for {song_id}: {error}");
                    return;
                }
            }
        }
        lyrics_sync::AsrProvider::OpenRouter => {
            karaoke_words_from_openrouter(&state, &config, &audio, None).await
        }
    };
    let words = match words {
        Ok(words) => words,
        Err(error) => {
            eprintln!("no karaoke for {song_id}: {error}");
            note_activity(&state, &song_id, &song.title, "karaoke", "failed", Some(format!("{error:#}"))).await;
            return;
        }
    };
    let lines = lyrics_sync::align_lyrics_words(&words, &song.lyrics);
    if lines.is_empty() {
        let reason = "karaoke.no-match";
        eprintln!("no karaoke for {song_id}: {reason}");
        note_activity(&state, &song_id, &song.title, "karaoke", "failed", Some(reason.to_string())).await;
        return;
    }
    match state.library.set_song_lrc(&song_id, &lyrics_sync::enhanced_lrc(&lines)) {
        Ok(_) => note_activity(&state, &song_id, &song.title, "karaoke", "done", None).await,
        Err(error) => {
            eprintln!("could not store karaoke for {song_id}: {error}");
            note_activity(&state, &song_id, &song.title, "karaoke", "failed", Some(format!("{error:#}"))).await;
        }
    }
}

async fn draw_cover_for(state: AppState, song_id: String) {
    if !*state.cover_auto.read().await {
        return;
    }
    // Drawing a cover needs a cloud key. Without one there is nothing to try,
    // and announcing a failure the user cannot act on is noise: the track keeps
    // the placeholder artwork the library already shows.
    if credentials::openrouter_api_key().is_none() {
        return;
    }
    let title = state.library.get_song(&song_id).ok().flatten().map(|song| song.title).unwrap_or_default();
    note_activity(&state, &song_id, &title, "cover", "running", None).await;
    match draw_cover(&state, &song_id).await {
        Ok(()) => note_activity(&state, &song_id, &title, "cover", "done", None).await,
        Err(error) => {
            eprintln!("no cover for {song_id}: {error}");
            note_activity(&state, &song_id, &title, "cover", "failed", Some(format!("{error:#}"))).await;
        }
    }
}

/// The work itself, with its reasons kept rather than printed.
async fn draw_cover(state: &AppState, song_id: &str) -> anyhow::Result<()> {
    use anyhow::Context as _;
    let song = state
        .library
        .get_song(song_id)?
        .context("the track is not in the library")?;
    if song.metadata.get("cover_filename").is_some() {
        return Ok(());
    }
    let model = {
        let configuration = state.configuration.read().await;
        configuration
            .selections
            .iter()
            .find(|selection| selection.capability == Capability::CoverArt)
            .filter(|selection| selection.mode == ExecutionMode::OpenRouter)
            .and_then(|selection| selection.cloud_model.clone())
            .filter(|model| !model.trim().is_empty())
    };
    let catalog = catalog_for(state).await.map_err(|error| anyhow::anyhow!(error))?;
    let model = match model {
        Some(model) => model,
        None => providers::openrouter::suggested_model(&catalog, Capability::CoverArt)
            .context("no image model is chosen for covers")?,
    };
    let templates = state.cover_templates.read().await.clone();
    let default_id = state.cover_template_default.read().await.clone();
    let template = templates
        .iter()
        .find(|entry| Some(&entry.id) == default_id.as_ref())
        .or_else(|| templates.first())
        .map(|entry| entry.template.clone())
        .context("there are no cover templates")?;
    let facts = cover_prompt::TrackFacts {
        title: song.title.clone(),
        style: song.caption.clone(),
        lyrics: song.lyrics.clone(),
        duration_seconds: song.metadata.get("duration_seconds").and_then(Value::as_f64).unwrap_or(0.0),
    };
    let prompt = match song.metadata.get("cover_prompt").and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()) {
        Some(written) => written.to_string(),
        None => cover_prompt::render(&template, &facts),
    };
    let request = providers::openrouter::request_for(&catalog, Capability::CoverArt, &model, &prompt)?;
    let answered = execute_openrouter_json(request).await.map_err(|error| anyhow::anyhow!(error))?;
    let first = answered
        .body
        .get("data")
        .and_then(|data| data.get(0))
        .context("the model returned no image")?;
    let image = first
        .get("b64_json")
        .and_then(Value::as_str)
        .context("the model returned no image")?;
    // The answer states its own format, and it is not always PNG.
    let media_type = first.get("media_type").and_then(Value::as_str).unwrap_or("image/png").to_string();
    use base64::{engine::general_purpose::STANDARD, Engine};
    let bytes = STANDARD.decode(image.trim()).context("the image was not valid base64")?;
    state.library.store_song_cover(song_id, &bytes, &media_type)?;
    tag_stored_song(state, song_id).await;
    Ok(())
}

async fn read_cover_templates(State(state): State<AppState>) -> Json<Value> {
    Json(serde_json::json!({
        "auto": *state.cover_auto.read().await,
        "templates": state.cover_templates.read().await.clone(),
        "default_id": state.cover_template_default.read().await.clone(),
        "placeholders": ["title", "style", "lyrics", "excerpt", "duration"],
    }))
}

async fn write_cover_templates(
    State(state): State<AppState>,
    Json(request): Json<CoverTemplatesRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let templates = if request.templates.is_empty() { cover_prompt::default_templates() } else { request.templates };
    *state.cover_templates.write().await = templates.clone();
    // A default that names a template nobody kept is worse than none.
    let default_id = request
        .default_id
        .filter(|id| !id.trim().is_empty() && templates.iter().any(|entry| entry.id == *id));
    *state.cover_template_default.write().await = default_id.clone();
    if let Some(auto) = request.auto {
        *state.cover_auto.write().await = auto;
    }
    persist_studio_settings(&state)
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(serde_json::json!({
        "templates": templates,
        "default_id": default_id,
        "auto": *state.cover_auto.read().await,
    })))
}

/// The prompt a template turns into for one track, exactly as it would be sent.
async fn render_cover_template(
    State(state): State<AppState>,
    Json(request): Json<RenderCoverPromptRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let mut facts = cover_prompt::TrackFacts {
        title: request.title.unwrap_or_default(),
        style: request.style.unwrap_or_default(),
        lyrics: request.lyrics.unwrap_or_default(),
        duration_seconds: 0.0,
    };
    if let Some(song_id) = request.song_id.as_deref().filter(|value| !value.trim().is_empty()) {
        let song = state
            .library
            .get_song(song_id)
            .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
            .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))?;
        facts.title = song.title.clone();
        facts.style = song.caption.clone();
        facts.lyrics = song.lyrics.clone();
        facts.duration_seconds = song
            .metadata
            .get("duration_seconds")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
    }
    Ok(Json(serde_json::json!({ "prompt": cover_prompt::render(&request.template, &facts) })))
}

async fn library_cover(State(state): State<AppState>, Path(id): Path<String>) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let song = state.library.get_song(&id).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))?;
    let (path, media_type) = state.library.cover_path_for_song(&song).ok_or_else(|| api_error(StatusCode::NOT_FOUND, "This song has no stored cover image".into()))?;
    let bytes = tokio::fs::read(&path).await.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("read cover: {error}")))?;
    Ok(axum::response::Response::builder()
        .header(header::CONTENT_TYPE, media_type)
        .header(header::CONTENT_LENGTH, bytes.len())
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(bytes))
        .expect("valid cover response"))
}


/// Writes ID3 tags onto a stored MP3 from what the library knows about it.
///
/// Called after a track is stored, after its cover changes, after it is renamed
/// and once at start for tracks stored before the studio tagged anything.
/// Failure is logged and never fails the request: an untagged track still
/// plays, a lost one does not.
async fn tag_stored_song(state: &AppState, song_id: &str) {
    let Ok(Some(song)) = state.library.get_song(song_id) else { return };
    // `audio_path` is a full path, not a filename: resolve it the way playback
    // does, or tagging silently skips every track.
    let Some(audio_path) = state.library.media_path_for_song(&song) else { return };
    if !tagging::taggable(&audio_path) {
        return;
    }
    let cover_file = state.library.cover_path_for_song(&song).map(|(path, media_type)| (path, media_type.to_string()));
    let tags = tagging::TrackTags {
        title: song.title.clone(),
        album: "YuE2 Studio".to_string(),
        // The engine is the performer here; the studio is the label.
        artist: "YuE2".to_string(),
        genre: tagging::genre_from_caption(&song.caption),
        lyrics: Some(song.lyrics.clone()).filter(|value| !value.trim().is_empty()),
        bpm: tagging::bpm_from_caption(&song.caption),
        cover: None,
    };
    let shown = audio_path.display().to_string();
    // reading the cover and rewriting the file are blocking file work
    let written = tokio::task::spawn_blocking(move || {
        let cover = cover_file.and_then(|(path, media_type)| std::fs::read(path).ok().map(|bytes| (media_type, bytes)));
        tagging::write_tags(&audio_path, &tagging::TrackTags { cover, ..tags })
    })
    .await;
    match written {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("could not tag {shown}: {error}"),
        Err(error) => eprintln!("could not tag {shown}: the tagging task stopped: {error}"),
    }
}

/// Tracks stored before the studio tagged anything carry no tag, and a download
/// of one lands in a player as an untitled file: each is tagged once, in the
/// background, when the service starts. Serving a track never writes to it.
async fn tag_untagged_songs(state: AppState) {
    let songs = match state.library.list_songs() {
        Ok(songs) => songs,
        Err(error) => {
            eprintln!("[ERROR] the library did not list its tracks for tagging: {error:#}");
            return;
        }
    };
    for song in songs {
        let Some(path) = state.library.media_path_for_song(&song) else { continue };
        if !tagging::taggable(&path) {
            continue;
        }
        // reads the tag, not the audio
        let untagged = matches!(tokio::task::spawn_blocking(move || tagging::is_tagged(&path)).await, Ok(Ok(false)));
        if untagged {
            tag_stored_song(&state, &song.id).await;
        }
    }
}

#[derive(Debug, Deserialize)]
struct CoverLookRequest {
    #[serde(default)]
    photo: Option<bool>,
    #[serde(default)]
    pattern: Option<String>,
    #[serde(default)]
    keep: Option<bool>,
}

async fn cover_look_reply(state: &AppState) -> Value {
    let look = state.cover_look.read().await.clone();
    serde_json::json!({
        "photo": look.photo,
        "pattern": look.pattern,
        "keep": look.keep,
        "patterns": cover_art::PATTERNS,
        "problem": state.cover_problem.read().await.clone(),
    })
}

async fn read_cover_look(State(state): State<AppState>) -> Json<Value> {
    Json(cover_look_reply(&state).await)
}

/// A new look is written into the tracks that wear the old one, in the
/// background: the answer does not wait for a pass over the whole library.
async fn write_cover_look(
    State(state): State<AppState>,
    Json(request): Json<CoverLookRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    if let Some(pattern) = request.pattern.as_deref().filter(|pattern| !cover_art::PATTERNS.contains(pattern)) {
        return Err(api_error(StatusCode::BAD_REQUEST, format!("there is no cover pattern '{pattern}'")));
    }
    {
        let mut look = state.cover_look.write().await;
        if let Some(photo) = request.photo {
            look.photo = photo;
        }
        if let Some(pattern) = request.pattern {
            look.pattern = pattern;
        }
        if let Some(keep) = request.keep {
            look.keep = keep;
        }
    }
    persist_studio_settings(&state)
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    // the placeholders drawn for the old look are never shown again
    let drawn = state.library.media_dir().join("cover-placeholders");
    match tokio::fs::remove_dir_all(&drawn).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => eprintln!("[ERROR] the old placeholders in {} stay: {error}", drawn.display()),
    }
    tokio::spawn(pin_placeholders(state.clone()));
    Ok(Json(cover_look_reply(&state).await))
}

/// The photograph a track's style calls up, picked by its seed: the same one
/// every time, the next one along for each further variant.
async fn photo_for(style: &str, seed: &str, variant: u64) -> anyhow::Result<Option<(Vec<u8>, cover_art::Photo)>> {
    let client = net::client();
    let photos = cover_art::photos_for(&client, style).await?;
    let Some(photo) = cover_art::chosen(&photos, seed, variant).cloned() else { return Ok(None) };
    let image = cover_art::download(&client, &photo.image).await?;
    Ok(Some((image, photo)))
}

/// The picture a track without a cover of its own wears in a look: its
/// photograph, or its pattern when the look is a pattern or no photograph
/// could be had. Why a photograph could not be had waits in Settings, and the
/// track tries again on the next pass, its placeholder not being the look's.
async fn placeholder_image(
    state: &AppState,
    song: &library::Song,
    look: &cover_art::CoverLook,
) -> anyhow::Result<(Vec<u8>, &'static str, String, Option<String>)> {
    let seed = cover_art::cover_seed(song);
    if look.photo && !cover_art::photos_resting() {
        match photo_for(&song.caption, &seed, 0).await {
            Ok(Some((image, photo))) => {
                cover_art::photo_failed(false);
                *state.cover_problem.write().await = None;
                return Ok((image, "image/jpeg", look.label(), Some(photo.page)));
            }
            Ok(None) => {}
            Err(error) => {
                cover_art::photo_failed(true);
                eprintln!("[ERROR] no photo for {}: {error:#}", song.id);
                *state.cover_problem.write().await = Some(format!("{error:#}"));
            }
        }
    }
    let pattern = look.pattern.clone();
    let label = format!("pattern:{pattern}");
    let image = tokio::task::spawn_blocking(move || cover_art::pattern_png(&pattern, &seed, cover_art::KEPT_SIZE))
        .await
        .context("the pattern drawing stopped")??;
    Ok((image, "image/png", label, None))
}

/// Writes the look's placeholder into a track without a cover of its own, or
/// takes it back out when keeping is off. Says whether the track changed.
async fn pin_placeholder(state: &AppState, song_id: &str) -> anyhow::Result<bool> {
    let Some(song) = state.library.get_song(song_id)? else { return Ok(false) };
    let pinned = song.metadata.get("cover_placeholder").and_then(Value::as_str).map(str::to_owned);
    if song.metadata.get("cover_filename").is_some() && pinned.is_none() {
        return Ok(false);
    }
    let look = state.cover_look.read().await.clone();
    if !look.keep {
        let removed = state.library.remove_placeholder_cover(song_id)?.is_some();
        if removed {
            tag_stored_song(state, song_id).await;
        }
        return Ok(removed);
    }
    if pinned.as_deref() == Some(look.label().as_str()) {
        return Ok(false);
    }
    // a stem wears its song's cover: it takes the song's, drawn for the song first
    if let Some((from, "stems")) = derived_from(&song) {
        if let Some(parent) = state.library.get_song(from)? {
            if parent.metadata.get("cover_filename").is_none() {
                return Box::pin(pin_placeholder(state, from)).await;
            }
            cover_like(state, &parent, song_id)?;
            tag_stored_song(state, song_id).await;
            return Ok(true);
        }
    }
    let (image, media_type, label, source) = placeholder_image(state, &song, &look).await?;
    if pinned.as_deref() == Some(label.as_str()) {
        return Ok(false);
    }
    state.library.store_placeholder_cover(song_id, &image, media_type, &label, source.as_deref())?;
    tag_stored_song(state, song_id).await;
    Ok(true)
}

/// Brings every track in line with the look: a placeholder written into each
/// track without a cover of its own, or taken out of all when keeping is off.
/// Tracks that wore their pattern while Commons rested try for a photograph
/// again once the rest is over.
async fn pin_placeholders(state: AppState) {
    loop {
        pin_pass(&state).await;
        if !(state.cover_look.read().await.photo && cover_art::photos_resting()) {
            return;
        }
        tokio::time::sleep(cover_art::PHOTO_REST).await;
    }
}

/// One pass over the library. Songs go first, so their stems take the covers from them.
async fn pin_pass(state: &AppState) {
    let _pass = state.cover_pinning.lock().await;
    let songs = match state.library.list_songs() {
        Ok(songs) => songs,
        Err(error) => {
            eprintln!("[ERROR] the library did not list its tracks for their covers: {error:#}");
            return;
        }
    };
    let (stems, songs): (Vec<_>, Vec<_>) =
        songs.into_iter().partition(|song| derived_from(song).is_some_and(|(_, tool)| tool == "stems"));
    for song in songs.into_iter().chain(stems) {
        if let Err(error) = pin_placeholder(state, &song.id).await {
            eprintln!("[ERROR] no placeholder cover for {}: {error:#}", song.id);
        }
    }
}

/// The placeholder a track shows while it has no cover of its own and keeping
/// is off: drawn once per look and kept beside the library.
async fn placeholder_cover(State(state): State<AppState>, Path(id): Path<String>) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let song = state
        .library
        .get_song(&id)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))?;
    let look = state.cover_look.read().await.clone();
    let folder = state.library.media_dir().join("cover-placeholders");
    // the seed can come from metadata an agent edits, so it never names a path
    let name: String = cover_art::cover_seed(&song)
        .chars()
        .map(|character| if character.is_ascii_alphanumeric() || character == '-' { character } else { '_' })
        .collect();
    let named = |label: &str, extension: &str| folder.join(format!("{name}-{}.{extension}", label.replace(':', "-")));
    let kept = [("jpg", "image/jpeg"), ("png", "image/png")]
        .into_iter()
        .map(|(extension, media_type)| (named(&look.label(), extension), media_type))
        .find(|(path, _)| path.is_file());
    let (bytes, media_type) = match kept {
        Some((path, media_type)) => (
            tokio::fs::read(&path).await.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("read the placeholder: {error}")))?,
            media_type,
        ),
        None => {
            let (image, media_type, label, _) = placeholder_image(&state, &song, &look)
                .await
                .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}")))?;
            let extension = if media_type == "image/png" { "png" } else { "jpg" };
            if let Err(error) = std::fs::create_dir_all(&folder).and_then(|()| std::fs::write(named(&label, extension), &image)) {
                eprintln!("[ERROR] the placeholder of {id} was drawn but not kept: {error}");
            }
            (image, media_type)
        }
    };
    Ok(axum::response::Response::builder()
        .header(header::CONTENT_TYPE, media_type)
        .header(header::CONTENT_LENGTH, bytes.len())
        .body(Body::from(bytes))
        .expect("valid placeholder response"))
}

#[derive(Debug, Deserialize)]
struct MediaSearch {
    #[serde(default)]
    q: String,
    #[serde(default)]
    from: usize,
}

#[derive(Debug, Deserialize)]
struct MediaStyle {
    #[serde(default)]
    style: String,
}

/// The scenes a style calls up: what the picture search starts from.
async fn media_scenes(Query(query): Query<MediaStyle>) -> Json<Value> {
    Json(serde_json::json!({ "scenes": cover_art::scenes(&query.style) }))
}

/// CC0 photographs from Commons for a search, twenty-four at a time.
async fn media_photos(Query(query): Query<MediaSearch>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let wanted = query.q.trim();
    if wanted.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "say what to look for".into()));
    }
    let photos = cover_art::photos_of(&net::client(), wanted)
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("{error:#}")))?;
    let shown: Vec<cover_art::Photo> = photos.iter().skip(query.from).take(24).cloned().collect();
    Ok(Json(serde_json::json!({ "photos": shown, "from": query.from, "total": photos.len() })))
}

/// Clips from Commons free of copyright for a search, twenty-four at a time.
async fn media_videos(Query(query): Query<MediaSearch>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let wanted = query.q.trim();
    if wanted.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "say what to look for".into()));
    }
    let clips = cover_art::clips_of(&net::client(), wanted)
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("{error:#}")))?;
    let shown: Vec<cover_art::Clip> = clips.iter().skip(query.from).take(24).cloned().collect();
    Ok(Json(serde_json::json!({ "videos": shown, "from": query.from, "total": clips.len() })))
}

#[derive(Debug, Deserialize)]
struct ChosenCoverPhoto {
    image: String,
    #[serde(default)]
    page: Option<String>,
}

/// A photograph chosen in the cover dialog becomes the track's own cover.
async fn choose_cover_photo(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<ChosenCoverPhoto>,
) -> Result<Json<library::Song>, (StatusCode, Json<ApiError>)> {
    let image = cover_art::download(&net::client(), &request.image)
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("{error:#}")))?;
    let song = state
        .library
        .store_chosen_cover(&id, &image, "image/jpeg", request.page.as_deref().filter(|page| page.starts_with(cover_art::COMMONS_PAGES)))
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    tag_stored_song(&state, &id).await;
    Ok(Json(song))
}

#[derive(Debug, Deserialize)]
struct ChosenCoverPattern {
    pattern: String,
    seed: String,
}

/// A pattern chosen in the cover dialog becomes the track's own cover.
async fn choose_cover_pattern(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<ChosenCoverPattern>,
) -> Result<Json<library::Song>, (StatusCode, Json<ApiError>)> {
    if !cover_art::PATTERNS.contains(&request.pattern.as_str()) {
        return Err(api_error(StatusCode::BAD_REQUEST, format!("there is no cover pattern '{}'", request.pattern)));
    }
    let image = tokio::task::spawn_blocking(move || cover_art::pattern_png(&request.pattern, &request.seed, cover_art::KEPT_SIZE))
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("the pattern drawing stopped: {error}")))?
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}")))?;
    let song = state
        .library
        .store_chosen_cover(&id, &image, "image/png", None)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    tag_stored_song(&state, &id).await;
    Ok(Json(song))
}

async fn store_library_cover(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<StoreCoverRequest>,
) -> Result<Json<library::Song>, (StatusCode, Json<ApiError>)> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let image = STANDARD
        .decode(request.image_base64.trim())
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, format!("cover image is not valid base64: {error}")))?;
    let song = state
        .library
        .store_song_cover(&id, &image, &request.media_type)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    // The cover belongs in the file too, not only beside it.
    tag_stored_song(&state, &id).await;
    Ok(Json(song))
}

async fn create_library_song(State(state):State<AppState>,Json(input):Json<library::SongInput>)->Result<(StatusCode,Json<library::Song>),(StatusCode,Json<ApiError>)>{state.library.create_song(input).map(|s|(StatusCode::CREATED,Json(s))).map_err(|e|api_error(StatusCode::BAD_REQUEST,e.to_string()))}
async fn import_library_audio(State(state): State<AppState>, mut multipart: Multipart) -> Result<(StatusCode, Json<library::Song>), (StatusCode, Json<ApiError>)> {
    let mut title = None; let mut caption = String::new(); let mut lyrics = String::new(); let mut audio = None; let mut filename = None;
    while let Some(field) = multipart.next_field().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read import form: {e}")))? {
        let name = field.name().unwrap_or_default().to_owned();
        if name == "audio" { filename = field.file_name().map(str::to_owned); audio = Some(field.bytes().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read audio upload: {e}")))?.to_vec()); }
        else { let value = field.text().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read import field: {e}")))?; match name.as_str() { "title" => title = Some(value), "caption" => caption = value, "lyrics" => lyrics = value, _ => {} } }
    }
    let filename = filename.ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "audio file is required".into()))?;
    let extension = std::path::Path::new(&filename).extension().and_then(|value| value.to_str()).unwrap_or_default().to_owned();
    let title = title.filter(|value| !value.trim().is_empty()).unwrap_or_else(|| std::path::Path::new(&filename).file_stem().and_then(|value| value.to_str()).unwrap_or("Imported audio").to_owned());
    let audio = audio.ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "audio file is required".into()))?;
    let duration = library::audio_duration_seconds(&audio, &extension.to_ascii_lowercase(), None);
    let song = state.library.import_audio_song(library::AudioImportInput { title, caption, lyrics, metadata: serde_json::json!({"imported_filename": filename, "duration_seconds": duration}), generation_settings: Value::Null, engine_id: "imported-audio".into(), profile_id: None, source: "audio_import".into(), audio_extension: extension, audio }).map_err(|e| api_error(StatusCode::BAD_REQUEST, e.to_string()))?.song;
    Ok((StatusCode::CREATED, Json(song)))
}
async fn update_library_song(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<library::SongInput>)->Result<Json<library::Song>,(StatusCode,Json<ApiError>)>{
    let song = state.library.update_song(&id,input).map_err(|e|api_error(StatusCode::BAD_REQUEST,e.to_string()))?.ok_or_else(||api_error(StatusCode::NOT_FOUND,"Song not found".into()))?;
    // A rename is a title change, and the file carries the title.
    tag_stored_song(&state, &id).await;
    Ok(Json(song))
}
/// Removes a song and its files; a file that cannot go (a player holds it)
/// is named in the answer instead of only in the log.
async fn delete_library_song(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let song = state.library.get_song(&id).map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !state.library.delete_song(&id).map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))? {
        return Err(api_error(StatusCode::NOT_FOUND, "Song not found".into()));
    }
    let mut left = Vec::new();
    if let Some(song) = song {
        for path in song_files(&state, &song) {
            if let Err(error) = std::fs::remove_file(&path) {
                eprintln!("[ERROR] delete song {id}: could not remove {}: {error}", path.display());
                left.push(format!("{}: {error}", plain_path(&path)));
            }
        }
    }
    Ok(Json(serde_json::json!({ "deleted": true, "files_left": left })))
}

/// The files in the media folder that belong to one song: its audio, its
/// stems and its cover. Anything outside the media folder is not ours to remove.
fn song_files(state: &AppState, song: &library::Song) -> Vec<PathBuf> {
    let media = state.library.media_dir();
    let mut files: Vec<PathBuf> = separation::STEMS.iter().map(|stem| stem_path(state, &song.id, stem)).collect();
    if let Some(audio) = state.library.media_path_for_song(song) {
        files.push(audio);
    }
    // a processed track owns its original and every version besides the one playing
    if let Some(original) = song.metadata.get("original_audio_path").and_then(Value::as_str).and_then(|path| state.library.resolve_media(path)) {
        files.push(original);
    }
    for version in song.metadata.get("audio_versions").and_then(Value::as_array).into_iter().flatten() {
        if let Some(file) = version.get("file").and_then(Value::as_str) {
            files.push(media.join(file));
        }
    }
    files.push(midi_path(state, &song.id));
    files.push(midi_sidecar(&midi_path(state, &song.id)));
    files.sort();
    files.dedup();
    if let Some((cover, _)) = state.library.cover_path_for_song(song) {
        files.push(cover);
    }
    files.retain(|path| in_media_folder(media, path));
    files.sort();
    files.dedup();
    files
}

/// Whether a file sits directly in the media folder, however either path is
/// written: the library resolves its paths to `\\?\F:\…` while the folder
/// is kept as `F:\…`, and comparing them as written removed nothing.
fn in_media_folder(media: &std::path::Path, path: &std::path::Path) -> bool {
    let (Ok(root), Ok(file)) = (media.canonicalize(), path.canonicalize()) else { return false };
    file.is_file() && file.parent() == Some(root.as_path())
}
async fn library_playlists(State(state):State<AppState>)->Result<Json<Vec<library::Playlist>>,(StatusCode,Json<ApiError>)>{state.library.list_playlists().map(Json).map_err(|e|api_error(StatusCode::INTERNAL_SERVER_ERROR,e.to_string()))}
async fn create_library_playlist(State(state):State<AppState>,Json(input):Json<library::PlaylistInput>)->Result<(StatusCode,Json<library::Playlist>),(StatusCode,Json<ApiError>)>{state.library.create_playlist(input).map(|p|(StatusCode::CREATED,Json(p))).map_err(|e|api_error(StatusCode::BAD_REQUEST,e.to_string()))}
async fn library_playlist(State(state):State<AppState>,Path(id):Path<String>)->Result<Json<library::Playlist>,(StatusCode,Json<ApiError>)>{state.library.get_playlist(&id).map_err(|e|api_error(StatusCode::INTERNAL_SERVER_ERROR,e.to_string()))?.map(Json).ok_or_else(||api_error(StatusCode::NOT_FOUND,"Playlist not found".into()))}
async fn update_library_playlist(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<library::PlaylistInput>)->Result<Json<library::Playlist>,(StatusCode,Json<ApiError>)>{state.library.update_playlist(&id,input).map_err(|e|api_error(StatusCode::BAD_REQUEST,e.to_string()))?.map(Json).ok_or_else(||api_error(StatusCode::NOT_FOUND,"Playlist not found".into()))}
async fn delete_library_playlist(State(state):State<AppState>,Path(id):Path<String>)->Result<StatusCode,(StatusCode,Json<ApiError>)>{if state.library.delete_playlist(&id).map_err(|e|api_error(StatusCode::INTERNAL_SERVER_ERROR,e.to_string()))?{Ok(StatusCode::NO_CONTENT)}else{Err(api_error(StatusCode::NOT_FOUND,"Playlist not found".into()))}}

/// The proxy settings, and why the saved address cannot be used when it cannot:
/// requests then go straight out, and Settings says so.
async fn read_proxy() -> Json<Value> {
    let settings = net::current();
    let mut value = serde_json::to_value(&settings).unwrap_or(Value::Null);
    value["problem"] = serde_json::json!(settings.validated().err().map(|error| format!("{error:#}")));
    Json(value)
}

/// Takes effect at once for the studio's own requests; the window's browser
/// takes it at the next start.
async fn update_proxy(
    State(state): State<AppState>,
    Json(settings): Json<net::ProxySettings>,
) -> Result<Json<net::ProxySettings>, (StatusCode, Json<ApiError>)> {
    let settings = settings.validated().map_err(|error| api_error(StatusCode::BAD_REQUEST, format!("{error:#}")))?;
    net::set(settings.clone());
    persist_studio_settings(&state)
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("the proxy is in use but was not saved: {error:#}")))?;
    Ok(Json(settings))
}

async fn test_proxy(Json(settings): Json<net::ProxySettings>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    net::test(settings).await.map(Json).map_err(|error| api_error(StatusCode::BAD_REQUEST, format!("{error:#}")))
}

async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}

async fn health(State(state): State<AppState>) -> Json<Value> {
    let engine_ready = state.music_server.health().await;
    // Which program is actually answering here, and whether it can start an
    // engine at all. A second copy of the studio - a development build, say -
    // takes this port and the window then waits forever on an engine that
    // copy has no bundle for. Saying whose service this is turns a hang into
    // a sentence.
    let executable = std::env::current_exe().ok().map(|path| path.display().to_string());
    let engine_available = engine_location(*state.engine_options.read().await).bundle_root.is_dir();
    Json(serde_json::json!({
        "status": "ok",
        "runtime": "native",
        "service_executable": executable,
        "engine_bundle_present": engine_available,
        "music_engine": {
            "id": PRIMARY_MUSIC_ENGINE_ID,
            "base_url": state.music_server.base_url,
            "reachable": engine_ready,
        }
    }))
}

async fn configuration(State(state): State<AppState>) -> Json<StudioConfiguration> {
    Json(state.configuration.read().await.clone())
}

async fn update_configuration(
    State(state): State<AppState>,
    Json(update): Json<StudioConfiguration>,
) -> Json<StudioConfiguration> {
    // A page that changes one capability sends one selection. Storing the
    // request verbatim then erased every other choice - which is how a studio
    // with a downloaded engine started answering "the local music engine is not
    // configured" after the assistant was pointed at a local model.
    let configuration = {
        let mut stored = state.configuration.write().await;
        for selection in update.selections {
            match stored.selections.iter_mut().find(|existing| existing.capability == selection.capability) {
                Some(existing) => *existing = selection,
                None => stored.selections.push(selection),
            }
        }
        stored.clone()
    };

    // The choice has to reach the code that does the work, or the button is
    // decoration. Speech-to-text is done by the karaoke stack, and the writing
    // assistant has its own provider; both follow this page now.
    for selection in &configuration.selections {
        match selection.capability {
            Capability::SpeechToText => {
                let mut sync = state.lyrics_sync_config.write().await;
                let installed_parakeet = state.lyrics_sync.installed_parakeet();
                sync.provider = match selection.mode {
                    ExecutionMode::OpenRouter => lyrics_sync::AsrProvider::OpenRouter,
                    ExecutionMode::Local => match selection.local_engine.as_deref() {
                        Some("whisper") => lyrics_sync::AsrProvider::Whisper,
                        Some("parakeet") => lyrics_sync::AsrProvider::Parakeet,
                        _ if installed_parakeet.is_some() => lyrics_sync::AsrProvider::Parakeet,
                        _ if state.lyrics_sync.whisper_binary().is_some() => lyrics_sync::AsrProvider::Whisper,
                        _ => sync.provider,
                    },
                };
                // picked because some Parakeet is there: the one named must be that one
                if sync.provider == lyrics_sync::AsrProvider::Parakeet && selection.local_engine.is_none() && !state.lyrics_sync.parakeet_ready(sync.whisper_model.as_deref()) {
                    sync.whisper_model = installed_parakeet.map(String::from);
                }
                if selection.mode == ExecutionMode::OpenRouter {
                    sync.openrouter_model = selection.cloud_model.clone();
                }
            }
            Capability::PromptEnhancement => {
                let mut assistant = state.assistant.write().await;
                match selection.mode {
                    ExecutionMode::OpenRouter => {
                        assistant.provider = AssistantProvider::OpenRouter;
                        if let Some(model) = selection.cloud_model.clone() {
                            assistant.openrouter_model = Some(model);
                        }
                    }
                    ExecutionMode::Local if assistant.provider == AssistantProvider::Agent => {}
                    ExecutionMode::Local => {
                        // Whichever local shape is set up: a managed model the
                        // studio downloaded, or a server the user runs.
                        assistant.provider = if assistant.managed_model.is_some() || assistant.managed_path.is_some() {
                            AssistantProvider::Managed
                        } else {
                            AssistantProvider::Local
                        };
                    }
                }
            }
            _ => {}
        }
    }

    let _ = persist_studio_settings(&state).await;
    Json(configuration)
}

async fn engine_options(State(state): State<AppState>) -> Json<Value> {
    let options = *state.engine_options.read().await;
    Json(serde_json::json!({
        "options": options,
        "effective_max_batch": options.effective_max_batch(),
        "restart_required_to_apply": true,
    }))
}

/// Stores the launch flags and restarts the engine if it is running, because
/// upstream reads them once at startup.
async fn update_engine_options(
    State(state): State<AppState>,
    Json(request): Json<EngineOptions>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    if request.max_batch.is_some_and(|value| value == 0 || value > 8) {
        return Err(api_error(StatusCode::BAD_REQUEST, "max_batch must be between 1 and 8".into()));
    }
    if request.max_seq.is_some_and(|value| !(4096..=24576).contains(&value)) {
        return Err(api_error(StatusCode::BAD_REQUEST, "max_seq must be between 4096 and 24576".into()));
    }
    if request.vae_core.is_some_and(|value| !(64..=4096).contains(&value)) {
        return Err(api_error(StatusCode::BAD_REQUEST, "vae_core must be between 64 and 4096".into()));
    }
    if request.vae_halo.is_some_and(|value| value > 256) {
        return Err(api_error(StatusCode::BAD_REQUEST, "vae_halo must be at most 256".into()));
    }
    let changed = {
        let mut options = state.engine_options.write().await;
        let changed = *options != request;
        *options = request;
        changed
    };
    let _ = persist_studio_settings(&state).await;

    let mut restarted = false;
    if changed && state.music_server.health().await {
        restart_engine(&state).await.map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, error))?;
        restarted = true;
    }
    Ok(Json(serde_json::json!({
        "options": request,
        "effective_max_batch": request.effective_max_batch(),
        "engine_restarted": restarted,
    })))
}

async fn restart_local_engine(State(state): State<AppState>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    restart_engine(&state).await.map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, error))?;
    Ok(Json(serde_json::json!({ "engine_id": PRIMARY_MUSIC_ENGINE_ID, "restarted": true })))
}

async fn restart_engine(state: &AppState) -> Result<(), String> {
    let mut supervisor = state.engine.lock().await;
    let owned = supervisor.is_some();
    if let Some(engine) = supervisor.as_mut() {
        tokio::task::block_in_place(|| engine.stop(std::time::Duration::from_secs(10)))
            .map_err(|error| format!("stopping the local engine failed: {error}"))?;
    }
    // Launch flags are read once at engine startup. If something this service
    // does not own is still listening, starting again would silently reuse it
    // and the new flags would never take effect — report that instead of
    // claiming a restart that did not happen.
    if !owned && state.music_server.health().await {
        return Err(
            "An engine that this application did not start is already running on the engine port.              Close it and try again, otherwise the new options cannot be applied."
                .into(),
        );
    }
    // Nothing can start until the libraries the engine binary imports are on
    // disk: Windows resolves them before the process runs, so a missing cuBLAS
    // is not a slow start, it is no start at all. This is the path the studio
    // actually takes on launch, so the fetch belongs here rather than only in
    // the endpoint nothing calls.
    let options = *state.engine_options.read().await;
    if options.backend == music_engine::yue_server::ComputeBackend::Cuda && hardware::hardware().cuda.is_none() {
        return Err("CUDA was chosen, but this card or its driver runs neither CUDA build of the engine: it needs an NVIDIA card from the GTX 900 series on and driver 525 or newer. Update the NVIDIA driver, or choose Vulkan in Settings.".into());
    }
    let chain = options.device_chain(&state.failed_devices.read().await);
    let mut last_error = String::new();
    for (index, device) in chain.iter().copied().enumerate() {
        let attempt = EngineOptions { backend: device, ..options };
        let cuda = attempt.cuda_build();
        if !state.engine_runtime.is_ready(cuda) {
            state
                .engine_runtime
                .install_missing(cuda)
                .await
                .map_err(|error| format!("the engine's runtime libraries could not be installed: {error}"))?;
        }
        // The engine loads eleven gigabytes of weights the moment it starts.
        // If the writing assistant is still holding the card, it does not finish.
        free_the_card_for_the_engine(state).await;
        let models = selected_engine_models(state).await?;
        let config = engine_location(attempt)
            .resolve(models)
            .map_err(|error| format!("the local engine runtime was not found: {error}"))?;
        let mut engine = music_engine::yue_server::YueServerSupervisor::new(config).map_err(|error| error.to_string())?;
        match tokio::task::block_in_place(|| engine.ensure_started(std::time::Duration::from_secs(60))) {
            Ok(_) => {
                *supervisor = Some(engine);
                *state.active_device.write().await = Some(device);
                if options.backend == music_engine::yue_server::ComputeBackend::Auto {
                    music_engine::yue_server::note_in_log(&format!("computing on {}", device_name(device)));
                }
                return Ok(());
            }
            Err(error) => {
                last_error = format!("the local engine did not start: {error}");
                // Only Auto moves on, and never past a card that ran out of
                // memory: Vulkan has no more of it, and the reason is shown.
                let next = chain.get(index + 1).copied();
                let out_of_memory = describes_exhausted_memory(&last_run_log().to_lowercase());
                match next {
                    Some(next) if options.backend == music_engine::yue_server::ComputeBackend::Auto && !out_of_memory => {
                        music_engine::yue_server::note_in_log(&format!(
                            "{} did not start ({error}); trying {}",
                            device_name(device),
                            device_name(next)
                        ));
                        state.failed_devices.write().await.push(device);
                    }
                    _ => return Err(last_error),
                }
            }
        }
    }
    Err(last_error)
}

/// The device's name as the settings show it.
fn device_name(device: music_engine::yue_server::ComputeBackend) -> &'static str {
    use music_engine::yue_server::ComputeBackend;
    match device {
        ComputeBackend::Auto => "Auto",
        ComputeBackend::Cuda => "CUDA",
        ComputeBackend::Vulkan => "Vulkan",
        ComputeBackend::Cpu => "the processor",
    }
}

/// The engine log since the last start, where the reason a run ended is.
fn last_run_log() -> String {
    let tail = music_engine::yue_server::startup_log_tail(400);
    let start = tail.iter().rposition(|line| line.contains("---- starting")).unwrap_or(0);
    tail[start..].join("\n")
}

/// The engine log of the run before the current one: after a restart, the
/// run that ended is where the reason is.
fn previous_run_log() -> String {
    let tail = music_engine::yue_server::startup_log_tail(600);
    let starts: Vec<usize> = tail.iter().enumerate().filter(|(_, line)| line.contains("---- starting")).map(|(index, _)| index).collect();
    match starts.as_slice() {
        [.., before, last] => tail[*before..*last].join("\n"),
        _ => tail.join("\n"),
    }
}

/// Why a song was lost with an engine that restarted during it, from the log
/// of the run that ended.
fn lost_job_reason(log: &str) -> String {
    let log = log.to_lowercase();
    if describes_exhausted_memory(&log) {
        "The graphics card ran out of memory on this song and the engine restarted, so the song was lost. Choose a smaller model set in the model manager or a shorter song, and close whatever else uses the card; a card below the smallest set cannot make songs.".into()
    } else if describes_device_failure(&log) {
        "The graphics card failed during this song (a CUDA or driver error) and the engine restarted, so the song was lost. The engine log is in Settings, Engine.".into()
    } else {
        "The engine stopped during this song and restarted, so the song was lost. The engine log is in Settings, Engine.".into()
    }
}

/// Whether the engine ended without a word about why. Its own failures -
/// an assertion, an error, running out of memory - are written before it
/// goes; a device lost under the driver takes the process with nothing said.
fn died_silently(log: &str) -> bool {
    !["error", "assert", "fatal", "exception", "abort", "failed", "out of memory"]
        .iter()
        .any(|marker| log.contains(marker))
}

/// Whether a lowercased engine log says the compute device itself failed -
/// a CUDA or Vulkan error, a device lost, the engine's self-test - as opposed
/// to running out of memory, which another device would not cure.
fn describes_device_failure(log: &str) -> bool {
    if describes_exhausted_memory(log) {
        return false;
    }
    [
        "cuda error",
        "no kernel image",
        "unsupported toolchain",
        "ggml_cuda_compute_forward",
        "fatal: self-test",
        "_cuda_backend=",
        "devicelost",
        "device lost",
        "vk::",
        "ggml_vulkan: error",
    ]
    .iter()
    .any(|marker| log.contains(marker))
}

/// Where the packaged or developer-built `yue-server` lives. Every value is
/// an explicit override or a documented default; nothing is downloaded here.
fn engine_bundle_root() -> PathBuf {
    env::var_os("YUE_ENGINE_ROOT")
        .map(PathBuf::from)
        .or_else(|| env::var_os("YUE_ENGINE_BIN").map(PathBuf::from).and_then(|path| path.parent().map(std::path::Path::to_path_buf)))
        .or_else(|| std::env::current_exe().ok().and_then(|path| path.parent().map(|parent| parent.join("resources").join("yue2-cpp"))).map(|beside| {
            // In a macOS app bundle the executable is Contents/MacOS/<name> and
            // bundled resources are in Contents/Resources.
            let in_bundle = beside.ancestors().nth(3).map(|contents| contents.join("Resources").join("resources").join("yue2-cpp"));
            // A Linux package puts the executable in usr/bin and the resources
            // in usr/lib/<product name>, the AppImage under its own usr.
            let in_package = beside.ancestors().nth(3).map(|usr| usr.join("lib").join("YuE2 Studio").join("resources").join("yue2-cpp"));
            match (in_bundle, in_package) {
                (Some(path), _) if cfg!(target_os = "macos") && path.is_dir() => path,
                (_, Some(path)) if cfg!(target_os = "linux") && !beside.is_dir() && path.is_dir() => path,
                _ => beside,
            }
        }))
        .unwrap_or_else(|| PathBuf::from("resources/yue2-cpp"))
}

fn engine_location(options: EngineOptions) -> music_engine::yue_server::YueServerLocation {
    music_engine::yue_server::YueServerLocation {
        bundle_root: engine_bundle_root(),
        configured_executable: env::var_os("YUE_ENGINE_BIN").map(PathBuf::from),
        host: env::var("YUE_ENGINE_HOST").ok(),
        port: env::var("YUE_ENGINE_PORT").ok().and_then(|value| value.parse().ok()),
        options: options.to_engine(),
    }
}

/// The weights the selected set resolves to, as paths the engine can open.
async fn selected_engine_models(state: &AppState) -> Result<music_engine::yue_server::YueModelFiles, String> {
    let selected_component_ids = state.selected_component_ids.read().await.clone();
    let selected_profile_id = state.selected_profile_id.read().await.clone();
    let files = match (selected_component_ids, selected_profile_id) {
        (Some(ids), _) => state.model_manager.installed_component_files(&ids),
        (None, Some(profile_id)) => state.model_manager.installed_profile_files(&profile_id),
        (None, None) => return Err("no model set is selected; choose one in Settings - Models".into()),
    }
    .map_err(|error| error.to_string())?;
    let root = state.model_manager.models_directory();
    let adapters = state.adapters.root().to_path_buf();
    fs::create_dir_all(&adapters).map_err(|error| format!("create the adapter folder {}: {error}", adapters.display()))?;
    Ok(music_engine::yue_server::YueModelFiles {
        backbone: root.join(&files.backbone),
        vae: root.join(&files.vae),
        transcriber: files.transcriber.map(|name| root.join(name)),
        adapters: Some(adapters),
        companion: root.join(&files.companion),
    })
}

/// yue-server takes its weights at launch, so a different selection only takes
/// effect through a restart. A running engine this studio owns is restarted
/// when the files it serves are not the ones now selected; an engine that is
/// not running is left to the supervisor loop.
async fn reload_engine_if_models_changed(state: &AppState) {
    let Ok(wanted) = selected_engine_models(state).await else { return };
    let current = {
        let supervisor = state.engine.lock().await;
        supervisor.as_ref().map(|engine| engine.config().models.clone())
    };
    let Some(current) = current else { return };
    let same = |a: &std::path::Path, b: &std::path::Path| {
        fs::canonicalize(a).ok().zip(fs::canonicalize(b).ok()).is_some_and(|(a, b)| a == b)
    };
    let unchanged = same(&current.backbone, &wanted.backbone)
        && same(&current.vae, &wanted.vae)
        && match (&current.transcriber, &wanted.transcriber) {
            (Some(a), Some(b)) => same(a, b),
            (None, None) => true,
            _ => false,
        }
        && match (&current.adapters, &wanted.adapters) {
            (Some(a), Some(b)) => same(a, b),
            (None, None) => true,
            _ => false,
        };
    if unchanged {
        return;
    }
    music_engine::yue_server::note_in_log("the selected model set changed; restarting the engine on it");
    if let Err(error) = restart_engine(state).await {
        eprintln!("the engine did not restart on the new model set: {error}");
    }
}

async fn openrouter_catalog(State(state): State<AppState>) -> Json<Value> {
    // Fetch it if this process has not yet: an empty answer here made every
    // capability read "no model in the refreshed catalog", which is a lie -
    // the catalog had simply never been read.
    let catalog = catalog_for(&state).await.ok();
    let refreshed_at = state.openrouter_catalog.read().await.refreshed_at.clone();
    // What the studio would pick for each capability if the user picks
    // nothing. The panel shows these as the selection, so adding a key is
    // enough to start rather than the beginning of a shopping trip.
    let suggested = catalog.as_ref().map(|catalog| {
        serde_json::json!({
            "speech_to_text": providers::openrouter::suggested_model(catalog, Capability::SpeechToText),
            "prompt_enhancement": providers::openrouter::suggested_model(catalog, Capability::PromptEnhancement),
            "cover_art": providers::openrouter::suggested_model(catalog, Capability::CoverArt),
        })
    });
    Json(serde_json::json!({
        "models": catalog.map(|catalog| catalog.models),
        "refreshed_at": refreshed_at,
        "suggested": suggested,
    }))
}



/// Where the provider catalog is kept between runs.
fn openrouter_catalog_path() -> Option<PathBuf> {
    studio_data_root().map(|root| root.join("openrouter-catalog.json"))
}

/// Reads the catalog saved by the last refresh. Nothing here touches the
/// network: the studio refreshes when a key is connected and when the user
/// asks, and lives off this file the rest of the time.
fn load_cached_catalog() -> Option<providers::openrouter::CapabilityCatalog> {
    let path = openrouter_catalog_path()?;
    let body = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&body).ok()
}

fn save_cached_catalog(catalog: &providers::openrouter::CapabilityCatalog) {
    let Some(path) = openrouter_catalog_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(body) = serde_json::to_string(catalog) {
        let _ = std::fs::write(path, body);
    }
}

/// The capability catalog, fetched if this process has not got it yet.
///
/// The catalog lives in memory, so it is empty after every restart. Telling
/// the user to "refresh the catalog" at the moment they press a button is
/// asking them to do the program's job - and the refresh button lives on
/// another screen entirely.
/// The catalogue, and a fresh one when the model asked about is not in it.
///
/// The record is what the request is built from - the model's own parameters,
/// the efforts it accepts - so a catalogue that predates the model would have
/// the studio guessing about a model OpenRouter can describe exactly.
async fn catalog_describing(state: &AppState, model: &str) -> Result<providers::openrouter::CapabilityCatalog, String> {
    let catalog = catalog_for(state).await?;
    if model.is_empty() || catalog.models.iter().any(|entry| entry.id == model) {
        return Ok(catalog);
    }
    {
        let mut cached = state.openrouter_catalog.write().await;
        cached.catalog = None;
    }
    catalog_for(state).await
}

async fn catalog_for(state: &AppState) -> Result<providers::openrouter::CapabilityCatalog, String> {
    if let Some(catalog) = state.openrouter_catalog.read().await.catalog.clone() {
        return Ok(catalog);
    }
    // The last refresh, read from disk. A restart should not cost a request.
    if let Some(catalog) = load_cached_catalog() {
        let mut cached = state.openrouter_catalog.write().await;
        cached.catalog = Some(catalog.clone());
        return Ok(catalog);
    }
    let client = net::client();
    let fetch = |path: &'static str| {
        let client = client.clone();
        async move {
            client
                .get(format!("{}{}", providers::openrouter::API_BASE_URL, path))
                .send()
                .await?
                .error_for_status()?
                .text()
                .await
        }
    };
    let general = fetch(providers::openrouter::MODELS_PATH)
        .await
        .map_err(|error| format!("OpenRouter catalog request failed: {error}"))?;
    let transcription = fetch(providers::openrouter::TRANSCRIPTION_MODELS_PATH).await.unwrap_or_default();
    let images = fetch(providers::openrouter::IMAGE_MODELS_PATH).await.unwrap_or_default();
    let parsed = providers::openrouter::CapabilityCatalog::parse_merged(&general, &[&transcription, &images])
    .map_err(|error| format!("OpenRouter catalog parse failed: {error}"))?;
    save_cached_catalog(&parsed);
    let mut cached = state.openrouter_catalog.write().await;
    cached.catalog = Some(parsed.clone());
    cached.refreshed_at = Some(chrono_like_timestamp());
    Ok(parsed)
}

async fn refresh_openrouter_catalog(State(state): State<AppState>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let client = net::client();
    let fetch = |path: &'static str| {
        let client = client.clone();
        async move {
            client
                .get(format!("{}{}", providers::openrouter::API_BASE_URL, path))
                .send()
                .await?
                .error_for_status()?
                .text()
                .await
        }
    };
    let general = fetch(providers::openrouter::MODELS_PATH)
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("OpenRouter catalog request failed: {error}")))?;
    // The recognisers live behind their own filter; without this second call
    // the catalog contains no model that can return timings.
    let transcription = fetch(providers::openrouter::TRANSCRIPTION_MODELS_PATH).await.unwrap_or_default();
    let images = fetch(providers::openrouter::IMAGE_MODELS_PATH).await.unwrap_or_default();
    let parsed = providers::openrouter::CapabilityCatalog::parse_merged(&general, &[&transcription, &images])
    .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("OpenRouter catalog parse failed: {error}")))?;
    let refreshed_at = chrono_like_timestamp();
    let models = parsed.models.clone();
    save_cached_catalog(&parsed);
    let mut cached = state.openrouter_catalog.write().await;
    cached.catalog = Some(parsed);
    cached.refreshed_at = Some(refreshed_at.clone());
    Ok(Json(serde_json::json!({ "models": models, "refreshed_at": refreshed_at })))
}

/// Sends an already catalog-validated OpenRouter JSON request. The frontend
/// supplies a model selection and input data, never an API key or endpoint.
async fn execute_openrouter_json(
    request: providers::openrouter::OpenRouterRequest,
) -> anyhow::Result<OpenRouterResponse> {
    let authenticated = providers::openrouter::authenticated_request_for(request)?;
    // Every cloud request passes through here, so this is where they are all
    // written down: which model, how long, and what came back when it was not
    // a success.
    let what = authenticated.request.path.trim_matches('/');
    let model = authenticated
        .request
        .body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("?")
        .to_string();
    request_log::asked(what, &model, authenticated.request.body.to_string().chars().count());
    let started = std::time::Instant::now();
    let response = net::client()
        .post(format!("{}{}", providers::openrouter::API_BASE_URL, authenticated.request.path))
        .bearer_auth(authenticated.api_key)
        .json(&authenticated.request.body)
        .send()
        .await
        .inspect_err(|error| request_log::failed(what, &model, &error.to_string()))?;
    let status = response.status();
    let response = match response.error_for_status_ref() {
        Ok(_) => response,
        Err(error) => {
            let text = response.text().await.unwrap_or_default();
            request_log::answered(what, &model, status.as_u16(), started.elapsed().as_secs_f64(), text.chars().count());
            request_log::unusable(what, &model, &error.to_string(), &text);
            return Err(error.into());
        }
    };
    let generation_id = response
        .headers()
        .get("X-Generation-Id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = response.json::<Value>().await?;
    request_log::answered(what, &model, status.as_u16(), started.elapsed().as_secs_f64(), body.to_string().chars().count());
    Ok(OpenRouterResponse { body, generation_id })
}

async fn create_openrouter_transcription(
    State(state): State<AppState>,
    Json(input): Json<OpenRouterTranscriptionRequest>,
) -> Result<Json<OpenRouterResponse>, (StatusCode, Json<ApiError>)> {
    let catalog = catalog_for(&state)
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, error))?;
    let request = providers::openrouter::stt_request_for(
        &catalog,
        &input.model_id,
        providers::openrouter::Base64AudioInput {
            timestamps: false,
            data: &input.audio_base64,
            format: &input.audio_format,
            language: input.language.as_deref(),
        },
    )
    .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    execute_openrouter_json(request)
        .await
        .map(Json)
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("OpenRouter transcription failed: {error}")))
}

async fn create_openrouter_cover(
    State(state): State<AppState>,
    Json(input): Json<OpenRouterCoverRequest>,
) -> Result<Json<OpenRouterResponse>, (StatusCode, Json<ApiError>)> {
    let catalog = catalog_for(&state)
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, error))?;
    let request = providers::openrouter::request_for(
        &catalog,
        Capability::CoverArt,
        &input.model_id,
        &input.prompt,
    )
    .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    execute_openrouter_json(request)
        .await
        .map(Json)
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("OpenRouter cover generation failed: {error}")))
}

/// Text assistance (caption and lyric drafting). The model must declare the
/// prompt-enhancement capability in the refreshed catalog, so the studio can
/// never send this to an image or audio-only endpoint.
async fn create_openrouter_completion(
    State(state): State<AppState>,
    Json(input): Json<OpenRouterCompletionRequest>,
) -> Result<Json<OpenRouterResponse>, (StatusCode, Json<ApiError>)> {
    let catalog = catalog_for(&state)
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, error))?;
    let request = providers::openrouter::request_for(
        &catalog,
        Capability::PromptEnhancement,
        &input.model_id,
        &input.prompt,
    )
    .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    execute_openrouter_json(request)
        .await
        .map(Json)
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("OpenRouter completion failed: {error}")))
}

/// The loopback port the studio serves on. Overridable for development, so a
/// second instance can run beside a released one.
fn listen_port() -> u16 {
    env::var("YUE_STUDIO_PORT").ok().and_then(|value| value.parse().ok()).unwrap_or(8791)
}

fn chrono_like_timestamp() -> String { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|value| value.as_secs().to_string()).unwrap_or_default() }

fn studio_settings_path() -> PathBuf {
    env::var_os("YUE_STUDIO_SETTINGS_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(default_studio_settings_path)
}

fn default_studio_settings_path() -> PathBuf {
    studio_data_root()
        .unwrap_or_else(|| env::temp_dir().join("yue2-studio"))
        .join("studio-settings.json")
}

/// Single per-user directory for every piece of Studio runtime data: settings,
/// library, media and locally stored provider credentials.
static STUDIO_VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// The studio's own version, set by the desktop shell before the service
/// starts; a service run on its own is a development build of its crate.
pub fn set_studio_version(version: String) {
    let _ = STUDIO_VERSION.set(version);
}

pub(crate) fn studio_version() -> &'static str {
    STUDIO_VERSION.get().map(String::as_str).unwrap_or(env!("CARGO_PKG_VERSION"))
}

pub fn studio_data_root() -> Option<PathBuf> {
    if let Some(root) = env::var_os("YUE_STUDIO_DATA_ROOT") {
        return Some(PathBuf::from(root));
    }

    #[cfg(windows)]
    {
        if let Some(root) = env::var_os("LOCALAPPDATA").or_else(|| env::var_os("APPDATA")) {
            return Some(PathBuf::from(root).join("YuE2 Studio"));
        }
    }

    #[cfg(not(windows))]
    {
        if let Some(root) = env::var_os("XDG_DATA_HOME") {
            return Some(PathBuf::from(root).join("yue2-studio"));
        }
        if let Some(home) = env::var_os("HOME") {
            return Some(PathBuf::from(home).join(".local/share/yue2-studio"));
        }
    }

    None
}

/// Points every CUDA process of the studio - the engine, the trainer, the
/// assistant, the captioner, and ONNX Runtime inside the service - at the card
/// chosen in the engine settings, numbered as nvidia-smi numbers them. Called
/// by the desktop shell and the standalone service before anything starts.
pub fn apply_saved_gpu() {
    let Some(index) = load_studio_settings(&studio_settings_path()).and_then(|settings| settings.engine_options.gpu) else { return };
    // SAFETY: called first thing in the process, before the service and its
    // runtimes read the environment; on Windows the variables are set through
    // SetEnvironmentVariableW, which is thread safe.
    unsafe {
        env::set_var("CUDA_DEVICE_ORDER", "PCI_BUS_ID");
        env::set_var("CUDA_VISIBLE_DEVICES", index.to_string());
    }
}

/// The NVIDIA cards nvidia-smi lists, and which one the studio runs on.
async fn system_gpus(State(state): State<AppState>) -> Json<Value> {
    let cards = tokio::task::spawn_blocking(hardware::nvidia_cards).await.unwrap_or_default();
    let chosen = state.engine_options.read().await.gpu;
    let running = env::var("CUDA_VISIBLE_DEVICES").ok().and_then(|value| value.trim().parse::<u32>().ok());
    Json(serde_json::json!({
        "cards": cards.iter().map(|(index, name, gigabytes)| serde_json::json!({ "index": index, "name": name, "memory_gb": gigabytes })).collect::<Vec<_>>(),
        "chosen": chosen,
        "running": running,
    }))
}

/// The saved proxy, for the desktop shell to hand the window's browser before
/// the service is up.
pub fn saved_proxy() -> net::ProxySettings {
    load_studio_settings(&studio_settings_path()).and_then(|settings| settings.proxy).unwrap_or_default()
}

fn load_studio_settings(path: &PathBuf) -> Option<PersistedStudioSettings> {
    fs::read_to_string(path).ok().and_then(|body| serde_json::from_str(&body).ok())
}

async fn persist_studio_settings(state: &AppState) -> anyhow::Result<()> {
    let settings = PersistedStudioSettings {
        engine_options: *state.engine_options.read().await,
        assistant: state.assistant.read().await.clone(),
        lyrics_sync: state.lyrics_sync_config.read().await.clone(),
        configuration: state.configuration.read().await.clone(),
        selected_profile_id: state.selected_profile_id.read().await.clone(),
        selected_component_ids: state.selected_component_ids.read().await.clone(),
        cover_templates: Some(state.cover_templates.read().await.clone()),
        cover_template_default: state.cover_template_default.read().await.clone(),
        separation: Some(state.separation_config.read().await.clone()),
        cover_auto: Some(*state.cover_auto.read().await),
        cover_look: Some(state.cover_look.read().await.clone()),
        proxy: Some(net::current()),
        network: Some(remote::current()),
    };
    if let Some(parent) = state.settings_path.parent() { fs::create_dir_all(parent)?; }
    let temporary = state.settings_path.with_extension("json.part");
    fs::write(&temporary, serde_json::to_vec_pretty(&settings)?)?;
    fs::rename(temporary, &state.settings_path)?;
    mcp::announce("settings");
    Ok(())
}

/// The set Studio will actually load. `None` means nothing has been selected
/// yet, so the manager falls back to the hardware recommendation for progress
/// reporting only — it still never downloads anything on its own.
async fn effective_install_target(state: &AppState) -> Option<InstallRequest> {
    if let Some(component_ids) = state.selected_component_ids.read().await.clone() {
        return Some(InstallRequest { profile_id: None, component_ids });
    }
    state
        .selected_profile_id
        .read()
        .await
        .clone()
        .map(|profile_id| InstallRequest { profile_id: Some(profile_id), component_ids: vec![] })
}

async fn compose_setup_status(state: &AppState, manager_status: model_manager::ManagerStatus) -> Value {
    let mut status = serde_json::to_value(manager_status).unwrap_or_else(|_| serde_json::json!({}));
    let selected_profile_id = state.selected_profile_id.read().await.clone();
    let selected_component_ids = state.selected_component_ids.read().await.clone();
    let selected_set_ready = match (&selected_profile_id, &selected_component_ids) {
        (_, Some(component_ids)) => state.model_manager.installed_component_files(component_ids).is_ok(),
        (Some(profile_id), None) => state.model_manager.installed_profile_files(profile_id).is_ok(),
        (None, None) => false,
    };
    if let Value::Object(ref mut fields) = status {
        fields.insert(
            "engine_ready".into(),
            Value::Bool(state.music_server.health().await),
        );
        fields.insert("engine_error".into(), serde_json::to_value(state.engine_start_error.read().await.clone()).unwrap_or(Value::Null));
        fields.insert("engine_id".into(), Value::String(PRIMARY_MUSIC_ENGINE_ID.into()));
        fields.insert("selected_profile_id".into(), serde_json::to_value(selected_profile_id).unwrap_or(Value::Null));
        fields.insert("selected_component_ids".into(), serde_json::to_value(selected_component_ids).unwrap_or(Value::Null));
        fields.insert("hardware".into(), serde_json::to_value(hardware::hardware()).unwrap_or(Value::Null));
        fields.insert("engine_options".into(), serde_json::to_value(*state.engine_options.read().await).unwrap_or(Value::Null));
        fields.insert("effective_max_batch".into(), Value::from(state.engine_options.read().await.effective_max_batch()));
        // Where everything the studio owns actually lives. People complained
        // they could not find the ten gigabytes afterwards, let alone delete
        // them; the model root is already reported, this is the folder that
        // holds it along with the library, the media and the logs.
        fields.insert(
            "data_directory".into(),
            studio_data_root().map(|root| Value::String(root.display().to_string())).unwrap_or(Value::Null),
        );
        fields.insert("portable".into(), Value::Bool(is_portable_installation()));
        // Half a gigabyte of CUDA libraries arriving is the difference between
        // an engine that starts in three seconds and one that starts in ten
        // minutes. A spinner that says nothing for ten minutes is the same
        // screen as a spinner that is stuck.
        let runtime_cuda = state.engine_options.read().await.cuda_build();
        let runtime_total = runtime_cuda.map(|build| engine_runtime::cublas_asset(build).bytes).unwrap_or(0);
        let runtime_active = state.engine_runtime.downloader().active().await;
        fields.insert(
            "engine_runtime".into(),
            serde_json::json!({
                "ready": state.engine_runtime.is_ready(runtime_cuda),
                "downloading": runtime_active.is_some(),
                "downloaded_bytes": runtime_active.as_ref().map(|progress| progress.downloaded_bytes).unwrap_or(0),
                "total_bytes": runtime_total,
                "error": runtime_active.and_then(|progress| progress.error),
            }),
        );
        fields.insert("ready".into(), Value::Bool(selected_set_ready));
        fields.insert("first_run".into(), Value::Bool(!selected_set_ready));
        if selected_set_ready { fields.insert("download_pending".into(), Value::from(0_u64)); }
    }
    status
}

/// Whether this copy keeps everything beside its own executable.
///
/// The desktop shell decides it by the marker file next to the binary and then
/// hands the service the data root; the service reports it so the interface can
/// say "this folder is the whole studio" rather than sending people hunting
/// through AppData.
fn is_portable_installation() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|directory| directory.join("portable.flag")))
        .is_some_and(|marker| marker.is_file())
}

/// Recent native engine output: `/job` reports a phase, everything finer lives
/// in the log. While the engine is starting it has no HTTP log yet, so the file
/// it writes from its first line is what gets shown.
async fn engine_logs(State(state): State<AppState>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let (lines, source) = match state.engine_log.lines() {
        Some(lines) => (lines, "engine"),
        None => (music_engine::yue_server::startup_log_tail(120), "startup"),
    };
    if lines.is_empty() {
        return Err(api_error(StatusCode::SERVICE_UNAVAILABLE, "the engine log is empty: the engine has not started yet".into()));
    }
    let progress = progress::from_log(&lines);
    Ok(Json(serde_json::json!({ "engine_id": PRIMARY_MUSIC_ENGINE_ID, "lines": lines, "source": source, "progress": progress })))
}

/// The running job's progress as the engine log tells it, pushed to the window
/// while songs are being made.
async fn engine_progress(State(state): State<AppState>) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    Sse::new(state.engine_log.progress_events()).keep_alive(KeepAlive::default())
}

/// Live machine resources. ACE Studio's resource readout is kept, but every
/// value now comes from a real measurement on this machine.
async fn system_resources() -> Json<Value> {
    let snapshot = tokio::task::spawn_blocking(resources::snapshot)
        .await
        .unwrap_or_else(|_| resources::snapshot());
    Json(serde_json::json!({
        "poll_interval_ms": resources::SUGGESTED_INTERVAL.as_millis() as u64,
        "resources": snapshot,
    }))
}

/// Fetches a remote image on behalf of the video composer.
///
/// The canvas has to stay untainted to read frames back, which a cross-origin
/// image without CORS headers prevents. Only http(s) is accepted and the
/// response must actually be an image, so this cannot be used to reach local
/// services or to pull arbitrary files.
async fn proxy_image(
    axum::extract::Query(request): axum::extract::Query<ProxyImageRequest>,
) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let url = reqwest::Url::parse(&request.url)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, format!("invalid image url: {error}")))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(api_error(StatusCode::BAD_REQUEST, "only http and https images can be proxied".into()));
    }
    if url.host_str().is_some_and(|host| host == "localhost" || host.starts_with("127.") || host == "0.0.0.0" || host == "[::1]") {
        return Err(api_error(StatusCode::BAD_REQUEST, "loopback addresses cannot be proxied".into()));
    }
    // Commons asks every client to name itself and a way to reach its authors
    let commons = url.host_str().is_some_and(|host| host.ends_with(".wikimedia.org"));
    let mut request = net::client().get(url);
    if commons {
        request = request.header(reqwest::header::USER_AGENT, cover_art::commons_agent());
    }
    let response = request
        .send()
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("image request failed: {error}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(api_error(StatusCode::BAD_GATEWAY, format!("image request returned {status}")));
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    if !content_type.starts_with("image/") {
        return Err(api_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "the proxied url is not an image".into()));
    }
    const LARGEST: usize = 32 * 1024 * 1024;
    let too_large = || api_error(StatusCode::PAYLOAD_TOO_LARGE, format!("the image is larger than {} MB", LARGEST / (1024 * 1024)));
    if response.content_length().is_some_and(|length| length > LARGEST as u64) {
        return Err(too_large());
    }
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("reading the image failed: {error}")))?
    {
        if bytes.len() + chunk.len() > LARGEST {
            return Err(too_large());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(axum::response::Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, bytes.len())
        .body(Body::from(bytes))
        .expect("valid image response"))
}

async fn assistant_status(State(state): State<AppState>) -> Json<Value> {
    let config = state.assistant.read().await.clone();
    let runtime = state.assistant_runtime.status().await;
    // A managed model is only usable once its file and the runtime are on disk.
    let available = match config.provider {
        AssistantProvider::Managed => {
            let has_runtime = runtime.server_path.is_some();
            let downloaded = config
                .managed_model
                .as_deref()
                .is_some_and(|model| runtime.installed_models.iter().any(|id| id == model));
            let own_file = config
                .managed_path
                .as_deref()
                .is_some_and(|path| !path.trim().is_empty() && std::path::Path::new(path.trim()).is_file());
            has_runtime && (downloaded || own_file)
        }
        _ => config.available(),
    };
    Json(serde_json::json!({
        "available": available,
        "managed_model": config.managed_model,
        "managed_path": config.managed_path,
        "reasoning_effort": config.reasoning_effort,
        "runtime_ready": runtime.ready,
        "provider": config.provider,
        "local_base_url": config.local_base_url,
        "local_model": config.local_model,
        "local_api_key_set": credentials::local_server_key().is_some(),
        "openrouter_model": config.openrouter_model,
    }))
}

/// Every field optional, so the download page can set the provider without
/// blanking the model, the path and the reasoning effort it knows nothing of.
#[derive(Debug, Deserialize)]
struct AssistantSettingsRequest {
    provider: Option<AssistantProvider>,
    local_base_url: Option<Option<String>>,
    local_model: Option<Option<String>>,
    openrouter_model: Option<Option<String>>,
    managed_model: Option<Option<String>>,
    managed_path: Option<Option<String>>,
    reasoning_effort: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
struct LocalModelsQuery {
    base: String,
}

/// The models an OpenAI-compatible server the user runs offers, fetched through
/// the studio so the browser is never asked to reach another origin itself.
///
/// LM Studio, llama-server, Ollama's OpenAI shim - all answer `GET <base>/models`
/// with `{ "data": [ { "id": ... } ] }`. Typing the model name by hand, which
/// is what this replaces, meant a typo read as a server that answered nothing.
async fn assistant_local_models(
    axum::extract::Query(query): axum::extract::Query<LocalModelsQuery>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let base = query.base.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "no server address".into()));
    }
    let url = format!("{base}/models");
    let response = local_server_auth(net::client().get(&url), AssistantProvider::Local)
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("the server at {base} did not answer: {error}")))?;
    if !response.status().is_success() {
        return Err(api_error(StatusCode::BAD_GATEWAY, format!("{url} answered {}", response.status())));
    }
    let body = response
        .json::<Value>()
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("{url} did not return JSON: {error}")))?;
    let models: Vec<String> = body
        .get("data")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(|item| item.get("id").and_then(Value::as_str).map(str::to_string)).collect())
        .unwrap_or_default();
    Ok(Json(serde_json::json!({ "models": models })))
}

/// A request to the user's own server with its key, when one is stored. The
/// studio's own llama-server and the other providers take none of it.
fn local_server_auth(request: reqwest::RequestBuilder, provider: AssistantProvider) -> reqwest::RequestBuilder {
    match (provider, credentials::local_server_key()) {
        (AssistantProvider::Local, Some(key)) => request.bearer_auth(key),
        _ => request,
    }
}

#[derive(Debug, Deserialize)]
struct LocalServerKeyRequest {
    api_key: Option<String>,
}

/// Stores or clears the key of the user's own server; the key itself is never
/// sent back, only whether one is set.
async fn set_local_server_key(Json(request): Json<LocalServerKeyRequest>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let set = credentials::store_local_server_key(request.api_key.as_deref()).map_err(|error| api_error(StatusCode::BAD_REQUEST, format!("{error:#}")))?;
    Ok(Json(serde_json::json!({ "local_api_key_set": set })))
}

/// Which local server runs `model` and with what context, asked of the
/// server's own API: LM Studio's `/api/v0/models`, then Ollama's `/api/ps`.
/// None for any other server, or while the model is not loaded yet.
async fn local_server_context(base: &str, model: &str) -> Option<(assistant::LocalServer, u64)> {
    let root = base.trim().trim_end_matches('/').trim_end_matches("/v1");
    let client = net::client();
    let read = |path: &'static str| {
        let request = local_server_auth(client.get(format!("{root}{path}")), AssistantProvider::Local).timeout(std::time::Duration::from_secs(3));
        async move {
            let response = request.send().await.ok()?;
            if !response.status().is_success() {
                return None;
            }
            response.json::<Value>().await.ok()
        }
    };
    if let Some(context) = read("/api/v0/models").await.and_then(|models| assistant::lm_studio_context(&models, model)) {
        return Some((assistant::LocalServer::LmStudio, context));
    }
    read("/api/ps").await.and_then(|running| assistant::ollama_context(&running, model)).map(|context| (assistant::LocalServer::Ollama, context))
}

async fn update_assistant_settings(
    State(state): State<AppState>,
    Json(incoming): Json<AssistantSettingsRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let request = {
        let current = state.assistant.read().await.clone();
        AssistantConfig {
            provider: incoming.provider.unwrap_or(current.provider),
            local_base_url: incoming.local_base_url.unwrap_or(current.local_base_url),
            local_model: incoming.local_model.unwrap_or(current.local_model),
            openrouter_model: incoming.openrouter_model.unwrap_or(current.openrouter_model),
            managed_model: incoming.managed_model.unwrap_or(current.managed_model),
            managed_path: incoming.managed_path.unwrap_or(current.managed_path),
            reasoning_effort: incoming.reasoning_effort.unwrap_or(current.reasoning_effort),
        }
    };
    if request.provider == AssistantProvider::Local {
        let base = request.local_base_url.as_deref().unwrap_or_default();
        let url = reqwest::Url::parse(base)
            .map_err(|error| api_error(StatusCode::BAD_REQUEST, format!("invalid assistant URL: {error}")))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(api_error(StatusCode::BAD_REQUEST, "the assistant URL must be http or https".into()));
        }
    }
    *state.assistant.write().await = request.clone();
    let _ = persist_studio_settings(&state).await;
    Ok(Json(serde_json::json!({ "available": request.available(), "provider": request.provider })))
}

#[derive(Debug, Deserialize)]
struct AssistantAssetRequest {
    asset_id: String,
    /// The model to install along with the runtime, when the capability has a
    /// choice of them. A runtime with no model does nothing.
    #[serde(default)]
    model_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AssistantModelRequest {
    #[serde(default)]
    model_id: String,
    /// A GGUF already on this machine, used instead of a downloaded one.
    #[serde(default)]
    model_path: Option<String>,
}

/// The runtime, and the choice it belongs to.
///
/// The panel reads this one address to draw itself, and the chosen engine is
/// kept with the assistant's settings rather than with its files - so without
/// it here the tabs came back to the first one on every open, whatever the user
/// had picked.
async fn assistant_runtime_status(State(state): State<AppState>) -> Json<Value> {
    let status = state.assistant_runtime.status().await;
    let provider = state.assistant.read().await.provider;
    let mut value = serde_json::to_value(&status).unwrap_or(Value::Null);
    let chosen = state.assistant.read().await.managed_model.clone();
    if let Value::Object(ref mut fields) = value {
        fields.insert("provider".into(), serde_json::to_value(provider).unwrap_or(Value::Null));
        // And which model, so the dropdown reopens on the one that was picked
        // rather than on whichever happens to be installed first.
        fields.insert("chosen_model".into(), serde_json::to_value(chosen).unwrap_or(Value::Null));
    }
    Json(value)
}

/// Starts one download. Nothing is fetched until this is called, and an
/// interrupted file resumes where it stopped.
/// The llama.cpp build for a device, with the CUDA libraries it needs.
///
/// The card build is useless without its runtime companion - two downloads
/// that are one decision, the same way a recogniser is.
fn assistant_set(device: &str) -> Vec<&'static str> {
    match (device, assistant_runtime::card_flavour()) {
        ("cpu", _) | (_, None) => vec!["llama-cpu"],
        (_, Some("cuda12")) => vec!["llama-cuda12", "llama-cuda12-runtime"],
        (_, Some("vulkan")) => vec!["llama-vulkan"],
        _ => vec!["llama-cuda", "llama-cuda-runtime"],
    }
}

async fn assistant_runtime_install(
    State(state): State<AppState>,
    Json(request): Json<AssistantAssetRequest>,
) -> Result<Json<assistant_runtime::RuntimeStatus>, (StatusCode, Json<ApiError>)> {
    let set = match request.asset_id.as_str() {
        "auto" | "cuda" | "cpu" => assistant_set(&request.asset_id),
        _ => Vec::new(),
    };
    if !set.is_empty() {
        // Downloading a model is choosing it. Nothing else recorded which one,
        // so a freshly installed Gemma left the studio reporting that no
        // assistant was set up - with the model sitting on the disk.
        if let Some(model) = request.model_id.clone() {
            let mut assistant = state.assistant.write().await;
            assistant.managed_model = Some(model);
            if assistant.provider == AssistantProvider::None {
                assistant.provider = AssistantProvider::Managed;
            }
        }
        let _ = persist_studio_settings(&state).await;
        let runtime = state.assistant_runtime.clone();
        // The whole thing - runtime, CUDA libraries, model - as one download.
        // Starting them one after another only looked like a queue: each call
        // returned before its file had arrived, so the next one was refused and
        // the model, always last, was never fetched at all.
        let ids: Vec<String> = set
            .into_iter()
            .map(str::to_string)
            .chain(request.model_id.clone())
            .collect();
        tokio::spawn(async move {
            if let Err(error) = runtime.install_all(&ids).await {
                eprintln!("the assistant could not be installed: {error}");
            }
        });
        return Ok(Json(state.assistant_runtime.status().await));
    }
    state
        .assistant_runtime
        .install(&request.asset_id)
        .await
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(state.assistant_runtime.status().await))
}

/// How much room the local model gets.
///
/// A score edit sends the whole ABC score and gets the whole revision back,
/// two or three thousand tokens each way; a model that runs out mid-JSON
/// produces an answer nothing can parse.
const ASSISTANT_CONTEXT: u32 = 16384;

async fn assistant_runtime_start(
    State(state): State<AppState>,
    Json(request): Json<AssistantModelRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let own_file = request.model_path.clone().unwrap_or_default();
    let reasoning = state.assistant.read().await.reasoning_effort.clone();
    let base_url = if own_file.trim().is_empty() {
        state.assistant_runtime.start(&request.model_id, ASSISTANT_CONTEXT, reasoning.as_deref()).await
    } else {
        state.assistant_runtime.start_path(std::path::Path::new(own_file.trim()), ASSISTANT_CONTEXT, reasoning.as_deref()).await
    }
    .map_err(|error| api_error(StatusCode::BAD_GATEWAY, error.to_string()))?;
    Ok(Json(serde_json::json!({ "base_url": base_url, "model_id": request.model_id, "model_path": own_file })))
}

async fn assistant_runtime_stop(State(state): State<AppState>) -> Json<Value> {
    state.assistant_runtime.stop().await;
    Json(serde_json::json!({ "running": false }))
}

#[derive(Debug, Deserialize)]
struct KaraokeRequest {
    /// Overrides the language guess for this one track.
    #[serde(default)]
    language: Option<String>,
}

/// What the chosen engine would install: its files, their weight, and how much
/// of it is already here.
fn set_progress(downloader: &crate::downloads::Downloader, set: &[&'static lyrics_sync::Asset]) -> Value {
    let total: u64 = set.iter().map(|asset| asset.bytes).sum();
    let installed: u64 = set.iter().filter(|asset| downloader.is_installed(asset)).map(|asset| asset.bytes).sum();
    serde_json::json!({
        "bytes": total,
        "installed_bytes": installed,
        "ready": !set.is_empty() && installed == total,
        "files": set.len(),
    })
}

async fn karaoke_status(State(state): State<AppState>) -> Json<Value> {
    let config = state.lyrics_sync_config.read().await.clone();
    let status = state.lyrics_sync.status(&config).await;
    let name = match config.provider {
        lyrics_sync::AsrProvider::Whisper => "whisper",
        _ => "parakeet",
    };
    let set = karaoke_set(name, config.runtime, config.whisper_model.as_deref());
    let mut value = serde_json::to_value(&status).unwrap_or(Value::Null);
    if let Value::Object(ref mut fields) = value {
        fields.insert("set".into(), set_progress(state.lyrics_sync.downloader(), &set));
    }
    Json(value)
}

/// Every field optional, so a panel that changes one thing changes one thing.
///
/// This took the whole configuration before: the download page, which knows
/// only which recogniser and which device were picked, would have blanked the
/// switch and both model choices by sending them absent.
#[derive(Debug, Deserialize)]
struct KaraokeSettingsRequest {
    enabled: Option<bool>,
    provider: Option<lyrics_sync::AsrProvider>,
    whisper_model: Option<Option<String>>,
    openrouter_model: Option<Option<String>>,
    runtime: Option<lyrics_sync::OnnxFlavour>,
}

async fn update_karaoke_settings(
    State(state): State<AppState>,
    Json(request): Json<KaraokeSettingsRequest>,
) -> Result<Json<lyrics_sync::SyncStatus>, (StatusCode, Json<ApiError>)> {
    let merged = {
        let mut config = state.lyrics_sync_config.write().await;
        if let Some(enabled) = request.enabled { config.enabled = enabled; }
        if let Some(provider) = request.provider { config.provider = provider; }
        if let Some(model) = request.whisper_model { config.whisper_model = model; }
        if let Some(model) = request.openrouter_model { config.openrouter_model = model; }
        if let Some(runtime) = request.runtime { config.runtime = runtime; }
        config.clone()
    };
    let _ = persist_studio_settings(&state).await;
    Ok(Json(state.lyrics_sync.status(&merged).await))
}

/// Frees the disk a karaoke recogniser takes.
/// Removes a recogniser the same way it was installed: whole.
async fn karaoke_remove(
    State(state): State<AppState>,
    Json(request): Json<AssistantAssetRequest>,
) -> Result<Json<lyrics_sync::SyncStatus>, (StatusCode, Json<ApiError>)> {
    let config = state.lyrics_sync_config.read().await.clone();
    let set = karaoke_set(&request.asset_id, config.runtime, config.whisper_model.as_deref());
    if !set.is_empty() {
        for asset in set {
            let _ = state.lyrics_sync.downloader().remove(asset);
        }
        return Ok(Json(state.lyrics_sync.status(&config).await));
    }
    let asset = lyrics_sync::asset(&request.asset_id)
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, format!("unknown karaoke asset: {}", request.asset_id)))?;
    state
        .lyrics_sync
        .downloader()
        .remove(asset)
        .map_err(|error| api_error(StatusCode::CONFLICT, error.to_string()))?;
    Ok(Json(state.lyrics_sync.status(&config).await))
}

/// Frees the disk the stem separation model takes.
async fn remove_separation_model(State(state): State<AppState>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let freed = state
        .separator
        .downloader()
        .remove(&separation::MODEL)
        .map_err(|error| api_error(StatusCode::CONFLICT, error.to_string()))?;
    Ok(Json(serde_json::json!({ "freed_bytes": freed })))
}

/// Installs a recogniser, not a file.
///
/// Parakeet is six downloads and Whisper is two, and which two depends on the
/// card. Asking a person to work that out from a list of file names - and to
/// notice that one of them is a runtime - is not a setup screen, it is a quiz.
/// The whole set is named here, in the order it is used.
fn karaoke_set(name: &str, device: lyrics_sync::OnnxFlavour, whisper_model: Option<&str>) -> Vec<&'static lyrics_sync::Asset> {
    let mut wanted: Vec<String> = Vec::new();
    match name {
        "parakeet" => {
            wanted.push("onnxruntime".into());
            wanted.extend(card_assets(device).iter().map(|id| id.to_string()));
            // The precision is chosen the same way a Whisper model is: through
            // the dropdown, which names one of the encoders.
            wanted.extend(lyrics_sync::parakeet_variant(whisper_model).0.iter().map(|id| id.to_string()));
        }
        "whisper" => {
            // One binary whichever device is chosen; the card needs CUDA 11's
            // libraries beside it, and without them CTranslate2 silently uses
            // the processor instead of saying so.
            wanted.push("whisper-engine".into());
            if device.uses_cuda() {
                wanted.push("whisper-cublas".into());
                wanted.push("whisper-cudnn".into());
            }
            // A model is a directory of files, and it is useless one file
            // short, so the whole set goes together.
            let chosen = whisper_model.unwrap_or("whisper-large-v3-turbo");
            if let Some(size) = chosen.strip_prefix("whisper-") {
                let prefix = format!("models/whisper/faster-whisper-{size}/");
                wanted.extend(
                    lyrics_sync::ASSETS
                        .iter()
                        .filter(|asset| asset.relative_path.starts_with(&prefix))
                        .map(|asset| asset.id.to_string()),
                );
            }
        }
        _ => {}
    }
    wanted.iter().filter_map(|id| lyrics_sync::asset(id)).collect()
}

async fn karaoke_install(
    State(state): State<AppState>,
    Json(request): Json<AssistantAssetRequest>,
) -> Result<Json<lyrics_sync::SyncStatus>, (StatusCode, Json<ApiError>)> {
    let config = state.lyrics_sync_config.read().await.clone();
    let set = karaoke_set(
        &request.asset_id,
        config.runtime,
        request.model_id.as_deref().or(config.whisper_model.as_deref()),
    );
    if !set.is_empty() {
        // In the background, so the panel keeps answering while half a
        // gigabyte arrives; the whole set is one button, not eight.
        let sync = state.lyrics_sync.clone();
        let installed_state = state.clone();
        let recogniser = match request.asset_id.as_str() {
            "parakeet" => Some(lyrics_sync::AsrProvider::Parakeet),
            "whisper" => Some(lyrics_sync::AsrProvider::Whisper),
            _ => None,
        };
        tokio::spawn(async move {
            match sync.downloader().install_all("karaoke", &set).await {
                Err(error) => eprintln!("the karaoke recogniser could not be installed: {error}"),
                // Installing a recogniser is choosing it: the timings button
                // appears once it is on disk, without a second trip to Settings.
                Ok(_) => {
                    if let Some(provider) = recogniser {
                        {
                            let mut config = installed_state.lyrics_sync_config.write().await;
                            if config.provider == lyrics_sync::AsrProvider::None {
                                config.provider = provider;
                            }
                            config.enabled = true;
                        }
                        let _ = persist_studio_settings(&installed_state).await;
                    }
                }
            }
        });
        return Ok(Json(state.lyrics_sync.status(&config).await));
    }

    let asset = lyrics_sync::asset(&request.asset_id)
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, format!("unknown karaoke asset: {}", request.asset_id)))?;
    state
        .lyrics_sync
        .downloader()
        .install(asset)
        .await
        .map_err(|error| api_error(StatusCode::CONFLICT, error.to_string()))?;
    Ok(Json(state.lyrics_sync.status(&config).await))
}

/// The words the chosen recogniser hears in a recording, each with its moment: the clock karaoke
/// and the cover's section matching put the lyrics on.
async fn recognised_words(
    state: &AppState,
    config: &lyrics_sync::LyricsSyncConfig,
    path: std::path::PathBuf,
    language: Option<String>,
    lyrics: String,
) -> Result<Vec<(f64, String)>, (StatusCode, Json<ApiError>)> {
    match config.provider {
        lyrics_sync::AsrProvider::None => Err(api_error(StatusCode::CONFLICT, "karaoke.no-recogniser".into())),
        lyrics_sync::AsrProvider::Parakeet => {
            let sync = state.lyrics_sync.clone();
            let (runtime, variant) = (config.runtime, config.whisper_model.clone());
            tokio::task::spawn_blocking(move || sync.parakeet_words(runtime, variant.as_deref(), &path))
                .await
                .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
                .map_err(|error| api_error(StatusCode::BAD_GATEWAY, error.to_string()))
        }
        lyrics_sync::AsrProvider::Whisper => {
            let sync = state.lyrics_sync.clone();
            let config = config.clone();
            tokio::task::spawn_blocking(move || sync.whisper_words(&config, &path, language.as_deref(), &lyrics))
                .await
                .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
                .map_err(|error| api_error(StatusCode::BAD_GATEWAY, error.to_string()))
        }
        lyrics_sync::AsrProvider::OpenRouter => karaoke_words_from_openrouter(state, config, &path.to_string_lossy(), language.as_deref())
            .await
            .map_err(|error| api_error(StatusCode::BAD_GATEWAY, error.to_string())),
    }
}

/// HOT-Step's "match sections to score" for a cover: the source recording (an uploaded file or a
/// library song) is recognised as karaoke is, and each lyric block is retagged with the score
/// section it is sung in. Nothing is changed here; the window shows the proposal before applying it.
async fn match_score_sections(
    State(state): State<AppState>,
    mut multipart: axum::extract::Multipart,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let bad = |message: String| api_error(StatusCode::BAD_REQUEST, message);
    let (mut abc, mut lyrics, mut language, mut song_id, mut upload) = (String::new(), String::new(), None::<String>, None::<String>, None::<(Vec<u8>, String)>);
    while let Some(field) = multipart.next_field().await.map_err(|error| bad(error.to_string()))? {
        match field.name().unwrap_or_default() {
            "abc" => abc = field.text().await.map_err(|error| bad(error.to_string()))?,
            "lyrics" => lyrics = field.text().await.map_err(|error| bad(error.to_string()))?,
            "language" => language = Some(field.text().await.map_err(|error| bad(error.to_string()))?).filter(|value| !value.trim().is_empty()),
            "song_id" => song_id = Some(field.text().await.map_err(|error| bad(error.to_string()))?.trim().to_string()),
            "audio" => {
                let name = field.file_name().unwrap_or("source.audio").to_string();
                upload = Some((field.bytes().await.map_err(|error| bad(error.to_string()))?.to_vec(), name));
            }
            _ => {}
        }
    }
    if abc.trim().is_empty() {
        return Err(bad("Send the cover's score as abc.".into()));
    }
    if !auto_title::has_sung_lines(&lyrics) {
        return Err(bad("karaoke.instrumental".into()));
    }
    let config = state.lyrics_sync_config.read().await.clone();
    if matches!(config.provider, lyrics_sync::AsrProvider::Whisper | lyrics_sync::AsrProvider::Parakeet) {
        card_free_of_training(&state, "match sections").await?;
    }
    if !config.available() {
        return Err(api_error(StatusCode::CONFLICT, "karaoke.off".into()));
    }
    if !ensure_local_recogniser(&state, &config, song_id.as_deref().unwrap_or_default()).await {
        return Err(api_error(StatusCode::CONFLICT, "karaoke.model-missing".into()));
    }
    let work = tempfile::tempdir().map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let path = match (song_id.as_deref(), upload) {
        (_, Some((bytes, name))) => {
            if bytes.is_empty() {
                return Err(bad("The recording is empty.".into()));
            }
            let extension = std::path::Path::new(&name).extension().and_then(|value| value.to_str()).unwrap_or("audio").to_string();
            let path = work.path().join(format!("source.{extension}"));
            tokio::fs::write(&path, bytes).await.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
            path
        }
        (Some(id), None) => {
            let song = state.library.get_song(id).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
                .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found.".into()))?;
            state.library.media_path_for_song(&song).ok_or_else(|| api_error(StatusCode::NOT_FOUND, "The track's audio is not in the library.".into()))?
        }
        (None, None) => return Err(bad("Send the source recording as audio or a song_id.".into())),
    };
    let words = recognised_words(&state, &config, path, language, lyrics.clone()).await?;
    let lines = lyrics_sync::heard_line_starts(&words, &lyrics);
    let proposal = score::section_match::match_sections(&abc, &lyrics, &lines).map_err(bad)?;
    Ok(Json(serde_json::to_value(proposal).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?))
}

/// Times one track's own lyrics and stores the result with it.
///
/// Recognition is CPU or GPU bound and takes tens of seconds, so it runs on a
/// blocking thread rather than holding an async worker hostage.
async fn create_song_karaoke(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<KaraokeRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let config = state.lyrics_sync_config.read().await.clone();
    if matches!(config.provider, lyrics_sync::AsrProvider::Whisper | lyrics_sync::AsrProvider::Parakeet) {
        card_free_of_training(&state, "make karaoke").await?;
    }
    if !config.available() {
        return Err(api_error(StatusCode::CONFLICT, "karaoke.off".into()));
    }
    // Pressing the button on a track is the instruction to time it. If the
    // chosen local recogniser is not on disk yet, that is a download to start,
    // not a refusal to hand back.
    if !ensure_local_recogniser(&state, &config, &id).await {
        return Err(api_error(StatusCode::CONFLICT, "karaoke.model-missing".into()));
    }
    let song = state
        .library
        .get_song(&id)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "no such song".into()))?;
    let audio = state
        .library
        .media_path_for_song(&song)
        .map(|path| path.to_string_lossy().into_owned())
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "this track has no audio to listen to".into()))?;
    if !auto_title::has_sung_lines(&song.lyrics) {
        return Err(api_error(StatusCode::BAD_REQUEST, "karaoke.instrumental".into()));
    }

    let words = recognised_words(&state, &config, std::path::PathBuf::from(&audio), request.language.clone(), song.lyrics.clone()).await?;

    // Word by word, because that is what karaoke means: a line time alone
    // leaves a player sweeping the highlight linearly through the line, which
    // drifts off the singing immediately.
    let lines = lyrics_sync::align_lyrics_words(&words, &song.lyrics);
    if lines.is_empty() {
        return Err(api_error(StatusCode::BAD_GATEWAY, "karaoke.no-match".into()));
    }
    let lrc = lyrics_sync::enhanced_lrc(&lines);
    state
        .library
        .set_song_lrc(&id, &lrc)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(serde_json::json!({ "lrc": lrc, "lines": lines.len(), "provider": config.provider })))
}

async fn delete_song_karaoke(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    state
        .library
        .set_song_lrc(&id, "")
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(serde_json::json!({ "lrc": Value::Null })))
}

/// The cloud path: base64 the audio, ask for verbose output, read the times.
async fn karaoke_words_from_openrouter(
    state: &AppState,
    config: &lyrics_sync::LyricsSyncConfig,
    audio: &str,
    language: Option<&str>,
) -> anyhow::Result<Vec<(f64, String)>> {
    use base64::Engine as _;
    let catalog = catalog_for(state).await.map_err(|error| anyhow::anyhow!(error))?;
    let model = config
        .openrouter_model
        .clone()
        .filter(|value| !value.trim().is_empty())
        // Only the Whisper family returns timings, and that is what karaoke is.
        .or_else(|| providers::openrouter::suggested_model(&catalog, Capability::SpeechToText))
        .unwrap_or_default();
    let bytes = std::fs::read(audio)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let format = std::path::Path::new(audio)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("mp3")
        .to_ascii_lowercase();
    let request = providers::openrouter::stt_request_for(
        &catalog,
        &model,
        providers::openrouter::Base64AudioInput { timestamps: true, data: &encoded, format: &format, language },
    )?;
    let response = execute_openrouter_json(request).await?;
    let segments = lyrics_sync::segments_from_verbose_json(&response.body);
    if segments.is_empty() {
        anyhow::bail!("{model} answered without timings; pick a model that returns them");
    }
    Ok(segments)
}

/// Writes lyrics and/or the structured caption. Optional by design: with no
/// provider configured this answers 409 and the manual form is unaffected.

/// The same request as `assistant_write`, reported while it happens.
///
/// A model can take a minute, and a button that only spins says nothing about
/// whether the request even left the machine. This sends the stages as they
/// occur - the request going out, the first token coming back - and then the
/// text itself, piece by piece, so the fields fill in front of the user.

/// The OpenRouter model the writing assistant should use.
///
/// Two screens name this: the provider page, where every capability picks its
/// model, and the assistant page, which has a field of its own. They disagreed,
/// and the request went to whichever the code happened to read - so the panel
/// showed one model while another answered. The provider selection wins,
/// because that page is where every other capability is chosen.
async fn assistant_openrouter_model(state: &AppState, config: &AssistantConfig) -> String {
    let selected = state
        .configuration
        .read()
        .await
        .selections
        .iter()
        .find(|selection| selection.capability == Capability::PromptEnhancement)
        .and_then(|selection| selection.cloud_model.clone())
        .filter(|model| !model.trim().is_empty());
    if let Some(model) = selected {
        return model;
    }
    if let Some(model) = config.openrouter_model.clone().filter(|model| !model.trim().is_empty()) {
        return model;
    }
    catalog_for(state)
        .await
        .ok()
        .and_then(|catalog| providers::openrouter::suggested_model(&catalog, Capability::PromptEnhancement))
        .unwrap_or_default()
}

async fn assistant_write_stream(
    State(state): State<AppState>,
    Json(request): Json<assistant::AssistRequest>,
) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let config = state.assistant.read().await.clone();
    if matches!(config.provider, AssistantProvider::Managed) {
        card_free_of_training(&state, "ask the assistant").await?;
    }
    if !config.available() {
        return Err(api_error(
            StatusCode::CONFLICT,
            "No writing assistant is configured. The manual form does not need one.".into(),
        ));
    }
    let (system, required) = assistant::instructions(&request);
    let user = assistant::user_message(&request);
    let task = request.target;
    let target = assist_target_name(task);

    let (sender, receiver) = tokio::sync::mpsc::channel::<Result<axum::body::Bytes, std::io::Error>>(64);
    let emit = |sender: tokio::sync::mpsc::Sender<Result<axum::body::Bytes, std::io::Error>>, event: Value| async move {
        let line = format!("data: {event}\n\n");
        let _ = sender.send(Ok(axum::body::Bytes::from(line))).await;
    };

    tokio::spawn(async move {
        emit(sender.clone(), serde_json::json!({ "stage": "preparing" })).await;

        // The connected agent answers whole: there is nothing to stream.
        if config.provider == AssistantProvider::Agent {
            emit(sender.clone(), serde_json::json!({ "stage": "sent", "model": "agent" })).await;
            match mcp::ask_agent(&system, &user, Some(assistant::draft_schema(&required)), &target).await {
                Ok(text) => {
                    emit(sender.clone(), serde_json::json!({ "stage": "writing" })).await;
                    emit(sender.clone(), serde_json::json!({ "delta": text })).await;
                    match assistant::parse_draft(&text, &required) {
                        Ok(draft) => emit(sender.clone(), serde_json::json!({ "stage": "done", "text": text, "draft": draft })).await,
                        Err(error) => emit(sender.clone(), serde_json::json!({ "error": format!("the agent's answer does not fit the schema: {error}") })).await,
                    }
                }
                Err(error) => emit(sender.clone(), serde_json::json!({ "error": error })).await,
            }
            return;
        }

        // Where the request goes, and with which model.
        let (base, model, key): (String, String, Option<String>) = match config.provider {
            AssistantProvider::OpenRouter => {
                let model = assistant_openrouter_model(&state, &config).await;
                let key = match credentials::openrouter_api_key().map(|(key, _)| key) {
                    Some(key) => key,
                    None => {
                        emit(sender.clone(), serde_json::json!({ "error": "no OpenRouter key is stored" })).await;
                        return;
                    }
                };
                ("https://openrouter.ai/api/v1".to_string(), model, Some(key))
            }
            AssistantProvider::Managed => {
                let own_file = config.managed_path.clone().unwrap_or_default();
                let id = config.managed_model.clone().unwrap_or_default();
                let reasoning = config.reasoning_effort.clone();
                let started = if own_file.trim().is_empty() {
                    state.assistant_runtime.start(&id, ASSISTANT_CONTEXT, reasoning.as_deref()).await
                } else {
                    state.assistant_runtime.start_path(std::path::Path::new(own_file.trim()), ASSISTANT_CONTEXT, reasoning.as_deref()).await
                };
                match started {
                    Ok(base) => (base, if own_file.trim().is_empty() { id } else { "local-model".to_string() }, None),
                    Err(error) => {
                        emit(sender.clone(), serde_json::json!({ "error": format!("the local assistant did not start: {error}") })).await;
                        return;
                    }
                }
            }
            _ => (
                config.local_base_url.clone().unwrap_or_default(),
                config.local_model.clone().unwrap_or_default(),
                credentials::local_server_key(),
            ),
        };

        // A local llama-server enforces the shape while it samples, so the
        // answer cannot come back as prose or as a list where a string belongs.
        let schema = matches!(config.provider, AssistantProvider::Managed | AssistantProvider::Local)
            .then(|| assistant::draft_schema(&required));
        // What the model publishes for itself, exactly as the non-streaming
        // path uses it. Passing nothing here meant every streamed request went
        // out with the studio's own temperature on top of models that had
        // stated their own - a different request from the one the catalogue
        // describes, and the streamed path is the one the window uses.
        let entry = if matches!(config.provider, AssistantProvider::OpenRouter) {
            catalog_describing(&state, &model)
                .await
                .ok()
                .and_then(|catalog| catalog.models.iter().find(|item| item.id == model).cloned())
        } else {
            None
        };
        let published = entry.as_ref().map(|entry| serde_json::to_value(&entry.defaults).unwrap_or(Value::Null));
        // Thinking on the model's own terms: it publishes which efforts it
        // takes, which one it prefers, and whether it can be asked not to
        // think at all. A setting of ours that is not on its list becomes the
        // one it named, because naming an unknown effort is refused outright.
        let effort = match (&entry, config.provider) {
            (Some(entry), AssistantProvider::OpenRouter) => entry
                .reasoning
                .as_ref()
                .and_then(|reasoning| reasoning.effort_for(config.reasoning_effort.as_deref())),
            (_, AssistantProvider::OpenRouter) => None,
            _ => config.reasoning_effort.clone(),
        };
        let fit = if matches!(config.provider, AssistantProvider::Managed | AssistantProvider::Local) {
            assistant::fit_to_local_task
        } else {
            assistant::fit_to_task
        };
        let mut body = fit(
            assistant::chat_body_constrained(&model, &system, &user, effort.as_deref(), published.as_ref(), schema),
            task,
        );
        body["stream"] = Value::Bool(true);
        // The last event then counts the tokens, which is what tells a full
        // context from a model that would not stop.
        body["stream_options"] = serde_json::json!({ "include_usage": true });
        let limit = body.get("max_tokens").and_then(Value::as_u64);

        let prompt_chars = system.chars().count() + user.chars().count();
        if config.provider == AssistantProvider::Local {
            if let Some(refusal) = local_server_context(&base, &model)
                .await
                .and_then(|(server, context)| assistant::context_refusal(server, context, prompt_chars))
            {
                request_log::failed("assistant", &model, &refusal);
                emit(sender.clone(), serde_json::json!({ "error": refusal })).await;
                return;
            }
        }

        emit(sender.clone(), serde_json::json!({ "stage": "sent", "model": model })).await;
        request_log::asked("assistant", &model, prompt_chars);
        let started = std::time::Instant::now();

        let client = net::client();
        let mut outgoing = client
            .post(format!("{}/chat/completions", base.trim_end_matches('/')))
            .json(&body)
            .timeout(std::time::Duration::from_secs(600));
        if let Some(key) = key {
            outgoing = outgoing
                .header(reqwest::header::AUTHORIZATION, format!("Bearer {key}"))
                .header("HTTP-Referer", "https://github.com/timoncool/YuE2-Studio")
                .header("X-Title", "YuE2 Studio");
        }

        // Until the first byte the window can be closed as well: a model
        // still loading or reading a long prompt sends nothing for minutes.
        let sent = tokio::select! {
            _ = sender.closed() => {
                request_log::failed("assistant", &model, "stopped by the user");
                release_assistant_unless_kept(&state).await;
                return;
            }
            sent = outgoing.send() => sent,
        };
        let response = match sent {
            Ok(response) => response,
            Err(error) => {
                request_log::failed("assistant", &model, &error.to_string());
                emit(sender.clone(), serde_json::json!({ "error": format!("the assistant is unreachable: {error}") })).await;
                return;
            }
        };
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            request_log::answered("assistant", &model, status.as_u16(), started.elapsed().as_secs_f64(), text.chars().count());
            request_log::unusable("assistant", &model, &format!("http {status}"), &text);
            emit(sender.clone(), serde_json::json!({ "error": format!("the assistant returned {status}: {text}") })).await;
            return;
        }

        // Server-sent events, one JSON object per `data:` line, with the text in
        // `choices[0].delta.content`.
        let mut stream = response.bytes_stream();
        let mut buffer: Vec<u8> = Vec::new();
        let mut first = true;
        let mut whole = String::new();
        let mut cut_short = false;
        let mut usage: Option<(u64, u64)> = None;
        let mut window_gone = false;
        loop {
            // The window stopped the run or was closed. Dropping the stream
            // closes the connection, which is what makes the provider stop
            // generating; reading on keeps the model busy for nobody.
            let chunk = tokio::select! {
                _ = sender.closed() => {
                    window_gone = true;
                    break;
                }
                chunk = stream.next() => chunk,
            };
            let Some(Ok(chunk)) = chunk else { break };
            // Bytes until a line is whole: a Cyrillic letter split between two
            // chunks, decoded chunk by chunk, came out as two replacement marks.
            buffer.extend_from_slice(&chunk);
            while let Some(line_end) = buffer.iter().position(|byte| *byte == b'\n') {
                let line = String::from_utf8_lossy(&buffer[..line_end]).trim().to_string();
                buffer.drain(..=line_end);
                let Some(payload) = line.strip_prefix("data:") else { continue };
                let payload = payload.trim();
                if payload == "[DONE]" {
                    continue;
                }
                let Ok(event): Result<Value, _> = serde_json::from_str(payload) else { continue };
                if let Some(counted) = event.get("usage").filter(|counted| !counted.is_null()) {
                    let count = |key: &str| counted.get(key).and_then(Value::as_u64);
                    if let (Some(prompt), Some(answer)) = (count("prompt_tokens"), count("completion_tokens")) {
                        usage = Some((prompt, answer));
                    }
                }
                let choice = event.get("choices").and_then(|choices| choices.get(0));
                if choice.and_then(|choice| choice.get("finish_reason")).and_then(Value::as_str) == Some("length") {
                    cut_short = true;
                }
                let delta = choice
                    .and_then(|choice| choice.get("delta"))
                    .and_then(|delta| delta.get("content"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if delta.is_empty() {
                    continue;
                }
                if first {
                    first = false;
                    // The moment the model started answering. Without it a log
                    // of a run that never came back cannot say whether it was
                    // thinking or simply gone.
                    request_log::answered("assistant first token", &model, 200, started.elapsed().as_secs_f64(), 0);
                    emit(sender.clone(), serde_json::json!({ "stage": "writing" })).await;
                }
                whole.push_str(delta);
                emit(sender.clone(), serde_json::json!({ "delta": delta })).await;
            }
        }
        drop(stream);

        request_log::answered("assistant", &model, 200, started.elapsed().as_secs_f64(), whole.chars().count());
        if window_gone {
            request_log::failed("assistant", &model, "stopped by the user");
            release_assistant_unless_kept(&state).await;
            return;
        }
        if let Some(message) = usage.and_then(|(prompt, _)| assistant::instructions_cut(prompt_chars, prompt)) {
            request_log::unusable("assistant", &model, &message, &whole);
            emit(sender.clone(), serde_json::json!({ "error": message })).await;
            release_assistant_unless_kept(&state).await;
            return;
        }
        if cut_short {
            let message = assistant::cut_short_message(limit, usage);
            request_log::unusable("assistant", &model, &message, &whole);
            emit(sender.clone(), serde_json::json!({ "error": message })).await;
            release_assistant_unless_kept(&state).await;
            return;
        }
        // The answer is kept whenever it cannot be turned into a draft. That is
        // the case this log exists for: the window shows one red line, and
        // without this the text behind it is gone the moment it is closed.
        // The draft the window shows is the one it gets: the finished fields
        // come at the end of this stream, not from asking the model again.
        match assistant::parse_draft(&whole, &required) {
            Ok(draft) => emit(sender.clone(), serde_json::json!({ "stage": "done", "text": whole, "draft": draft })).await,
            Err(error) => {
                request_log::unusable("assistant", &model, &error.to_string(), &whole);
                emit(sender.clone(), serde_json::json!({ "error": error.to_string() })).await;
            }
        }
        // The card belongs to whatever runs next unless the user asked for
        // everything to stay resident.
        release_assistant_unless_kept(&state).await;
    });

    // A channel of chunks becomes the response body; the receiver is turned into
    // a stream by hand to avoid another dependency for four lines.
    let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|item| (item, receiver))
    });
    let body = Body::from_stream(stream);
    Ok(axum::response::Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(body)
        .expect("valid stream response"))
}

async fn assistant_write(
    State(state): State<AppState>,
    Json(request): Json<assistant::AssistRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let draft = assistant_draft(&state, &request).await;
    release_assistant_unless_kept(&state).await;
    draft.map(Json)
}

#[derive(Deserialize)]
struct LyricsSectionsRequest {
    lyrics: String,
}

/// Lyrics laid out in tagged sections with their words untouched, the layout
/// a dataset's published lyric sheets get. Tags already there are replaced.
async fn assistant_sections(
    State(state): State<AppState>,
    Json(request): Json<LyricsSectionsRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let lines = assistant::without_section_tags(&request.lyrics);
    if lines.trim().is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "there are no lyrics to lay out".into()));
    }
    let laid_out = prepare::lay_out_lyrics(&state, &lines, assistant::AssistTarget::Sheet).await;
    release_assistant_unless_kept(&state).await;
    laid_out.map(|lyrics| Json(serde_json::json!({ "lyrics": lyrics }))).map_err(|error| api_error(StatusCode::BAD_GATEWAY, error))
}

/// A draft from the assistant, which stays loaded after it: a run of songs
/// asks it many times and releases it once at the end.
async fn assistant_draft(state: &AppState, request: &assistant::AssistRequest) -> Result<Value, (StatusCode, Json<ApiError>)> {
    let (system, required) = assistant::instructions(request);
    let user = assistant::user_message(request);
    let content = assistant_ask(state, &system, &user, Some(assistant::draft_schema(&required)), request.target).await?;
    let draft = assistant::parse_draft(&content, required).map_err(|error| {
        // The answer, kept: this is the difference between "invalid JSON" and
        // seeing that the model wrote an apology instead of a song.
        request_log::unusable("assistant", "", &error.to_string(), &content);
        api_error(StatusCode::BAD_GATEWAY, error.to_string())
    })?;
    Ok(serde_json::to_value(draft).unwrap_or(Value::Null))
}

/// What the assistant is asked for, by the name the API uses.
fn assist_target_name(target: assistant::AssistTarget) -> String {
    serde_json::to_value(target).ok().and_then(|value| value.as_str().map(str::to_string)).unwrap_or_default()
}

/// One chat completion from the configured writing assistant: the system and
/// user messages, the answer's text. `schema` enforces the answer's shape where
/// the provider can; `target` sizes the answer. The assistant stays loaded:
/// the caller releases it when its questions are done.
async fn assistant_ask(
    state: &AppState,
    system: &str,
    user: &str,
    schema: Option<Value>,
    target: assistant::AssistTarget,
) -> Result<String, (StatusCode, Json<ApiError>)> {
    let state = state.clone();
    let config = state.assistant.read().await.clone();
    if matches!(config.provider, AssistantProvider::Managed) {
        card_free_of_training(&state, "ask the assistant").await?;
    }
    if !config.available() {
        return Err(api_error(
            StatusCode::CONFLICT,
            "No writing assistant is configured. The manual form does not need one.".into(),
        ));
    }

    if config.provider == AssistantProvider::Agent {
        return mcp::ask_agent(system, user, schema, &assist_target_name(target)).await.map_err(|error| api_error(StatusCode::BAD_GATEWAY, error));
    }

    let response: Value = match config.provider {
        AssistantProvider::Local | AssistantProvider::Managed => {
            // A managed model is started on first use and then stays loaded, so
            // the second request does not pay for the load again.
            let (base, model) = match config.provider {
                AssistantProvider::Managed => {
                    let own_file = config.managed_path.clone().unwrap_or_default();
                    let id = config.managed_model.clone().unwrap_or_default();
                    let reasoning = config.reasoning_effort.as_deref();
                    let base = if own_file.trim().is_empty() {
                        state.assistant_runtime.start(&id, 8192, reasoning).await
                    } else {
                        state.assistant_runtime.start_path(std::path::Path::new(own_file.trim()), 8192, reasoning).await
                    }
                    .map_err(|error| api_error(StatusCode::BAD_GATEWAY, error.to_string()))?;
                    (base, if own_file.trim().is_empty() { id } else { own_file })
                }
                _ => (
                    config.local_base_url.clone().unwrap_or_default(),
                    config.local_model.clone().unwrap_or_default(),
                ),
            };
            let sent = local_server_auth(net::client().post(format!("{}/chat/completions", base.trim_end_matches('/'))), config.provider)
                .json(&assistant::fit_to_local_task(assistant::chat_body_constrained(
                    &model,
                    system,
                    user,
                    None,
                    None,
                    schema.filter(|_| matches!(config.provider, AssistantProvider::Managed | AssistantProvider::Local)),
                ), target))
                .timeout(std::time::Duration::from_secs(180))
                .send()
                .await
                .map_err(|error| {
                    // A sidecar that died mid-request leaves nothing but a
                    // refused connection unless its own log is quoted back.
                    let tail = state.assistant_runtime.log_tail();
                    let detail = if tail.is_empty() { String::new() } else { format!("
{tail}") };
                    api_error(StatusCode::BAD_GATEWAY, format!("the local assistant is unreachable: {error}{detail}"))
                })?;
            let status = sent.status();
            let body = sent.text().await.unwrap_or_default();
            if !status.is_success() {
                return Err(api_error(StatusCode::BAD_GATEWAY, format!("the local assistant returned {status}: {body}")));
            }
            serde_json::from_str(&body)
                .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("invalid assistant response: {error}")))?
        }
        AssistantProvider::OpenRouter => {
            let catalog_now = catalog_for(&state).await.ok();
            let model = assistant_openrouter_model(&state, &config).await;
            let catalog = catalog_for(&state)
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, error))?;
            catalog
                .selected(Capability::PromptEnhancement, &model)
                .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
            let authenticated = providers::openrouter::authenticated_request_for(providers::openrouter::OpenRouterRequest {
                method: providers::openrouter::HttpMethod::Post,
                path: providers::openrouter::CHAT_COMPLETIONS_PATH,
                // Whatever this model publishes for itself; the studio's own
                // temperature is only for models that publish nothing.
                body: {
                    let entry = catalog_now.as_ref().and_then(|catalog| catalog.models.iter().find(|entry| entry.id == model));
                    let effort = entry
                        .and_then(|entry| entry.reasoning.as_ref())
                        .and_then(|reasoning| reasoning.effort_for(config.reasoning_effort.as_deref()));
                    assistant::fit_to_task(
                        assistant::chat_body_full(
                            &model,
                            system,
                            user,
                            effort.as_deref(),
                            entry.map(|entry| serde_json::to_value(&entry.defaults).unwrap_or(Value::Null)).as_ref(),
                        ),
                        target,
                    )
                },
            })
            .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
            execute_openrouter_json(authenticated.request)
                .await
                .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("OpenRouter assistant failed: {error}")))?
                .body
        }
        AssistantProvider::None => return Err(api_error(StatusCode::CONFLICT, "No writing assistant is configured.".into())),
        AssistantProvider::Agent => unreachable!("the agent answered above"),
    };

    let content = assistant::content_of(&response).map_err(|error| {
        request_log::unusable("assistant", "", &error.to_string(), &response.to_string());
        api_error(StatusCode::BAD_GATEWAY, error.to_string())
    })?;
    Ok(content)
}

/// Frees the assistant's five gigabytes as soon as it has answered.
///
/// "Keep models in VRAM between jobs" is off by default, and it means what it
/// says: nothing stays loaded. The assistant was the exception nobody chose -
/// it wrote a draft, kept the card, and the engine then had too little to load its
/// own weights. With the setting on, it stays, because that is what the
/// setting is for. Either way the next request starts it again.
async fn release_assistant_unless_kept(state: &AppState) {
    if state.engine_options.read().await.keep_loaded {
        return;
    }
    if state.assistant_runtime.base_url().await.is_some() {
        state.assistant_runtime.stop().await;
    }
}

/// Where a library song's MIDI lives: beside its audio, named after it.
fn midi_path(state: &AppState, song_id: &str) -> PathBuf {
    state.library.media_dir().join(format!("{song_id}.mid"))
}

/// What a MIDI file says about itself: its model, its instruments and notes.
fn midi_sidecar(midi: &std::path::Path) -> PathBuf {
    PathBuf::from(format!("{}.json", midi.display()))
}

fn midi_size(asked: Option<&str>) -> Result<&'static midi::Size, (StatusCode, Json<ApiError>)> {
    let id = asked.map(str::trim).filter(|id| !id.is_empty()).unwrap_or(midi::DEFAULT_SIZE);
    midi::size(id).ok_or_else(|| api_error(StatusCode::BAD_REQUEST, format!("no model size {id}; the sizes are small, medium and large")))
}

/// The transcriber, its model sizes, the download and the run.
async fn midi_status(State(state): State<AppState>) -> Json<Value> {
    let sizes: Vec<Value> = midi::SIZES
        .iter()
        .map(|size| serde_json::json!({ "id": size.id, "params": size.params, "bytes": size.bytes, "installed": state.midi.model_installed(size), "missing_bytes": state.midi.missing_bytes(size) }))
        .collect();
    let run = state.midi_run.read().await.clone().map(|run| {
        let progress = run.progress();
        let mut value = serde_json::to_value(run).unwrap_or(Value::Null);
        value["progress"] = progress.into();
        value
    });
    Json(serde_json::json!({
        "tool_installed": state.midi.tool_installed(),
        "sizes": sizes,
        "default_size": midi::DEFAULT_SIZE,
        "download": state.midi.downloader().active().await,
        "run": run,
        "license": "MuScriptor by Kyutai & Mirelo (arXiv:2607.08168): code MIT, weights CC BY-NC 4.0 - non-commercial use. Native port: HOT-Step-CPP ace-midi.",
    }))
}

/// The notes the run at work has heard so far, for the live piano roll.
async fn midi_live_notes(State(state): State<AppState>) -> Json<Value> {
    Json(serde_json::json!({ "notes": state.midi_notes.read().await.clone() }))
}

#[derive(Debug, Deserialize)]
struct MidiSizeRequest {
    /// `model_id` when the models page sends it, as for every optional part.
    #[serde(default, alias = "model_id")]
    size: Option<String>,
}

/// Downloads the transcriber and a model size ahead of the first use.
async fn install_midi(State(state): State<AppState>, Json(input): Json<MidiSizeRequest>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let size = midi_size(input.size.as_deref())?;
    let missing = state.midi.missing(size);
    if missing.is_empty() {
        return Ok(Json(serde_json::json!({ "installed": true, "size": size.id })));
    }
    let transcriber = state.midi.clone();
    tokio::spawn(async move {
        match transcriber.downloader().install_all("midi", &missing).await {
            Ok(_) if transcriber.tool_installed() => transcriber.remove_older_tools(),
            Ok(_) => {}
            Err(error) => eprintln!("[ERROR] midi: download failed: {error:#}"),
        }
    });
    Ok(Json(serde_json::json!({ "started": true, "size": size.id })))
}

async fn remove_midi_model(State(state): State<AppState>, Json(input): Json<MidiSizeRequest>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let size = midi_size(input.size.as_deref())?;
    let freed = state.midi.remove(size).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}")))?;
    Ok(Json(serde_json::json!({ "removed": size.id, "freed_bytes": freed })))
}

/// Stops the transcription at work, or the download it waits for.
async fn cancel_midi(State(state): State<AppState>) -> Json<Value> {
    state.midi_stop.0.store(true, std::sync::atomic::Ordering::Relaxed);
    state.midi_stop.1.notify_waiters();
    state.midi.downloader().cancel();
    Json(serde_json::json!({ "cancelled": true }))
}

#[derive(Debug, Deserialize)]
struct MidiRequest {
    /// A library track: its MIDI is kept beside it.
    #[serde(default)]
    song_id: Option<String>,
    /// Or any audio file on this computer: its MIDI goes to the studio's folder.
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    size: Option<String>,
}

/// Turns a track, a stem or any audio file into MIDI. What the transcriber
/// needs is downloaded first if it is not here yet.
async fn start_midi(State(state): State<AppState>, Json(input): Json<MidiRequest>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    card_free_of_training(&state, "turn tracks into MIDI").await?;
    if state.midi_run.read().await.as_ref().is_some_and(|run| !run.done) {
        return Err(api_error(StatusCode::CONFLICT, "a track is already being turned into MIDI".into()));
    }
    let size = midi_size(input.size.as_deref())?;
    let (song_id, title, audio, output) = match (input.song_id.filter(|id| !id.trim().is_empty()), input.path.filter(|path| !path.trim().is_empty())) {
        (Some(id), _) => {
            let song = state
                .library
                .get_song(&id)
                .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
                .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))?;
            let audio = state.library.media_path_for_song(&song).ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "this track has no stored audio".into()))?;
            (Some(id.clone()), song.title.clone(), audio, midi_path(&state, &id))
        }
        (None, Some(path)) => {
            let audio = PathBuf::from(path.trim());
            if !audio.is_file() {
                return Err(api_error(StatusCode::BAD_REQUEST, format!("no audio file at {}", audio.display())));
            }
            let title = audio.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
            let output = state.midi.loose_output(&audio);
            (None, title, audio, output)
        }
        _ => return Err(api_error(StatusCode::BAD_REQUEST, "song_id or path is required".into())),
    };
    state.midi_stop.0.store(false, std::sync::atomic::Ordering::Relaxed);
    state.midi_notes.write().await.clear();
    *state.midi_run.write().await = Some(midi::Run { song_id: song_id.clone(), title, size: size.id, stage: "preparing", chunks_done: 0, chunks_total: 0, notes: 0, done: false, error: None, file: None });
    let background = state.clone();
    tokio::spawn(async move {
        let outcome = transcribe_to_midi(&background, size, &audio, &output).await;
        if let Some(run) = background.midi_run.write().await.as_mut() {
            run.done = true;
            match outcome {
                Ok(notes) => {
                    run.stage = "done";
                    run.notes = notes;
                    run.file = Some(plain_path(&output));
                }
                Err(error) => run.error = Some(format!("{error:#}")),
            }
        }
    });
    Ok(Json(serde_json::json!({ "started": true, "song_id": song_id, "size": size.id })))
}

async fn set_midi_stage(state: &AppState, stage: &'static str) {
    if let Some(run) = state.midi_run.write().await.as_mut() {
        run.stage = stage;
    }
}

fn midi_stopped(state: &AppState) -> bool {
    state.midi_stop.0.load(std::sync::atomic::Ordering::Relaxed)
}

/// The device the transcriber computes on, as its `--device` names it: left to it (CUDA) where
/// the engine runs the CUDA 13 build, else the processor - Pascal and Maxwell have no CUDA 13 code,
/// AMD and Intel no CUDA, and its Vulkan path writes wrong notes.
fn midi_device(options: &EngineOptions) -> &'static str {
    use music_engine::yue_server::ComputeBackend;
    // the macOS build carries Metal alone, which its own choice finds
    if cfg!(target_os = "macos") {
        return if options.backend == ComputeBackend::Cpu { "cpu" } else { "auto" };
    }
    match options.backend {
        ComputeBackend::Cpu => "cpu",
        _ if options.cuda_build() == Some(hardware::CudaBuild::Cuda13) => "auto",
        _ => "cpu",
    }
}

/// Fetches what is missing, reads the audio as the transcriber wants it and
/// runs it, following its notes as they come. Returns how many it heard.
async fn transcribe_to_midi(state: &AppState, size: &'static midi::Size, audio: &std::path::Path, output: &std::path::Path) -> anyhow::Result<usize> {
    use tokio::io::AsyncBufReadExt;
    let missing = state.midi.missing(size);
    if !missing.is_empty() {
        // the first use fetches what the tool needs, as the karaoke recogniser does
        set_midi_stage(state, "downloading").await;
        state.midi.downloader().install_all("midi", &missing).await.context("download the MIDI transcriber")?;
        if midi_stopped(state) || !state.midi.missing(size).is_empty() {
            anyhow::bail!("stopped before everything the transcriber needs had arrived");
        }
        state.midi.remove_older_tools();
    }

    // WAV and MP3 go to the transcriber as they are: its own decoder is the
    // one its port was checked against, and the model hears the difference.
    // Anything else is decoded here to the 16 kHz mono it reads raw.
    let native = audio.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| ["wav", "mp3"].contains(&extension.to_ascii_lowercase().as_str()));
    let raw = if native {
        None
    } else {
        set_midi_stage(state, "reading").await;
        let work = state.midi.work_dir();
        std::fs::create_dir_all(&work).with_context(|| format!("create {}", work.display()))?;
        let raw = work.join(format!("{}.f32", uuid::Uuid::now_v7().simple()));
        let (from, to) = (audio.to_path_buf(), raw.clone());
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let samples = audio_pcm::decode_mono_16k(&from)?;
            let bytes: Vec<u8> = samples.iter().flat_map(|sample| sample.to_le_bytes()).collect();
            std::fs::write(&to, bytes).with_context(|| format!("write {}", to.display()))
        })
        .await??;
        Some(raw)
    };

    set_midi_stage(state, "transcribing").await;
    let partial = PathBuf::from(format!("{}.part", output.display()));
    if let Some(folder) = output.parent() {
        std::fs::create_dir_all(folder).with_context(|| format!("create {}", folder.display()))?;
    }
    let tool = state.midi.tool();
    let device = midi_device(&*state.engine_options.read().await);
    let mut command = tokio::process::Command::new(&tool);
    command
        .arg("--device")
        .arg(device)
        .arg("--model")
        .arg(state.midi.model_dir(size))
        .arg(if raw.is_some() { "--transcribe-raw" } else { "--transcribe" })
        .arg(raw.as_deref().unwrap_or(audio))
        .arg("--out")
        .arg(&partial)
        .arg("--jsonl")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    // the CUDA runtime it imports lives beside the engine, as for the trainer
    let mut paths = vec![engine_bundle_root()];
    paths.extend(std::env::var_os("PATH").iter().flat_map(std::env::split_paths));
    if let Ok(path) = std::env::join_paths(paths) {
        command.env("PATH", path);
    }
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let mut child = command.spawn().with_context(|| format!("start {}", tool.display()))?;
    let stdout = child.stdout.take().context("the transcriber's output")?;
    let stderr = child.stderr.take().context("the transcriber's errors")?;
    let said = Arc::new(std::sync::Mutex::new(std::collections::VecDeque::<String>::new()));
    let listener = {
        let said = said.clone();
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut said = said.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                said.push_back(line);
                if said.len() > 20 {
                    said.pop_front();
                }
            }
        })
    };

    let mut events = midi::Events::default();
    let mut lines = tokio::io::BufReader::new(stdout).lines();
    let outcome: anyhow::Result<()> = loop {
        if midi_stopped(state) {
            break Err(anyhow::anyhow!("stopped"));
        }
        tokio::select! {
            line = lines.next_line() => match line {
                Ok(Some(line)) => {
                    let chunks = events.chunks_done;
                    events.take(&line);
                    // the notes are handed on a piece at a time: every note of
                    // a piece is closed by the time the next one starts
                    if events.chunks_done != chunks || events.finished {
                        *state.midi_notes.write().await = events.notes.clone();
                        if let Some(run) = state.midi_run.write().await.as_mut() {
                            run.chunks_done = events.chunks_done;
                            run.chunks_total = events.chunks_total;
                            run.notes = events.notes.len();
                        }
                    }
                }
                Ok(None) => break Ok(()),
                Err(error) => break Err(error.into()),
            },
            _ = state.midi_stop.1.notified() => break Err(anyhow::anyhow!("stopped")),
        }
    };
    if outcome.is_err() {
        let _ = child.kill().await;
    }
    let status = child.wait().await.context("wait for the transcriber")?;
    let _ = listener.await;
    if let Some(raw) = &raw {
        let _ = std::fs::remove_file(raw);
    }
    if let Err(error) = outcome {
        let _ = std::fs::remove_file(&partial);
        return Err(error);
    }
    if !status.success() || !events.finished {
        let _ = std::fs::remove_file(&partial);
        let said = said.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).iter().cloned().collect::<Vec<_>>().join("\n");
        // Windows' "a DLL was not found": a transcriber from before its backends were libraries
        if status.code() == Some(-1073741515) {
            anyhow::bail!("the transcriber could not load a library it needs; remove Audio to MIDI in Settings - Models and download it again: {said}");
        }
        anyhow::bail!("the transcriber stopped with {status} on {device}: {said}");
    }
    std::fs::rename(&partial, output).with_context(|| format!("keep {}", output.display()))?;
    let sidecar = midi::Sidecar { size: size.id.to_string(), made_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|since| since.as_secs().to_string()).unwrap_or_default(), instruments: events.instruments(), notes: events.notes.clone() };
    std::fs::write(midi_sidecar(output), serde_json::to_vec(&sidecar)?).with_context(|| format!("write {}", midi_sidecar(output).display()))?;
    let heard = events.notes.len();
    *state.midi_notes.write().await = events.notes;
    Ok(heard)
}

/// A library song's MIDI: where it is, the model, its instruments and notes.
async fn read_song_midi(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let file = midi_path(&state, &id);
    if !file.is_file() {
        return Err(api_error(StatusCode::NOT_FOUND, "this track has no MIDI yet".into()));
    }
    let sidecar: Option<midi::Sidecar> = std::fs::read(midi_sidecar(&file)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok());
    Ok(Json(serde_json::json!({
        "song_id": id,
        "file": plain_path(&file),
        "size": sidecar.as_ref().map(|sidecar| sidecar.size.clone()),
        "made_at": sidecar.as_ref().map(|sidecar| sidecar.made_at.clone()),
        "instruments": sidecar.as_ref().map(|sidecar| sidecar.instruments.clone()).unwrap_or_default(),
        "notes": sidecar.map(|sidecar| sidecar.notes).unwrap_or_default(),
    })))
}

/// The .mid itself, named after the song, to save or open elsewhere.
async fn song_midi_file(State(state): State<AppState>, Path(id): Path<String>) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let file = midi_path(&state, &id);
    let bytes = tokio::fs::read(&file).await.map_err(|_| api_error(StatusCode::NOT_FOUND, "this track has no MIDI yet".into()))?;
    let title = state.library.get_song(&id).ok().flatten().map(|song| song.title).unwrap_or_else(|| id.clone());
    let name: String = title.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '_' } else { c }).collect();
    let disposition = format!("attachment; filename=\"midi.mid\"; filename*=UTF-8''{}", mcp::segment(&format!("{name}.mid")));
    Ok(axum::response::Response::builder()
        .header(header::CONTENT_TYPE, "audio/midi")
        .header(header::CONTENT_DISPOSITION, disposition)
        .body(axum::body::Body::from(bytes))
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?)
}

#[derive(Deserialize)]
struct MidiUpload {
    data: String,
}

/// A .mid from the MIDI editor kept as a library track's MIDI: the file as it came, and the notes the player reads from it.
async fn write_song_midi(State(state): State<AppState>, Path(id): Path<String>, Json(body): Json<MidiUpload>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    use base64::Engine as _;
    if state.library.get_song(&id).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?.is_none() {
        return Err(api_error(StatusCode::NOT_FOUND, format!("no library track {id}")));
    }
    let data = base64::engine::general_purpose::STANDARD.decode(body.data.trim()).map_err(|_| api_error(StatusCode::BAD_REQUEST, "'data' is not base64".into()))?;
    if data.len() > 8 * 1024 * 1024 {
        return Err(api_error(StatusCode::PAYLOAD_TOO_LARGE, "that file is larger than 8 MB, far more than the MIDI of any song".into()));
    }
    let notes = midi_edit::read(&data).map_err(|error| api_error(StatusCode::BAD_REQUEST, format!("this file could not be read as MIDI: {error}")))?;
    let file = midi_path(&state, &id);
    let made_at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|since| since.as_secs().to_string()).unwrap_or_default();
    let sidecar = midi::Sidecar { size: "edited".into(), made_at, instruments: midi_edit::instruments(&notes), notes };
    let sidecar_bytes = serde_json::to_vec(&sidecar).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    tokio::fs::write(&file, data).await.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("write {}: {error}", file.display())))?;
    tokio::fs::write(midi_sidecar(&file), sidecar_bytes).await.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("write {}: {error}", midi_sidecar(&file).display())))?;
    Ok(Json(serde_json::json!({
        "song_id": id,
        "file": plain_path(&file),
        "size": sidecar.size,
        "made_at": sidecar.made_at,
        "instruments": sidecar.instruments,
        "notes": sidecar.notes,
    })))
}

async fn delete_song_midi(State(state): State<AppState>, Path(id): Path<String>) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    let file = midi_path(&state, &id);
    if !file.is_file() {
        return Err(api_error(StatusCode::NOT_FOUND, "this track has no MIDI".into()));
    }
    for path in [file.clone(), midi_sidecar(&file)] {
        if path.is_file() {
            std::fs::remove_file(&path).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("remove {}: {error}", path.display())))?;
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

/// A path as other programs take it, without the `\\?\` prefix a
/// canonical Windows path carries.
fn plain_path(path: &std::path::Path) -> String {
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(text)
}

/// Where a library song's files are, for an agent that reads or opens them.
async fn library_song_files(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let song = state
        .library
        .get_song(&id)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))?;
    let stems: Vec<Value> = stems_on_disk(&state, &id).into_iter().map(|stem| serde_json::json!({ "stem": stem, "path": plain_path(&stem_path(&state, &id, &stem)) })).collect();
    Ok(Json(serde_json::json!({
        "audio": state.library.media_path_for_song(&song).map(|path| plain_path(&path)),
        "cover": state.library.cover_path_for_song(&song).map(|(path, _)| plain_path(&path)),
        "stems": stems,
        "midi": Some(midi_path(&state, &id)).filter(|path| path.is_file()).map(|path| plain_path(&path)),
    })))
}

/// Where a dataset song's audio and separated vocals are.
async fn dataset_song_files(State(state): State<AppState>, Path((id, item)): Path<(String, String)>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let audio = state.training.item_audio(&id, &item).map_err(training_error)?;
    let vocals = state.training.item_vocals(&id, &item).map_err(training_error)?;
    Ok(Json(serde_json::json!({ "audio": plain_path(&audio), "vocals": vocals.is_file().then(|| plain_path(&vocals)) })))
}

#[derive(Debug, Deserialize)]
struct GuideQuery {
    #[serde(default)]
    topic: String,
}

/// How to write for YuE2, from the rules the studio's assistant follows.
async fn writing_guide(Query(query): Query<GuideQuery>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let topics: Vec<Value> = assistant::GUIDE_TOPICS.iter().map(|(topic, about)| serde_json::json!({ "topic": topic, "about": about })).collect();
    match assistant::writing_guide(query.topic.trim()) {
        Some(guide) => Ok(Json(serde_json::json!({ "topic": query.topic, "guide": guide }))),
        None if query.topic.trim().is_empty() => Ok(Json(serde_json::json!({ "topics": topics }))),
        None => Err(api_error(StatusCode::BAD_REQUEST, format!("No guide '{}'; the topics are: {}", query.topic, assistant::GUIDE_TOPICS.iter().map(|(topic, _)| *topic).collect::<Vec<_>>().join(", ")))),
    }
}

#[derive(Debug, Deserialize)]
struct ExamplesQuery {
    #[serde(default)]
    brief: String,
}

/// The official YuE2 requests closest to a brief, to write in their shape.
async fn writing_examples(Query(query): Query<ExamplesQuery>) -> Json<Value> {
    let found: Vec<Value> = skill::references(&query.brief).into_iter().map(|reference| serde_json::json!({ "title": reference.title, "style": reference.style, "lyrics": reference.lyrics })).collect();
    Json(serde_json::json!({ "examples": found }))
}

#[derive(Debug, Deserialize)]
struct VideoName {
    name: String,
}

/// A clip the video editor rendered for an agent, kept in the studio's
/// `videos` folder under a name that does not overwrite another.
async fn store_video(Query(query): Query<VideoName>, body: axum::body::Bytes) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let folder = studio_data_root().unwrap_or_else(|| PathBuf::from(".")).join("videos");
    std::fs::create_dir_all(&folder).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("create {}: {error}", folder.display())))?;
    let name = std::path::Path::new(&query.name).file_name().and_then(|name| name.to_str()).filter(|name| !name.is_empty()).unwrap_or("clip.mp4").to_string();
    let (stem, extension) = name.rsplit_once('.').map_or((name.clone(), "mp4".to_string()), |(stem, extension)| (stem.to_string(), extension.to_string()));
    let mut path = folder.join(&name);
    let mut number = 2;
    while path.exists() {
        path = folder.join(format!("{stem}-{number}.{extension}"));
        number += 1;
    }
    tokio::fs::write(&path, &body).await.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("write {}: {error}", path.display())))?;
    Ok(Json(serde_json::json!({ "path": plain_path(&path) })))
}

#[derive(Debug, Deserialize)]
struct FindLyricsRequest {
    #[serde(default)]
    artist: String,
    title: String,
    #[serde(default)]
    seconds: f64,
}

/// A song's published lyrics from the lyric databases, for the MCP tools.
async fn find_lyrics(Json(request): Json<FindLyricsRequest>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let sources = lyrics_db::Sources::new().map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}")))?;
    let mut failed = Vec::new();
    let song = lyrics_db::Song { artist: request.artist, title: request.title, seconds: request.seconds };
    let found = match sources.find(&song, &mut failed).await {
        Some(lyrics_db::Found::Lyrics { plain, timed, source }) => serde_json::json!({ "source": source, "lyrics": plain, "timed": timed }),
        Some(lyrics_db::Found::Instrumental { source }) => serde_json::json!({ "source": source, "instrumental": true }),
        None => serde_json::json!({ "found": false }),
    };
    Ok(Json(serde_json::json!({ "result": found, "unreachable": failed })))
}

/// What was asked of the cloud and what came back, newest last.
///
/// A failed draft used to leave one red line and nothing behind it; this is
/// where the answer itself is kept, so a model that wrote almost the right
/// thing can be told from one that wrote nothing.
async fn openrouter_logs() -> Json<Value> {
    Json(serde_json::json!({ "path": request_log::path().display().to_string(), "lines": request_log::tail(400) }))
}

async fn openrouter_settings() -> Json<Value> {
    let source = credentials::openrouter_source();
    Json(serde_json::json!({
        "configured": source.is_some(),
        "source": source,
        "environment_variable": credentials::OPENROUTER_ENV_VAR,
    }))
}

async fn update_openrouter_settings(
    State(state): State<AppState>,
    Json(request): Json<OpenRouterSettingsRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let source = credentials::store_openrouter_api_key(request.api_key.as_deref())
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;

    // Connecting a key is the moment to learn what it can reach. After this
    // the catalog is served from the cache on disk until the user asks for a
    // refresh, so the studio never goes to the network on its own again.
    let mut refreshed = false;
    if source.is_some() {
        {
            let mut cached = state.openrouter_catalog.write().await;
            cached.catalog = None;
        }
        refreshed = catalog_for(&state).await.is_ok();
    }

    Ok(Json(serde_json::json!({
        "configured": source.is_some(),
        "source": source,
        "environment_variable": credentials::OPENROUTER_ENV_VAR,
        "catalog_refreshed": refreshed,
    })))
}

async fn setup_status(State(state): State<AppState>) -> Json<Value> {
    let target = effective_install_target(&state).await;
    let manager_status = state.model_manager.status(target).await;
    Json(compose_setup_status(&state, manager_status).await)
}

/// Frees the disk a set of components takes.
///
/// The studio downloads ten gigabytes on request; it must be able to give them
/// back on request too, without sending anyone to hunt through a profile folder.
async fn setup_remove(
    State(state): State<AppState>,
    Json(request): Json<SetupDownloadRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let report = state
        .model_manager
        .remove(&request.ids)
        .await
        .map_err(|error| api_error(StatusCode::CONFLICT, error.to_string()))?;
    Ok(Json(serde_json::json!({
        "removed": report.removed,
        "freed_bytes": report.freed_bytes,
    })))
}

const ADOPT_FOLDER_DEPTH: usize = 8;

/// Files in a folder and its subfolders, `depth` levels down; hidden folders
/// (caches, `.git`) are left out.
fn adoptable_files(folder: &std::path::Path, depth: usize) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(folder) else { return files };
    for path in entries.flatten().map(|entry| entry.path()) {
        if path.is_file() {
            files.push(path);
        } else if depth > 0 && path.is_dir() && !path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with('.')) {
            files.extend(adoptable_files(&path, depth - 1));
        }
    }
    files
}

/// Takes models the user already has instead of downloading them again.
///
/// Anyone who has run yue2.cpp by hand already has these weights on disk, and
/// they are gigabytes each. This opens a folder picker,
/// looks for the files the catalogue names - by name, then by matching size -
/// and hard-links or copies them into the studio's own model directory.
async fn setup_adopt(State(state): State<AppState>, body: axum::body::Bytes) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let Some(models_root) = studio_data_root().map(|root| root.join("models").join(model_manager::ENGINE_ID)) else {
        return Err(api_error(StatusCode::INTERNAL_SERVER_ERROR, "the studio has no data directory".into()));
    };
    let catalog = state.model_manager.catalog();
    // a folder an agent names is taken as it is; the window asks the user
    let named = serde_json::from_slice::<Value>(&body).ok().and_then(|value| value.get("path").and_then(Value::as_str).map(std::path::PathBuf::from));
    let picked = match named {
        Some(folder) => Some(folder),
        None => tokio::task::spawn_blocking(move || {
            rfd::FileDialog::new().set_title("Folder with YuE2 models").pick_folder()
        })
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?,
    };

    let Some(folder) = picked else {
        return Ok(Json(serde_json::json!({ "picked": false, "adopted": [] })));
    };

    let _ = std::fs::create_dir_all(&models_root);
    let mut adopted: Vec<String> = Vec::new();
    // a copy is a second set of gigabytes on disk; the window says which files took one
    let mut copied: Vec<Value> = Vec::new();
    std::fs::read_dir(&folder).map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    // Model sets are often kept one component per folder, so the picked
    // folder is searched with its subfolders.
    let searched = folder.clone();
    let entries = tokio::task::spawn_blocking(move || adoptable_files(&searched, ADOPT_FOLDER_DEPTH))
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;

    for component in &catalog.components {
        let target = models_root.join(component.filename);
        let published = |path: &std::path::Path| std::fs::metadata(path).map(|meta| meta.is_file() && meta.len() == component.bytes).unwrap_or(false);
        if published(&target) {
            continue;
        }
        // The size always, since an earlier release keeps the same name; the
        // name first among those, then any file of that size, because other
        // builds rename the same file.
        let source = entries
            .iter()
            .find(|path| path.file_name().is_some_and(|name| name == component.filename) && published(path))
            .or_else(|| entries.iter().find(|path| published(path)));
        let Some(source) = source else { continue };
        // a file of the earlier release under this name gives way to the one found
        let _ = std::fs::remove_file(&target);
        // A hard link costs nothing and keeps one copy on disk; a folder on
        // another drive cannot have one, so that falls back to a copy.
        if std::fs::hard_link(source, &target).is_err() {
            match std::fs::copy(source, &target) {
                Ok(bytes) => copied.push(serde_json::json!({ "id": component.id, "bytes": bytes })),
                Err(_) => continue,
            }
        }
        adopted.push(component.id.to_string());
    }

    let target = effective_install_target(&state).await;
    let status = state.model_manager.status(target).await;
    Ok(Json(serde_json::json!({
        "picked": true,
        "folder": folder.display().to_string(),
        "adopted": adopted,
        "copied": copied,
        "status": compose_setup_status(&state, status).await,
    })))
}

/// Opens the studio's own folder in the system file manager.
///
/// Saying where the ten gigabytes are is half an answer; the other half is
/// getting there without retyping a path from a settings screen.
async fn open_data_directory() -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let Some(root) = studio_data_root() else {
        return Err(api_error(StatusCode::NOT_FOUND, "the studio has no data directory".into()));
    };
    let _ = std::fs::create_dir_all(&root);
    #[cfg(windows)]
    let opened = std::process::Command::new("explorer.exe").arg(&root).spawn();
    #[cfg(target_os = "macos")]
    let opened = std::process::Command::new("open").arg(&root).spawn();
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let opened = std::process::Command::new("xdg-open").arg(&root).spawn();
    opened.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(serde_json::json!({ "opened": root.display().to_string() })))
}

async fn setup_catalog(State(state): State<AppState>) -> Json<model_manager::Catalog> {
    Json(state.model_manager.catalog())
}

async fn setup_download(
    State(state): State<AppState>,
    Json(request): Json<SetupDownloadRequest>,
) -> Result<(StatusCode, Json<model_manager::DownloadJob>), (StatusCode, Json<ApiError>)> {
    let job = state
        .model_manager
        .install(InstallRequest {
            profile_id: request.profile_id,
            component_ids: request.ids,
        })
        .await
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    let state_for_completion = state.clone();
    let job_id = job.id.clone();
    tokio::spawn(async move { persist_completed_download_profile(state_for_completion, job_id).await });
    Ok((StatusCode::ACCEPTED, Json(job)))
}

async fn persist_completed_download_profile(state: AppState, job_id: String) {
    loop {
        let Some(job) = state.model_manager.download_job(&job_id).await else { return; };
        match job.status {
            model_manager::DownloadStatus::Completed => {
                if let Some(profile_id) = job.profile_id.or_else(|| model_manager::profile_matching(&job.component_ids).map(str::to_owned)) {
                    *state.selected_profile_id.write().await = Some(profile_id);
                    *state.selected_component_ids.write().await = None;
                } else if state.model_manager.installed_component_files(&job.component_ids).is_ok() {
                    *state.selected_profile_id.write().await = None;
                    *state.selected_component_ids.write().await = Some(job.component_ids);
                }
                let _ = persist_studio_settings(&state).await;
                reload_engine_if_models_changed(&state).await;
                return;
            }
            model_manager::DownloadStatus::Cancelled | model_manager::DownloadStatus::Failed => return,
            model_manager::DownloadStatus::Downloading => tokio::time::sleep(std::time::Duration::from_millis(250)).await,
        }
    }
}

/// Uses a set that is already on disk.
///
/// Downloading was the only way to change which quantisation the studio runs,
/// so a machine with two sets installed was stuck on whichever arrived last.
/// This switches between what is already there, and refuses a set with a
/// missing file rather than failing at generation time.
async fn setup_select(
    State(state): State<AppState>,
    Json(request): Json<SetupSelectRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let matched = request.component_ids.as_deref().and_then(model_manager::profile_matching).map(str::to_owned);
    if let Some(profile_id) = request.profile_id.clone().filter(|value| !value.trim().is_empty()).or(matched) {
        let known = state.model_manager.catalog().profiles.iter().any(|profile| profile.id == profile_id);
        if !known {
            return Err(api_error(StatusCode::BAD_REQUEST, format!("unknown profile {profile_id}")));
        }
        *state.selected_profile_id.write().await = Some(profile_id);
        *state.selected_component_ids.write().await = None;
    } else {
        let ids = request.component_ids.unwrap_or_default();
        if ids.is_empty() {
            return Err(api_error(StatusCode::BAD_REQUEST, "nothing selected".to_string()));
        }
        state
            .model_manager
            .installed_component_files(&ids)
            .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
        *state.selected_profile_id.write().await = None;
        *state.selected_component_ids.write().await = Some(ids);
    }
    let _ = persist_studio_settings(&state).await;
    reload_engine_if_models_changed(&state).await;
    let target = effective_install_target(&state).await;
    let manager_status = state.model_manager.status(target).await;
    Ok(Json(serde_json::to_value(compose_setup_status(&state, manager_status).await).unwrap_or(Value::Null)))
}

async fn setup_cancel(State(state): State<AppState>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let target = effective_install_target(&state).await;
    let manager_status = state
        .model_manager
        .cancel(target)
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(compose_setup_status(&state, manager_status).await))
}

async fn capabilities(State(state): State<AppState>) -> Json<CapabilitiesResponse> {
    let primary_installed = state.music_server.health().await;
    let parakeet_installed = state.lyrics_sync.parakeet_any_ready();
    let whisper_installed = state.lyrics_sync.whisper_binary().is_some();
    let assistant = state.assistant.read().await.clone();
    let assistant_installed = assistant.available();
    Json(CapabilitiesResponse {
        engines: capability_engines_with(primary_installed, parakeet_installed, whisper_installed, assistant_installed),
    })
}

fn capability_engines(primary_installed: bool) -> Vec<EngineDescriptor> {
    capability_engines_with(primary_installed, false, false, false)
}

/// The engines the studio can offer, including the local ones it only has when
/// their models are installed. Without these the "Local" button on the provider
/// page was disabled for ever: the studio recognises speech and writes captions
/// locally, but never said so here.
fn capability_engines_with(
    primary_installed: bool,
    parakeet_installed: bool,
    whisper_installed: bool,
    assistant_installed: bool,
) -> Vec<EngineDescriptor> {
    let openrouter_capabilities = vec![
        Capability::SpeechToText,
        Capability::PromptEnhancement,
        Capability::CoverArt,
    ];
    vec![
        EngineDescriptor {
            id: PRIMARY_MUSIC_ENGINE_ID.into(),
            display_name: "YuE2 (yue2.cpp)".into(),
            capabilities: vec![Capability::MusicGeneration],
            execution_mode: ExecutionMode::Local,
            installed: primary_installed,
        },
        // Two different recognisers, named. "Parakeet / Whisper" was not a
        // choice, it was a shrug.
        EngineDescriptor {
            id: "parakeet".into(),
            display_name: "Parakeet TDT 0.6B (local)".into(),
            capabilities: vec![Capability::SpeechToText],
            execution_mode: ExecutionMode::Local,
            installed: parakeet_installed,
        },
        EngineDescriptor {
            id: "whisper".into(),
            display_name: "Whisper.cpp (local)".into(),
            capabilities: vec![Capability::SpeechToText],
            execution_mode: ExecutionMode::Local,
            installed: whisper_installed,
        },
        EngineDescriptor {
            id: "local-assistant".into(),
            display_name: "Local GGUF model".into(),
            capabilities: vec![Capability::PromptEnhancement],
            execution_mode: ExecutionMode::Local,
            installed: assistant_installed,
        },
        EngineDescriptor {
            id: "openrouter".into(),
            display_name: "OpenRouter".into(),
            capabilities: openrouter_capabilities,
            execution_mode: ExecutionMode::OpenRouter,
            installed: false,
        },
    ]
}

/// Gets the writing assistant off the graphics card before the engine needs it.
///
/// There is one card: Gemma holds five gigabytes from the moment it writes
/// a draft, and YuE2 needs its own few on top. The assistant starts itself on the next request it
/// receives, so stopping it here costs a reload later and nothing else.
async fn free_the_card_for_the_engine(state: &AppState) {
    if state.assistant_runtime.base_url().await.is_some() {
        state.assistant_runtime.stop().await;
    }
}

/// What the engine's own log says about why it is not there any more.
///
/// A card that ran out of memory says so in the log and then the process is
/// gone; the studio saw only a refused connection, and told the user to
/// download models that were already on disk.
fn engine_failure_reason(state: &AppState, job_id: &str) -> Option<String> {
    let tail = music_engine::yue_server::startup_log_tail(80).join("\n").to_lowercase();
    if describes_exhausted_memory(&tail) {
        return Some("The graphics card ran out of memory while the engine was loading the models. Choose a smaller quantisation in the model manager, or close whatever else is using the card - the writing assistant holds several gigabytes of its own.".to_string());
    }
    let lines = state.engine_log.lines()?;
    last_fatal(&lines, job_id)
}

/// What the engine said when it gave this job up: the last FATAL line after the
/// job's own start, without the stage tag.
fn last_fatal(lines: &[String], job_id: &str) -> Option<String> {
    let start = format!("Job {job_id}");
    let from = lines.iter().rposition(|line| line.contains(&start))?;
    lines[from..].iter().rev().find_map(|line| line.split_once("FATAL:").map(|(_, why)| why.trim().to_string())).filter(|why| !why.is_empty())
}

/// Whether a lowercased log says the card ran out of room.
fn describes_exhausted_memory(log: &str) -> bool {
    [
        "out of memory",
        "cudamalloc",
        "failed to allocate",
        "insufficient memory",
        "cudaerrormemoryallocation",
        "bad_alloc",
    ]
    .iter()
    .any(|marker| log.contains(marker))
}

async fn create_music_job(
    State(state): State<AppState>,
    Json(request): Json<CreateMusicJobRequest>,
) -> (StatusCode, Json<MusicJob>) {
    submit_music_job(state, request, 0).await
}

/// Sends a request to the engine and keeps it; `attempt` counts the times it
/// was started again after the studio closed on it.
async fn submit_music_job(state: AppState, mut request: CreateMusicJobRequest, attempt: u32) -> (StatusCode, Json<MusicJob>) {
    let engine_id = selected_local_music_engine(&*state.configuration.read().await)
        .unwrap_or_else(|| "unconfigured".into());
    if engine_id != PRIMARY_MUSIC_ENGINE_ID {
        let job = queued_not_configured_job(request, engine_id);
        state.jobs.write().await.insert(job.id.clone(), job.clone());
        return (StatusCode::ACCEPTED, Json(job));
    }

    if state.training.active_run().await.is_some() {
        let error = "a training run has the card; songs can be made once it finishes or is stopped".to_string();
        return (StatusCode::CONFLICT, Json(failed_request_job(request, engine_id, error)));
    }
    if request.vocals_only {
        if !state.separator.is_installed() {
            return (StatusCode::BAD_REQUEST, Json(failed_request_job(request, engine_id, "Install the stem separator in Tools before requesting vocals only.".into())));
        }
        let runtime = state.separation_config.read().await.runtime;
        if let Err(error) = state.lyrics_sync.onnx_card(runtime) {
            return (StatusCode::BAD_REQUEST, Json(failed_request_job(request, engine_id, format!("The stem separator runtime is unavailable: {error:#}"))));
        }
    }
    // an adapter named without strengths starts where the create page starts it
    let slot_ids: Vec<&str> = music_engine::yue_server::ADAPTER_SLOTS.iter().map(|slot| slot.id).collect();
    for adapter in request.adapters.iter_mut().filter(|adapter| adapter.scales.is_empty()) {
        adapter.scales = state.adapters.starting_scales(&adapter.id, &slot_ids);
    }
    if let Some(missing) = request.adapters.iter().find(|adapter| !state.adapters.exists(&adapter.id)).map(|adapter| adapter.id.clone()) {
        let error = format!("adapter {missing} is not installed; add it again on the LoRA page");
        return (StatusCode::BAD_REQUEST, Json(failed_request_job(request, engine_id, error)));
    }
    let max_batch = state.engine_options.read().await.effective_max_batch();
    let mut body = match yue_request_from(&request, max_batch) {
        Ok(value) => value,
        Err(error) => return (StatusCode::BAD_REQUEST, Json(failed_request_job(request, engine_id, error))),
    };
    if let Some(style) = trained_style(&state.adapters, &request) {
        body["style"] = Value::String(style);
    }
    if request.companion_scale.is_none() {
        body["companion_scale"] = Value::from(companion_default(&state.adapters, &request));
    }
    let laid = laid_out(&mut body, request.duration_seconds.is_none());
    if request.lyric_timing != Some(false) {
        lyric_schedule(&mut body);
    }
    let derived = match request.cover_of.clone() {
        Some(id) => match state.library.get_song(&id) {
            Ok(Some(original)) => Some(derivation(&original, "cover", serde_json::json!({ "cot": request.cot, "style": request.style }))),
            Ok(None) => return (StatusCode::BAD_REQUEST, Json(failed_request_job(request, engine_id, format!("cover_of names no library song: {id}")))),
            Err(error) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(failed_request_job(request, engine_id, error.to_string()))),
        },
        None => None,
    };
    let stored_request = serde_json::to_value(&request).unwrap_or(Value::Null);
    match state.music_server.submit(engine_submission(&body)).await {
        Ok(remote) => {
            let job = MusicJob {
                derived,
                id: remote.id,
                client_ref: request.client_ref.clone(),
                submitted_at: unix_millis(),
                engine_id,
                cover_prompt: request.cover_prompt.clone(),
                title: Some(titled(&request, &state.adapters)),
                status: MusicJobStatus::Queued,
                dispatch: MusicJobDispatch::Local,
                phase: MusicJobPhase::Queued,
                style: request.style,
                lyrics: request.lyrics,
                duration_seconds: request.duration_seconds.unwrap_or_default(),
                generation_settings: body,
                song: None,
                songs: vec![],
                message: "Submitted to yue-server.".into(),
                playlist_id: request.playlist_id.clone(),
                laid,
            };
            state.jobs.write().await.insert(job.id.clone(), job.clone());
            // kept at once, so a song the studio is closed on is not lost
            if let Err(error) = state.library.save_music_job(&stored_job(&job, stored_request, attempt)) {
                eprintln!("[ERROR] the song's request could not be kept: {error:#}");
            }
            spawn_job_watcher(state.clone(), job.id.clone());
            (StatusCode::ACCEPTED, Json(job))
        }
        Err(error) => {
            let job = failed_request_job(request, engine_id, format!("the engine refused the job: {error}"));
            state.jobs.write().await.insert(job.id.clone(), job.clone());
            (StatusCode::SERVICE_UNAVAILABLE, Json(job))
        }
    }
}

/// Re-renders a track from its semantic stream: the prefix and the codes
/// prefill in one forward, so only the flow-matching side (steps, noise seed,
/// variations, output encoding) can change while the music stays the same.
async fn replay_music_job(
    State(state): State<AppState>,
    Json(request): Json<ReplayMusicJobRequest>,
) -> Result<(StatusCode, Json<MusicJob>), (StatusCode, Json<ApiError>)> {
    submit_replay_job(state, request, 0).await
}

/// Sends a re-render to the engine and keeps it, as a new song is kept.
async fn submit_replay_job(state: AppState, request: ReplayMusicJobRequest, attempt: u32) -> Result<(StatusCode, Json<MusicJob>), (StatusCode, Json<ApiError>)> {
    if selected_local_music_engine(&*state.configuration.read().await).as_deref() != Some(PRIMARY_MUSIC_ENGINE_ID) {
        return Err(api_error(StatusCode::CONFLICT, "Re-rendering requires the local YuE2 engine.".into()));
    }
    let mut source_title = None;
    let replay = match (&request.song_id, &request.replay_request) {
        (Some(_), Some(_)) => return Err(api_error(StatusCode::BAD_REQUEST, "Provide either song_id or replay_request, not both.".into())),
        (Some(song_id), None) => {
            let song = state.library.get_song(song_id).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
                .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found.".into()))?;
            source_title = Some(song.title.clone());
            song.replay_request.ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "This track has no replay request: it was not generated by YuE2.".into()))?
        }
        (None, Some(replay)) => replay.clone(),
        (None, None) => return Err(api_error(StatusCode::BAD_REQUEST, "Provide song_id or replay_request.".into())),
    };
    if state.training.active_run().await.is_some() {
        return Err(api_error(StatusCode::CONFLICT, "a training run has the card; re-render once it finishes or is stopped".into()));
    }
    let mut body = prepare_replay_synthesis(replay, &request).map_err(|error| api_error(StatusCode::BAD_REQUEST, error))?;
    let continuing = body.get("continue_semantic_tokens").and_then(Value::as_bool) == Some(true);
    // the part composed on keeps to the score the way a new song does
    if continuing {
        lyric_schedule(&mut body);
    }
    let style = body.get("style").and_then(Value::as_str).unwrap_or_default().to_owned();
    let lyrics = body.get("lyrics").and_then(Value::as_str).unwrap_or_default().to_owned();
    let remote = state
        .music_server
        .submit(engine_submission(&body))
        .await
        .map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, format!("the engine refused the job: {error}")))?;
    let title = request.title.clone().filter(|value| !value.trim().is_empty()).or(source_title);
    let mut job = MusicJob {
        derived: None,
        cover_prompt: None,
        id: remote.id,
        client_ref: request.client_ref.clone(),
        submitted_at: unix_millis(),
        engine_id: PRIMARY_MUSIC_ENGINE_ID.into(),
        title,
        status: MusicJobStatus::Queued,
        dispatch: MusicJobDispatch::Local,
        phase: MusicJobPhase::Queued,
        style,
        lyrics,
        duration_seconds: body.get("duration").and_then(Value::as_f64).unwrap_or_default(),
        generation_settings: body,
        song: None,
        songs: vec![],
        message: if continuing {
            "Submitted a re-render that composes on from the track's last frame.".into()
        } else {
            "Submitted a re-render: the semantic stream is present, so the autoregressive stage is skipped.".into()
        },
        playlist_id: None,
        laid: None,
    };
    if let Some(song_id) = &request.song_id {
        if let Some(original) = state.library.get_song(song_id).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))? {
            job.derived = Some(derivation(&original, "replay", serde_json::json!({ "steps": request.steps, "seed": request.seed, "synth_batch_size": request.synth_batch_size, "output_format": request.output_format, "mp3_bitrate": request.mp3_bitrate, "extend_seconds": request.extend_seconds })));
        }
    }
    state.jobs.write().await.insert(job.id.clone(), job.clone());
    let stored_request = serde_json::json!({ "replay": serde_json::to_value(&request).unwrap_or(Value::Null) });
    if let Err(error) = state.library.save_music_job(&stored_job(&job, stored_request, attempt)) {
        eprintln!("[ERROR] the re-render's request could not be kept: {error:#}");
    }
    spawn_job_watcher(state.clone(), job.id.clone());
    Ok((StatusCode::ACCEPTED, Json(job)))
}

fn prepare_replay_synthesis(mut replay: Value, overrides: &ReplayMusicJobRequest) -> Result<Value, String> {
    let object = replay.as_object_mut().ok_or("replay_request must be a JSON object")?;
    let tokens = object.get("semantic_tokens").and_then(Value::as_str).unwrap_or_default().to_owned();
    let tokens = tokens.as_str();
    validate_semantic_tokens(tokens)?;
    if tokens.trim().is_empty() {
        return Err("replay_request has no semantic_tokens; it cannot skip the autoregressive stage".into());
    }
    if let Some(steps) = overrides.steps {
        if steps < 1 { return Err("steps must be at least 1".into()); }
        object.insert("steps".into(), Value::from(steps));
    }
    if let Some(seed) = overrides.seed { object.insert("seed".into(), Value::from(seed)); }
    if let Some(variations) = overrides.synth_batch_size {
        if !(1..=9).contains(&variations) { return Err("synth_batch_size must be between 1 and 9".into()); }
        object.insert("synth_batch_size".into(), Value::from(variations));
    }
    if let Some(format) = &overrides.output_format {
        validate_output_format(format)?;
        object.insert("output_format".into(), Value::String(format.clone()));
    }
    if let Some(bitrate) = overrides.mp3_bitrate {
        validate_mp3_bitrate(bitrate)?;
        object.insert("mp3_bitrate".into(), Value::from(bitrate));
    }
    if let Some(extra) = overrides.extend_seconds {
        if !extra.is_finite() || extra <= 0.0 {
            return Err("extend_seconds must be a positive number of seconds".into());
        }
        let frames = tokens.split(',').filter(|token| !token.trim().is_empty()).count();
        let total = frames as f64 / SEMANTIC_FRAMES_PER_SECOND + extra;
        if total > LONGEST_SONG_SECONDS {
            return Err(format!("the song would last {total:.0} seconds; songs go up to {LONGEST_SONG_SECONDS:.0}"));
        }
        object.insert("continue_semantic_tokens".into(), Value::Bool(true));
        object.insert("duration".into(), Value::from(total));
        let budget = (total * SEMANTIC_FRAMES_PER_SECOND).ceil() as u64;
        if total > CHECKPOINT_SONG_SECONDS {
            let sampling = object.entry("semantic_sampling").or_insert_with(|| serde_json::json!({}));
            if !sampling.is_object() {
                *sampling = serde_json::json!({});
            }
            sampling["max_tokens"] = Value::from(budget);
        }
    }
    // A replay is one song by construction; the engine ignores the counter
    // but the stored provenance should not claim a batch.
    object.insert("lm_batch_size".into(), Value::from(1));
    Ok(replay)
}

/// Covers and karaoke timings follow every finished track, local or cloud. A
/// track the cloud drew no cover for wears the look's placeholder.
fn after_import(state: &AppState, song_id: &str) {
    let (cover_state, cover_song) = (state.clone(), song_id.to_owned());
    let (timing_state, timing_song) = (state.clone(), song_id.to_owned());
    tokio::spawn(async move {
        draw_cover_for(cover_state.clone(), cover_song.clone()).await;
        if let Err(error) = pin_placeholder(&cover_state, &cover_song).await {
            eprintln!("[ERROR] no placeholder cover for {cover_song}: {error:#}");
        }
    });
    tokio::spawn(async move { time_lyrics_for(timing_state, timing_song).await });
}

/// The jobs still in flight, oldest first, so a reloaded window can show them.
/// Jobs that ended without a song, stopped or failed, newest first. They stay
/// in the window, across restarts, until the person removes them.
async fn list_ended_music_jobs(State(state): State<AppState>) -> Json<Vec<MusicJob>> {
    let mut ended: Vec<MusicJob> = state
        .jobs
        .read()
        .await
        .values()
        .filter(|job| matches!(job.status, MusicJobStatus::Failed | MusicJobStatus::Cancelled))
        .cloned()
        .collect();
    ended.sort_by(|a, b| b.submitted_at.cmp(&a.submitted_at));
    Json(ended)
}

/// Removes a stopped or failed job for good; a running one is stopped first.
async fn dismiss_music_job(State(state): State<AppState>, Path(job_id): Path<String>) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    {
        let mut jobs = state.jobs.write().await;
        match jobs.get(&job_id).map(|job| matches!(job.status, MusicJobStatus::Failed | MusicJobStatus::Cancelled)) {
            None => return Err(api_error(StatusCode::NOT_FOUND, "Music job was not found.".into())),
            Some(false) => return Err(api_error(StatusCode::CONFLICT, "The job is still running; stop it first.".into())),
            Some(true) => {
                jobs.remove(&job_id);
            }
        }
    }
    if let Err(error) = state.library.forget_music_job(&job_id) {
        eprintln!("[ERROR] the job could not be removed from the library: {error:#}");
    }
    mcp::announce("jobs");
    Ok(StatusCode::NO_CONTENT)
}

async fn list_active_music_jobs(State(state): State<AppState>) -> Json<Vec<MusicJob>> {
    let mut active: Vec<MusicJob> = state
        .jobs
        .read()
        .await
        .values()
        .filter(|job| matches!(job.status, MusicJobStatus::Queued | MusicJobStatus::Running))
        .cloned()
        .collect();
    // the engine's ids are random, so the order is when each came in
    active.sort_by_key(|job| job.submitted_at);
    Json(active)
}

async fn music_job_status(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<MusicJob>, (StatusCode, Json<ApiError>)> {
    state
        .jobs
        .read()
        .await
        .get(&job_id)
        .cloned()
        .map(Json)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Music job was not found.".into()))
}

#[derive(Debug, Deserialize)]
struct ComfyExportQuery {
    /// Strength of the planner half, folded into the file; 1 when left out.
    ar: Option<f32>,
    /// Strength of the sound half.
    nar: Option<f32>,
}

/// A trained adapter as one LoRA file for ComfyUI's native YuE2, served for
/// the page to save where the user says.
async fn export_adapter_comfyui(
    State(state): State<AppState>,
    Path(id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<ComfyExportQuery>,
) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let work = tempfile::tempdir().map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let out = work.path().join("comfyui.safetensors");
    export_comfyui_to(&state, &id, query.ar, query.nar, out.clone()).await?;
    let exported = std::fs::read(&out).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(axum::response::Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, exported.len())
        .body(axum::body::Body::from(exported))
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?)
}

/// Writes an installed adapter as one ComfyUI file at `out`; returns its size.
async fn export_comfyui_to(state: &AppState, id: &str, ar: Option<f32>, nar: Option<f32>, out: PathBuf) -> Result<u64, (StatusCode, Json<ApiError>)> {
    if !state.adapters.exists(id) || id.contains(['/', '\\']) || id.contains("..") {
        return Err(api_error(StatusCode::NOT_FOUND, format!("no adapter {id}")));
    }
    let folder = state.adapters.root().join(id);
    let meta: Value = std::fs::read(folder.join("adapter.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or(Value::Null);
    let name = meta.get("name").and_then(Value::as_str).unwrap_or(id).to_string();
    let trigger = meta.get("trigger").and_then(Value::as_str).map(str::to_string);
    let (ar_strength, nar_strength) = (ar.unwrap_or(1.0), nar.unwrap_or(1.0));
    tokio::task::spawn_blocking(move || -> anyhow::Result<u64> {
        let (mut ar, mut nar) = (None, None);
        for entry in std::fs::read_dir(&folder)? {
            let path = entry?.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("safetensors") {
                continue;
            }
            match comfy_export::yue2_half(&path)? {
                Some(true) => nar = Some(path),
                Some(false) => ar = Some(path),
                None => {}
            }
        }
        if ar.is_none() && nar.is_none() {
            anyhow::bail!("this adapter is not in the studio's own format, so there is nothing to convert: a LoRA downloaded for ComfyUI loads there as it is");
        }
        comfy_export::export_yue2(ar.as_deref(), nar.as_deref(), ar_strength, nar_strength, &out, &name, trigger.as_deref())?;
        Ok(std::fs::metadata(&out)?.len())
    })
    .await
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| api_error(StatusCode::BAD_REQUEST, format!("{error:#}")))
}

#[derive(Debug, Deserialize)]
struct ComfySaveRequest {
    /// Where the file goes, a full path ending in .safetensors.
    path: String,
    ar: Option<f32>,
    nar: Option<f32>,
}

/// The ComfyUI file written where an agent says, for it has no Save dialog.
/// A file already there is refused, not replaced: the path could name a model.
async fn save_adapter_comfyui(State(state): State<AppState>, Path(id): Path<String>, Json(request): Json<ComfySaveRequest>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let target = PathBuf::from(request.path.trim());
    if !target.is_absolute() || target.extension().and_then(|extension| extension.to_str()) != Some("safetensors") {
        return Err(api_error(StatusCode::BAD_REQUEST, "path must be a full path ending in .safetensors".into()));
    }
    if target.exists() {
        return Err(api_error(StatusCode::CONFLICT, format!("{} already exists; give a new file name", target.display())));
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| api_error(StatusCode::BAD_REQUEST, format!("create {}: {error}", parent.display())))?;
    }
    // written beside it and renamed once whole, so a failed export leaves no half file
    let partial = target.with_extension("safetensors.part");
    let bytes = match export_comfyui_to(&state, &id, request.ar, request.nar, partial.clone()).await {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = std::fs::remove_file(&partial);
            return Err(error);
        }
    };
    std::fs::rename(&partial, &target).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("rename to {}: {error}", target.display())))?;
    Ok(Json(serde_json::json!({ "path": target.display().to_string(), "bytes": bytes })))
}

/// Songs appended to a playlist, each once.
fn add_to_playlist(library: &library::Library, playlist_id: &str, songs: impl Iterator<Item = String>) -> anyhow::Result<()> {
    let playlist = library.get_playlist(playlist_id)?.with_context(|| format!("no playlist {playlist_id}"))?;
    let mut song_ids = playlist.song_ids;
    for song in songs {
        if !song_ids.contains(&song) {
            song_ids.push(song);
        }
    }
    library.update_playlist(playlist_id, library::PlaylistInput { name: playlist.name, description: playlist.description, song_ids })?;
    Ok(())
}

/// Follows one engine job to its end and imports what it made.
///
/// The service owns this, not the window: a track finished while the
/// interface was reloading, closed or on another page still lands in the
/// library, and only one task ever imports a result.
fn spawn_job_watcher(state: AppState, job_id: String) {
    tokio::spawn(async move {
        follow_job(&state, &job_id).await;
        // however it ended, it is no longer one the studio's closing could cut off
        let ended = state.jobs.read().await.get(&job_id).map(|job| (job_status_name(&job.status), job.message.clone()));
        if let Some((status, message)) = ended {
            // a song that reached the library needs no record of its request any more
            let kept = if status == "completed" { state.library.forget_music_job(&job_id) } else { state.library.set_music_job_status(&job_id, status, &message) };
            if let Err(error) = kept {
                eprintln!("[ERROR] the song's state could not be kept: {error:#}");
            }
        }
    });
}

async fn follow_job(state: &AppState, job_id: &str) {
    let (state, job_id) = (state.clone(), job_id.to_string());
    {
        let mut unreachable = 0u32;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
            let Some(existing) = state.jobs.read().await.get(&job_id).cloned() else { return };
            if matches!(existing.status, MusicJobStatus::Completed | MusicJobStatus::Failed | MusicJobStatus::Cancelled) {
                return;
            }
            let remote = match state.music_server.job(&job_id).await {
                Ok(remote) => {
                    unreachable = 0;
                    remote
                }
                Err(error) => {
                    // The engine restarting drops its job table; a few missed
                    // polls are a restart, a minute of them is a lost job. A
                    // job the engine answers it does not know was lost with
                    // the process that had it, and the log of that process
                    // says why.
                    let forgotten = error.to_string().contains("404");
                    unreachable += 1;
                    if forgotten || unreachable >= 60 {
                        if let Some(job) = state.jobs.write().await.get_mut(&job_id) {
                            job.status = MusicJobStatus::Failed;
                            job.phase = MusicJobPhase::Failed;
                            job.message = if forgotten { lost_job_reason(&previous_run_log()) } else { format!("The engine stopped answering about this job: {error}") };
                        }
                        return;
                    }
                    continue;
                }
            };
            if remote.status == "done" {
                let imported = import_completed_result(&state, &existing, &job_id).await;
                let mut jobs = state.jobs.write().await;
                let Some(job) = jobs.get_mut(&job_id) else { return };
                if matches!(job.status, MusicJobStatus::Cancelled) { return; }
                match imported {
                    Ok(songs) => {
                        job.status = MusicJobStatus::Completed;
                        job.phase = MusicJobPhase::Completed;
                        job.song = songs.first().cloned();
                        job.message = "The engine finished this job and its tracks were imported into the library.".into();
                        if let Some(playlist) = existing.playlist_id.as_deref() {
                            if let Err(error) = add_to_playlist(&state.library, playlist, songs.iter().map(|song| song.id.clone())) {
                                job.message = format!("{} They could not be added to the playlist: {error:#}", job.message);
                            }
                        }
                        job.songs = songs;
                    }
                    Err(error) => {
                        job.status = MusicJobStatus::Failed;
                        job.phase = MusicJobPhase::Failed;
                        job.message = format!("The engine finished the job, but the studio could not import its result: {error}");
                    }
                }
                return;
            }
            // a stop asked for while the engine was being polled stays a stop
            let failure = (remote.status == "failed").then(|| engine_failure_reason(&state, &job_id)).flatten();
            if let Some(job) = state.jobs.write().await.get_mut(&job_id).filter(|job| !matches!(job.status, MusicJobStatus::Cancelled)) {
                apply_remote_status(job, &remote.status, failure);
            }
        }
    }
}

async fn import_completed_result(state: &AppState, job: &MusicJob, job_id: &str) -> anyhow::Result<Vec<CompletedSong>> {
    let result = state.music_server.result(job_id).await?;
    let tracks = engine_result::parse_multipart_result(&result.content_type, &result.body)?;
    let profile_id = state.selected_profile_id.read().await.clone();
    // The engine's defaults fill whatever the request left out, so a stored
    // track records every value it was made with, not only the ones typed.
    let engine_defaults = state.music_server.props().await.ok().and_then(|props| props.get("defaults").cloned());
    let count = tracks.len();
    let mut imported = Vec::with_capacity(count);
    for (index, track) in tracks.into_iter().enumerate() {
        let mut replay = track.replay_request;
        let style = replay.get("style").and_then(Value::as_str).unwrap_or_default().to_owned();
        let lyrics = replay.get("lyrics").and_then(Value::as_str).unwrap_or_default().to_owned();
        let semantic_tokens = replay
            .get("semantic_tokens")
            .filter(|value| value.as_str().is_some_and(|value| !value.is_empty()))
            .context("the engine returned a track without its semantic stream")?
            .clone();
        // The replay request is sparse: fields at their default are omitted.
        // Start from what was submitted and let the per-track values - the
        // seeds it consumed, the score it wrote - win over it.
        let mut generation_settings = job.generation_settings.clone();
        match (generation_settings.as_object_mut(), replay.as_object()) {
            (Some(target), Some(source)) => {
                for (key, value) in source {
                    target.insert(key.clone(), value.clone());
                }
            }
            _ => generation_settings = replay.clone(),
        }
        let settings = generation_settings.as_object_mut().context("generation settings are not a JSON object")?;
        if let Some(Value::Object(defaults)) = &engine_defaults {
            for (key, value) in defaults {
                if !settings.contains_key(key) && !matches!(key.as_str(), "semantic_tokens" | "lm_seed" | "seed") {
                    settings.insert(key.clone(), value.clone());
                }
            }
        }
        settings.remove("semantic_tokens");
        if let Some(edit) = job.generation_settings.get("score_edit") {
            if let (Some(abc), Some(words)) = (settings.get("abc").and_then(Value::as_str), edit.get("words").and_then(Value::as_str)) {
                let marked = score::edits::attach(abc, Some(words), edit.get("keep").and_then(Value::as_bool).unwrap_or(false));
                settings.insert("abc".into(), Value::String(marked));
            }
        }
        settings.insert("lm_batch_size".into(), Value::from(1));
        settings.insert("synth_batch_size".into(), Value::from(1));
        let mut extension = engine_result::audio_extension(&track.audio_content_type)?;
        let mut audio = track.audio;
        let vocals_only = job.generation_settings.get("vocals_only").and_then(Value::as_bool) == Some(true);
        let format = output_format(&job.generation_settings).to_string();
        // a track that is not encoded below is checked on its own pass; vocals are checked on the mix
        if extension == "wav" && (vocals_only || format == "wav32") {
            let shared = std::sync::Arc::new(audio);
            let probe = shared.clone();
            let problem = tokio::task::spawn_blocking(move || audio_pcm::output_problem(probe, "wav")).await.context("the output check stopped")??;
            if let Some(problem) = problem {
                anyhow::bail!("The engine returned {problem} instead of a song, so nothing was kept. Make it again; if it repeats, the engine log has the cause.");
            }
            audio = std::sync::Arc::try_unwrap(shared).map_err(|_| anyhow::anyhow!("the engine's track is still held by its check"))?;
        }
        let mut vocals_used_gpu = None;
        if vocals_only {
            let model = state.separator.model_path();
            let config = state.separation_config.read().await.clone();
            let card = state.lyrics_sync.onnx_card(config.runtime)?.filter(|card| separates_on(*card));
            let progress_state = state.clone();
            let progress_id = job_id.to_string();
            {
                let mut jobs = state.jobs.write().await;
                let active = jobs.get_mut(job_id).context("the vocals-only job no longer exists")?;
                if matches!(active.status, MusicJobStatus::Cancelled) { anyhow::bail!("Vocals extraction cancelled"); }
                active.status = MusicJobStatus::Running;
                active.phase = MusicJobPhase::ExtractingVocals;
                active.message = "Loading the vocals separator.".into();
            }
            let (voice_audio, used_gpu) = tokio::task::spawn_blocking(move || -> anyhow::Result<(Vec<u8>, bool)> {
                let temporary = tempfile::tempdir()?;
                let input = temporary.path().join(format!("mix.{extension}"));
                std::fs::write(&input, audio)?;
                let samples = audio_pcm::decode_stereo_44k(&input)?;
                let mut loaded = separation::load(&model, card)?;
                if card.is_some() && !loaded.used_gpu { anyhow::bail!("The requested GPU separator could not load its execution provider; vocals extraction was stopped instead of running on CPU"); }
                let separated = separation::separate_with_checked(&mut loaded, &samples, separation::STEMS.len(), config.sane_overlap(), |fraction| {
                    let mut jobs = progress_state.jobs.blocking_write();
                    let active = jobs.get_mut(&progress_id).context("the vocals-only job no longer exists")?;
                    if matches!(active.status, MusicJobStatus::Cancelled) { anyhow::bail!("Vocals extraction cancelled"); }
                    active.status = MusicJobStatus::Running;
                    active.phase = MusicJobPhase::ExtractingVocals;
                    active.message = format!("Extracting vocals: {:.0}%", fraction * 100.0);
                    Ok(())
                })?;
                let voice = separated.stems.into_iter().find(|stem| stem.name == "vocals").context("the separator returned no vocals")?;
                let stereo = audio_post::Stereo {
                    rate: 44_100,
                    left: voice.samples.iter().step_by(2).copied().collect(),
                    right: voice.samples.iter().skip(1).step_by(2).copied().collect(),
                };
                let output = temporary.path().join("vocals.wav");
                audio_pcm::write_wav_f32(&output, &stereo)?;
                Ok((std::fs::read(output).context("read the extracted vocals")?, separated.used_gpu))
            }).await.context("the vocals separator stopped")??;
            audio = voice_audio;
            vocals_used_gpu = Some(used_gpu);
            extension = "wav";
            replay["vocals_only"] = Value::Bool(true);
        }
        // the engine's float output is kept at the level it came: lossless FLAC unless MP3 was asked for
        if extension == "wav" {
            let kbps = job.generation_settings.get("mp3_bitrate").and_then(Value::as_u64).map_or(DEFAULT_MP3_KBPS, |value| value as u32);
            if format != "wav32" {
                let target = format.clone();
                audio = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<u8>> {
                    let stereo = audio_pcm::decode_stereo_bytes(audio, "wav")?;
                    if !vocals_only {
                        if let Some(problem) = audio_pcm::stereo_problem(&stereo) {
                            anyhow::bail!("The engine returned {problem} instead of a song, so nothing was kept. Make it again; if it repeats, the engine log has the cause.");
                        }
                    }
                    if target == "mp3" { audio_post::encode::mp3(&stereo, kbps) } else { audio_post::encode::flac(&stereo) }
                })
                .await
                .context("the encoder stopped")??;
                extension = if format == "mp3" { "mp3" } else { "flac" };
            }
            for record in [&mut *settings, replay.as_object_mut().context("the replay request is not a JSON object")?] {
                record.insert("output_format".into(), Value::from(if format == "mp3" { "mp3" } else if format == "wav32" { "wav32" } else { "flac" }));
                if format == "mp3" {
                    // the track says what it is: an MP3 at the rate LAME wrote
                    record.insert("mp3_bitrate".into(), Value::from(audio_post::encode::mp3_bitrate(kbps)));
                } else {
                    record.remove("mp3_bitrate");
                }
            }
        }
        let metadata = serde_json::json!({
            "duration_seconds": library::audio_duration_seconds(
                &audio,
                extension,
                replay.get("mp3_bitrate").and_then(Value::as_u64).map(|value| value as u32),
            ),
            "seed": replay.get("seed"),
            "lm_seed": replay.get("lm_seed"),
            "cot": generation_settings.get("cot"),
            "output_format": generation_settings.get("output_format"),
            "cover_prompt": job.cover_prompt.clone(),
            "derived": job.derived.clone(),
            // the window showing this job's card hands the card to the song
            "job_id": job_id,
            "vocals_extraction": vocals_used_gpu.map(|used_gpu| serde_json::json!({ "used_gpu": used_gpu, "model": separation::MODEL.id })),
        });
        // Several tracks from one request share its name; number them so the
        // library can tell the takes apart.
        let title = match (&job.title, count) {
            (Some(title), count) if count > 1 => Some(format!("{title} ({})", index + 1)),
            (title, _) => title.clone(),
        };
        if state.jobs.read().await.get(job_id).is_some_and(|active| matches!(active.status, MusicJobStatus::Cancelled)) {
            anyhow::bail!("The job was cancelled before its audio reached the library");
        }
        let imported_song = state.library.import_generated_song(library::GeneratedSongInput {
            title,
            metadata,
            caption: style,
            lyrics,
            generation_settings,
            replay_request: Some(replay),
            audio_codes: Some(semantic_tokens),
            engine_id: job.engine_id.clone(),
            profile_id: profile_id.clone(),
            source: "local_generation".into(),
            audio_extension: extension,
            audio,
        })?;
        let audio_url = format!("/v1/library/media/{}", imported_song.song.id);
        tag_stored_song(state, &imported_song.song.id).await;
        after_import(state, &imported_song.song.id);
        imported.push(CompletedSong { id: imported_song.song.id.clone(), song: imported_song.song, audio_url });
    }
    Ok(imported)
}

async fn cancel_music_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<MusicJob>, (StatusCode, Json<ApiError>)> {
    let existing = state
        .jobs
        .read()
        .await
        .get(&job_id)
        .cloned()
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Music job was not found.".into()))?;
    if existing.engine_id != PRIMARY_MUSIC_ENGINE_ID {
        return Err(api_error(
            StatusCode::NOT_IMPLEMENTED,
            format!("The selected engine '{}' has no cancel adapter.", existing.engine_id),
        ));
    }
    if matches!(existing.phase, MusicJobPhase::ExtractingVocals) {
        let mut jobs = state.jobs.write().await;
        let job = jobs.get_mut(&job_id).ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Music job was not found.".into()))?;
        job.status = MusicJobStatus::Cancelled;
        job.phase = MusicJobPhase::Cancelled;
        job.message = "Vocals extraction cancelled.".into();
        // kept at once, as an engine stop is
        if let Err(error) = state.library.set_music_job_status(&job_id, job_status_name(&job.status), &job.message) {
            eprintln!("[ERROR] the stopped song's state could not be kept: {error:#}");
        }
        return Ok(Json(job.clone()));
    }
    let remote = state.music_server.cancel(&job_id).await.map_err(|error| {
        api_error(StatusCode::SERVICE_UNAVAILABLE, format!("the engine did not accept the cancel: {error}"))
    })?;
    let failure = (remote.status == "failed").then(|| engine_failure_reason(&state, &job_id)).flatten();
    let mut jobs = state.jobs.write().await;
    let job = jobs
        .get_mut(&job_id)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Music job was not found.".into()))?;
    apply_remote_status(job, &remote.status, failure);
    stop_on_request(job);
    // kept at once: a studio closed right after the stop must not start the song again
    if let Err(error) = state.library.set_music_job_status(&job_id, job_status_name(&job.status), &job.message) {
        eprintln!("[ERROR] the stopped song's state could not be kept: {error:#}");
    }
    Ok(Json(job.clone()))
}

/// The engine ends a cancelled job at its next checkpoint, and a job that has
/// not started yet only when its turn comes; until then it still reports
/// "running". The person asked for the stop, so the job stops being shown as
/// running now - the windows drop it - instead of an answer that looks as if
/// nothing happened. The watcher ends with the job, so nothing is imported.
fn stop_on_request(job: &mut MusicJob) {
    if matches!(job.status, MusicJobStatus::Queued | MusicJobStatus::Running) {
        job.status = MusicJobStatus::Cancelled;
        job.dispatch = MusicJobDispatch::Cancelled;
        job.phase = MusicJobPhase::Cancelled;
        job.message = "Stopped; the engine ends it at its next checkpoint.".into();
    }
}

/// The engine's own defaults, version and the weights it serves: the source
/// of truth for every placeholder in the request form.
async fn local_music_model_catalog(
    State(state): State<AppState>,
) -> Result<Json<LocalMusicModelCatalog>, (StatusCode, Json<ApiError>)> {
    let engine_id = selected_local_music_engine(&*state.configuration.read().await).ok_or_else(|| {
        api_error(StatusCode::SERVICE_UNAVAILABLE, "No local music engine is selected in the capability configuration.".into())
    })?;
    if engine_id != PRIMARY_MUSIC_ENGINE_ID {
        return Err(api_error(StatusCode::NOT_IMPLEMENTED, format!("The selected engine '{engine_id}' has no catalog adapter.")));
    }
    let mut catalog = state.music_server.props().await.map_err(|error| {
        api_error(StatusCode::SERVICE_UNAVAILABLE, format!("the engine catalog is unavailable: {error}"))
    })?;
    let transcriber = {
        let supervisor = state.engine.lock().await;
        supervisor.as_ref().and_then(|engine| engine.config().models.transcriber.clone())
    };
    if let Value::Object(fields) = &mut catalog {
        fields.insert("transcriber".into(), transcriber.map(|path| Value::String(path.display().to_string())).unwrap_or(Value::Null));
        fields.insert("max_batch".into(), Value::from(state.engine_options.read().await.effective_max_batch()));
    }
    Ok(Json(LocalMusicModelCatalog { engine_id, catalog }))
}

/// A job whose answer is a score: a transcription of a recording, or a
/// composition from a style and lyrics.
#[derive(Debug, Serialize)]
struct ScoreJob {
    id: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    abc: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    plans: Vec<ScorePlan>,
    /// The token seed a composition drew its score with.
    #[serde(skip_serializing_if = "Option::is_none")]
    lm_seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl ScoreJob {
    fn running(id: String) -> Self {
        Self { id, status: "running".into(), abc: None, plans: vec![], lm_seed: None, error: None }
    }
}

#[derive(Debug, Serialize)]
struct ScorePlan { abc: String, lm_seed: Option<i64> }

#[derive(Debug, Deserialize)]
struct ComposeScoreRequest {
    #[serde(default)]
    style: String,
    #[serde(default)]
    lyrics: String,
    #[serde(default)]
    cot: Option<String>,
    #[serde(default)]
    lm_seed: Option<i64>,
    #[serde(default)]
    abc_sampling: Option<SamplingPreset>,
    #[serde(default)]
    harmony: Option<harmony::Harmony>,
    #[serde(default)]
    lm_batch_size: Option<u32>,
}

/// The engine request that runs the planning stage and next to nothing else.
/// The score is written from the prompt alone - the duration never reaches the
/// prompt, it only caps the semantic stage - so a one second budget, one solver
/// step and one track return the same score a full song would have been sung
/// from, the equivalent of yue2.cpp's `yue-plan`.
fn compose_request_from(request: &ComposeScoreRequest) -> Result<Value, String> {
    if request.lm_batch_size.is_some_and(|count| !(1..=8).contains(&count)) { return Err("lm_batch_size must be between 1 and 8".into()); }
    if request.style.trim().is_empty() && request.lyrics.trim().is_empty() {
        return Err("write a style or lyrics: the engine needs at least one of them".into());
    }
    let cot = request.cot.as_deref().unwrap_or("full");
    if !matches!(cot, "full" | "melody") {
        return Err("a score is composed in full or melody mode".into());
    }
    let mut body = serde_json::json!({
        "style": request.style,
        "lyrics": request.lyrics.replace("\r\n", "\n"),
        "cot": cot,
        "duration": 1.0,
        "steps": 1,
        "lm_batch_size": request.lm_batch_size.unwrap_or(1),
        "synth_batch_size": 1,
        "output_format": "wav16",
    });
    if let Some(seed) = request.lm_seed.filter(|seed| *seed >= 0) {
        body["lm_seed"] = Value::from(seed);
    }
    if let Some(sampling) = &request.abc_sampling {
        sampling.validate("abc_sampling")?;
        body["abc_sampling"] = serde_json::to_value(sampling).map_err(|error| error.to_string())?;
    }
    if let Some(harmony) = request.harmony.as_ref().filter(|harmony| harmony.active()) {
        body["harmony"] = harmony.engine_field(&request.lyrics)?;
    }
    Ok(body)
}

async fn compose_score(
    State(state): State<AppState>,
    Json(request): Json<ComposeScoreRequest>,
) -> Result<(StatusCode, Json<ScoreJob>), (StatusCode, Json<ApiError>)> {
    let maximum = state.engine_options.read().await.effective_max_batch();
    if request.lm_batch_size.unwrap_or(1) > maximum { return Err(api_error(StatusCode::BAD_REQUEST, format!("lm_batch_size exceeds the configured song limit {maximum}"))); }
    let body = compose_request_from(&request).map_err(|error| api_error(StatusCode::BAD_REQUEST, error))?;
    let remote = state
        .music_server
        .submit(body)
        .await
        .map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, format!("the engine refused the composition: {error}")))?;
    Ok((StatusCode::ACCEPTED, Json(ScoreJob::running(remote.id))))
}

/// Reads a recording into the ABC score a cover takes as its `abc`. The audio
/// is either uploaded (`audio` part) or a library track (`song_id` field);
/// `melody_only` drops the chord symbols, which is what the `melody` mode wants.
async fn create_transcription(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<ScoreJob>), (StatusCode, Json<ApiError>)> {
    let transcriber_loaded = {
        let supervisor = state.engine.lock().await;
        supervisor.as_ref().map(|engine| engine.config().models.transcriber.is_some())
    };
    if transcriber_loaded == Some(false) {
        return Err(api_error(
            StatusCode::CONFLICT,
            "The running engine has no SheetSage2 transcriber. Add one to the model set in Settings - Models.".into(),
        ));
    }
    let mut audio: Option<(Vec<u8>, String)> = None;
    let mut melody_only = false;
    while let Some(field) = multipart.next_field().await.map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))? {
        match field.name().unwrap_or_default() {
            "audio" => {
                let name = field.file_name().unwrap_or("input.audio").to_owned();
                let bytes = field.bytes().await.map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
                audio = Some((bytes.to_vec(), name));
            }
            "song_id" => {
                let song_id = field.text().await.map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
                let song = state.library.get_song(song_id.trim()).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
                    .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found.".into()))?;
                let path = state.library.media_path_for_song(&song).ok_or_else(|| api_error(StatusCode::NOT_FOUND, "The track's audio is not in the library.".into()))?;
                let bytes = tokio::fs::read(&path).await.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("read {}: {error}", path.display())))?;
                let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| "input.audio".into());
                audio = Some((bytes, name));
            }
            "melody_only" => {
                let value = field.text().await.unwrap_or_default();
                melody_only = matches!(value.trim(), "1" | "true" | "yes");
            }
            _ => {}
        }
    }
    let (bytes, name) = audio.ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "Send an audio part or a song_id.".into()))?;
    if bytes.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "The audio is empty.".into()));
    }
    let remote = state
        .music_server
        .transcribe(bytes, name, melody_only)
        .await
        .map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, format!("the engine refused the transcription: {error}")))?;
    Ok((StatusCode::ACCEPTED, Json(ScoreJob::running(remote.id))))
}

/// The score a finished job carries. A transcription answers with JSON; a
/// composition with the engine's multipart result, whose replay request holds
/// the score it wrote and the seed it drew.
fn score_from_result(content_type: &str, body: &[u8]) -> Result<(String, Option<i64>), String> {
    let (abc, lm_seed) = if content_type.starts_with("multipart/") {
        let tracks = engine_result::parse_multipart_result(content_type, body).map_err(|error| error.to_string())?;
        let replay = tracks.into_iter().next().ok_or("the composition returned no track")?.replay_request;
        (replay.get("abc").and_then(Value::as_str).map(str::to_owned), replay.get("lm_seed").and_then(Value::as_i64))
    } else {
        let value: Value = serde_json::from_slice(body).map_err(|error| format!("the result is not JSON: {error}"))?;
        (value.get("abc").and_then(Value::as_str).map(str::to_owned), None)
    };
    let abc = abc.filter(|value| !value.trim().is_empty()).ok_or("the engine returned no score")?;
    Ok((abc.trim_end().to_owned(), lm_seed))
}

fn plans_from_result(content_type: &str, body: &[u8]) -> Result<Vec<ScorePlan>, String> {
    if !content_type.starts_with("multipart/") { return Ok(vec![]); }
    let tracks = engine_result::parse_multipart_result(content_type, body).map_err(|error| error.to_string())?;
    tracks.into_iter().map(|track| {
        let abc = track.replay_request.get("abc").and_then(Value::as_str).filter(|abc| !abc.trim().is_empty()).ok_or("A plan returned no score")?.trim_end().to_owned();
        Ok(ScorePlan { abc, lm_seed: track.replay_request.get("lm_seed").and_then(Value::as_i64) })
    }).collect()
}

async fn score_job_status(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<ScoreJob>, (StatusCode, Json<ApiError>)> {
    let remote = state.music_server.job(&job_id).await.map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, error.to_string()))?;
    let mut job = ScoreJob { status: remote.status.clone(), ..ScoreJob::running(job_id.clone()) };
    match remote.status.as_str() {
        "done" => {
            let result = state.music_server.result(&job_id).await.map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, error.to_string()))?;
            let (abc, lm_seed) = score_from_result(&result.content_type, &result.body).map_err(|error| api_error(StatusCode::BAD_GATEWAY, error))?;
            job.abc = Some(abc);
            job.lm_seed = lm_seed;
            job.plans = plans_from_result(&result.content_type, &result.body).map_err(|error| api_error(StatusCode::BAD_GATEWAY, error))?;
        }
        "failed" => job.error = Some(engine_failure_reason(&state, &job_id).unwrap_or_else(|| "The engine could not write this score.".into())),
        _ => {}
    }
    Ok(Json(job))
}

async fn cancel_score_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<ScoreJob>, (StatusCode, Json<ApiError>)> {
    let remote = state.music_server.cancel(&job_id).await.map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, error.to_string()))?;
    Ok(Json(ScoreJob { status: remote.status, ..ScoreJob::running(job_id) }))
}

impl EngineClient {
    fn from_environment() -> Self {
        let base_url = env::var("YUE_ENGINE_BASE_URL")
            .unwrap_or_else(|_| format!("http://127.0.0.1:{}", music_engine::yue_server::DEFAULT_PORT))
            .trim_end_matches('/')
            .to_owned();
        // the engine's server drops a connection idle for 5 s; a pooled one can be
        // taken as it closes and the request is lost, so every request opens its own
        let http = net::builder().pool_max_idle_per_host(0).build().expect("an HTTP client with a proxy callback builds");
        Self { base_url, http, health_cache: Arc::new(std::sync::Mutex::new(None)) }
    }

    async fn health(&self) -> bool {
        const FRESH: std::time::Duration = std::time::Duration::from_millis(1500);
        if let Some((at, up)) = *self.health_cache.lock().expect("health cache") {
            if at.elapsed() < FRESH {
                return up;
            }
        }
        let up = self
            .http
            .get(self.url("/health"))
            .timeout(std::time::Duration::from_millis(500))
            .send()
            .await
            .map(|response| response.status().is_success())
            .unwrap_or(false);
        *self.health_cache.lock().expect("health cache") = Some((std::time::Instant::now(), up));
        up
    }

    async fn props(&self) -> anyhow::Result<Value> {
        self.json_response(self.http.get(self.url("/props")).send().await?).await
    }

    async fn submit(&self, request: Value) -> anyhow::Result<EngineSubmitResponse> {
        self.json_response(self.send(self.http.post(self.url("/synth")).json(&request), false).await?).await
    }

    /// Sends a request to the engine on loopback, again when the connection
    /// drops under it: a refused connection on any method, since nothing was
    /// sent, and a reset mid-request only for a read. Three tries, a short
    /// pause between. A blip that polling rode out made a submission fail.
    async fn send(&self, request: reqwest::RequestBuilder, read: bool) -> anyhow::Result<reqwest::Response> {
        let dropped = |error: &reqwest::Error| {
            let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
            while let Some(cause) = source {
                if let Some(io) = cause.downcast_ref::<std::io::Error>() {
                    return matches!(io.kind(), std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted | std::io::ErrorKind::BrokenPipe);
                }
                source = cause.source();
            }
            false
        };
        let mut attempt = 0u64;
        loop {
            let Some(copy) = request.try_clone() else { return Ok(request.send().await?) };
            match copy.send().await {
                Ok(response) => return Ok(response),
                Err(error) if attempt < 2 && (error.is_connect() || (read && dropped(&error))) => {
                    attempt += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(300 * attempt)).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    async fn transcribe(&self, audio: Vec<u8>, filename: String, melody_only: bool) -> anyhow::Result<EngineSubmitResponse> {
        let mut form = reqwest::multipart::Form::new().part("audio", reqwest::multipart::Part::bytes(audio).file_name(filename));
        if melody_only {
            form = form.text("melody_only", "1");
        }
        self.json_response(self.http.post(self.url("/transcribe")).multipart(form).send().await?).await
    }

    async fn job(&self, job_id: &str) -> anyhow::Result<EngineJobResponse> {
        self.json_response(self.send(self.http.get(self.url("/job")).query(&[("id", job_id)]), true).await?).await
    }

    async fn cancel(&self, job_id: &str) -> anyhow::Result<EngineJobResponse> {
        self.json_response(self.send(self.http.post(self.url("/job")).query(&[("id", job_id), ("cancel", "1")]), false).await?).await
    }

    async fn result(&self, job_id: &str) -> anyhow::Result<EngineResultResponse> {
        let response = self.send(self.http.get(self.url("/job")).query(&[("id", job_id), ("result", "1")]), true).await?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("yue-server returned {status}: {}", response.text().await?);
        }
        let content_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).unwrap_or("").to_owned();
        Ok(EngineResultResponse { content_type, body: response.bytes().await?.to_vec() })
    }

    async fn json_response<T: serde::de::DeserializeOwned>(&self, response: reqwest::Response) -> anyhow::Result<T> {
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            let message = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|value| value.get("error").and_then(Value::as_str).map(str::to_owned))
                .unwrap_or(body);
            anyhow::bail!("yue-server returned {status}: {message}");
        }
        Ok(serde_json::from_str(&body)?)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }
}

fn initial_configuration() -> StudioConfiguration {
    let mut configuration = StudioConfiguration::default();
    if let Some(selection) = configuration
        .selections
        .iter_mut()
        .find(|selection| selection.capability == Capability::MusicGeneration)
    {
        selection.mode = ExecutionMode::Local;
        selection.local_engine = Some(PRIMARY_MUSIC_ENGINE_ID.into());
        selection.cloud_model = None;
    }
    configuration
}

/// Settings may name local engines this build does not ship; any local engine
/// not declared by `capability_engines` is dropped so the interface never
/// offers a provider nobody can serve.
fn sanitize_persisted_configuration(mut configuration: StudioConfiguration) -> StudioConfiguration {
    let declared = capability_engines(false);
    for selection in &mut configuration.selections {
        let engine_serves_capability = selection.local_engine.as_deref().is_some_and(|engine_id| {
            declared.iter().any(|engine| {
                engine.id == engine_id
                    && engine.execution_mode == ExecutionMode::Local
                    && engine.capabilities.contains(&selection.capability)
            })
        });
        if engine_serves_capability {
            continue;
        }
        selection.local_engine = None;
        if selection.mode == ExecutionMode::Local {
            selection.mode = ExecutionMode::OpenRouter;
        }
    }
    configuration
}

fn selected_local_music_engine(configuration: &StudioConfiguration) -> Option<String> {
    configuration
        .selections
        .iter()
        .find(|selection| selection.capability == Capability::MusicGeneration && selection.mode == ExecutionMode::Local)
        .and_then(|selection| selection.local_engine.clone())
}

fn validate_output_format(format: &str) -> Result<(), String> {
    if matches!(format, "flac" | "mp3" | "wav32") {
        Ok(())
    } else {
        Err("output_format must be one of: flac, mp3, wav32".into())
    }
}

fn validate_mp3_bitrate(bitrate: u32) -> Result<(), String> {
    if (32..=320).contains(&bitrate) {
        Ok(())
    } else {
        Err("mp3_bitrate must be between 32 and 320 kbps".into())
    }
}

fn validate_semantic_tokens(tokens: &str) -> Result<(), String> {
    let invalid = tokens
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .find(|value| value.parse::<u32>().map(|code| code >= 32768).unwrap_or(true));
    match invalid {
        Some(value) => Err(format!("semantic_tokens must be comma-separated codes below 32768; found `{value}`")),
        None => Ok(()),
    }
}

/// The bitrate a track is encoded at when the request names none.
const DEFAULT_MP3_KBPS: u32 = 320;

/// The format a track is kept in: what was asked for, else lossless FLAC.
fn output_format(settings: &Value) -> &str {
    settings.get("output_format").and_then(Value::as_str).unwrap_or("flac")
}

/// A seed for a request that leaves it to chance, drawn here as a 32-bit
/// number: the engine's own draw is 64-bit, more than the page's JavaScript
/// numbers hold exactly, so a song made from it could not be made again.
fn drawn_seed() -> i64 {
    i64::from(uuid::Uuid::now_v7().as_u128() as u32)
}

/// What the engine is asked for: always its unencoded 32-bit float output, the
/// model's own rate, precision and level, so a track is encoded once, here, and
/// nothing changes its loudness on the way.
fn engine_submission(body: &Value) -> Value {
    let mut engine = body.clone();
    if let Some(fields) = engine.as_object_mut() { fields.remove("vocals_only"); fields.remove("score_edit"); }
    if let Some(fields) = engine.as_object_mut() {
        fields.insert("output_format".into(), Value::from("wav32"));
        fields.remove("mp3_bitrate");
        fields.remove("peak_clip");
    }
    engine
}

/// The decoder companion's strength when the request leaves it unsaid: the
/// checkpoint alone, as the model's authors and every other frontend render
/// it, except under a LoRA of the studio's own trainer, which was trained over
/// the companion and sounds as trained only with it.
fn companion_default(adapters: &adapters::AdapterLibrary, request: &CreateMusicJobRequest) -> f64 {
    let over_companion = request.adapters.iter().any(|adapter| adapter.scales.values().any(|scale| *scale != 0.0) && adapters.trained_over_companion(&adapter.id));
    if over_companion { 1.0 } else { 0.0 }
}

/// The style a request's LoRA was trained to hear: an adapter whose weights say
/// its trigger lived inside the style sentence is sent that sentence, not the
/// style as typed. One sentence carries one trigger, the one the style opens with.
fn trained_style(adapters: &adapters::AdapterLibrary, request: &CreateMusicJobRequest) -> Option<String> {
    let triggers: Vec<String> = request
        .adapters
        .iter()
        .filter(|adapter| adapter.scales.values().any(|scale| *scale != 0.0) && adapters.trained_in_sentence(&adapter.id))
        .filter_map(|adapter| adapters.trigger_of(&adapter.id))
        .collect();
    let trigger = triggers.iter().find(|trigger| adapters::opens_with(&request.style, trigger)).or(triggers.first())?;
    Some(adapters::upstream_style(&request.style, trigger))
}

/// The checkpoint's own semantic budget, 9000 frames at 25 a second.
const CHECKPOINT_SONG_SECONDS: f64 = 360.0;
/// What the model's 24576-token context holds with a prompt and a score beside the song.
const LONGEST_SONG_SECONDS: f64 = 600.0;
const SEMANTIC_FRAMES_PER_SECOND: f64 = 25.0;
/// The checkpoint's own score budget.
const CHECKPOINT_SCORE_TOKENS: u64 = 4096;
/// What a written score takes a second: 3951 tokens for 271 s of song.
const SCORE_TOKENS_PER_SECOND: f64 = 15.0;

/// Builds the yue-server request. Only what the user set travels: an absent
/// field is the engine's protocol default, and the replay request the engine
/// returns records the values it actually used.
fn yue_request_from(request: &CreateMusicJobRequest, max_batch: u32) -> Result<Value, String> {
    let semantic_tokens = request.semantic_tokens.as_deref().map(str::trim).filter(|value| !value.is_empty());
    if request.style.trim().is_empty() && request.lyrics.trim().is_empty() && semantic_tokens.is_none() {
        return Err("write a style or lyrics: the engine needs at least one of them".into());
    }
    if let Some(cot) = request.cot.as_deref() {
        if !matches!(cot, "full" | "melody" | "off") {
            return Err("cot must be full, melody or off".into());
        }
    }
    if let Some(duration) = request.duration_seconds {
        if !duration.is_finite() || !(1.0..=LONGEST_SONG_SECONDS).contains(&duration) {
            return Err(format!("duration_seconds must be between 1 and {LONGEST_SONG_SECONDS}"));
        }
    }
    if request.steps.is_some_and(|steps| !(1..=200).contains(&steps)) {
        return Err("steps must be between 1 and 200".into());
    }
    if request.lm_batch_size.is_some_and(|size| size < 1 || size > max_batch) {
        return Err(format!("lm_batch_size must be between 1 and {max_batch}; raise the song limit in Settings - Engine for more"));
    }
    if request.synth_batch_size.is_some_and(|size| !(1..=9).contains(&size)) {
        return Err("synth_batch_size must be between 1 and 9".into());
    }
    if request.cfg_scale.is_some_and(|value| !value.is_finite() || value > 10.0) {
        return Err("cfg_scale must be a finite number up to 10".into());
    }
    if request.companion_scale.is_some_and(|value| !(0.0..=1.0).contains(&value)) {
        return Err("companion_scale must be between 0 and 1".into());
    }
    if let Some(format) = request.output_format.as_deref() {
        validate_output_format(format)?;
    }
    if let Some(bitrate) = request.mp3_bitrate {
        validate_mp3_bitrate(bitrate)?;
    }
    if let Some(tokens) = semantic_tokens {
        validate_semantic_tokens(tokens)?;
    }
    let mut body = serde_json::json!({
        "style": request.style,
        "lyrics": request.lyrics.replace("\r\n", "\n"),
    });
    if request.vocals_only {
        body["vocals_only"] = Value::Bool(true);
    }
    let step = request.transpose.unwrap_or(0);
    if let Some(source) = request.abc.as_deref() {
        let edit = score::edits::read(source);
        if let Some(words) = &edit.words { body["score_edit"] = serde_json::json!({ "words": words, "keep": edit.keep }); }
        if score::edits::mismatch(&edit, &request.style, &request.lyrics, request.cot.as_deref().unwrap_or("full")) == Some("other_words") {
            return Err("This edited score belongs to other words or style. Open the MIDI editor again or enable Keep for new words.".into());
        }
    }
    if !(-24..=24).contains(&step) { return Err("transpose must be between -24 and 24 semitones".into()); }
    if step != 0 && (semantic_tokens.is_some() || request.cot.as_deref() == Some("off")) {
        return Err("transpose needs a score sung with full or melody mode, without an existing semantic performance".into());
    }
    if let Some(abc) = request.abc.as_deref().map(|text| score::edits::read(text).score).filter(|value| !value.is_empty()) {
        let moved = score::transpose::move_score(&abc, step)?;
        body["abc"] = Value::String(format!("{moved}\n"));
    } else if step != 0 {
        return Err("compose or load a score before transposing it".into());
    }
    insert_optional(&mut body, "cot", request.cot.clone());
    insert_optional(&mut body, "duration", request.duration_seconds);
    body["lm_seed"] = Value::from(request.lm_seed.filter(|seed| *seed >= 0).unwrap_or_else(drawn_seed));
    body["seed"] = Value::from(request.seed.filter(|seed| *seed >= 0).unwrap_or_else(drawn_seed));
    insert_optional(&mut body, "steps", request.steps);
    insert_optional(&mut body, "lm_batch_size", request.lm_batch_size);
    insert_optional(&mut body, "synth_batch_size", request.synth_batch_size);
    insert_optional(&mut body, "cfg_scale", request.cfg_scale.filter(|value| *value >= 0.0));
    insert_optional(&mut body, "companion_scale", request.companion_scale);
    insert_optional(&mut body, "semantic_tokens", semantic_tokens);
    insert_optional(&mut body, "output_format", request.output_format.clone());
    insert_optional(&mut body, "mp3_bitrate", request.mp3_bitrate);
    for (key, preset) in [("abc_sampling", &request.abc_sampling), ("semantic_sampling", &request.semantic_sampling)] {
        if let Some(preset) = preset.as_ref().filter(|preset| !preset.is_empty()) {
            preset.validate(key)?;
            body[key] = serde_json::to_value(preset).map_err(|error| error.to_string())?;
        }
    }
    // the checkpoint's semantic budget stops a song at 6 minutes; a longer one asked for gets its budget
    if let Some(duration) = request.duration_seconds.filter(|duration| *duration > CHECKPOINT_SONG_SECONDS) {
        if request.semantic_sampling.as_ref().and_then(|preset| preset.max_tokens).is_none() {
            if !body["semantic_sampling"].is_object() {
                body["semantic_sampling"] = serde_json::json!({});
            }
            body["semantic_sampling"]["max_tokens"] = Value::from((duration * SEMANTIC_FRAMES_PER_SECOND).ceil() as u64);
        }
    }
    // a score the model writes for a long song outgrows the checkpoint's score budget
    let writes_score = request.cot.as_deref() != Some("off") && body.get("abc").is_none() && semantic_tokens.is_none();
    if let Some(duration) = request.duration_seconds.filter(|_| writes_score) {
        let tokens = (duration * SCORE_TOKENS_PER_SECOND).ceil() as u64;
        if tokens > CHECKPOINT_SCORE_TOKENS && request.abc_sampling.as_ref().and_then(|preset| preset.max_tokens).is_none() {
            if !body["abc_sampling"].is_object() {
                body["abc_sampling"] = serde_json::json!({});
            }
            body["abc_sampling"]["max_tokens"] = Value::from(tokens);
        }
    }
    if let Some(harmony) = request.harmony.as_ref().filter(|harmony| harmony.active()) {
        harmony.validate()?;
        if writes_score {
            body["harmony"] = harmony.engine_field(&request.lyrics)?;
        }
    }
    if !request.adapters.is_empty() {
        body["adapters"] = Value::Array(adapter_fields(&request.adapters)?);
    }
    Ok(body)
}

/// The engine's lyric schedule for a supplied score sung under the protocol's own guidance: each
/// sung section's words wait until the score reaches it, so the voice keeps to the band.
fn lyric_schedule(body: &mut Value) {
    if body.get("cot").and_then(Value::as_str) == Some("off")
        || (body.get("semantic_tokens").is_some() && body.get("continue_semantic_tokens").and_then(Value::as_bool) != Some(true))
        || body.get("cfg_scale").and_then(Value::as_f64).is_some_and(|scale| scale != 1.0)
    {
        return;
    }
    let (Some(score), Some(lyrics)) = (body.get("abc").and_then(Value::as_str), body.get("lyrics").and_then(Value::as_str)) else {
        return;
    };
    if let Some(timed) = score::schedule::schedule(score, lyrics) {
        let sections: Vec<Value> = timed.iter().map(|section| serde_json::json!({ "start_sec": section.start_sec, "lyric": [section.lyric.0, section.lyric.1] })).collect();
        body["lyric_schedule"] = serde_json::json!({ "mode": "bias", "bias": score::schedule::BIAS, "sections": sections });
    }
}

/// A score that names no section, as a tune from a MIDI file comes, is laid out for the lyrics
/// before it is sung; at an automatic length the song is stopped a little after the laid-out tune.
fn laid_out(body: &mut Value, automatic_length: bool) -> Option<Value> {
    if body.get("cot").and_then(Value::as_str) == Some("off") {
        return None;
    }
    let lyrics = body.get("lyrics").and_then(Value::as_str).unwrap_or_default();
    let laid = score::phrasing::lay(body.get("abc")?.as_str()?, lyrics)?;
    let ceiling = score::phrasing::ceiling(laid.seconds).min(LONGEST_SONG_SECONDS);
    body["abc"] = Value::String(laid.score);
    if automatic_length {
        body["duration"] = Value::from(ceiling);
    }
    Some(serde_json::json!({
        "seconds": laid.seconds,
        "ceiling": ceiling,
        "crowded": laid.crowded,
        "notices": laid.notices.iter().map(|(level, text)| serde_json::json!({ "level": level, "text": text })).collect::<Vec<_>>(),
    }))
}

/// The engine's own spelling of an adapter list: the folder as `name`, and
/// `<slot>_scale` for every slot, zero where the request leaves one out.
fn adapter_fields(uses: &[AdapterUse]) -> Result<Vec<Value>, String> {
    let slots = music_engine::yue_server::ADAPTER_SLOTS;
    uses.iter()
        .map(|adapter| {
            if adapter.id.trim().is_empty() {
                return Err("an adapter has no id".to_string());
            }
            // no strengths at all would send every slot at zero: a LoRA that does nothing
            if adapter.scales.is_empty() {
                return Err(format!("adapter {} has no strengths: give one per slot ({}); lora_list shows the ones it has", adapter.id, slots.iter().map(|slot| slot.id).collect::<Vec<_>>().join(", ")));
            }
            if let Some(unknown) = adapter.scales.keys().find(|key| !slots.iter().any(|slot| slot.id == key.as_str())) {
                return Err(format!("adapter {} names an unknown slot {unknown}", adapter.id));
            }
            let mut entry = serde_json::json!({ "name": adapter.id });
            for slot in slots {
                let scale = adapter.scales.get(slot.id).copied().unwrap_or(0.0);
                if !scale.is_finite() || !(-10.0..=10.0).contains(&scale) {
                    return Err(format!("adapter {} strength must be between -10 and 10", adapter.id));
                }
                entry[format!("{}_scale", slot.id)] = serde_json::json!(scale);
            }
            Ok(entry)
        })
        .collect()
}

fn insert_optional<T: Serialize>(body: &mut Value, key: &str, value: Option<T>) {
    if let Some(value) = value {
        body[key] = serde_json::to_value(value).expect("serializable request value");
    }
}

fn queued_not_configured_job(request: CreateMusicJobRequest, engine_id: String) -> MusicJob {
    MusicJob {
        derived: None,
        cover_prompt: None,
        id: format!("unconfigured-{}", uuid_suffix()),
        client_ref: request.client_ref.clone(),
        submitted_at: unix_millis(),
        engine_id,
        title: request.title.clone(),
        status: MusicJobStatus::Queued,
        dispatch: MusicJobDispatch::NotConfigured,
        phase: MusicJobPhase::Queued,
        style: request.style,
        lyrics: request.lyrics,
        duration_seconds: request.duration_seconds.unwrap_or_default(),
        generation_settings: Value::Null,
        song: None,
        songs: vec![],
        message: "The selected local music engine is not configured; this job remains queued and no inference has started.".into(),
        playlist_id: None,
        laid: None,
    }
}

fn failed_request_job(request: CreateMusicJobRequest, engine_id: String, error: String) -> MusicJob {
    MusicJob {
        derived: None,
        cover_prompt: None,
        title: request.title.clone(),
        id: format!("rejected-{}", uuid_suffix()),
        client_ref: request.client_ref.clone(),
        submitted_at: unix_millis(),
        engine_id,
        status: MusicJobStatus::Failed,
        dispatch: MusicJobDispatch::NotConfigured,
        phase: MusicJobPhase::Failed,
        style: request.style,
        lyrics: request.lyrics,
        duration_seconds: request.duration_seconds.unwrap_or_default(),
        generation_settings: Value::Null,
        song: None,
        songs: vec![],
        message: error,
        playlist_id: None,
        laid: None,
    }
}

fn uuid_suffix() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// yue-server has no queued state: a job is `running` from the moment it is
/// accepted, whether the worker has reached it or not.
fn apply_remote_status(job: &mut MusicJob, remote_status: &str, failure: Option<String>) {
    match remote_status {
        "running" => {
            job.status = MusicJobStatus::Running;
            job.phase = MusicJobPhase::Running;
            job.message = "The engine has this job.".into();
        }
        "done" => {
            job.status = MusicJobStatus::Completed;
            job.phase = MusicJobPhase::Completed;
            job.message = "The engine finished this job.".into();
        }
        "failed" => {
            job.status = MusicJobStatus::Failed;
            job.phase = MusicJobPhase::Failed;
            job.message = failure.unwrap_or_else(|| "The engine reported a failed job; its log has the reason.".into());
        }
        "cancelled" => {
            job.status = MusicJobStatus::Cancelled;
            job.dispatch = MusicJobDispatch::Cancelled;
            job.phase = MusicJobPhase::Cancelled;
            job.message = "The engine cancelled this job.".into();
        }
        other => {
            job.status = MusicJobStatus::Failed;
            job.phase = MusicJobPhase::Failed;
            job.message = format!("The engine returned an unknown job status: {other}");
        }
    }
}

fn api_error(status: StatusCode, error: String) -> (StatusCode, Json<ApiError>) {
    (status, Json(ApiError { error }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lost_song_is_told_why_from_the_run_that_ended() {
        assert!(lost_job_reason("ggml_backend_cuda_buffer_type_alloc_buffer: allocating 2048 MB on device 0: cudaMalloc failed: out of memory").contains("ran out of memory"));
        assert!(lost_job_reason("CUDA error: an illegal memory access was encountered").contains("CUDA or driver error"));
        assert!(lost_job_reason("[Server] listening").contains("stopped during this song"));
    }

    #[test]
    fn a_song_file_is_found_in_the_media_folder_however_its_path_is_written() {
        let root = tempfile::tempdir().unwrap();
        let media = root.path().join("media");
        std::fs::create_dir_all(&media).unwrap();
        let track = media.join("song.mp3");
        std::fs::write(&track, b"mp3").unwrap();
        std::fs::write(root.path().join("outside.mp3"), b"mp3").unwrap();
        assert!(in_media_folder(&media, &track.canonicalize().unwrap()), "a canonical path is in the folder");
        assert!(in_media_folder(&media, &track));
        assert!(!in_media_folder(&media, &root.path().join("outside.mp3")));
        assert!(!in_media_folder(&media, &media.join("missing.mp3")));
    }

    #[test]
    fn the_window_mark_comes_back_on_the_job() {
        let request: CreateMusicJobRequest = serde_json::from_value(serde_json::json!({ "style": "synth-pop", "lyrics": "[verse]", "client_ref": "temp_1" })).unwrap();
        let job = failed_request_job(request, "engine".into(), "x".into());
        assert_eq!(serde_json::to_value(&job).unwrap()["client_ref"], "temp_1");

        let agent: CreateMusicJobRequest = serde_json::from_value(serde_json::json!({ "style": "synth-pop", "lyrics": "[verse]" })).unwrap();
        let job = failed_request_job(agent, "engine".into(), "x".into());
        assert!(serde_json::to_value(&job).unwrap().get("client_ref").is_none());
    }

    #[test]
    fn models_are_found_in_subfolders_but_not_hidden_ones() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("YuE2").join("vae");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(root.path().join(".cache")).unwrap();
        std::fs::write(root.path().join("top.gguf"), b"a").unwrap();
        std::fs::write(nested.join("deep.gguf"), b"b").unwrap();
        std::fs::write(root.path().join(".cache").join("hidden.gguf"), b"c").unwrap();
        let mut names: Vec<String> = adoptable_files(root.path(), ADOPT_FOLDER_DEPTH).iter().map(|path| path.file_name().unwrap().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["deep.gguf", "top.gguf"]);
        assert_eq!(adoptable_files(root.path(), 0).len(), 1, "depth 0 is the folder itself");
    }

    #[test]
    fn a_device_failure_is_told_from_running_out_of_memory() {
        let ptx = "[lm-kv] allocated 2 sets\nggml_cuda_compute_forward: get_rows failed\ncuda error: the provided ptx was compiled with an unsupported toolchain.";
        assert!(describes_device_failure(ptx));
        assert!(describes_device_failure("[load] fatal: self-test on vulkan0 failed (status -1, result nan, expected 132)"));
        assert!(describes_device_failure("[load] fatal: yue_cuda_backend=c:\\x\\cuda13\\ggml-cuda.dll did not load"));
        assert!(!describes_device_failure("cuda error: out of memory\ncudamalloc failed"));
        assert!(!describes_device_failure("[load] self-test on cuda0: ok (1.2 ms)\n[server] listening on 127.0.0.1:18087"));
    }

    #[test]
    fn an_engine_that_left_without_a_word_counts_as_a_lost_device() {
        assert!(died_silently("[dit] graph: 1236 nodes, t=689, b=2"));
        assert!(!died_silently("ggml_assert: x failed"));
        assert!(!died_silently("cuda error: out of memory"));
    }

    #[test]
    fn a_device_chosen_in_settings_is_the_only_one_tried() {
        use music_engine::yue_server::ComputeBackend;
        for device in [ComputeBackend::Cuda, ComputeBackend::Vulkan, ComputeBackend::Cpu] {
            let options = EngineOptions { backend: device, ..EngineOptions::default() };
            assert_eq!(options.device_chain(&[device]), vec![device]);
        }
        // Auto always ends on the processor and never retries a failed device.
        // Off Windows the chain starts from Auto itself - the engine's own
        // best device, Metal on macOS - so only the CUDA/Vulkan failures of a
        // Windows chain remove entries before it.
        let auto = EngineOptions::default();
        let chain = auto.device_chain(&[ComputeBackend::Cuda, ComputeBackend::Vulkan]);
        if cfg!(windows) {
            assert_eq!(chain, vec![ComputeBackend::Cpu]);
        } else {
            assert_eq!(chain, vec![ComputeBackend::Auto, ComputeBackend::Cpu]);
        }
        assert_eq!(auto.device_chain(&[]).last(), Some(&ComputeBackend::Cpu));
    }

    /// Every file the editor page loads has to be embedded; a missing
    /// WaveSurfer bundle left the editor blank in 1.0.0 to 1.0.3.
    #[test]
    fn the_editor_page_loads_only_embedded_files() {
        let page = EDITOR
            .get_file("index.html")
            .and_then(|file| file.contents_utf8())
            .expect("the editor page is embedded");
        let mut checked = 0;
        for attribute in ["src=\"", "href=\""] {
            for (at, _) in page.match_indices(attribute) {
                let rest = &page[at + attribute.len()..];
                let target = &rest[..rest.find('"').expect("a closed attribute")];
                if target.contains(':') || target.starts_with('#') || target.is_empty() {
                    continue;
                }
                assert!(EDITOR.get_file(target).is_some(), "the editor page loads {target}, which is not embedded");
                checked += 1;
            }
        }
        assert!(checked > 20, "only {checked} local references were found in the editor page");
    }

    /// Nothing stays in VRAM unless the user asked for it. This is the setting
    /// the assistant's unload is tied to, and it is off to begin with.
    #[test]
    fn nothing_is_kept_in_memory_by_default() {
        assert!(!EngineOptions::default().keep_loaded);
        assert!(!EngineOptions::default().to_engine().keep_loaded);
    }

    /// A card that ran out of memory has to say so. The studio used to answer
    /// with "download the five components", pointing at models already on disk.
    #[test]
    fn an_out_of_memory_engine_is_named_as_one() {
        for line in [
            "ggml_backend_cuda_buffer_type_alloc_buffer: allocating 4096 MB on device 0: cudaMalloc failed: out of memory",
            "CUDA error: out of memory",
            "std::bad_alloc",
        ] {
            assert!(
                describes_exhausted_memory(&line.to_lowercase()),
                "this is what running out of memory looks like and it was not recognised: {line}"
            );
        }
        assert!(!describes_exhausted_memory("loading model from disk"));
    }

    #[test]
    fn primary_engine_is_selected_by_default() {
        assert_eq!(
            selected_local_music_engine(&initial_configuration()).as_deref(),
            Some(PRIMARY_MUSIC_ENGINE_ID)
        );
    }

    fn sample_request() -> CreateMusicJobRequest {
        CreateMusicJobRequest {
            client_ref: None,
            style: "warm piano pop, female voice, 88 BPM".into(),
            lyrics: "[Verse]\r\none line".into(),
            ..CreateMusicJobRequest::default()
        }
    }

    #[test]
    fn vocals_only_is_studio_processing_and_never_an_engine_flag() {
        let request = CreateMusicJobRequest { vocals_only: true, output_format: Some("flac".into()), ..sample_request() };
        let body = yue_request_from(&request, 1).unwrap();
        assert_eq!(body["vocals_only"], true);
        assert!(engine_submission(&body).get("vocals_only").is_none());
    }

    #[test]
    fn score_transposition_reaches_the_engine_once_and_rejects_existing_performances() {
        let abc = "X:1\nT:\nM:4/4\nL:1/16\nQ:1/4=120\nV: Vocal clef=treble name=\"Vocal Melody\" snm=\"Vocal\"\nV: Ins clef=treble name=\"Ins Melody\" snm=\"Inst.\"\nK:C\n% verse\nV: Vocal\nC4D4E4F4|Z3|\nV: Ins\nZ4|\n".to_string();
        let request = CreateMusicJobRequest { abc: Some(abc.clone()), transpose: Some(2), ..sample_request() };
        let body = yue_request_from(&request, 1).unwrap();
        assert!(body.get("transpose").is_none());
        let moved = score::notation::read(body["abc"].as_str().unwrap()).unwrap();
        let original = score::notation::read(&abc).unwrap();
        assert_eq!(moved.notes.Vocal[0].pitch, original.notes.Vocal[0].pitch + 2);
        for rejected in [
            CreateMusicJobRequest { semantic_tokens: Some("1,2,3".into()), ..request.clone() },
            CreateMusicJobRequest { cot: Some("off".into()), ..request.clone() },
            CreateMusicJobRequest { abc: None, ..request },
        ] { assert!(yue_request_from(&rejected, 1).is_err()); }
    }

    #[test]
    fn signed_adapter_limits_allow_both_boundaries_but_reject_values_beyond() {
        for strength in [-10.0, 10.0] {
            let use_ = AdapterUse { id: "adapter".into(), scales: [("ar".into(), strength)].into() };
            assert_eq!(adapter_fields(&[use_]).unwrap()[0]["ar_scale"], strength);
        }
        for strength in [-10.01, 10.01, f64::NAN, f64::INFINITY] {
            let use_ = AdapterUse { id: "adapter".into(), scales: [("nar".into(), strength)].into() };
            assert!(adapter_fields(&[use_]).is_err());
        }
    }

    #[test]
    fn the_engine_is_always_asked_for_its_float_output() {
        let mp3 = serde_json::json!({ "style": "x", "output_format": "mp3", "mp3_bitrate": 320 });
        let sent = engine_submission(&mp3);
        assert_eq!(sent["output_format"], "wav32");
        assert!(sent.get("mp3_bitrate").is_none());
        assert_eq!(engine_submission(&serde_json::json!({ "style": "x" }))["output_format"], "wav32");
        assert_eq!(engine_submission(&serde_json::json!({ "style": "x", "output_format": "wav24", "peak_clip": 10 })), serde_json::json!({ "style": "x", "output_format": "wav32" }));
    }

    #[test]
    fn adapters_travel_as_engine_fields_with_every_slot_spelled_out() {
        let mut scales = std::collections::BTreeMap::new();
        scales.insert("ar".to_string(), 0.75);
        let request = CreateMusicJobRequest {
            client_ref: None,
            adapters: vec![AdapterUse { id: "yue2-instrumental".into(), scales }],
            ..sample_request()
        };
        let body = yue_request_from(&request, 1).unwrap();
        assert_eq!(body["adapters"], serde_json::json!([{ "name": "yue2-instrumental", "ar_scale": 0.75, "nar_scale": 0.0 }]));
        assert!(yue_request_from(&sample_request(), 1).unwrap().get("adapters").is_none());

        let mut unknown = std::collections::BTreeMap::new();
        unknown.insert("dit".to_string(), 1.0);
        let refused = CreateMusicJobRequest { client_ref: None, adapters: vec![AdapterUse { id: "x".into(), scales: unknown }], ..sample_request() };
        assert!(yue_request_from(&refused, 1).unwrap_err().contains("unknown slot"));
        let mut huge = std::collections::BTreeMap::new();
        huge.insert("nar".to_string(), 40.0);
        let refused = CreateMusicJobRequest { client_ref: None, adapters: vec![AdapterUse { id: "x".into(), scales: huge }], ..sample_request() };
        assert!(yue_request_from(&refused, 1).is_err());
    }

    #[test]
    fn a_sparse_request_stays_sparse() {
        let body = yue_request_from(&sample_request(), 1).unwrap();
        let object = body.as_object().unwrap();
        // the seeds are drawn here when left out, so the song can be made again
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["lm_seed", "lyrics", "seed", "style"], "only style, lyrics and the drawn seeds travel when nothing else was set: {body}");
        assert_eq!(body["lyrics"], "[Verse]\none line");
    }

    #[test]
    fn every_set_field_reaches_the_engine_under_its_own_name() {
        let request = CreateMusicJobRequest {
            client_ref: None,
            abc: Some("X:1\nK:C\nC".into()),
            cot: Some("melody".into()),
            duration_seconds: Some(95.0),
            lm_seed: Some(42),
            seed: Some(-1),
            steps: Some(40),
            lm_batch_size: Some(2),
            synth_batch_size: Some(3),
            cfg_scale: Some(1.2),
            companion_scale: Some(0.0),
            output_format: Some("flac".into()),
            mp3_bitrate: Some(320),
            abc_sampling: Some(SamplingPreset { temperature: Some(0.8), ..SamplingPreset::default() }),
            semantic_sampling: Some(SamplingPreset::default()),
            ..sample_request()
        };
        let body = yue_request_from(&request, 2).unwrap();
        assert_eq!(body["abc"], "X:1\nK:C\nC\n");
        assert_eq!(body["cot"], "melody");
        assert_eq!(body["duration"], 95.0);
        assert_eq!(body["lm_seed"], 42);
        let drawn = body["seed"].as_i64().expect("a negative seed is drawn here");
        assert!((0..1_i64 << 32).contains(&drawn), "a drawn seed fits 32 bits: {drawn}");
        assert_eq!(body["steps"], 40);
        assert_eq!(body["lm_batch_size"], 2);
        assert_eq!(body["synth_batch_size"], 3);
        assert_eq!(body["cfg_scale"], 1.2);
        assert_eq!(body["companion_scale"], 0.0);
        assert_eq!(body["output_format"], "flac");
        assert_eq!(body["abc_sampling"], serde_json::json!({ "temperature": 0.8 }));
        assert!(body.get("semantic_sampling").is_none(), "an empty preset is the checkpoint preset");
    }

    #[test]
    fn a_score_from_the_comfyui_node_reaches_the_engine_without_its_edit_mark() {
        let request = CreateMusicJobRequest {
            client_ref: None,
            abc: Some("X:1\n%yue2-words 0123456789abcdef keep\nK:C\nC\n".into()),
            ..sample_request()
        };
        assert_eq!(yue_request_from(&request, 1).unwrap()["abc"], "X:1\nK:C\nC\n");
        let marked_only = CreateMusicJobRequest { client_ref: None, abc: Some("%yue2-words 0123456789abcdef\n".into()), ..sample_request() };
        assert!(yue_request_from(&marked_only, 1).unwrap().get("abc").is_none());
    }

    #[test]
    fn a_marked_edit_needs_its_words_or_explicit_keep() {
        let base = sample_request();
        let mark = score::edits::mark(&base.style, &base.lyrics, "full");
        let marked = score::edits::attach("X:1\nK:C\nC", Some(&mark), false);
        let same = CreateMusicJobRequest { abc: Some(marked.clone()), ..base.clone() };
        assert!(yue_request_from(&same, 1).is_ok());
        let changed = CreateMusicJobRequest { lyrics: "different words".into(), ..same.clone() };
        assert!(yue_request_from(&changed, 1).unwrap_err().contains("Keep for new words"));
        let kept = CreateMusicJobRequest { abc: Some(format!("{marked} keep")), ..changed };
        assert!(yue_request_from(&kept, 1).is_ok());
    }

    #[test]
    fn a_tune_without_sections_is_laid_out_for_the_lyrics_and_stops_after_it() {
        let tune = "X:1\nT:\nM:4/4\nL:1/16\nQ:1/4=120\nV: Vocal clef=treble name=\"Vocal Melody\" snm=\"Vocal\"\nV: Ins clef=treble name=\"Ins Melody\" snm=\"Inst.\"\nK:C\nV: Vocal\nC4D4E4F4|G8z8|C4D4E4F4|G8z8|\nV: Ins\nZ4|\n";
        let lyrics = "[Verse]\nOne two three four five\nSix seven eight nine ten";
        let request = CreateMusicJobRequest { client_ref: None, abc: Some(tune.into()), lyrics: lyrics.into(), ..sample_request() };
        let mut body = yue_request_from(&request, 1).unwrap();
        let laid = laid_out(&mut body, true).expect("a bare tune is laid out");
        assert!(body["abc"].as_str().unwrap().contains("% verse"), "{}", body["abc"]);
        assert_eq!(body["duration"], laid["ceiling"]);
        assert!(laid["notices"][0]["text"].as_str().unwrap().starts_with("The score came without sections"));

        let mut chosen = yue_request_from(&CreateMusicJobRequest { duration_seconds: Some(90.0), ..request.clone() }, 1).unwrap();
        laid_out(&mut chosen, false).unwrap();
        assert_eq!(chosen["duration"], 90.0);
        let mut off = yue_request_from(&CreateMusicJobRequest { cot: Some("off".into()), ..request.clone() }, 1).unwrap();
        assert!(laid_out(&mut off, true).is_none());
        let mut named = yue_request_from(&CreateMusicJobRequest { abc: Some(tune.replace("V: Vocal\nC4", "% verse\nV: Vocal\nC4")), ..request }, 1).unwrap();
        assert!(laid_out(&mut named, true).is_none());
    }

    #[test]
    fn requests_the_engine_would_refuse_are_refused_first_with_a_reason() {
        let cases: Vec<(CreateMusicJobRequest, &str)> = vec![
            (CreateMusicJobRequest { client_ref: None, style: " ".into(), lyrics: String::new(), ..CreateMusicJobRequest::default() }, "style or lyrics"),
            (CreateMusicJobRequest { client_ref: None, cot: Some("half".into()), ..sample_request() }, "cot"),
            (CreateMusicJobRequest { client_ref: None, lm_batch_size: Some(2), ..sample_request() }, "lm_batch_size"),
            (CreateMusicJobRequest { client_ref: None, synth_batch_size: Some(10), ..sample_request() }, "synth_batch_size"),
            (CreateMusicJobRequest { client_ref: None, output_format: Some("ogg".into()), ..sample_request() }, "output_format"),
            (CreateMusicJobRequest { client_ref: None, duration_seconds: Some(700.0), ..sample_request() }, "duration"),
            (CreateMusicJobRequest { client_ref: None, semantic_tokens: Some("1,2,x".into()), ..sample_request() }, "semantic_tokens"),
            (CreateMusicJobRequest { client_ref: None, semantic_tokens: Some("1,40000".into()), ..sample_request() }, "semantic_tokens"),
            (CreateMusicJobRequest { client_ref: None, abc_sampling: Some(SamplingPreset { top_p: Some(1.5), ..SamplingPreset::default() }), ..sample_request() }, "top_p"),
            (CreateMusicJobRequest { client_ref: None, semantic_sampling: Some(SamplingPreset { min_tokens: Some(10), max_tokens: Some(5), ..SamplingPreset::default() }), ..sample_request() }, "min_tokens"),
        ];
        for (request, expected) in cases {
            let error = yue_request_from(&request, 1).expect_err(expected);
            assert!(error.contains(expected), "{expected}: {error}");
        }
    }

    #[test]
    fn a_long_song_the_model_scores_gets_room_for_its_score() {
        let long = yue_request_from(&CreateMusicJobRequest { client_ref: None, duration_seconds: Some(480.0), ..sample_request() }, 1).unwrap();
        assert_eq!(long["abc_sampling"]["max_tokens"], 7200);
        let short = yue_request_from(&CreateMusicJobRequest { client_ref: None, duration_seconds: Some(240.0), ..sample_request() }, 1).unwrap();
        assert!(short.get("abc_sampling").is_none());
        let unscored = yue_request_from(&CreateMusicJobRequest { client_ref: None, duration_seconds: Some(480.0), cot: Some("off".into()), ..sample_request() }, 1).unwrap();
        assert!(unscored.get("abc_sampling").is_none());
    }

    #[test]
    fn a_failed_job_names_the_engines_reason() {
        let lines: Vec<String> = ["[Pipeline] FATAL: an earlier job's reason", "[Server] Job abc: {", "[AR] Semantic 10/9000", "[Pipeline] FATAL: the prompt and the score take 9700 of the model's 24576 tokens, so a song can last 595 s and 600 s were asked for", "[Server] job failed"].iter().map(|line| line.to_string()).collect();
        assert_eq!(last_fatal(&lines, "abc").unwrap(), "the prompt and the score take 9700 of the model's 24576 tokens, so a song can last 595 s and 600 s were asked for");
        assert!(last_fatal(&lines[..2], "abc").is_none());
        assert!(last_fatal(&lines, "other").is_none());
    }

    #[test]
    fn a_song_longer_than_the_checkpoint_budget_brings_its_own() {
        let long = yue_request_from(&CreateMusicJobRequest { client_ref: None, duration_seconds: Some(480.0), ..sample_request() }, 1).unwrap();
        assert_eq!(long["semantic_sampling"]["max_tokens"], 12000);
        let short = yue_request_from(&CreateMusicJobRequest { client_ref: None, duration_seconds: Some(200.0), ..sample_request() }, 1).unwrap();
        assert!(short.get("semantic_sampling").is_none());
        let chosen = CreateMusicJobRequest { client_ref: None, duration_seconds: Some(480.0), semantic_sampling: Some(SamplingPreset { max_tokens: Some(9500), ..SamplingPreset::default() }), ..sample_request() };
        assert_eq!(yue_request_from(&chosen, 1).unwrap()["semantic_sampling"]["max_tokens"], 9500);
    }

    #[test]
    fn codes_alone_are_a_valid_request() {
        let request = CreateMusicJobRequest { client_ref: None, semantic_tokens: Some("12, 8433 ,22418".into()), ..CreateMusicJobRequest::default() };
        let body = yue_request_from(&request, 1).unwrap();
        assert_eq!(body["semantic_tokens"], "12, 8433 ,22418");
    }

    #[test]
    fn persisted_settings_round_trip_a_complete_custom_component_selection() {
        let settings = PersistedStudioSettings {
            engine_options: EngineOptions { keep_loaded: true, max_batch: Some(2), ..EngineOptions::default() },
            assistant: AssistantConfig { provider: AssistantProvider::Local, local_base_url: Some("http://127.0.0.1:8080/v1".into()), local_model: Some("gemma".into()), openrouter_model: None, managed_model: None, managed_path: None, reasoning_effort: None },
            configuration: initial_configuration(),
            // Karaoke is off by default and has to survive a restart the same
            // way the assistant does.
            lyrics_sync: lyrics_sync::LyricsSyncConfig {
                enabled: true,
                provider: lyrics_sync::AsrProvider::Parakeet,
                whisper_model: None,
                openrouter_model: None,
                runtime: lyrics_sync::OnnxFlavour::default(),
            },
            selected_profile_id: None,
            selected_component_ids: Some(vec!["backbone-q8".into(), "vae-f32".into()]),
            cover_templates: Some(cover_prompt::default_templates()),
            cover_auto: Some(true),
            cover_look: None,
            proxy: None,
            network: None,
            separation: Some(separation::SeparationConfig::default()),
            cover_template_default: Some("photographic".into()),
        };
        let restored: PersistedStudioSettings = serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert!(restored.lyrics_sync.available());
        assert_eq!(restored.lyrics_sync.provider, lyrics_sync::AsrProvider::Parakeet);
        assert!(restored.selected_profile_id.is_none());
        assert_eq!(restored.selected_component_ids.unwrap(), vec!["backbone-q8", "vae-f32"]);
        // Engine flags survive a restart, and the songs-per-request ceiling is
        // derived from them rather than assumed.
        assert!(restored.engine_options.keep_loaded);
        assert_eq!(restored.engine_options.effective_max_batch(), 2);
        // Four by default: songs decoded together are nearly free after the
        // first, and the upstream default of 1 left the slider disabled.
        // Nothing is reserved that nobody asked for.
        assert_eq!(EngineOptions::default().effective_max_batch(), 1);
        // And whatever it is, the engine is started with it: the request
        // carries `lm_batch_size`, and the engine refuses anything above the
        // ceiling it was loaded with. Offering more in the panel than the
        // engine was given is what made a request for two songs fail at once.
        assert_eq!(EngineOptions::default().to_engine().max_batch, Some(1));
        assert_eq!(EngineOptions { max_batch: Some(3), ..EngineOptions::default() }.to_engine().max_batch, Some(3));
        let vulkan = EngineOptions { backend: music_engine::yue_server::ComputeBackend::Vulkan, ..EngineOptions::default() }.to_engine();
        assert!(vulkan.clamp_fp16, "Vulkan runs clamp hidden states to FP16");
        let cuda = EngineOptions { backend: music_engine::yue_server::ComputeBackend::Cuda, ..EngineOptions::default() }.to_engine();
        assert!(!cuda.clamp_fp16);
        // The assistant is optional: it must survive a restart when configured,
        // and stay unavailable when it is not.
        assert!(restored.assistant.available());
        assert!(!AssistantConfig::default().available());
    }

    #[test]
    fn remote_statuses_never_claim_success_for_an_unknown_value() {
        let mut job = queued_not_configured_job(sample_request(), PRIMARY_MUSIC_ENGINE_ID.into());
        apply_remote_status(&mut job, "not-a-real-status", None);
        assert!(matches!(job.status, MusicJobStatus::Failed));
    }

    #[test]
    fn a_stop_is_shown_at_once_but_never_undoes_a_finished_job() {
        let mut running = queued_not_configured_job(sample_request(), PRIMARY_MUSIC_ENGINE_ID.into());
        apply_remote_status(&mut running, "running", None);
        stop_on_request(&mut running);
        assert!(matches!(running.status, MusicJobStatus::Cancelled));
        let mut finished = queued_not_configured_job(sample_request(), PRIMARY_MUSIC_ENGINE_ID.into());
        apply_remote_status(&mut finished, "done", None);
        stop_on_request(&mut finished);
        assert!(matches!(finished.status, MusicJobStatus::Completed));
    }

    #[test]
    fn capabilities_use_the_engines_envelope_and_music_stays_local() {
        let after_refresh = CapabilitiesResponse { engines: capability_engines(false) };
        assert!(!after_refresh.engines.iter().find(|engine| engine.id == "openrouter").unwrap().capabilities.contains(&Capability::MusicGeneration));
        // The music engine, two recognisers, the local assistant and
        // OpenRouter: everything listed is something the studio can actually do.
        assert_eq!(serde_json::to_value(after_refresh).unwrap()["engines"].as_array().unwrap().len(), 5);
    }

    #[test]
    fn a_composition_runs_the_planning_stage_and_next_to_nothing_else() {
        let request = ComposeScoreRequest { style: "folk rock".into(), lyrics: "[Verse]\r\nline".into(), cot: Some("melody".into()), lm_seed: Some(7), abc_sampling: None, harmony: None, lm_batch_size: None };
        let body = compose_request_from(&request).unwrap();
        assert_eq!(body["cot"], "melody");
        assert_eq!(body["duration"], 1.0);
        assert_eq!(body["steps"], 1);
        assert_eq!(body["lm_batch_size"], 1);
        assert_eq!(body["lm_seed"], 7);
        assert_eq!(body["lyrics"], "[Verse]\nline");
        assert!(body.get("abc").is_none());
    }

    #[test]
    fn a_composition_needs_a_mode_that_writes_a_score_and_a_prompt() {
        let off = ComposeScoreRequest { style: "pop".into(), lyrics: String::new(), cot: Some("off".into()), lm_seed: None, abc_sampling: None, harmony: None, lm_batch_size: None };
        assert!(compose_request_from(&off).is_err());
        let empty = ComposeScoreRequest { style: " ".into(), lyrics: String::new(), cot: None, lm_seed: None, abc_sampling: None, harmony: None, lm_batch_size: None };
        assert!(compose_request_from(&empty).is_err());
    }

    #[test]
    fn a_transcription_result_is_read_as_json() {
        let (abc, seed) = score_from_result("application/json", br#"{"abc":"X:1\nK:C\n|C|\n"}"#).unwrap();
        assert_eq!(abc, "X:1\nK:C\n|C|");
        assert_eq!(seed, None);
        assert!(score_from_result("application/json", br#"{"abc":""}"#).is_err());
    }

    #[test]
    fn a_plan_batch_keeps_every_abc_and_its_own_seed() {
        let packet = "--b\r\nContent-Type: application/json\r\n\r\n{\"abc\":\"first score\",\"lm_seed\":7}\r\n--b\r\nContent-Type: audio/wav\r\n\r\nwave1\r\n--b\r\nContent-Type: application/json\r\n\r\n{\"abc\":\"second score\",\"lm_seed\":8}\r\n--b\r\nContent-Type: audio/wav\r\n\r\nwave2\r\n--b--\r\n";
        let plans = plans_from_result("multipart/mixed; boundary=b", packet.as_bytes()).unwrap();
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].abc, "first score"); assert_eq!(plans[0].lm_seed, Some(7));
        assert_eq!(plans[1].abc, "second score"); assert_eq!(plans[1].lm_seed, Some(8));
    }

    fn replay_overrides() -> ReplayMusicJobRequest {
        ReplayMusicJobRequest { client_ref: None, song_id: None, replay_request: None, steps: None, seed: None, synth_batch_size: None, output_format: None, mp3_bitrate: None, title: None, extend_seconds: None }
    }

    #[test]
    fn a_rerender_keeps_the_music_and_changes_only_the_acoustic_side() {
        let request = ReplayMusicJobRequest { client_ref: None, steps: Some(48), seed: Some(9), synth_batch_size: Some(2), output_format: Some("flac".into()), mp3_bitrate: Some(192), ..replay_overrides() };
        let replay = serde_json::json!({"style":"piano pop","lyrics":"[Verse] hi","abc":"X:1\nK:C\n","semantic_tokens":"1,2,3","lm_seed":123,"seed":1,"steps":32,"cot":"full"});
        let prepared = prepare_replay_synthesis(replay, &request).unwrap();
        assert_eq!(prepared["semantic_tokens"], "1,2,3");
        assert_eq!(prepared["abc"], "X:1\nK:C\n");
        assert_eq!(prepared["lm_seed"], 123);
        assert_eq!(prepared["steps"], 48);
        assert_eq!(prepared["seed"], 9);
        assert_eq!(prepared["synth_batch_size"], 2);
        assert_eq!(prepared["output_format"], "flac");
        assert_eq!(prepared["mp3_bitrate"], 192);
        assert_eq!(prepared["lm_batch_size"], 1);
    }

    #[test]
    fn a_longer_rerender_composes_on_from_the_tracks_last_frame() {
        let codes = vec!["7"; 250].join(",");
        let replay = serde_json::json!({"style":"pop","lyrics":"[Verse] hi","abc":"X:1\nK:C\n","semantic_tokens":codes,"lm_seed":5,"seed":1,"cot":"full"});
        let longer = prepare_replay_synthesis(replay.clone(), &ReplayMusicJobRequest { extend_seconds: Some(30.0), ..replay_overrides() }).unwrap();
        assert_eq!(longer["continue_semantic_tokens"], true);
        assert_eq!(longer["duration"], 40.0);
        assert!(longer.get("semantic_sampling").is_none());
        let past = serde_json::json!({"semantic_tokens":vec!["7"; 8750].join(","),"cot":"off"});
        let long = prepare_replay_synthesis(past, &ReplayMusicJobRequest { extend_seconds: Some(60.0), ..replay_overrides() }).unwrap();
        assert_eq!(long["semantic_sampling"]["max_tokens"], 10250);
        assert!(prepare_replay_synthesis(replay, &ReplayMusicJobRequest { extend_seconds: Some(700.0), ..replay_overrides() }).is_err());
    }

    #[test]
    fn a_track_without_its_semantic_stream_cannot_be_rerendered() {
        assert!(prepare_replay_synthesis(serde_json::json!({"style":"s","lyrics":"l"}), &replay_overrides()).is_err());
        assert!(prepare_replay_synthesis(serde_json::json!({"style":"s","semantic_tokens":"1,oops"}), &replay_overrides()).is_err());
    }

    #[test]
    fn engine_options_become_launch_flags_with_a_batch_ceiling() {
        let options = EngineOptions { max_seq: Some(8192), vae_core: Some(256), ..EngineOptions::default() }.to_engine();
        assert_eq!(options.max_batch, Some(1));
        assert_eq!(options.max_seq, Some(8192));
        assert_eq!(options.vae_core, Some(256));
    }
}
