//! The studio as an MCP server: Streamable HTTP, stateless JSON-RPC at `/mcp`.
//!
//! Every tool is a route of the studio's own API, called inside the process
//! through the same router the page talks to, so an agent does exactly what
//! the page does through the same code: create songs, write lyrics and styles,
//! prepare datasets, train, install LoRA, draw covers, split stems. A file an
//! agent names by its path is sent to the route as the multipart upload the
//! page would send.

use std::collections::HashMap;
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde_json::{json, Value};
use tower::ServiceExt;

/// The studio's API router, set once the service has built it.
static API: OnceLock<Router> = OnceLock::new();

pub fn install(api: Router) {
    let _ = API.set(api);
}

/// The studio's name, as clients show it.
const STUDIO: &str = "YuE2 Studio";
/// The skill an agent reads, served as a resource and a prompt.
const SKILL: &str = include_str!("../../../docs/mcp-skill.md");
const SKILL_URI: &str = "studio://skill";
const GUIDE_URI: &str = "studio://guide/";
/// Answers longer than this are cut: a library of hundreds of songs is more
/// than an agent reads in one call.
const LIMIT: usize = 60_000;

/// What a tool sends to its route.
enum Payload {
    None,
    Json(Value),
    /// Multipart form fields and files, as the page uploads them.
    Form { fields: Vec<(String, String)>, files: Vec<(String, PathBuf, String)> },
    /// A command for the studio's window: what is on screen, the buttons, the
    /// player, the video editor. Answered by the page itself.
    Window { command: &'static str, args: Value, seconds: u64 },
}

struct Call {
    method: Method,
    path: String,
    payload: Payload,
}

struct Tool {
    name: &'static str,
    description: &'static str,
    schema: fn() -> Value,
    call: fn(&Value) -> Result<Call, String>,
}

fn get(path: String) -> Result<Call, String> {
    Ok(Call { method: Method::GET, path, payload: Payload::None })
}

fn post(path: String, body: Value) -> Result<Call, String> {
    Ok(Call { method: Method::POST, path, payload: Payload::Json(body) })
}

fn send(method: Method, path: String, body: Value) -> Result<Call, String> {
    Ok(Call { method, path, payload: Payload::Json(body) })
}

fn window(command: &'static str, args: &Value, seconds: u64) -> Result<Call, String> {
    Ok(Call { method: Method::GET, path: String::new(), payload: Payload::Window { command, args: args.clone(), seconds } })
}

fn composite(kind: &'static str) -> Result<Call, String> {
    Ok(Call { method: Method::GET, path: format!("composite:{kind}"), payload: Payload::None })
}

/// A GET inside the process, as JSON.
async fn fetch(path: &str) -> Value {
    match call_route(Call { method: Method::GET, path: path.into(), payload: Payload::None }).await {
        Ok((_, text)) => serde_json::from_str(&text).unwrap_or(Value::String(text)),
        Err(problem) => json!({ "error": problem }),
    }
}

fn compact_training(training: &Value) -> Value {
    let datasets: Vec<Value> = training["datasets"].as_array().into_iter().flatten().map(|dataset| {
        let items = dataset["items"].as_array().cloned().unwrap_or_default();
        let count = |field: &str, value: &str| items.iter().filter(|item| item[field] == value).count();
        json!({
            "id": dataset["id"], "name": dataset["name"], "trigger": dataset["trigger"], "songs": items.len(),
            "ready": items.iter().filter(|item| item["lyrics_state"] == "done" && item["style_state"] == "done").count(),
            "lyrics": { "wanted": count("lyrics_state", "wanted"), "found": count("lyrics_state", "found"), "done": count("lyrics_state", "done") },
            "style": { "wanted": count("style_state", "wanted"), "heard": count("style_state", "heard"), "done": count("style_state", "done") },
        })
    }).collect();
    let runs: Vec<Value> = training["runs"].as_array().into_iter().flatten().map(compact_run).collect();
    json!({
        "trainer_installed": training["pack_ready"],
        "listening_installed": training["listen"]["ready"],
        "download": training["download"],
        "preparation": compact_preparation(&training["prepare"]),
        "active_run": training["active"],
        "datasets": datasets,
        "runs": runs,
        "recipe_defaults": training["recipe_defaults"],
    })
}

fn compact_run(run: &Value) -> Value {
    let steps = run["steps"].as_array().cloned().unwrap_or_default();
    let last = steps.last().cloned().unwrap_or(Value::Null);
    // only the tuned preset stops on the planner's KL
    let tuned = run["recipe"]["preset"].as_str() == Some("tuned");
    json!({
        "id": run["id"], "name": run["name"], "dataset": run["dataset_name"], "trigger": run["trigger"], "status": run["status"], "stage": run["stage"],
        "steps_done": steps.len(), "step_limit": run["recipe"]["steps"], "preset": run["recipe"]["preset"], "songs_per_step": run["recipe"]["grad_accum"],
        "stop": if tuned { run["recipe"]["stop"].clone() } else { Value::Null }, "target_kl": if tuned { run["recipe"]["target_kl"].clone() } else { Value::Null },
        "last_step": last, "checkpoints": run["checkpoints"], "installed": run["installed"], "error": run["error"],
    })
}

fn compact_preparation(prepare: &Value) -> Value {
    if prepare.is_null() {
        return Value::Null;
    }
    json!({
        "dataset": prepare["dataset"], "finished": prepare["finished"], "cancelled": prepare["cancelled"],
        "stages": prepare["stages"], "songs_left": prepare["pending"].as_array().map_or(0, Vec::len),
        "failures": prepare["failures"], "notices": prepare["notices"], "run": prepare["run"],
    })
}

/// A song job as an agent follows it: where it is, what it made. The
/// request's lyrics, score and audio codes are left to response_format detailed.
fn compact_job(job: &Value) -> Value {
    if !job.is_object() || job.get("error").is_some_and(|error| error.is_string()) && job.get("status").is_none() {
        return job.clone();
    }
    let mut songs: Vec<Value> = job["songs"].as_array().into_iter().flatten().map(|song| json!({ "id": song["id"], "title": song["song"]["title"] })).collect();
    if songs.is_empty() && job["song"].is_object() {
        songs.push(json!({ "id": job["song"]["id"], "title": job["song"]["song"]["title"] }));
    }
    let settings = &job["generation_settings"];
    json!({
        "id": job["id"], "title": job["title"], "status": job["status"], "phase": job["phase"], "message": job["message"],
        "duration_seconds": job["duration_seconds"], "seed": settings["seed"], "lm_seed": settings["lm_seed"],
        "adapters": settings["adapters"], "songs": songs,
    })
}

/// A library song without what only a re-render reads: its audio codes and
/// the request they were made from, both large.
fn compact_song(song: &Value) -> Value {
    let mut song = song.clone();
    if let Some(fields) = song.as_object_mut() {
        let codes = fields.remove("audio_codes").is_some_and(|codes| !codes.is_null());
        fields.remove("replay_request");
        fields.insert("has_audio_codes".into(), codes.into());
        // the score and the karaoke timings are long; they say they are there
        if let Some(settings) = fields.get_mut("generation_settings").and_then(Value::as_object_mut) {
            let score = settings.remove("abc").is_some_and(|abc| abc.as_str().is_some_and(|abc| !abc.trim().is_empty()));
            settings.remove("lyrics");
            settings.insert("has_score".into(), score.into());
        }
        if let Some(metadata) = fields.get_mut("metadata").and_then(Value::as_object_mut) {
            let karaoke = metadata.remove("lrc").is_some_and(|lrc| lrc.as_str().is_some_and(|lrc| !lrc.is_empty()));
            metadata.insert("has_karaoke".into(), karaoke.into());
        }
    }
    song
}

/// The recogniser settings and the models to choose from, without the files
/// each model is made of.
fn compact_karaoke(status: &Value) -> Value {
    let models: Vec<Value> = status["assets"].as_array().into_iter().flatten()
        .filter(|asset| asset["kind"] == "model" && !asset["vram_gb"].is_null())
        .map(|asset| json!({ "id": asset["id"], "label": asset["label"], "installed": asset["installed"], "vram_gb": asset["vram_gb"], "about": asset["note"] }))
        .collect();
    json!({
        "enabled": status["enabled"], "provider": status["provider"], "whisper_model": status["whisper_model"], "openrouter_model": status["openrouter_model"],
        "runtime": status["runtime"], "ready": status["ready"], "download": status["active_download"], "models": models,
    })
}

/// A name or a description the catalogue gives in several languages, in English.
fn english(value: &Value) -> Value {
    if value.is_object() { value["en"].clone() } else { value.clone() }
}

/// The LoRA an agent picks from: installed ones with their slots and trigger,
/// and the catalogue in one line each.
fn compact_loras(loras: &Value) -> Value {
    let installed: Vec<Value> = loras["installed"].as_array().into_iter().flatten().map(|lora| json!({
        "id": lora["id"], "name": english(&lora["name"]), "kind": lora["kind"], "trigger": lora["trigger"],
        "slots": lora["slots"], "scales": lora["scales"], "range": lora["range"], "error": lora["error"],
    })).collect();
    let catalog: Vec<Value> = loras["catalog"].as_array().into_iter().flatten().map(|lora| json!({
        "id": lora["id"], "name": english(&lora["name"]), "kind": lora["kind"], "installed": lora["installed"],
        "trigger": lora["trigger"], "slots": lora["slots"], "about": english(&lora["description"]),
    })).collect();
    json!({ "installed": installed, "catalog": catalog, "slots": loras["slots"], "download": loras["download"], "installing": loras["installing"] })
}

/// What an agent is answered: the parts it acts on, unless it asked for
/// every field with response_format detailed.
fn shape(name: &str, args: &Value, value: Value) -> Value {
    let detailed = args.get("response_format").and_then(Value::as_str) == Some("detailed");
    match name {
        "training_status" if !detailed => compact_training(&value),
        "song_create" | "song_job_get" | "song_replay" if !detailed => compact_job(&value),
        "song_jobs_list" if !detailed => Value::Array(value.as_array().into_iter().flatten().map(compact_job).collect()),
        "library_song_get" if !detailed => compact_song(&value),
        "lora_list" if !detailed => compact_loras(&value),
        "midi_get" if !detailed => {
            let mut value = value;
            let notes = value.as_object_mut().and_then(|fields| fields.remove("notes")).and_then(|notes| notes.as_array().map(Vec::len)).unwrap_or(0);
            value["notes_count"] = notes.into();
            value
        }
        "karaoke_settings_get" if !detailed => compact_karaoke(&value),
        // any other answer that is a whole library song
        _ if !detailed && value.get("audio_codes").is_some() && value.get("replay_request").is_some() => compact_song(&value),
        "training_checkpoint_install" | "lora_install_hf" | "lora_import_files" if value["slots"].as_array().is_some_and(Vec::is_empty) => {
            let mut value = value;
            value["slots"] = json!("not known yet: the engine reads them from the file; lora_list shows them");
            value
        }
        "dataset_get" => {
            let wanted = args.get("dataset_id").and_then(Value::as_str).unwrap_or_default();
            let Some(dataset) = value["datasets"].as_array().into_iter().flatten().find(|dataset| dataset["id"] == wanted).cloned() else {
                return json!({ "error": format!("No dataset {wanted}; training_status lists them.") });
            };
            match args.get("song_id").and_then(Value::as_str) {
                Some(song) => dataset["items"].as_array().into_iter().flatten().find(|item| item["id"] == song).cloned().unwrap_or_else(|| json!({ "error": format!("No song {song} in the dataset.") })),
                None => dataset,
            }
        }
        "library_songs_list" => {
            let query = args.get("query").and_then(Value::as_str).unwrap_or_default().to_lowercase();
            let bound = |name: &str, end: bool| match args.get(name).and_then(Value::as_str) {
                Some(text) => moment(text, end).map(Some),
                None => Ok(None),
            };
            let (since, until) = match (bound("since", false), bound("until", true)) {
                (Ok(since), Ok(until)) => (since, until),
                (Err(problem), _) | (_, Err(problem)) => return json!({ "error": problem }),
            };
            let songs = value.as_array().cloned().unwrap_or_default().into_iter().filter(|song| {
                let made = song["created_at"].as_str().and_then(|text| text.parse::<i64>().ok()).or_else(|| song["created_at"].as_i64()).unwrap_or_default();
                (query.is_empty() || song["title"].as_str().unwrap_or_default().to_lowercase().contains(&query) || song["caption"].as_str().unwrap_or_default().to_lowercase().contains(&query))
                    && since.is_none_or(|since| made >= since)
                    && until.is_none_or(|until| made <= until)
            });
            if detailed {
                return Value::Array(songs.collect());
            }
            Value::Array(songs.map(|song| {
                let style: String = song["caption"].as_str().unwrap_or_default().chars().take(90).collect();
                let mut row = json!({ "id": song["id"], "title": song["title"], "made": song["created_at"], "style": style });
                // a track a tool made names the one it was made from
                if song["metadata"]["derived"].is_object() {
                    row["made_from"] = song["metadata"]["derived"]["from"].clone();
                    row["made_by"] = song["metadata"]["derived"]["tool"].clone();
                }
                row
            }).collect())
        }
        _ => value,
    }
}

async fn status_summary() -> Value {
    let (jobs, activity, training, processing, separation) = tokio::join!(fetch("/v1/music/jobs"), fetch("/v1/activity"), fetch("/v1/training"), fetch("/v1/processing"), fetch("/v1/separation/status"));
    let midi = fetch("/v1/midi").await;
    let running_activity: Vec<Value> = activity["activity"].as_array().into_iter().flatten().filter(|entry| entry["state"] == "running").cloned().collect();
    let active_run = training["runs"].as_array().into_iter().flatten().find(|run| Some(run["id"].as_str().unwrap_or_default()) == training["active"].as_str()).map(compact_run);
    json!({
        "window_open": !open_windows().is_empty(),
        "assistant_requests_waiting": open_questions().len(),
        "song_jobs": Value::Array(jobs.as_array().into_iter().flatten().map(compact_job).collect()),
        "covers_and_karaoke": running_activity,
        "stems": separation["run"],
        "midi": midi["run"],
        "preparation": compact_preparation(&training["prepare"]),
        "training": active_run,
        "processing": processing["run"],
    })
}

/// Whether anything in a status summary is still at work.
fn busy(summary: &Value) -> Vec<&'static str> {
    let unfinished = |run: &Value| run.is_object() && run["done"] != true;
    [
        ("song_jobs", summary["song_jobs"].as_array().is_some_and(|jobs| !jobs.is_empty())),
        ("covers_and_karaoke", summary["covers_and_karaoke"].as_array().is_some_and(|entries| !entries.is_empty())),
        ("stems", unfinished(&summary["stems"])),
        ("processing", unfinished(&summary["processing"])),
        ("midi", unfinished(&summary["midi"])),
        ("preparation", summary["preparation"].is_object() && summary["preparation"]["finished"] != true),
        ("training", !summary["training"].is_null()),
    ]
    .into_iter()
    .filter_map(|(what, working)| working.then_some(what))
    .collect()
}

/// How long one wait holds a call: clients give up on a tool call after about
/// a minute, so a wait answers before that and the agent calls it again.
const WAIT_DEFAULT: u64 = 30;
const WAIT_LONGEST: u64 = 55;

/// Waits for a job, the preparation, training or everything, a slice at a time.
async fn wait_for(args: &Value) -> Result<Value, String> {
    let seconds = args.get("seconds").and_then(Value::as_u64).unwrap_or(WAIT_DEFAULT).clamp(2, WAIT_LONGEST);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
    let job = args.get("job_id").and_then(Value::as_str).map(str::to_string);
    let until = args.get("until").and_then(Value::as_str).unwrap_or("idle").to_string();
    loop {
        let (done, now) = match &job {
            Some(job) => {
                let mut state = fetch(&format!("/v1/music/jobs/{}", segment(job))).await;
                if state.get("error").is_some() || state.get("status").is_none() {
                    state = fetch(&format!("/v1/scores/{}", segment(job))).await;
                }
                if state.get("status").is_none() {
                    let why = state["error"].as_str().or(state.as_str()).unwrap_or("not found");
                    return Err(format!("No song or score job {job} ({why}): job_id is what song_create, song_replay, score_compose or score_transcribe returned. Wait for other work with until."));
                }
                let status = state["status"].as_str().unwrap_or_default().to_string();
                // a score job's state is its score; a song job's is summed up
                if state.get("generation_settings").is_some() {
                    state = compact_job(&state);
                }
                (["completed", "failed", "cancelled", "done", "error"].contains(&status.as_str()), state)
            }
            None => {
                let summary = status_summary().await;
                let working = busy(&summary);
                let done = if until == "idle" { working.is_empty() } else { !working.contains(&until.as_str()) };
                (done, summary)
            }
        };
        if done {
            return Ok(json!({ "done": true, "state": now }));
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(json!({ "done": false, "note": "still running; call studio_wait again to keep waiting", "state": now }));
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// MCP tool annotations, from what each tool does.
fn annotations(name: &str) -> Value {
    const READS: &[&str] = &["_get", "_status", "_list", "_catalog", "_files", "_logs", "_log", "_runtime", "_local_models", "_hf_files", "_search_hf", "_capabilities", "_system", "_state", "_read_page", "_screenshot"];
    // a verb that changes something outweighs a noun that reads
    const CHANGES: &[&str] = &["install", "import", "remove", "delete", "refresh", "create", "update", "start", "cancel", "select", "download", "apply", "restart"];
    // reads whose names the rules above miss: create_form names the create page
    const READ_NAMES: &[&str] = &["lyrics_find", "cover_prompt_render", "studio_wait", "engine_presets_get", "assistant_requests_wait", "ui_console", "song_defaults", "create_form_get"];
    let changes = CHANGES.iter().any(|verb| name.split('_').any(|word| word == *verb));
    let read_only = READ_NAMES.contains(&name) || !changes && (READS.iter().any(|part| name.ends_with(part) || name.contains(&format!("{part}_"))) || name.starts_with("writing_"));
    let destructive = name.ends_with("_delete") || name.ends_with("_remove") || name == "processing_discard" || name.ends_with("_cancel") || name.contains("_cancel_");
    // what reaches the internet: OpenRouter, Hugging Face, the lyric databases and every download
    let open_world = name.starts_with("openrouter_") || name.contains("_hf") || name == "lyrics_find" || name == "models_download" || name == "lora_install_catalog" || name.ends_with("_install") && name != "training_checkpoint_install";
    let title = name.replace('_', " ");
    json!({ "title": title, "readOnlyHint": read_only, "destructiveHint": destructive, "idempotentHint": read_only || name.ends_with("_set") || name.contains("_select"), "openWorldHint": open_world })
}

/// The window's side of the bridge: the page subscribes to the commands and
/// posts each answer back.
struct Bridge {
    commands: tokio::sync::broadcast::Sender<String>,
    pending: Mutex<HashMap<String, tokio::sync::oneshot::Sender<Result<Value, String>>>>,
    /// The open windows, oldest first; a command goes to the newest only, so
    /// a second window never runs it again.
    windows: Mutex<Vec<u64>>,
    /// The windows the person has looked at, oldest first: a question belongs
    /// to the one they look at now, not to the one that happened to subscribe last.
    focused: Mutex<Vec<u64>>,
    sequence: AtomicU64,
}

fn bridge() -> &'static Bridge {
    static BRIDGE: OnceLock<Bridge> = OnceLock::new();
    BRIDGE.get_or_init(|| Bridge { commands: tokio::sync::broadcast::channel(64).0, pending: Mutex::new(HashMap::new()), windows: Mutex::new(Vec::new()), focused: Mutex::new(Vec::new()), sequence: AtomicU64::new(0) })
}

fn open_windows() -> std::sync::MutexGuard<'static, Vec<u64>> {
    bridge().windows.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn focused_windows() -> std::sync::MutexGuard<'static, Vec<u64>> {
    bridge().focused.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The window a question belongs to: where the person works, or - while no
/// window has told the studio it holds their attention - the newest one.
fn window_to_ask() -> Option<u64> {
    let open = open_windows();
    if let Some(window) = focused_windows().iter().rev().find(|window| open.contains(window)) {
        return Some(*window);
    }
    open.last().copied()
}

/// The page says it is the one being looked at, so questions reach its screen.
pub async fn window_focus(headers: HeaderMap, Json(body): Json<Value>) -> StatusCode {
    if !local_origin(&headers) {
        return foreign_origin().status();
    }
    let Some(window) = body.get("window").and_then(Value::as_u64) else {
        return StatusCode::BAD_REQUEST;
    };
    if !open_windows().contains(&window) {
        return StatusCode::NOT_FOUND;
    }
    let mut focused = focused_windows();
    focused.retain(|seen| *seen != window);
    focused.push(window);
    StatusCode::NO_CONTENT
}

/// Keeps the window on the list while its stream is open.
struct Listening(u64);

impl Drop for Listening {
    fn drop(&mut self) {
        open_windows().retain(|window| *window != self.0);
        focused_windows().retain(|window| *window != self.0);
    }
}

/// Only the studio's own page and local agents may drive it: a web page in
/// the user's browser, or one rebinding a domain to this computer, sends its
/// own origin and is refused.
fn local_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else { return true };
    let Ok(origin) = origin.to_str() else { return false };
    let Some((scheme, rest)) = origin.split_once("://") else { return false };
    let host = rest.split('/').next().unwrap_or_default();
    let host = if host.starts_with('[') { host.split(']').next().map(|name| format!("{name}]")).unwrap_or_default() } else { host.split(':').next().unwrap_or_default().to_string() };
    scheme == "tauri" || ["localhost", "127.0.0.1", "[::1]", "tauri.localhost"].contains(&host.as_str())
}

fn foreign_origin() -> Response {
    (StatusCode::FORBIDDEN, "This studio answers only its own window and agents on this computer.").into_response()
}

/// The stream of commands the studio's page executes. Its first message
/// names the window, so it knows the commands addressed to it.
pub async fn window_events(headers: HeaderMap) -> Response {
    if !local_origin(&headers) {
        return foreign_origin();
    }
    let window = bridge().sequence.fetch_add(1, Ordering::Relaxed);
    open_windows().push(window);
    let receiver = bridge().commands.subscribe();
    let hello = futures_util::stream::once(async move { Ok::<Event, Infallible>(Event::default().data(json!({ "window": window }).to_string())) });
    let commands = futures_util::stream::unfold((receiver, Listening(window)), |(mut receiver, listening)| async move {
        loop {
            match receiver.recv().await {
                Ok(command) => return Some((Ok(Event::default().data(command)), (receiver, listening))),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(futures_util::StreamExt::chain(hello, commands)).keep_alive(KeepAlive::default()).into_response()
}

#[derive(serde::Deserialize)]
pub struct WindowAnswer {
    id: String,
    #[serde(default)]
    result: Value,
    #[serde(default)]
    error: Option<String>,
}

/// The page's answer to one command.
pub async fn window_result(headers: HeaderMap, Json(answer): Json<WindowAnswer>) -> StatusCode {
    if !local_origin(&headers) {
        return StatusCode::FORBIDDEN;
    }
    let waiting = bridge().pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&answer.id);
    match waiting {
        Some(sender) => {
            let _ = sender.send(match answer.error {
                Some(error) => Err(error),
                None => Ok(answer.result),
            });
            StatusCode::NO_CONTENT
        }
        None => StatusCode::NOT_FOUND,
    }
}

/// Tells every open window that something changed behind it, so the screens
/// showing songs, jobs, LoRA and settings read them again: an agent's call, or
/// background work finishing after the call that started it returned.
pub fn announce(what: &str) {
    tell_windows(json!({ "changed": what }));
}

/// An event for every open window.
pub fn tell_windows(event: Value) {
    let _ = bridge().commands.send(event.to_string());
}

/// A tool whose work belongs in the user's message log: it makes something,
/// changes something, removes it or starts it. Reading the library, looking at
/// statuses and moving the window's mouse do not go there - the log is what the
/// agent did TO the studio, not how it found its way around.
fn is_noteworthy(name: &str) -> bool {
    const ACTIONS: [&str; 24] = [
        "_create", "_update", "_delete", "_set", "_like", "_add", "_remove", "_import", "_install",
        "_download", "_adopt", "_start", "_cancel", "_split", "_make", "_transcribe", "_render",
        "_replay", "_open", "_close", "_prepare", "_keep", "_discard", "_rename",
    ];
    if name.starts_with("ui_") || name.starts_with("agent_") {
        return false;
    }
    ACTIONS.iter().any(|action| name.contains(action))
}

/// What a tool acted on, in a person's words: the title or name it was given, or
/// the one the studio answered with. Only the name of a thing travels to the log -
/// never an id, and never anything a tool was told in confidence.
fn acted_on(args: &Value, reply: &str) -> String {
    let shorten = |text: &str| text.trim().chars().take(80).collect::<String>();
    for field in ["title", "name"] {
        if let Some(value) = args.get(field).and_then(Value::as_str) {
            if !value.trim().is_empty() {
                return shorten(value);
            }
        }
    }
    serde_json::from_str::<Value>(reply)
        .ok()
        .and_then(|value| {
            value
                .get("title")
                .or_else(|| value.get("name"))
                .and_then(Value::as_str)
                .map(shorten)
        })
        .unwrap_or_default()
}

/// What the log says the agent did, in facts rather than in a tool's name: the action, and
/// the kind of thing it was done to. The window turns both into a sentence of its own
/// language - "workspace_delete" tells a person nothing about what was deleted.
fn journal_facts(name: &str, args: &Value) -> (&'static str, &'static str) {
    let verb = if name.contains("_delete") {
        "deleted"
    } else if name.contains("_create") {
        "created"
    } else if name.contains("_update") || name.contains("_rename") {
        "updated"
    } else if name.contains("_open") {
        "opened"
    } else if name.contains("_close") {
        "closed"
    } else if name.contains("_like") {
        if args.get("liked").and_then(Value::as_bool) == Some(false) { "unliked" } else { "liked" }
    } else if name.contains("_split") {
        "split"
    } else if name.contains("_import") {
        "imported"
    } else {
        "ran"
    };
    let kind = if name.contains("workspace") {
        "session"
    } else if name.contains("playlist") {
        "playlist"
    } else if name.contains("stems") {
        "stems"
    } else if name.contains("song") || name.contains("library") {
        "song"
    } else {
        "other"
    };
    (verb, kind)
}

/// What a removal is about, in the person's words: a question about deleting
/// something has to say what that something is, not only the tool's own name.
async fn removal_target(name: &str, args: &Value) -> String {
    let argument = |field: &str| args.get(field).and_then(Value::as_str).map(str::to_string);
    let lookup = if name.contains("workspace") {
        argument("workspace_id").map(|id| ("/v1/workspaces".to_string(), id))
    } else if name.contains("playlist") {
        argument("playlist_id").map(|id| ("/v1/playlists".to_string(), id))
    } else {
        argument("song_id").map(|id| (format!("/v1/library/songs/{id}"), id))
    };
    let Some((path, wanted)) = lookup else {
        return acted_on(args, "");
    };
    let found = fetch(&path).await;
    let item = match found {
        Value::Array(items) => items.into_iter().find(|item| item.get("id").and_then(Value::as_str) == Some(wanted.as_str())),
        Value::Object(_) => Some(found),
        _ => None,
    };
    item.and_then(|item| item.get("title").or_else(|| item.get("name")).and_then(Value::as_str).map(|name| name.trim().chars().take(80).collect::<String>()))
        .unwrap_or_default()
}

/// An agent's call that changes something reaches the windows. The window
/// never calls the MCP server itself, so each such notice is an agent's doing.
fn announce_change(tool: &str) {
    if annotations(tool)["readOnlyHint"].as_bool() == Some(false) {
        announce(tool);
    }
}

async fn ask_window(command: &str, args: Value, seconds: u64) -> Result<Value, String> {
    let Some(window) = window_to_ask() else {
        return Err("The studio's window is not open. Open YuE2 Studio and call the tool again; everything else works without it.".into());
    };
    let id = format!("w{}", bridge().sequence.fetch_add(1, Ordering::Relaxed));
    let (sender, receiver) = tokio::sync::oneshot::channel();
    bridge().pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(id.clone(), sender);
    let _ = bridge().commands.send(json!({ "id": id, "window": window, "command": command, "args": args }).to_string());
    match tokio::time::timeout(Duration::from_secs(seconds), receiver).await {
        Ok(Ok(answer)) => answer,
        _ => {
            bridge().pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&id);
            Err(format!("The window did not answer '{command}' within {seconds} s."))
        }
    }
}

/// The studio's questions for its writing assistant when the user has made
/// the connected agent that assistant: each waits here until the agent
/// answers it with assistant_request_answer.
struct AgentQuestion {
    question: Value,
    answer: tokio::sync::oneshot::Sender<String>,
}

struct Agent {
    questions: Mutex<Vec<AgentQuestion>>,
    /// When an agent last called the server, and what it called.
    last_call: Mutex<Option<(std::time::Instant, String)>>,
    calls: AtomicU64,
}

fn agent() -> &'static Agent {
    static AGENT: OnceLock<Agent> = OnceLock::new();
    AGENT.get_or_init(|| Agent { questions: Mutex::new(Vec::new()), last_call: Mutex::new(None), calls: AtomicU64::new(0) })
}

fn questions() -> std::sync::MutexGuard<'static, Vec<AgentQuestion>> {
    agent().questions.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// An agent that called within this long still counts as connected.
const AGENT_PRESENT: Duration = Duration::from_secs(600);
/// How long a question waits for the agent's answer.
const AGENT_ANSWER: Duration = Duration::from_secs(900);

fn seen(what: &str) {
    *agent().last_call.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((std::time::Instant::now(), what.to_string()));
    agent().calls.fetch_add(1, Ordering::Relaxed);
}

fn agent_present() -> bool {
    agent().last_call.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_ref().is_some_and(|(at, _)| at.elapsed() < AGENT_PRESENT)
}

/// The questions still waiting for an answer; one whose asker gave up is gone.
fn open_questions() -> Vec<Value> {
    let mut waiting = questions();
    waiting.retain(|question| !question.answer.is_closed());
    waiting.iter().map(|question| question.question.clone()).collect()
}

/// Asks the connected agent what the studio would ask its own assistant: the
/// same instructions, the same request, the same answer schema.
pub async fn ask_agent(system: &str, user: &str, schema: Option<Value>, target: &str) -> Result<String, String> {
    if !agent_present() {
        return Err("No agent is connected to the studio. Connect one to its MCP server (Settings, Agent) or choose another assistant.".into());
    }
    let id = format!("q{}", bridge().sequence.fetch_add(1, Ordering::Relaxed));
    let (sender, receiver) = tokio::sync::oneshot::channel();
    questions().push(AgentQuestion { question: json!({ "id": id, "target": target, "instructions": system, "request": user, "answer_schema": schema }), answer: sender });
    match tokio::time::timeout(AGENT_ANSWER, receiver).await {
        Ok(Ok(answer)) => Ok(answer),
        _ => {
            questions().retain(|question| question.question["id"] != id.as_str());
            Err(format!("The agent did not answer within {} minutes.", AGENT_ANSWER.as_secs() / 60))
        }
    }
}

/// Waits until the studio has a question for the agent, a slice at a time.
async fn wait_for_questions(args: &Value) -> Value {
    let seconds = args.get("seconds").and_then(Value::as_u64).unwrap_or(WAIT_DEFAULT).clamp(1, WAIT_LONGEST);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
    loop {
        let waiting = open_questions();
        if !waiting.is_empty() {
            return json!({ "requests": waiting });
        }
        if tokio::time::Instant::now() >= deadline {
            return json!({ "requests": [], "note": "no request yet; call assistant_requests_wait again to keep listening" });
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn answer_question(args: &Value) -> Result<String, String> {
    let id = text(args, "request_id")?;
    let answer = match args.get("answer") {
        Some(Value::String(text)) => text.clone(),
        Some(value) if !value.is_null() => value.to_string(),
        _ => return Err("'answer' is required".into()),
    };
    let found = {
        let mut waiting = questions();
        waiting.iter().position(|question| question.question["id"] == id.as_str()).map(|at| waiting.remove(at))
    };
    let Some(question) = found else {
        return Err(format!("No request {id} is waiting; assistant_requests_wait lists the open ones."));
    };
    question.answer.send(answer).map_err(|_| format!("Request {id} was given up by the studio before the answer came."))?;
    Ok("Answered; the studio goes on with it.".into())
}

/// Whether an agent and the window are connected, for the settings page.
pub async fn status() -> Json<Value> {
    let last = agent().last_call.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    Json(json!({
        "window_open": !open_windows().is_empty(),
        "agent_connected": agent_present(),
        "agent_last_call": last.as_ref().map(|(_, what)| what.clone()),
        "agent_seconds_ago": last.as_ref().map(|(at, _)| at.elapsed().as_secs()),
        "agent_calls": agent().calls.load(Ordering::Relaxed),
        "requests_waiting": open_questions().len(),
    }))
}

/// A required text argument, or a message saying which is missing.
fn text(args: &Value, name: &str) -> Result<String, String> {
    args.get(name).and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()).map(str::to_string).ok_or_else(|| format!("'{name}' is required"))
}

/// A path segment, escaped.
pub(crate) fn segment(value: &str) -> String {
    value.bytes().map(|byte| if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) { (byte as char).to_string() } else { format!("%{byte:02X}") }).collect()
}

/// The arguments without the ones that went into the path.
fn body_without(args: &Value, taken: &[&str]) -> Value {
    let mut body = args.as_object().cloned().unwrap_or_default();
    for name in taken {
        body.remove(*name);
    }
    Value::Object(body)
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": properties, "required": required })
}

fn id_only(name: &str, what: &str) -> Value {
    object(json!({ name: { "type": "string", "description": what } }), &[name])
}

fn nothing() -> Value {
    json!({ "type": "object", "additionalProperties": false })
}

/// The files under a folder that `keep` takes, with their path inside it: the
/// folders a song sits in name its artist when its tags do not. A folder
/// linked from inside itself is walked once.
fn folder_files(folder: &Path, keep: fn(&Path) -> bool, what: &str) -> Result<Vec<(String, PathBuf, String)>, String> {
    let root = folder.parent().unwrap_or(folder);
    let mut found = Vec::new();
    let mut walked = std::collections::HashSet::new();
    let mut stack = vec![folder.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if !walked.insert(std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone())) {
            continue;
        }
        let entries = std::fs::read_dir(&dir).map_err(|error| format!("read {}: {error}", dir.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if keep(&path) {
                let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                found.push(("files".to_string(), path, relative));
            }
        }
    }
    found.sort_by(|a, b| a.2.cmp(&b.2));
    if found.is_empty() {
        return Err(format!("no {what} in {}", folder.display()));
    }
    Ok(found)
}

/// Audio, lyrics and cue files: what a dropped folder of songs brings.
fn song_file(path: &Path) -> bool {
    let extension = path.extension().and_then(|value| value.to_str()).unwrap_or_default().to_lowercase();
    ["wav", "mp3", "flac", "ogg", "m4a", "aiff", "aif", "txt", "lrc", "cue"].contains(&extension.as_str())
}

/// A dataset folder of the studio family: its dataset.json and WAV files.
fn dataset_file(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "dataset.json") || path.extension().is_some_and(|value| value.eq_ignore_ascii_case("wav"))
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| "audio".into())
}

/// The tools that put a new track in the library: a track always belongs to the
/// session it was made in, so these need one open - `song_create`, a replay, a
/// stem split and audio processing. Reading (karaoke words, MIDI notes, a score
/// for a later song) does not make a track and needs no session.
fn makes_a_track(name: &str) -> bool {
    matches!(name, "song_create" | "song_replay" | "stems_split" | "processing_start")
}

/// The makers whose work belongs to a session: every one of them can be told where its tracks
/// go, and without a word the studio asks the person - once for the whole pack of work.
/// `score_compose` and `score_transcribe` are not here: a score is not a track, and a session
/// of its own for every score would only leave empty sessions behind.
fn takes_a_session(name: &str) -> bool {
    matches!(
        name,
        "song_create" | "song_replay" | "stems_split" | "processing_start" | "karaoke_make" | "midi_transcribe" | "library_import_audio"
    )
}

/// What the agent may say about where its tracks go, offered on every maker.
fn session_field() -> Value {
    json!({
        "type": "string",
        "description": "Where the tracks go: each (a session of its own for every track, named after the track), current (the session open now), or new:<name> (one new session for them all). Leave it out and the studio asks the person once for the whole pack of work."
    })
}

/// The schema the agent sees for a tool: only the makers carry the extra word about sessions,
/// so one place adds it instead of ten schemas repeating it.
fn schema_of(tool: &Tool) -> Value {
    let mut schema = (tool.schema)();
    if takes_a_session(tool.name) {
        if let Some(fields) = schema.get_mut("properties").and_then(Value::as_object_mut) {
            fields.insert("session".into(), session_field());
        }
    }
    schema
}

/// Tools whose work cannot be undone: deleting, overwriting, closing a session,
/// splitting a track. On the `risky` leash (the default) these are the ones the
/// user is asked about before anything happens.
fn is_destructive(name: &str) -> bool {
    matches!(name, "song_delete" | "library_song_delete" | "playlist_delete" | "library_song_update" | "workspace_close" | "workspace_delete" | "stems_split" | "processing_discard" | "library_version_delete" | "song_job_cancel" | "processing_cancel")
}

/// Is this a removal - something gone for good? The person is asked one plain question about
/// those: do it, or do not. Remembering a session-wide "yes" for a deletion, or a tool that
/// keeps asking after they already answered, are answers nobody offered.
fn is_removal(name: &str) -> bool {
    name.contains("delete")
}

/// Tools that only read or only look: on the `all` leash everything else is put
/// to the user, so a question about reading the library would be pure noise.
fn is_read_only(name: &str) -> bool {
    (name.starts_with("library_") && !name.contains("delete") && !name.contains("update") && !name.contains("like"))
        || (name.starts_with("workspace_") && name != "workspace_close" && name != "workspace_create")
        || matches!(name, "studio_status" | "studio_wait" | "studio_system" | "player_state" | "player_get" | "equalizer_get" | "visualizer_get" | "winamp_get" | "create_form_get" | "agent_settings" | "agent_leash" | "ui_screenshot" | "ui_read_page" | "ui_console" | "writing_guide" | "writing_examples")
}

/// The person's "never allow": tools the agent may not run at all, whatever it asks.
async fn forbidden_tools() -> Vec<String> {
    fetch("/v1/agent/forbidden")
        .await
        .get("forbidden")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

/// Forbidden outright? Nothing else about the request matters then.
async fn is_forbidden(name: &str) -> bool {
    forbidden_tools().await.iter().any(|tool| tool == name)
}

/// May the agent work inside this session without being asked? The person answered
/// "in this session" once and the answer is kept with the session itself.
async fn session_allows(session: Option<&str>) -> bool {
    let Some(id) = session else {
        return false;
    };
    fetch(&format!("/v1/workspaces/{id}"))
        .await
        .get("agent_allow")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// The one switch for the whole studio: the agent makes tracks without asking.
async fn tracks_without_asking() -> bool {
    fetch("/v1/agent/allowance")
        .await
        .get("allow")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// The active session, if one is open.
async fn open_session() -> Option<String> {
    fetch("/v1/workspaces/active").await.get("id").and_then(Value::as_str).map(str::to_string)
}

/// Answer "in this session": the session keeps it, not a table of its own.
async fn allow_in_session(session: &str) {
    if let Ok(call) = send(Method::PUT, format!("/v1/workspaces/{session}/agent"), json!({ "allow": true })) {
        let _ = call_route(call).await;
    }
}

/// Answer "always": the studio's one switch for tracks.
async fn allow_tracks_always() {
    if let Ok(call) = send(Method::PUT, "/v1/agent/allowance".into(), json!({ "allow": true })) {
        let _ = call_route(call).await;
    }
}

/// Answer "ask every time": take back both answers, so the question returns.
async fn ask_every_time_again(session: Option<&str>) {
    if let Ok(call) = send(Method::PUT, "/v1/agent/allowance".into(), json!({ "allow": false })) {
        let _ = call_route(call).await;
    }
    if let Some(id) = session {
        if let Ok(call) = send(Method::PUT, format!("/v1/workspaces/{id}/agent"), json!({ "allow": false })) {
            let _ = call_route(call).await;
        }
    }
}

/// Answer "never allow": the tool joins the list the studio keeps for good.
async fn forbid_for_good(name: &str) {
    let mut tools = forbidden_tools().await;
    if !tools.iter().any(|tool| tool == name) {
        tools.push(name.to_string());
    }
    if let Ok(call) = send(Method::PUT, "/v1/agent/forbidden".into(), json!({ "forbidden": tools })) {
        let _ = call_route(call).await;
    }
}
/// The leash, in front of every tool that changes something: `free` lets it pass, a
/// remembered answer is taken as it is, and anything else is put to the user in the
/// window - once, for the session, always, ask or never. The studio asks BEFORE it
/// acts, so nothing happens behind the user's back and the agent never widens its
/// own leash: only the user answers, and only the window records the answer.
async fn agent_may(name: &str, args: &Value) -> Result<(), String> {
    if is_forbidden(name).await {
        return Err(format!("The user forbade {name}; agent_settings lists what is forbidden."));
    }
    let settings = fetch("/v1/agent/leash").await;
    let leash = settings.get("leash").and_then(Value::as_str).unwrap_or("risky");
    if leash == "free" {
        return Ok(());
    }
    let guarded = if leash == "all" { !is_read_only(name) } else { is_destructive(name) };
    if !guarded {
        return Ok(());
    }
    let session = open_session().await;
    if session_allows(session.as_deref()).await {
        return Ok(());
    }
    let removal = is_removal(name);
    let target = if removal { removal_target(name, &args).await } else { String::new() };
    let question = json!({ "action": name, "scope": session.clone().map(|id| format!("session:{id}")).unwrap_or_else(|| "always".into()), "leash": leash, "removal": removal, "target": target, "details": args });
    let answer = ask_window("agent_confirm", question, 300).await?;
    let decision = answer.get("decision").and_then(Value::as_str).unwrap_or("deny");
    match decision {
        "once" => Ok(()),
        "session" => match session {
            Some(id) => {
                allow_in_session(&id).await;
                Ok(())
            }
            None => Err("There is no open session to allow this in; open one first.".into()),
        },
        "deny_always" => {
            forbid_for_good(name).await;
            Err(format!("The user forbade {name}; agent_settings lists what is forbidden."))
        }
        "ask" => {
            ask_every_time_again(session.as_deref()).await;
            Err(format!("The user asked to be asked every time about {name}; nothing was done."))
        }
        _ => Err(format!("The user did not answer {name} with a yes; nothing was done.")),
    }
}

/// The person's answer to "where do these tracks go", remembered for the pack of work at hand:
/// while calls keep coming the answer stands, and after a quiet spell the next pack is asked
/// about again. Saying "always" writes it among the agent settings instead.
struct PackChoice {
    word: String,
    /// The session `new:<name>` made for this pack, so it is not made again for every track.
    workspace: Option<String>,
    at: std::time::Instant,
}

static PACK: std::sync::OnceLock<std::sync::Mutex<Option<PackChoice>>> = std::sync::OnceLock::new();

/// How long a quiet stretch has to be before the studio asks about the session again.
const PACK_QUIET_SECONDS: u64 = 15 * 60;

fn pack_store() -> &'static std::sync::Mutex<Option<PackChoice>> {
    PACK.get_or_init(|| std::sync::Mutex::new(None))
}

/// The word the pack of work is running under, if that pack is still going; reading it also
/// keeps the pack alive, so a run that never stops asking never gets asked twice.
fn pack_word() -> Option<String> {
    let mut held = pack_store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let going = held.as_ref().is_some_and(|choice| choice.at.elapsed().as_secs() < PACK_QUIET_SECONDS);
    if !going {
        *held = None;
        return None;
    }
    let word = held.as_ref().map(|choice| choice.word.clone());
    if let Some(choice) = held.as_mut() {
        choice.at = std::time::Instant::now();
    }
    word
}

fn hold_pack(word: &str, workspace: Option<String>) {
    *pack_store().lock().unwrap_or_else(|poisoned| poisoned.into_inner()) =
        Some(PackChoice { word: word.to_string(), workspace, at: std::time::Instant::now() });
}

/// The session `new:<name>` already made for the pack at hand.
fn pack_new_workspace(name: &str) -> Option<String> {
    let wanted = format!("new:{name}");
    let held = pack_store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    held.as_ref().filter(|choice| choice.word == wanted).and_then(|choice| choice.workspace.clone())
}

/// The person's standing answer, when they said "always" instead of "for this pack".
async fn session_choice_for_good() -> Option<String> {
    let word = fetch("/v1/agent/session-choice").await.get("choice").and_then(Value::as_str).unwrap_or_default().to_string();
    (!word.is_empty()).then_some(word)
}

async fn remember_session_choice_for_good(word: &str) {
    if let Ok(call) = send(Method::PUT, "/v1/agent/session-choice".into(), json!({ "choice": word })) {
        let _ = call_route(call).await;
    }
}

/// A session made on the spot, by its name. It is made closed, exactly as when the agent asks
/// for one by hand: only the person opens a session.
async fn make_session(name: &str) -> Result<String, String> {
    let call = Call { method: Method::POST, path: "/v1/workspaces".into(), payload: Payload::Json(json!({ "name": name, "song_ids": [] })) };
    match call_route(call).await {
        Ok((status, text)) if status.is_success() => serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|created| created.get("id").and_then(Value::as_str).map(str::to_string))
            .ok_or_else(|| format!("the studio made a session but answered: {text}")),
        Ok((status, text)) => Err(format!("the studio refused a session named {name}: {status} {text}")),
        Err(problem) => Err(problem),
    }
}

/// The name a session gets when it is made for one track: the track's own name.
fn track_name(args: &Value) -> String {
    let title = args.get("title").and_then(Value::as_str).map(str::trim).filter(|title| !title.is_empty());
    let style = args.get("style").and_then(Value::as_str).map(str::trim).filter(|style| !style.is_empty());
    title.or(style).unwrap_or("Track").chars().take(48).collect()
}

/// The session this call's tracks go into. The agent may name it on the call; saying nothing
/// means the person decides - once for the pack of work at hand, and for good if they say so.
async fn session_for_call(name: &str, args: &Value) -> Result<Option<String>, String> {
    if let Some(word) = args.get("session").and_then(Value::as_str).map(str::trim).filter(|word| !word.is_empty()).map(str::to_string) {
        return session_by_word(&word, args).await;
    }
    if let Some(word) = pack_word() {
        return session_by_word(&word, args).await;
    }
    if let Some(word) = session_choice_for_good().await {
        return session_by_word(&word, args).await;
    }
    // Nobody has said where the work goes: the person is asked, the way the leash asks.
    let active = fetch("/v1/workspaces/active").await;
    let asked = ask_window(
        "session_choice",
        json!({
            "tool": name,
            "title": acted_on(args, "{}"),
            "has_session": active.get("id").and_then(Value::as_str).is_some(),
            "session_name": active.get("name").and_then(Value::as_str).unwrap_or_default(),
        }),
        300,
    )
    .await;
    let Ok(answer) = asked else {
        // Nobody to ask - the window is away: the open session is the only place work can go.
        return Ok(open_session().await);
    };
    let word = answer.get("choice").and_then(Value::as_str).unwrap_or_default().trim().to_string();
    if word.is_empty() || word == "deny" {
        return Err("The person did not say where the track goes; nothing was made.".into());
    }
    if answer.get("always").and_then(Value::as_bool).unwrap_or(false) && !word.starts_with("new:") {
        remember_session_choice_for_good(&word).await;
    }
    if answer.get("remember").and_then(Value::as_bool).unwrap_or(true) {
        let made = session_by_word(&word, args).await?;
        if !word.starts_with("new:") {
            hold_pack(&word, made.clone());
        }
        return Ok(made);
    }
    session_by_word(&word, args).await
}

/// One of the three words carried out: a session per track, the open one, or a new one.
async fn session_by_word(word: &str, args: &Value) -> Result<Option<String>, String> {
    match word {
        "current" => match open_session().await {
            Some(id) => Ok(Some(id)),
            None => Err("No session is open and the person has not said where the track goes: open a session, or name one - session: current, each, or new:<name>.".into()),
        },
        "each" => Ok(Some(make_session(&track_name(args)).await?)),
        other if other.starts_with("new:") => {
            let name = other.trim_start_matches("new:").trim().to_string();
            if name.is_empty() {
                return Err("new: needs a name for the session, like new:Rain.".into());
            }
            if let Some(made) = pack_new_workspace(&name) {
                return Ok(Some(made));
            }
            let made = make_session(&name).await?;
            hold_pack(&format!("new:{name}"), Some(made.clone()));
            Ok(Some(made))
        }
        other => Err(format!("session is \"each\", \"current\" or \"new:<name>\", not \"{other}\".")),
    }
}

/// Hands the chosen session to the route the tool is about to call: a field of the JSON body,
/// a field of the form, or the only body a POST without one can carry.
fn give_the_session(call: &mut Call, workspace: &str) {
    match &mut call.payload {
        Payload::Json(body) => {
            if let Some(fields) = body.as_object_mut() {
                fields.insert("workspace_id".into(), Value::String(workspace.to_string()));
            } else {
                call.payload = Payload::Json(json!({ "workspace_id": workspace }));
            }
        }
        Payload::Form { fields, .. } => fields.push(("workspace_id".into(), workspace.to_string())),
        Payload::None if call.method == Method::POST => call.payload = Payload::Json(json!({ "workspace_id": workspace })),
        _ => {}
    }
}
fn tools() -> &'static [Tool] {
    static TOOLS: OnceLock<Vec<Tool>> = OnceLock::new();
    TOOLS.get_or_init(|| {
        vec![
            // ---------------------------------------------------------------- the studio
            Tool {
                name: "studio_status",
                description: "What the studio is doing now, in one short summary: song jobs, covers and karaoke, the stem split, the MIDI transcription, audio processing, the dataset preparation and its stages, the training run with its step, loss and KL, and whether the studio's window is open. Call it first, and use studio_wait to wait.",
                schema: nothing,
                call: |_| composite("status"),
            },
            Tool {
                name: "studio_wait",
                description: "Wait for work to finish instead of polling: a song or score job (job_id), or until one kind of work is over - song_jobs, covers_and_karaoke, stems, midi, processing, preparation, training - or everything (until: idle, the default). Returns when it is done or after seconds (30 by default, at most 55, under the minute clients allow a call) with how far it got; call it again to keep waiting.",
                schema: || object(json!({ "job_id": { "type": "string" }, "until": { "type": "string", "enum": ["idle", "song_jobs", "covers_and_karaoke", "stems", "midi", "processing", "preparation", "training"] }, "seconds": { "type": "integer" } }), &[]),
                call: |_| composite("wait"),
            },
            Tool {
                name: "studio_system",
                description: "The graphics card, its free video memory, RAM, CPU load and the engine's memory.",
                schema: nothing,
                call: |_| get("/v1/system/resources".into()),
            },
            Tool {
                name: "models_status",
                description: "The installed model set, what is downloaded and what is missing, and download progress.",
                schema: nothing,
                call: |_| get("/setup/status".into()),
            },
            Tool {
                name: "models_catalog",
                description: "Every model set and file the studio can download, with sizes and the video memory each needs.",
                schema: nothing,
                call: |_| get("/setup/catalog".into()),
            },
            Tool {
                name: "models_download",
                description: "Download a model set (profile_id from models_catalog) or single components (ids). Resumes what is partly there.",
                schema: || object(json!({ "profile_id": { "type": "string" }, "ids": { "type": "array", "items": { "type": "string" } } }), &[]),
                call: |args| post("/setup/download".into(), args.clone()),
            },
            Tool {
                name: "models_adopt",
                description: "Take model files already on this computer instead of downloading them: every file of the catalogue found in the folder (by name, else by exact size) is linked into the studio's models. models_status shows the result.",
                schema: || id_only("path", "folder with the model files"),
                call: |args| post("/setup/adopt".into(), json!({ "path": text(args, "path")? })),
            },
            Tool {
                name: "song_defaults",
                description: "What the engine does with a song_create field left out: its default steps, guidance, duration and sampling, its version, and the weights it serves.",
                schema: nothing,
                call: |_| get("/v1/local-models/music".into()),
            },
            Tool {
                name: "models_select",
                description: "Use an installed model set (profile_id) or a custom mix of components (component_ids) for generation.",
                schema: || object(json!({ "profile_id": { "type": "string" }, "component_ids": { "type": "array", "items": { "type": "string" } } }), &[]),
                call: |args| post("/setup/select".into(), args.clone()),
            },
            Tool {
                name: "engine_options_get",
                description: "The engine's launch options: backend (auto, cuda, vulkan, cpu), keep models loaded, and the rest.",
                schema: nothing,
                call: |_| get("/engine/options".into()),
            },
            Tool {
                name: "engine_options_set",
                description: "Change the engine's launch options (the whole object as engine_options_get returns it, with your changes); engine_restart applies them.",
                schema: || json!({ "type": "object" }),
                call: |args| send(Method::PUT, "/engine/options".into(), args.clone()),
            },
            Tool {
                name: "engine_restart",
                description: "Restart the music engine, for example after changing its options or models.",
                schema: nothing,
                call: |_| post("/engine/restart".into(), json!({})),
            },
            // ---------------------------------------------------------------- the window: what the user sees
            Tool {
                name: "ui_screenshot",
                description: "A picture of the studio's window as the user sees it now. Use it to check what a change looks like, and before clicking anything.",
                schema: || object(json!({ "max_width": { "type": "integer", "description": "pixels, 1600 by default" } }), &[]),
                call: |args| window("screenshot", args, 30),
            },
            Tool {
                name: "ui_read_page",
                description: "Every visible control of the window, one per line: its ref (e12), kind, label and value. Refs are what ui_click, ui_type and ui_select take; read the page again after it changes.",
                schema: nothing,
                call: |args| window("read_page", args, 15),
            },
            Tool {
                name: "ui_click",
                description: "Click a control of the window like the user would: by ref from ui_read_page, or by its visible label in text.",
                schema: || object(json!({ "ref": { "type": "string" }, "text": { "type": "string" } }), &[]),
                call: |args| window("click", args, 15),
            },
            Tool {
                name: "ui_type",
                description: "Type into a field of the window (replaces its text); submit presses Enter after.",
                schema: || object(json!({ "ref": { "type": "string" }, "text": { "type": "string", "description": "the field's label, when there is no ref" }, "value": { "type": "string" }, "submit": { "type": "boolean" } }), &["value"]),
                call: |args| window("type", args, 15),
            },
            Tool {
                name: "ui_select",
                description: "Choose an option of a list in the window.",
                schema: || object(json!({ "ref": { "type": "string" }, "text": { "type": "string" }, "value": { "type": "string" } }), &["value"]),
                call: |args| window("select", args, 15),
            },
            Tool {
                name: "ui_press_key",
                description: "Press a key in the window: Enter, Escape (closes a dialog), Tab, ArrowDown..., or a shortcut with modifiers joined by +: Ctrl+M (the Winamp mode on and off), Shift+Tab.",
                schema: || id_only("key", "the key"),
                call: |args| window("press_key", args, 15),
            },
            Tool {
                name: "ui_scroll",
                description: "Scroll the page (direction down or up, amount in pixels), or bring a control into view by ref or text.",
                schema: || object(json!({ "direction": { "type": "string", "enum": ["down", "up"] }, "amount": { "type": "integer" }, "ref": { "type": "string" }, "text": { "type": "string" } }), &[]),
                call: |args| window("scroll", args, 15),
            },
            Tool {
                name: "ui_navigate",
                description: "Open a page of the studio: create, library, search, playlist, adapters (LoRA and training), tools, news.",
                schema: || object(json!({ "view": { "type": "string", "enum": ["create", "library", "search", "playlist", "adapters", "tools", "news"] } }), &["view"]),
                call: |args| window("navigate", args, 15),
            },
            Tool {
                name: "ui_open_settings",
                description: "Open the settings window, at a section when given.",
                schema: || object(json!({ "section": { "type": "string" } }), &[]),
                call: |args| window("open_settings", args, 15),
            },
            // ---------------------------------------------------------------- the agent as the studio's assistant
            Tool {
                name: "assistant_requests_wait",
                description: "When the user has chosen you as the studio's writing assistant (assistant engine 'Agent (MCP)'), what the studio would ask its own assistant waits here: the create page's write buttons, and the lyric layout and the styles of a dataset preparation. Returns the waiting requests - id, target, the instructions the studio's assistant would get, the request, and the JSON schema the answer must match - as soon as there is one, or after seconds (30 by default, at most 55). Answer each with assistant_request_answer; keep calling while the user works.",
                schema: || object(json!({ "seconds": { "type": "integer" } }), &[]),
                call: |_| composite("questions"),
            },
            Tool {
                name: "assistant_request_answer",
                description: "Answer a request from assistant_requests_wait: answer is the JSON object its answer_schema describes (or plain text for a request without a schema), written by its instructions. The studio goes on with it as with its own assistant's answer.",
                schema: || object(json!({ "request_id": { "type": "string" }, "answer": { "anyOf": [{ "type": "object" }, { "type": "string" }] } }), &["request_id", "answer"]),
                call: |_| composite("answer"),
            },
            Tool {
                name: "ui_notify",
                description: "Show the user a short message in the studio's window, for a few seconds: what you did, what you need from them. tone: info (default), success or error.",
                schema: || object(json!({ "text": { "type": "string" }, "tone": { "type": "string", "enum": ["info", "success", "error"] } }), &["text"]),
                call: |args| window("notify", args, 15),
            },
            Tool {
                name: "ui_console",
                description: "The errors and warnings the studio's window logged lately, newest last: what went wrong on the page when a button did nothing.",
                schema: nothing,
                call: |args| window("console", args, 15),
            },
            // ---------------------------------------------------------------- the create page's form
            Tool {
                name: "create_form_get",
                description: "The create page's form as the user sees it now: every field, the request it would send, whether the model is ready, and the form's error if it refused. The create page must be open (ui_navigate create).",
                schema: nothing,
                call: |args| window("create_get", args, 15),
            },
            Tool {
                name: "create_form_set",
                description: "Fill the create page's form in the window, as if typed - the user sees every field change; fields not given stay. fields: title, style, lyrics, abc, cot (full|melody|off), duration_seconds, lm_batch_size, synth_batch_size, steps, cfg_scale, lm_seed, seed, randomize_seed, cover_prompt, output_format, mp3_bitrate, peak_clip, adapters, mode (studio|simple|cover). Use it when the user wants to see and adjust the song before it is made; song_create makes one directly.",
                schema: || object(json!({ "fields": { "type": "object", "description": "field -> value" } }), &["fields"]),
                call: |args| window("create_set", args, 15),
            },
            Tool {
                name: "create_form_submit",
                description: "Press Create on the create page: the song is made from the form as it stands. studio_wait until idle waits for it; create_form_get shows why the form refused, if it did.",
                schema: nothing,
                call: |args| window("create_submit", args, 15),
            },
            // ---------------------------------------------------------------- the player
            Tool {
                name: "player_state",
                description: "What the studio's player plays: the song, playing or paused, position, length, volume, shuffle, repeat.",
                schema: nothing,
                call: |args| window("player_state", args, 15),
            },
            Tool {
                name: "player_play",
                description: "Play a library song (song_id) in the studio's player, or resume what is loaded. song_ids plays a list of songs as the queue, from its first (the day's best: library_songs_list since today, library_liked). With stem (drums, bass, other, vocals, guitar, piano) it plays that separated stem of the song alone; stems_split makes them, stems_get lists them.",
                schema: || object(json!({ "song_id": { "type": "string" }, "song_ids": { "type": "array", "items": { "type": "string" } }, "stem": { "type": "string", "enum": ["drums", "bass", "other", "vocals", "guitar", "piano"] } }), &[]),
                call: |args| window("player_play", args, 15),
            },
            Tool {
                name: "player_pause",
                description: "Pause the studio's player.",
                schema: nothing,
                call: |args| window("player_pause", args, 15),
            },
            Tool {
                name: "player_seek",
                description: "Move the player to a position in seconds.",
                schema: || object(json!({ "seconds": { "type": "number" } }), &["seconds"]),
                call: |args| window("player_seek", args, 15),
            },
            Tool {
                name: "player_next",
                description: "Play the next song of the queue.",
                schema: nothing,
                call: |args| window("player_next", args, 15),
            },
            Tool {
                name: "player_previous",
                description: "Play the previous song of the queue.",
                schema: nothing,
                call: |args| window("player_previous", args, 15),
            },
            Tool {
                name: "player_set",
                description: "Set the player's volume (0 to 1), shuffle, and repeat (none, all, one).",
                schema: || object(json!({ "volume": { "type": "number" }, "shuffle": { "type": "boolean" }, "repeat": { "type": "string", "enum": ["none", "all", "one", "stop"], "description": "stop: play the current track to its end and stay there" } }), &[]),
                call: |args| window("player_set", args, 15),
            },
            // ---------------------------------------------------------------- the equalizer, the visualiser, the Winamp mode
            Tool {
                name: "equalizer_get",
                description: "The player's ten-band equalizer (Winamp's bands, 60 Hz to 16 kHz, -12..+12 dB): on or off, preamp, every band, the preset it came from, balance, mono, whether its panel is open, and every preset by name - Winamp's, ones for common situations (Bass Booster, Vocal Booster, Night Listening...), the user's own. Balance and mono apply with the equalizer off too. In the Winamp mode, winamp_get shows Winamp's own equalizer.",
                schema: nothing,
                call: |args| window("equalizer_get", args, 15),
            },
            Tool {
                name: "equalizer_set",
                description: "Set the player's equalizer, as its panel does; what is not given stays. preset: a preset by name (turns the equalizer on). bands: ten values in dB from 60 Hz up, or an object hz -> dB for some. preamp_db. enabled. balance: -1 all left .. 1 all right. mono. panel_open: show or hide the equalizer panel. panel_position: {x, y} of the panel in the window, in pixels. save_preset: keep the current curve as the user's preset under this name. delete_preset: remove a preset of the user's. Returns the equalizer as equalizer_get does.",
                schema: || object(json!({
                    "preset": { "type": "string" },
                    "bands": { "anyOf": [{ "type": "array", "items": { "type": "number" }, "minItems": 10, "maxItems": 10 }, { "type": "object" }] },
                    "preamp_db": { "type": "number" },
                    "enabled": { "type": "boolean" },
                    "balance": { "type": "number", "minimum": -1, "maximum": 1 },
                    "mono": { "type": "boolean" },
                    "panel_open": { "type": "boolean" },
                    "panel_position": { "type": "object", "properties": { "x": { "type": "number" }, "y": { "type": "number" } } },
                    "save_preset": { "type": "string" },
                    "delete_preset": { "type": "string" }
                }), &[]),
                call: |args| window("equalizer_set", args, 15),
            },
            Tool {
                name: "equalizer_import",
                description: "Import a Winamp .EQF preset file from this computer: its presets join the user's own and the first is switched on.",
                schema: || object(json!({ "path": { "type": "string" } }), &["path"]),
                call: |args| {
                    use base64::Engine;
                    let path = PathBuf::from(text(args, "path")?);
                    let bytes = std::fs::read(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
                    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                    window("equalizer_import", &json!({ "name": name, "data": base64::engine::general_purpose::STANDARD.encode(bytes) }), 15)
                },
            },
            Tool {
                name: "equalizer_export",
                description: "Save the equalizer's current curve as a Winamp .EQF file at path (a full path ending in .eqf); name is the preset's name inside it, the current preset's by default.",
                schema: || object(json!({ "path": { "type": "string" }, "name": { "type": "string" } }), &["path"]),
                call: |_| composite("equalizer_export"),
            },
            Tool {
                name: "visualizer_get",
                description: "The visualiser: where it is (closed, panel over the studio, window of its own), fullscreen, the engine (milkdrop or spectrum), the MilkDrop preset on screen, the spectrum look and the looks there are, held or cycling, random or in order, the preset name shown, seconds per preset, the panel's size.",
                schema: nothing,
                call: |args| window("visualizer_get", args, 15),
            },
            Tool {
                name: "visualizer_set",
                description: "Work the visualiser as its buttons and keys do; what is not given stays. place: closed | panel (floating over the studio, resizable) | window (a window of its own). fullscreen. engine: milkdrop | spectrum. preset: a MilkDrop preset by name or part of one (visualizer_presets lists them). look: a spectrum look (visualizer_get lists them). step: next | previous. locked: stay on this preset. random: random or in order. show_name. cycle_seconds: how long MilkDrop stays on a preset. panel_size: {width, height} and panel_position: {x, y} in pixels. It hears the player after the equalizer; press play for it to move.",
                schema: || object(json!({
                    "place": { "type": "string", "enum": ["closed", "panel", "window"] },
                    "fullscreen": { "type": "boolean" },
                    "engine": { "type": "string", "enum": ["milkdrop", "spectrum"] },
                    "preset": { "type": "string" },
                    "look": { "type": "string" },
                    "step": { "type": "string", "enum": ["next", "previous"] },
                    "locked": { "type": "boolean" },
                    "random": { "type": "boolean" },
                    "show_name": { "type": "boolean" },
                    "cycle_seconds": { "type": "number", "minimum": 3 },
                    "panel_size": { "type": "object", "properties": { "width": { "type": "number" }, "height": { "type": "number" } } },
                    "panel_position": { "type": "object", "properties": { "x": { "type": "number" }, "y": { "type": "number" } } }
                }), &[]),
                call: |args| window("visualizer_set", args, 30),
            },
            Tool {
                name: "visualizer_presets",
                description: "The MilkDrop presets by name (several hundred, from Butterchurn's packs); search narrows them to names containing the text.",
                schema: || object(json!({ "search": { "type": "string" } }), &[]),
                call: |args| window("visualizer_presets", args, 30),
            },
            Tool {
                name: "winamp_get",
                description: "The Winamp mode, where the whole studio window becomes a classic Winamp 2 player: whether it is on, the skin, random skin, double size, always on top, scale, skip_menu, and while on its windows (main, equalizer, playlist, milkdrop: open, shaded), the song and position, playing, volume, balance, shuffle, repeat, its equalizer and its MilkDrop preset. While it is on, the player_* tools drive Winamp.",
                schema: nothing,
                call: |args| window("winamp_get", args, 15),
            },
            Tool {
                name: "winamp_set",
                description: "Work the Winamp mode as its buttons do; what is not given stays. on: switch the studio into Winamp, starting from the player's queue, song and position, or back (the song, position, volume and equalizer go back to the studio's player). skin: a skin by name (winamp_skins). random_skin: a different skin each time it opens. double_size. always_on_top. scale: how large the whole player is drawn, 1 to 3 (1.2 = 120 %). skip_menu: the player bar's Winamp button starts the mode at once, its settings open on a long press. windows: {main|equalizer|playlist|milkdrop: {open, shade}}. song_id: play a library song in it. action: play | pause | stop | next | previous. seek_seconds. volume 0..1. balance -1..1. shuffle. repeat. equalizer: {on, preamp, bands: ten dB values}. milkdrop: {preset (name), next, random, cycling}.",
                schema: || object(json!({
                    "on": { "type": "boolean" },
                    "skin": { "type": "string" },
                    "random_skin": { "type": "boolean" },
                    "double_size": { "type": "boolean" },
                    "always_on_top": { "type": "boolean" },
                    "scale": { "type": "number", "minimum": 1, "maximum": 3 },
                    "skip_menu": { "type": "boolean" },
                    "windows": { "type": "object" },
                    "song_id": { "type": "string" },
                    "action": { "type": "string", "enum": ["play", "pause", "stop", "next", "previous"] },
                    "seek_seconds": { "type": "number" },
                    "volume": { "type": "number" },
                    "balance": { "type": "number" },
                    "shuffle": { "type": "boolean" },
                    "repeat": { "type": "boolean" },
                    "equalizer": { "type": "object", "properties": { "on": { "type": "boolean" }, "preamp": { "type": "number" }, "bands": { "type": "array", "items": { "type": "number" } } } },
                    "milkdrop": { "type": "object", "properties": { "preset": { "type": "string" }, "next": { "type": "boolean" }, "random": { "type": "boolean" }, "cycling": { "type": "boolean" } } }
                }), &[]),
                call: |args| window("winamp_set", args, 30),
            },
            Tool {
                name: "winamp_skins",
                description: "The Winamp skins there are: the ones the studio carries and the ones the user added, which is chosen, whether it is random, and the Winamp Skin Museum's address.",
                schema: nothing,
                call: |args| window("winamp_skins", args, 15),
            },
            Tool {
                name: "winamp_skin_add",
                description: "Add a Winamp 2 skin (.wsz) from this computer to the studio's skins; name defaults to the file's. winamp_set skin then puts it on.",
                schema: || object(json!({ "path": { "type": "string" }, "name": { "type": "string" } }), &["path"]),
                call: |_| composite("winamp_skin_add"),
            },
            Tool {
                name: "winamp_museum",
                description: "Open the Winamp Skin Museum (skins.webamp.org) in the user's browser, where skins are found and downloaded.",
                schema: nothing,
                call: |args| window("winamp_museum", args, 15),
            },
            // ---------------------------------------------------------------- the video editor
            Tool {
                name: "video_open",
                description: "Open the video editor for a library song: an audio-reactive clip with a visualiser, background, cover, text layers and karaoke lyrics, rendered to MP4. The studio's window must be visible (not minimised, not a hidden tab): a hidden window holds the preview and the render.",
                schema: || id_only("song_id", "library song id"),
                call: |args| window("video_open", args, 15),
            },
            Tool {
                name: "video_get",
                description: "Everything the open video editor is set to: preset and the list of presets, aspect ratio, colours, positions, effects and their intensities, text layers, lyrics overlay, background, cover, playback, and the render's progress and saved file.",
                schema: nothing,
                call: |args| window("video_get", args, 15),
            },
            Tool {
                name: "video_set",
                description: "Change the open video editor; any part, the rest stays. config: {preset, aspectRatio 16:9|9:16|1:1, primaryColor, secondaryColor, bgDim 0-1, particleCount, visualizerX/Y 0-100, visualizerScale 0.3-2, lyricsX/Y}. effects: {shake, glitch, vhs, cctv, scanlines, chromatic, bloom, filmGrain, pixelate, strobe, vignette, hueShift, letterbox: true/false}; intensities: the same names, 0-1. text_layers: [{text, x, y, size, color, font}], replaces all. lyrics: {enabled, style lines|scroll|karaoke, position, font_size, lines, show_sections, color, background_color, background_opacity, highlight_color, offset_seconds}. background: {image_path | video_path | image_url | video_url | type: random}. album_art_path, or album_art: null for the song cover.",
                schema: || object(json!({
                    "config": { "type": "object" },
                    "effects": { "type": "object" },
                    "intensities": { "type": "object" },
                    "text_layers": { "type": "array", "items": { "type": "object" } },
                    "lyrics": { "type": "object" },
                    "background": { "type": "object" },
                    "album_art_path": { "type": "string" },
                    "album_art": { "anyOf": [{ "type": "string" }, { "type": "null" }] }
                }), &[]),
                call: |args| {
                    let mut args = args.clone();
                    // files on this computer reach the page as data URLs
                    let data_url = |path: &str| -> Result<String, String> {
                        use base64::Engine;
                        let path = PathBuf::from(path);
                        let bytes = std::fs::read(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
                        let kind = match path.extension().and_then(|value| value.to_str()).unwrap_or_default().to_lowercase().as_str() {
                            "jpg" | "jpeg" => "image/jpeg",
                            "png" => "image/png",
                            "webp" => "image/webp",
                            "gif" => "image/gif",
                            "mp4" => "video/mp4",
                            "webm" => "video/webm",
                            "mov" => "video/quicktime",
                            other => return Err(format!("{other} files cannot be a background or a cover")),
                        };
                        Ok(format!("data:{kind};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
                    };
                    if let Some(background) = args.get_mut("background").and_then(Value::as_object_mut) {
                        if let Some(path) = background.remove("image_path").and_then(|value| value.as_str().map(str::to_string)) {
                            background.insert("image".into(), data_url(&path)?.into());
                        }
                        if let Some(path) = background.remove("video_path").and_then(|value| value.as_str().map(str::to_string)) {
                            background.insert("video".into(), data_url(&path)?.into());
                        }
                        if let Some(url) = background.remove("image_url") {
                            background.insert("image".into(), url);
                        }
                        if let Some(url) = background.remove("video_url") {
                            background.insert("video".into(), url);
                        }
                    }
                    if let Some(path) = args.get("album_art_path").and_then(Value::as_str).map(str::to_string) {
                        args["album_art"] = data_url(&path)?.into();
                        args.as_object_mut().map(|fields| fields.remove("album_art_path"));
                    }
                    window("video_set", &args, 30)
                },
            },
            Tool {
                name: "video_render",
                description: "Render the open video editor's clip to MP4. It runs in the window and returns at once; video_get shows the progress; export.saved names the file on this computer when it is done, export.error says why it failed.",
                schema: || object(json!({ "name": { "type": "string", "description": "file name, the song title by default" } }), &[]),
                call: |args| window("video_render", args, 15),
            },
            Tool {
                name: "video_play",
                description: "Play the clip's preview in the video editor.",
                schema: nothing,
                call: |args| window("video_play", args, 15),
            },
            Tool {
                name: "video_pause",
                description: "Pause the clip's preview.",
                schema: nothing,
                call: |args| window("video_pause", args, 15),
            },
            Tool {
                name: "video_seek",
                description: "Move the clip's preview to a position in seconds, to look at a frame with ui_screenshot.",
                schema: || object(json!({ "seconds": { "type": "number" } }), &["seconds"]),
                call: |args| window("video_seek", args, 15),
            },
            Tool {
                name: "video_close",
                description: "Close the video editor.",
                schema: nothing,
                call: |args| window("video_close", args, 15),
            },
            // ---------------------------------------------------------------- songs
            Tool {
                name: "song_create",
                description: "Generate a song with YuE2. style: one English sentence (language, genre, vocal, instruments, mood, production, 'N BPM'). lyrics: sections tagged [Verse 1], [Chorus], [Bridge]... one per line, blank line between sections; empty for an instrumental. abc: a score to sing (from score_compose or an edited one), else the model writes its own. cot: full (melody and chords), melody, or off. adapters: installed LoRA ids with strengths per slot (see lora_list). Returns a job; poll song_job_get until completed, the song then is in the library.",
                schema: || object(json!({
                    "style": { "type": "string" },
                    "lyrics": { "type": "string" },
                    "title": { "type": "string" },
                    "abc": { "type": "string" },
                    "playlist_id": { "type": "string", "description": "a playlist (see playlist_list) the made songs are added to" },
                    "cot": { "type": "string", "enum": ["full", "melody", "off"] },
                    "duration_seconds": { "type": "number" },
                    "seed": { "type": "integer" },
                    "lm_seed": { "type": "integer" },
                    "steps": { "type": "integer" },
                    "cfg_scale": { "type": "number" },
                    "lm_batch_size": { "type": "integer", "description": "compositions written from the request (1 by default)" },
                    "synth_batch_size": { "type": "integer", "description": "performances rendered of each composition (1 by default)" },
                    "semantic_tokens": { "type": "string", "description": "audio codes of a song already sung (library_song_get audio_codes): renders that take again" },
                    "abc_sampling": { "type": "object", "description": "temperature, top_p, top_k, repetition_penalty, penalty_window, min_tokens, max_tokens", "properties": { "temperature": { "type": "number" }, "top_p": { "type": "number" }, "top_k": { "type": "integer" }, "repetition_penalty": { "type": "number" }, "penalty_window": { "type": "integer" }, "min_tokens": { "type": "integer" }, "max_tokens": { "type": "integer" } } },
                    "semantic_sampling": { "type": "object", "description": "temperature, top_p, top_k, repetition_penalty, penalty_window, min_tokens, max_tokens", "properties": { "temperature": { "type": "number" }, "top_p": { "type": "number" }, "top_k": { "type": "integer" }, "repetition_penalty": { "type": "number" }, "penalty_window": { "type": "integer" }, "min_tokens": { "type": "integer" }, "max_tokens": { "type": "integer" } } },
                    "peak_clip": { "type": "integer", "description": "peak limiter, dB below full scale" },
                    "mp3_bitrate": { "type": "integer" },
                    "output_format": { "type": "string", "enum": ["mp3", "wav16", "wav24", "wav32"] },
                    "cover_prompt": { "type": "string", "description": "what the cover should show; it is drawn only when an image model is set up (settings_get, covers), else the song has no cover" },
                    "cover_of": { "type": "string", "description": "for a cover: the library song whose melody abc came from (score_transcribe of it); the new song names it as the track it was made from" },
                    "adapters": { "type": "array", "items": { "type": "object", "properties": { "id": { "type": "string" }, "scales": { "type": "object", "description": "slot -> strength, e.g. {\"ar\": 1, \"nar\": 1}; left out, the LoRA's own strengths, else 1 on each slot it touches" } }, "required": ["id"] } }
                }), &["style"]),
                call: |args| post("/v1/music/jobs".into(), args.clone()),
            },
            Tool {
                name: "song_job_get",
                description: "A song job in short: status, phase, title, seeds, LoRA and, when completed, the library songs it made. response_format detailed gives the whole request.",
                schema: || object(json!({ "job_id": { "type": "string", "description": "from song_create or song_replay" }, "response_format": { "type": "string", "enum": ["concise", "detailed"], "description": "detailed gives every field" } }), &["job_id"]),
                call: |args| get(format!("/v1/music/jobs/{}", segment(&text(args, "job_id")?))),
            },
            Tool {
                name: "song_jobs_list",
                description: "Song jobs queued or running now.",
                schema: || object(json!({ "response_format": { "type": "string", "enum": ["concise", "detailed"], "description": "detailed gives every field" } }), &[]),
                call: |_| get("/v1/music/jobs".into()),
            },
            Tool {
                name: "song_job_cancel",
                description: "Stop a queued or running song job.",
                schema: || id_only("job_id", "the job to stop"),
                call: |args| post(format!("/v1/music/jobs/{}", segment(&text(args, "job_id")?)), json!({})),
            },
            Tool {
                name: "song_replay",
                description: "Render a library song again from its saved audio codes, bit for bit or with other steps, a new sound seed, a batch of variations, or another format - without composing again.",
                schema: || object(json!({ "song_id": { "type": "string" }, "steps": { "type": "integer" }, "seed": { "type": "integer" }, "synth_batch_size": { "type": "integer" }, "output_format": { "type": "string" }, "title": { "type": "string" } }), &["song_id"]),
                call: |args| post("/v1/music/replay".into(), args.clone()),
            },
            Tool {
                name: "score_compose",
                description: "Write only the score (ABC notation) for a style and lyrics, without singing it. Returns a job; poll score_job_get. Read and edit the score, then pass it to song_create as abc.",
                schema: || object(json!({ "style": { "type": "string" }, "lyrics": { "type": "string" }, "cot": { "type": "string", "enum": ["full", "melody"] }, "lm_seed": { "type": "integer" } }), &["style"]),
                call: |args| post("/v1/scores".into(), args.clone()),
            },
            Tool {
                name: "score_job_get",
                description: "A score job's status; when done it holds the ABC score. Works for score_compose and score_transcribe jobs.",
                schema: || id_only("job_id", "from score_compose or score_transcribe"),
                call: |args| get(format!("/v1/scores/{}", segment(&text(args, "job_id")?))),
            },
            Tool {
                name: "score_job_cancel",
                description: "Stop a score_compose or score_transcribe job.",
                schema: || id_only("job_id", "the job to stop"),
                call: |args| post(format!("/v1/scores/{}", segment(&text(args, "job_id")?)), json!({})),
            },
            Tool {
                name: "score_transcribe",
                description: "SheetSage2 listens to a recording and writes its melody (and chords) as a score, for a cover: pass song_id of a library track, or path of an audio file on this computer. melody_only leaves the chords out. Poll score_job_get.",
                schema: || object(json!({ "song_id": { "type": "string" }, "path": { "type": "string" }, "melody_only": { "type": "boolean" } }), &[]),
                call: |args| {
                    let mut fields = Vec::new();
                    let mut files = Vec::new();
                    if let Some(path) = args.get("path").and_then(Value::as_str).filter(|path| !path.trim().is_empty()) {
                        let path = PathBuf::from(path.trim());
                        files.push(("audio".to_string(), path.clone(), file_name(&path)));
                    } else {
                        fields.push(("song_id".to_string(), text(args, "song_id").map_err(|_| "song_id or path is required".to_string())?));
                    }
                    if args.get("melody_only").and_then(Value::as_bool).unwrap_or(false) {
                        fields.push(("melody_only".into(), "1".into()));
                    }
                    Ok(Call { method: Method::POST, path: "/v1/transcriptions".into(), payload: Payload::Form { fields, files } })
                },
            },
            // ---------------------------------------------------------------- how to write for the model
            Tool {
                name: "writing_guide",
                description: "How YuE2 wants to be written for - the rules the studio's own assistant follows. Read it before writing a style, lyrics or a score yourself. topic: song, style, lyrics, score, transcript, sections; none lists them.",
                schema: || object(json!({ "topic": { "type": "string", "enum": ["song", "style", "lyrics", "score", "transcript", "sections"] } }), &[]),
                call: |args| get(format!("/v1/writing/guide?topic={}", segment(args.get("topic").and_then(Value::as_str).unwrap_or_default()))),
            },
            Tool {
                name: "writing_examples",
                description: "The official YuE2 requests (style and lyrics) closest to a brief such as 'russian folk rock with accordion': write in their shape.",
                schema: || id_only("brief", "the idea, genre or mood"),
                call: |args| get(format!("/v1/writing/examples?brief={}", segment(&text(args, "brief")?))),
            },
            // ---------------------------------------------------------------- the writing assistant
            Tool {
                name: "assistant_sections",
                description: "Lay lyrics out in tagged sections (verse, chorus, bridge...) with their words untouched - the form's tag button. Tags already in the text are replaced. Returns the tagged lyrics.",
                schema: || object(json!({ "lyrics": { "type": "string" } }), &["lyrics"]),
                call: |args| post("/v1/assistant/sections".into(), args.clone()),
            },
            Tool {
                name: "assistant_write",
                description: "The studio's writing assistant. target: all (lyrics, style, title and cover prompt from an idea in description), lyrics (rewrite the lyrics to fit style), style (write the style for the lyrics), score (edit abc as instruction asks), transcript (lay out recognised words as a lyric sheet). Returns a draft to use in song_create.",
                schema: || object(json!({
                    "target": { "type": "string", "enum": ["all", "lyrics", "style", "score", "transcript"] },
                    "description": { "type": "string", "description": "The idea, or the text to work on" },
                    "instruction": { "type": "string" },
                    "lyrics": { "type": "string" },
                    "style": { "type": "string" },
                    "abc": { "type": "string" },
                    "duration_seconds": { "type": "number" }
                }), &["target"]),
                call: |args| post("/v1/assistant/write".into(), args.clone()),
            },
            Tool {
                name: "assistant_status",
                description: "Which writing assistant is set up (local model or OpenRouter) and whether it is running.",
                schema: nothing,
                call: |_| get("/v1/assistant/status".into()),
            },
            // ---------------------------------------------------------------- the library
            Tool {
                name: "library_songs_list",
                description: "The songs in the library, newest first: id, title, when made (unix seconds), made_from and made_by for a track a tool made from another (stems, processing, replay), and the start of the style. query filters by title or style; since and until (a date or date-time, or today / yesterday, in this computer's time zone) by when it was made. library_liked names the songs the user liked - the best ones; library_song_get gives one song whole. response_format detailed gives every field of every song.",
                schema: || object(json!({ "query": { "type": "string" }, "since": { "type": "string" }, "until": { "type": "string" }, "response_format": { "type": "string", "enum": ["concise", "detailed"] } }), &[]),
                call: |_| get("/v1/library/songs".into()),
            },
            Tool {
                name: "library_liked",
                description: "The ids of the songs the user liked (the thumbs-up in the library): the ones they count as the best. With library_songs_list since today it gives today's best. The mark lives with the song, so it survives a profile change.",
                schema: nothing,
                call: |_| get("/v1/library/liked".into()),
            },
            Tool {
                name: "library_song_like",
                description: "Like a library song (liked true, the default) or take the like back (liked false), as the thumbs-up in the library does. Kept in the song itself, not in one window.",
                schema: || object(json!({ "song_id": { "type": "string" }, "liked": { "type": "boolean" } }), &["song_id"]),
                call: |args| post(format!("/v1/library/songs/{}/liked", segment(&text(args, "song_id")?)), json!({ "liked": args.get("liked").and_then(Value::as_bool).unwrap_or(true) })),
            },
            Tool {
                name: "library_song_get",
                description: "One library song with its style, lyrics, score, settings and metadata (derived: what it was made from). Its audio codes and replay request only with response_format detailed.",
                schema: || object(json!({ "song_id": { "type": "string", "description": "library song id" }, "response_format": { "type": "string", "enum": ["concise", "detailed"], "description": "detailed gives every field" } }), &["song_id"]),
                call: |args| get(format!("/v1/library/songs/{}", segment(&text(args, "song_id")?))),
            },
            Tool {
                name: "library_song_update",
                description: "Change a library song: send the song as library_song_get returns it with your changes (title, caption, lyrics, metadata).",
                schema: || object(json!({ "song_id": { "type": "string" }, "song": { "type": "object" } }), &["song_id", "song"]),
                call: |args| send(Method::PUT, format!("/v1/library/songs/{}", segment(&text(args, "song_id")?)), args.get("song").cloned().unwrap_or_default()),
            },
            Tool {
                name: "library_song_delete",
                description: "Remove a song from the library.",
                schema: || id_only("song_id", "library song id"),
                call: |args| send(Method::DELETE, format!("/v1/library/songs/{}", segment(&text(args, "song_id")?)), json!({})),
            },
            Tool {
                name: "library_import_audio",
                description: "Add an audio file from this computer to the library.",
                schema: || object(json!({ "path": { "type": "string" }, "title": { "type": "string" } }), &["path"]),
                call: |args| {
                    let path = PathBuf::from(text(args, "path")?);
                    let title = args.get("title").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default());
                    let name = file_name(&path);
                    Ok(Call { method: Method::POST, path: "/v1/library/import".into(), payload: Payload::Form { fields: vec![("title".into(), title)], files: vec![("audio".into(), path, name)] } })
                },
            },
            Tool {
                name: "playlist_list",
                description: "The library's playlists.",
                schema: nothing,
                call: |_| get("/v1/library/playlists".into()),
            },
            Tool {
                name: "playlist_create",
                description: "Make a playlist of library songs.",
                schema: || object(json!({ "name": { "type": "string" }, "description": { "type": "string" }, "song_ids": { "type": "array", "items": { "type": "string" } } }), &["name"]),
                call: |args| post("/v1/library/playlists".into(), args.clone()),
            },
            Tool {
                name: "playlist_update",
                description: "Rename a playlist or set its songs.",
                schema: || object(json!({ "playlist_id": { "type": "string" }, "name": { "type": "string" }, "description": { "type": "string" }, "song_ids": { "type": "array", "items": { "type": "string" } } }), &["playlist_id", "name"]),
                call: |args| send(Method::PUT, format!("/v1/library/playlists/{}", segment(&text(args, "playlist_id")?)), body_without(args, &["playlist_id"])),
            },
            Tool {
                name: "playlist_delete",
                description: "Remove a playlist; its songs stay in the library.",
                schema: || id_only("playlist_id", "playlist id"),
                call: |args| send(Method::DELETE, format!("/v1/library/playlists/{}", segment(&text(args, "playlist_id")?)), json!({})),
            },
            // ---------------------------------------------------------------- sessions (workspaces)
            Tool {
                name: "workspace_list",
                description: "The work sessions of the studio, newest first: id, name, the songs each holds, when it was made and whether it is open. A session is where every track is made, and one session is open at a time - a track that appears joins the open one.",
                schema: nothing,
                call: |_| get("/v1/workspaces".into()),
            },
            Tool {
                name: "workspace_get",
                description: "One session by id: its name, the ids of the songs it holds, and whether it is open.",
                schema: || id_only("workspace_id", "session id from workspace_list"),
                call: |args| get(format!("/v1/workspaces/{}", segment(&text(args, "workspace_id")?))),
            },
            Tool {
                name: "workspace_create",
                description: "Make a session and hand it to the person: it is made closed and waits in the list, because opening a session is theirs to do in the window - the studio never switches their work behind their back. Names are unique: the studio does not rename on its own, so add \"(2)\" when a session already carries the name.",
                schema: || object(json!({ "name": { "type": "string", "description": "what the session is called" }, "song_ids": { "type": "array", "items": { "type": "string" }, "description": "library songs to put in it; a new session usually starts empty" } }), &["name"]),
                call: |args| post("/v1/workspaces".into(), json!({ "name": text(args, "name")?, "song_ids": args.get("song_ids").cloned().unwrap_or_else(|| json!([])) })),
            },
            Tool {
                name: "workspace_open",
                description: "Open a session: it becomes the current one, and whichever session was open is closed - the service keeps one open at a time.",
                schema: || id_only("workspace_id", "session id from workspace_list"),
                call: |args| post(format!("/v1/workspaces/{}/open", segment(&text(args, "workspace_id")?)), json!({})),
            },
            Tool {
                name: "workspace_close",
                description: "Close a session: it is filed away with its songs and no session is open afterwards, so open or make another before making tracks.",
                schema: || id_only("workspace_id", "session id from workspace_list"),
                call: |args| post(format!("/v1/workspaces/{}/close", segment(&text(args, "workspace_id")?)), json!({})),
            },
            Tool {
                name: "workspace_delete",
                description: "Remove a session for good; the tracks it holds stay in the library, only the session and its mark on them go. Tidy up the sessions you made and left empty - a session the person made is theirs to remove.",
                schema: || id_only("workspace_id", "session id from workspace_list"),
                call: |args| send(Method::DELETE, format!("/v1/workspaces/{}", segment(&text(args, "workspace_id")?)), json!({})),
            },
            // ---------------------------------------------------------------- the agent's leash
            Tool {
                name: "agent_settings",
                description: "What the person decided about the agent, all of it: the leash (free, risky or all), `tracks_without_asking` (the studio's one switch for making tracks), `forbidden` (tools the agent may never run) and `sessions_that_allow_the_agent` (sessions where the person answered \"in this session\").",
                schema: nothing,
                call: |_| composite("agent_settings"),
            },
            Tool {
                name: "agent_leash",
                description: "How close the agent is kept: free (no questions), risky (ask before what cannot be undone) or all (ask before everything that changes anything). The user sets it in Settings, Agent (MCP).",
                schema: nothing,
                call: |_| get("/v1/agent/leash".into()),
            },
            // ---------------------------------------------------------------- covers, karaoke, stems, processing
            Tool {
                name: "cover_draw",
                description: "Draw a library song's cover now, from its cover prompt or the cover template.",
                schema: || id_only("song_id", "library song id"),
                call: |args| post(format!("/v1/library/songs/{}/cover/auto", segment(&text(args, "song_id")?)), json!({})),
            },
            Tool {
                name: "cover_templates_get",
                description: "The cover prompt templates, with {title}, {style} and {excerpt} placeholders.",
                schema: nothing,
                call: |_| get("/v1/cover-templates".into()),
            },
            Tool {
                name: "cover_templates_set",
                description: "Save the cover templates (the object as cover_templates_get returns it, with your changes).",
                schema: || json!({ "type": "object" }),
                call: |args| send(Method::PUT, "/v1/cover-templates".into(), args.clone()),
            },
            Tool {
                name: "karaoke_make",
                description: "Time every word of a library song's lyrics against its vocals (enhanced LRC). language helps the recogniser.",
                schema: || object(json!({ "song_id": { "type": "string" }, "language": { "type": "string" } }), &["song_id"]),
                call: |args| post(format!("/v1/library/songs/{}/karaoke", segment(&text(args, "song_id")?)), body_without(args, &["song_id"])),
            },
            Tool {
                name: "stems_split",
                description: "Split a library song into six stems (drums, bass, other, vocals, guitar, piano) with HT-Demucs. Each stem becomes a track of the library made from the song (made_from, made_by stems), replacing the stems of an earlier split; stems_get names them. Wait with studio_wait until stems.",
                schema: || id_only("song_id", "library song id"),
                call: |args| post(format!("/v1/library/songs/{}/stems", segment(&text(args, "song_id")?)), json!({})),
            },
            Tool {
                name: "stems_archive_list",
                description: "The stems that were put away when a new set took their place: each keeps the song it was separated from (song_id names one song; leave it out for every song). stems_split makes a set, stems_get shows the current one.",
                schema: || object(json!({ "song_id": { "type": "string", "description": "library song id (every song when left out)" } }), &[]),
                call: |args| match args.get("song_id").and_then(Value::as_str) {
                    Some(id) => get(format!("/v1/library/stems-archive?song_id={}", segment(id))),
                    None => get("/v1/library/stems-archive".to_string()),
                },
            },
            Tool {
                name: "stems_get",
                description: "The stems of a library song and the progress of a split.",
                schema: || id_only("song_id", "library song id"),
                call: |args| get(format!("/v1/library/songs/{}/stems", segment(&text(args, "song_id")?))),
            },
            // ---------------------------------------------------------------- audio to MIDI
            Tool {
                name: "midi_status",
                description: "Audio to MIDI (MuScriptor, weights CC BY-NC 4.0 - non-commercial): whether the transcriber is installed, the model sizes (small 103M, medium 307M, large 1.4B) with what each still has to download, the download at work, and the transcription at work or the last one with its progress and file.",
                schema: nothing,
                call: |_| get("/v1/midi".into()),
            },
            Tool {
                name: "midi_transcribe",
                description: "Turn a library track - a song, a stem, a processed take - or any audio file on this computer into multi-instrument MIDI (34 instrument groups and drums). Give song_id or path; size small, medium (default) or large. What the transcriber needs is downloaded first when it is missing. Wait with studio_wait until midi; a track's MIDI is then kept beside it (midi_get, library_song_files), a file's in the studio's midi folder (midi_status run.file).",
                schema: || object(json!({ "song_id": { "type": "string" }, "path": { "type": "string", "description": "an audio file on this computer, instead of song_id" }, "size": { "type": "string", "enum": ["small", "medium", "large"] } }), &[]),
                call: |args| post("/v1/midi/transcribe".into(), args.clone()),
            },
            Tool {
                name: "midi_get",
                description: "A library track's MIDI: the file on this computer, the model size, when it was made, its instruments and how many notes. response_format detailed adds every note (pitch, start and end in seconds, instrument).",
                schema: || object(json!({ "song_id": { "type": "string" }, "response_format": { "type": "string", "enum": ["concise", "detailed"], "description": "detailed gives every note" } }), &["song_id"]),
                call: |args| get(format!("/v1/library/songs/{}/midi", segment(&text(args, "song_id")?))),
            },
            Tool {
                name: "midi_delete",
                description: "Remove a library track's MIDI.",
                schema: || id_only("song_id", "library song id"),
                call: |args| send(Method::DELETE, format!("/v1/library/songs/{}/midi", segment(&text(args, "song_id")?)), json!({})),
            },
            Tool {
                name: "midi_install",
                description: "Download the transcriber and a model size ahead of the first transcription (it also happens by itself on first use). midi_status shows the progress.",
                schema: || object(json!({ "size": { "type": "string", "enum": ["small", "medium", "large"] } }), &[]),
                call: |args| post("/v1/midi/install".into(), args.clone()),
            },
            Tool {
                name: "midi_remove",
                description: "Delete a model size's weights; the transcriber and the MIDI files stay.",
                schema: || object(json!({ "size": { "type": "string", "enum": ["small", "medium", "large"] } }), &["size"]),
                call: |args| post("/v1/midi/remove".into(), args.clone()),
            },
            Tool {
                name: "midi_cancel",
                description: "Stop the transcription at work, or the download it waits for.",
                schema: nothing,
                call: |_| post("/v1/midi/cancel".into(), json!({})),
            },
            Tool {
                name: "processing_start",
                description: "Run audio processing on a library song: any of denoise, lifter (Spectral Lifter), naturalize (vocal naturaliser), vst (plugin chain), master (to a reference). Each is an object of its settings; processing_get shows the result, then processing_keep or processing_discard.",
                schema: || object(json!({ "song_id": { "type": "string" }, "denoise": { "type": "object" }, "lifter": { "type": "object" }, "naturalize": { "type": "object" }, "vst": { "type": "array" }, "master": { "type": "object" } }), &["song_id"]),
                call: |args| post(format!("/v1/library/songs/{}/process", segment(&text(args, "song_id")?)), body_without(args, &["song_id"])),
            },
            Tool {
                name: "processing_get",
                description: "The processing at work or its finished result, waiting to be kept or discarded.",
                schema: nothing,
                call: |_| get("/v1/processing".into()),
            },
            Tool {
                name: "processing_keep",
                description: "Keep the processed audio as a new track of the library, made from the one processed: it names that track and the processing with its settings (metadata.derived), and wears its cover.",
                schema: || object(json!({ "label": { "type": "string", "description": "what the processing is called on the new track; its stages when left out" } }), &[]),
                call: |args| post("/v1/processing/keep".into(), args.clone()),
            },
            Tool {
                name: "processing_discard",
                description: "Throw the processed audio away.",
                schema: nothing,
                call: |_| post("/v1/processing/discard".into(), json!({})),
            },
            // ---------------------------------------------------------------- LoRA
            Tool {
                name: "lora_list",
                description: "Installed LoRA (id, name, trigger word, slots, default strengths) and the catalogue of ready ones in one line each, with download progress. Pass an installed id with a strength per slot in song_create adapters. response_format detailed gives every field in every language.",
                schema: || object(json!({ "response_format": { "type": "string", "enum": ["concise", "detailed"], "description": "detailed gives every field" } }), &[]),
                call: |_| get("/v1/adapters".into()),
            },
            Tool {
                name: "lora_install_catalog",
                description: "Download LoRA from the studio's catalogue by id.",
                schema: || object(json!({ "ids": { "type": "array", "items": { "type": "string" } } }), &["ids"]),
                call: |args| post("/v1/adapters/install".into(), args.clone()),
            },
            Tool {
                name: "lora_search_hf",
                description: "Search Hugging Face for LoRA for this model.",
                schema: || object(json!({ "q": { "type": "string" } }), &[]),
                call: |args| get(format!("/v1/adapters/hub?q={}", segment(args.get("q").and_then(Value::as_str).unwrap_or_default()))),
            },
            Tool {
                name: "lora_hf_files",
                description: "The adapter files in one Hugging Face repository.",
                schema: || id_only("repo", "owner/name"),
                call: |args| get(format!("/v1/adapters/hub/files?repo={}", segment(&text(args, "repo")?))),
            },
            Tool {
                name: "lora_install_hf",
                description: "Download adapter files from a Hugging Face repository into the studio.",
                schema: || object(json!({ "repo": { "type": "string" }, "paths": { "type": "array", "items": { "type": "string" } } }), &["repo", "paths"]),
                call: |args| post("/v1/adapters/hub/install".into(), args.clone()),
            },
            Tool {
                name: "lora_update",
                description: "Rename an installed LoRA, set its trigger word or its default strengths per slot.",
                schema: || object(json!({ "lora_id": { "type": "string" }, "name": { "type": "string" }, "trigger": { "type": "string" }, "scales": { "type": "object" } }), &["lora_id"]),
                call: |args| send(Method::PATCH, format!("/v1/adapters/{}", segment(&text(args, "lora_id")?)), body_without(args, &["lora_id"])),
            },
            Tool {
                name: "lora_delete",
                description: "Remove an installed LoRA.",
                schema: || id_only("lora_id", "installed LoRA id"),
                call: |args| send(Method::DELETE, format!("/v1/adapters/{}", segment(&text(args, "lora_id")?)), json!({})),
            },
            // ---------------------------------------------------------------- everything else the page does
            Tool {
                name: "models_cancel_download",
                description: "Stop the model download at work.",
                schema: nothing,
                call: |_| post("/setup/cancel".into(), json!({})),
            },
            Tool {
                name: "models_remove",
                description: "Delete downloaded model files by component id (see models_catalog).",
                schema: || object(json!({ "ids": { "type": "array", "items": { "type": "string" } } }), &["ids"]),
                call: |args| post("/setup/remove".into(), args.clone()),
            },
            Tool {
                name: "engine_logs",
                description: "The music engine's recent log, for when a song fails or the engine will not start.",
                schema: nothing,
                call: |_| get("/v1/engine/logs".into()),
            },
            Tool {
                name: "studio_capabilities",
                description: "What the studio can do with the installed models and providers, and which provider runs each capability.",
                schema: nothing,
                call: |_| get("/v1/capabilities".into()),
            },
            Tool {
                name: "settings_get",
                description: "The studio's settings: which provider (local or OpenRouter) runs music, lyrics timing, the assistant and covers, and their models.",
                schema: nothing,
                call: |_| get("/v1/configuration".into()),
            },
            Tool {
                name: "settings_set",
                description: "Save the studio's settings: the object as settings_get returns it, with your changes.",
                schema: || json!({ "type": "object" }),
                call: |args| send(Method::PUT, "/v1/configuration".into(), args.clone()),
            },
            Tool {
                name: "studio_open_data_folder",
                description: "Open the studio's data folder (models, songs, settings) in the file explorer for the user.",
                schema: nothing,
                call: |_| post("/v1/open-data-directory".into(), json!({})),
            },
            Tool {
                name: "assistant_set",
                description: "Choose the writing assistant: provider none, local (an OpenAI-compatible server at local_base_url with local_model), managed (a model the studio runs: managed_model, or a GGUF at managed_path) or openrouter (openrouter_model); reasoning_effort off, minimal, low, medium or high.",
                schema: || object(json!({ "provider": { "type": "string", "enum": ["none", "local", "managed", "openrouter"] }, "local_base_url": { "type": "string" }, "local_model": { "type": "string" }, "managed_model": { "type": "string" }, "managed_path": { "type": "string" }, "openrouter_model": { "type": "string" }, "reasoning_effort": { "type": "string" } }), &[]),
                call: |args| send(Method::PUT, "/v1/assistant/status".into(), args.clone()),
            },
            Tool {
                name: "assistant_local_models",
                description: "The models an OpenAI-compatible local server (LM Studio, Ollama...) at base offers.",
                schema: || id_only("base", "the server's base URL, e.g. http://127.0.0.1:1234/v1"),
                call: |args| get(format!("/v1/assistant/local-models?base={}", segment(&text(args, "base")?))),
            },
            Tool {
                name: "assistant_runtime",
                description: "The assistant models the studio can run itself: what is downloaded, what is running, download progress.",
                schema: nothing,
                call: |_| get("/v1/assistant/runtime".into()),
            },
            Tool {
                name: "assistant_model_install",
                description: "Download a managed assistant model or its runtime (asset_id and model_id from assistant_runtime).",
                schema: || object(json!({ "asset_id": { "type": "string" }, "model_id": { "type": "string" } }), &["asset_id"]),
                call: |args| post("/v1/assistant/runtime/install".into(), args.clone()),
            },
            Tool {
                name: "assistant_model_remove",
                description: "Delete a downloaded assistant model or runtime.",
                schema: || object(json!({ "asset_id": { "type": "string" }, "model_id": { "type": "string" } }), &["asset_id"]),
                call: |args| post("/v1/assistant/runtime/remove".into(), args.clone()),
            },
            Tool {
                name: "assistant_cancel_download",
                description: "Stop the assistant model download at work.",
                schema: nothing,
                call: |_| post("/v1/assistant/runtime/cancel".into(), json!({})),
            },
            Tool {
                name: "assistant_start",
                description: "Load a managed assistant model on the card now (it also starts by itself on first use).",
                schema: || object(json!({ "model_id": { "type": "string" }, "model_path": { "type": "string" } }), &["model_id"]),
                call: |args| post("/v1/assistant/runtime/start".into(), args.clone()),
            },
            Tool {
                name: "assistant_stop",
                description: "Unload the managed assistant model and free its video memory.",
                schema: nothing,
                call: |_| post("/v1/assistant/runtime/stop".into(), json!({})),
            },
            Tool {
                name: "karaoke_settings_get",
                description: "Lyrics timing and recognition: which recogniser (parakeet, whisper, openrouter), its model, on card or processor, what is downloaded.",
                schema: || object(json!({ "response_format": { "type": "string", "enum": ["concise", "detailed"], "description": "detailed gives every file of every model" } }), &[]),
                call: |_| get("/v1/karaoke/status".into()),
            },
            Tool {
                name: "karaoke_settings_set",
                description: "Choose the recogniser used for karaoke and for dataset lyrics: enabled, provider (parakeet, whisper, openrouter), whisper_model, openrouter_model, runtime (cpu, cuda).",
                schema: || object(json!({ "enabled": { "type": "boolean" }, "provider": { "type": "string" }, "whisper_model": { "type": "string" }, "openrouter_model": { "type": "string" }, "runtime": { "type": "string" } }), &[]),
                call: |args| send(Method::PUT, "/v1/karaoke/status".into(), args.clone()),
            },
            Tool {
                name: "recogniser_install",
                description: "Download a speech recogniser or its runtime (asset_id and model_id from karaoke_settings_get).",
                schema: || object(json!({ "asset_id": { "type": "string" }, "model_id": { "type": "string" } }), &["asset_id"]),
                call: |args| post("/v1/karaoke/install".into(), args.clone()),
            },
            Tool {
                name: "recogniser_remove",
                description: "Delete a downloaded speech recogniser or runtime.",
                schema: || object(json!({ "asset_id": { "type": "string" }, "model_id": { "type": "string" } }), &["asset_id"]),
                call: |args| post("/v1/karaoke/remove".into(), args.clone()),
            },
            Tool {
                name: "recogniser_cancel_download",
                description: "Stop the recogniser download at work.",
                schema: nothing,
                call: |_| post("/v1/karaoke/cancel".into(), json!({})),
            },
            Tool {
                name: "karaoke_delete",
                description: "Remove a library song's karaoke timings.",
                schema: || id_only("song_id", "library song id"),
                call: |args| send(Method::DELETE, format!("/v1/library/songs/{}/karaoke", segment(&text(args, "song_id")?)), json!({})),
            },
            Tool {
                name: "separator_status",
                description: "The stem separator: whether its model and runtime are installed, and download progress.",
                schema: nothing,
                call: |_| get("/v1/separation/status".into()),
            },
            Tool {
                name: "separator_runtime",
                description: "The ONNX runtimes the separator can use (card or processor) and what is downloaded.",
                schema: nothing,
                call: |_| get("/v1/separation/runtime".into()),
            },
            Tool {
                name: "separator_runtime_install",
                description: "Download a runtime for the separator (asset_id from separator_runtime).",
                schema: || id_only("asset_id", "runtime asset id"),
                call: |args| post("/v1/separation/runtime/install".into(), args.clone()),
            },
            Tool {
                name: "separator_cancel_download",
                description: "Stop the separator runtime download at work.",
                schema: nothing,
                call: |_| post("/v1/separation/runtime/cancel".into(), json!({})),
            },
            Tool {
                name: "separator_install",
                description: "Download the HT-Demucs stem model.",
                schema: nothing,
                call: |_| post("/v1/separation/install".into(), json!({})),
            },
            Tool {
                name: "separator_remove",
                description: "Delete the HT-Demucs stem model.",
                schema: nothing,
                call: |_| post("/v1/separation/remove".into(), json!({})),
            },
            Tool {
                name: "separator_settings_get",
                description: "Separator settings: runtime (card or processor) and overlap.",
                schema: nothing,
                call: |_| get("/v1/separation/settings".into()),
            },
            Tool {
                name: "separator_settings_set",
                description: "Save separator settings: the object as separator_settings_get returns it, with your changes.",
                schema: || json!({ "type": "object" }),
                call: |args| send(Method::PUT, "/v1/separation/settings".into(), args.clone()),
            },
            Tool {
                name: "library_song_files",
                description: "Where a library song's files are on this computer: its audio, its cover, each stem and its MIDI; read or open them directly.",
                schema: || id_only("song_id", "library song id"),
                call: |args| get(format!("/v1/library/songs/{}/files", segment(&text(args, "song_id")?))),
            },
            Tool {
                name: "cover_set_from_file",
                description: "Make an image file on this computer (PNG, JPEG, WebP) a library song's cover.",
                schema: || object(json!({ "song_id": { "type": "string" }, "path": { "type": "string" } }), &["song_id", "path"]),
                call: |args| {
                    use base64::Engine;
                    let path = PathBuf::from(text(args, "path")?);
                    let bytes = std::fs::read(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
                    let media_type = match path.extension().and_then(|value| value.to_str()).unwrap_or_default().to_lowercase().as_str() {
                        "jpg" | "jpeg" => "image/jpeg",
                        "webp" => "image/webp",
                        _ => "image/png",
                    };
                    send(Method::PUT, format!("/v1/library/songs/{}/cover", segment(&text(args, "song_id")?)), json!({ "image_base64": base64::engine::general_purpose::STANDARD.encode(bytes), "media_type": media_type }))
                },
            },
            Tool {
                name: "library_version_select",
                description: "Make one of a song's audio versions (the original, a processed one) the one it plays.",
                schema: || object(json!({ "song_id": { "type": "string" }, "version": { "type": "string" } }), &["song_id", "version"]),
                call: |args| send(Method::PUT, format!("/v1/library/songs/{}/version", segment(&text(args, "song_id")?)), json!({ "version": text(args, "version")? })),
            },
            Tool {
                name: "library_version_delete",
                description: "Remove one audio version of a song.",
                schema: || object(json!({ "song_id": { "type": "string" }, "version": { "type": "string" } }), &["song_id", "version"]),
                call: |args| send(Method::DELETE, format!("/v1/library/songs/{}/versions/{}", segment(&text(args, "song_id")?), segment(&text(args, "version")?)), json!({})),
            },
            Tool {
                name: "cover_prompt_render",
                description: "Fill a cover template for a song, to see the prompt a cover would be drawn from.",
                schema: || object(json!({ "template": { "type": "string" }, "song_id": { "type": "string" }, "title": { "type": "string" } }), &["template"]),
                call: |args| post("/v1/cover-templates/render".into(), args.clone()),
            },
            Tool {
                name: "openrouter_status",
                description: "Whether an OpenRouter key is set, and where it comes from.",
                schema: nothing,
                call: |_| get("/v1/openrouter/settings".into()),
            },
            Tool {
                name: "openrouter_set_key",
                description: "Store the user's OpenRouter API key (empty to remove it).",
                schema: || object(json!({ "api_key": { "type": "string" } }), &["api_key"]),
                call: |args| send(Method::PUT, "/v1/openrouter/settings".into(), args.clone()),
            },
            Tool {
                name: "openrouter_catalog",
                description: "OpenRouter's live model catalogue, by capability: music, transcription, text, images.",
                schema: nothing,
                call: |_| get("/v1/openrouter/catalog".into()),
            },
            Tool {
                name: "openrouter_catalog_refresh",
                description: "Fetch OpenRouter's model catalogue again.",
                schema: nothing,
                call: |_| post("/v1/openrouter/catalog/refresh".into(), json!({})),
            },
            Tool {
                name: "openrouter_log",
                description: "What was asked of OpenRouter and what came back, newest last.",
                schema: nothing,
                call: |_| get("/v1/openrouter/logs".into()),
            },
            Tool {
                name: "openrouter_complete",
                description: "Ask an OpenRouter text model a prompt.",
                schema: || object(json!({ "model_id": { "type": "string" }, "prompt": { "type": "string" } }), &["model_id", "prompt"]),
                call: |args| post("/v1/openrouter/completions".into(), args.clone()),
            },
            Tool {
                name: "openrouter_cover",
                description: "Draw an image with an OpenRouter image model from a prompt.",
                schema: || object(json!({ "model_id": { "type": "string" }, "prompt": { "type": "string" } }), &["model_id", "prompt"]),
                call: |args| post("/v1/openrouter/covers".into(), args.clone()),
            },
            Tool {
                name: "openrouter_transcribe",
                description: "Transcribe an audio file on this computer with an OpenRouter speech model.",
                schema: || object(json!({ "model_id": { "type": "string" }, "path": { "type": "string" }, "language": { "type": "string" } }), &["model_id", "path"]),
                call: |args| {
                    use base64::Engine;
                    let path = PathBuf::from(text(args, "path")?);
                    let bytes = std::fs::read(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
                    let format = path.extension().and_then(|value| value.to_str()).unwrap_or("wav").to_lowercase();
                    post("/v1/openrouter/transcriptions".into(), json!({ "model_id": text(args, "model_id")?, "audio_base64": base64::engine::general_purpose::STANDARD.encode(bytes), "audio_format": format, "language": args.get("language") }))
                },
            },
            Tool {
                name: "processing_set_reference",
                description: "Use an audio file on this computer as the reference processing_start masters to.",
                schema: || id_only("path", "reference audio file"),
                call: |args| {
                    let path = PathBuf::from(text(args, "path")?);
                    let name = file_name(&path);
                    Ok(Call { method: Method::POST, path: "/v1/processing/reference".into(), payload: Payload::Form { fields: Vec::new(), files: vec![("audio".into(), path, name)] } })
                },
            },
            Tool {
                name: "vst_list",
                description: "The VST3 plugins the studio found, for processing_start's vst chain.",
                schema: nothing,
                call: |_| get("/v1/processing/vst".into()),
            },
            Tool {
                name: "vst_scan",
                description: "Look for VST3 plugins again.",
                schema: nothing,
                call: |_| post("/v1/processing/vst/scan".into(), json!({})),
            },
            Tool {
                name: "vst_open_editor",
                description: "Open a VST3 plugin's own window for the user to set it; state_id keeps its settings for the chain.",
                schema: || object(json!({ "path": { "type": "string" }, "state_id": { "type": "string" } }), &["path"]),
                call: |args| post("/v1/processing/vst/editor".into(), args.clone()),
            },
            Tool {
                name: "lora_import_files",
                description: "Install LoRA files from this computer (.safetensors, with adapter_config.json or lora.json beside them when there are).",
                schema: || object(json!({ "name": { "type": "string" }, "paths": { "type": "array", "items": { "type": "string" } } }), &["name", "paths"]),
                call: |args| {
                    let paths: Vec<PathBuf> = args.get("paths").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(PathBuf::from).collect();
                    if paths.is_empty() {
                        return Err("'paths' is required".into());
                    }
                    let files = paths.into_iter().map(|path| { let name = file_name(&path); ("files".to_string(), path, name) }).collect();
                    Ok(Call { method: Method::POST, path: "/v1/adapters/import".into(), payload: Payload::Form { fields: vec![("name".into(), text(args, "name")?)], files } })
                },
            },
            Tool {
                name: "lora_cancel_download",
                description: "Stop the LoRA download at work.",
                schema: nothing,
                call: |_| post("/v1/adapters/cancel".into(), json!({})),
            },
            Tool {
                name: "dataset_import_folder",
                description: "Take a dataset folder another studio of the family wrote (its dataset.json and WAV files).",
                schema: || id_only("path", "the dataset folder"),
                call: |args| {
                    let files = folder_files(&PathBuf::from(text(args, "path")?), dataset_file, "dataset.json or WAV files")?;
                    Ok(Call { method: Method::POST, path: "/v1/training/datasets/import".into(), payload: Payload::Form { fields: Vec::new(), files } })
                },
            },
            Tool {
                name: "dataset_reveal",
                description: "Open a dataset's folder in the file explorer for the user.",
                schema: || id_only("dataset_id", "dataset id"),
                call: |args| post(format!("/v1/training/datasets/{}/reveal", segment(&text(args, "dataset_id")?)), json!({})),
            },
            Tool {
                name: "dataset_song_files",
                description: "Where a dataset song's audio and its separated vocals are on this computer.",
                schema: || object(json!({ "dataset_id": { "type": "string" }, "song_id": { "type": "string" } }), &["dataset_id", "song_id"]),
                call: |args| get(format!("/v1/training/datasets/{}/items/{}/files", segment(&text(args, "dataset_id")?), segment(&text(args, "song_id")?))),
            },
            Tool {
                name: "training_pack_cancel",
                description: "Stop the trainer download at work.",
                schema: nothing,
                call: |_| post("/v1/training/pack/cancel".into(), json!({})),
            },
            Tool {
                name: "dataset_prepare_train_after",
                description: "For the preparation at work: start this training run once every song is ready (train: {name, recipe}), or not (train: null).",
                schema: || object(json!({ "train": { "anyOf": [{ "type": "object" }, { "type": "null" }] } }), &[]),
                call: |args| post("/v1/training/prepare/train-after".into(), args.clone()),
            },
            // ---------------------------------------------------------------- training
            Tool {
                name: "training_status",
                description: "Training at a glance: every dataset with its song count and how many songs are ready, every run with its status, stage, last step, loss, KL and checkpoints, the preparation at work, whether the trainer and the listening pack are installed, and the recipe defaults to start a run from. response_format detailed gives everything, recipe fields included.",
                schema: || object(json!({ "response_format": { "type": "string", "enum": ["concise", "detailed"] } }), &[]),
                call: |_| get("/v1/training".into()),
            },
            Tool {
                name: "dataset_get",
                description: "One dataset with every song: id, title, artist, lyrics, style, lyrics_state (wanted, found, done) and style_state (wanted, heard, done), lyrics_source, heard (what MOSS heard: genre, caption, bpm) and length. Give song_id for one song only.",
                schema: || object(json!({ "dataset_id": { "type": "string" }, "song_id": { "type": "string" } }), &["dataset_id"]),
                call: |_| get("/v1/training".into()),
            },
            Tool {
                name: "training_pack_install",
                description: "Download the trainer and its weights (needed once before training).",
                schema: nothing,
                call: |_| post("/v1/training/pack/install".into(), json!({})),
            },
            Tool {
                name: "training_listen_pack_install",
                description: "Download MOSS-Music and the tempo model, which describe songs by ear during preparation.",
                schema: nothing,
                call: |_| post("/v1/training/listen/install".into(), json!({})),
            },
            Tool {
                name: "dataset_create",
                description: "Make an empty dataset. The trigger word is made from the name when left out.",
                schema: || object(json!({ "name": { "type": "string" }, "trigger": { "type": "string" } }), &["name"]),
                call: |args| post("/v1/training/datasets".into(), args.clone()),
            },
            Tool {
                name: "dataset_add_folder",
                description: "Add every song under a folder on this computer to a dataset, as a dropped folder: audio, .txt/.lrc lyrics beside it, albums cut by their .cue. Titles and artists come from tags, file names and folders.",
                schema: || object(json!({ "dataset_id": { "type": "string" }, "path": { "type": "string", "description": "folder or single audio file" } }), &["dataset_id", "path"]),
                call: |args| {
                    let path = PathBuf::from(text(args, "path")?);
                    let files = if path.is_dir() { folder_files(&path, song_file, "audio")? } else { vec![("files".to_string(), path.clone(), file_name(&path))] };
                    Ok(Call { method: Method::POST, path: format!("/v1/training/datasets/{}/files", segment(&text(args, "dataset_id")?)), payload: Payload::Form { fields: Vec::new(), files } })
                },
            },
            Tool {
                name: "dataset_add_library_songs",
                description: "Add library songs to a dataset.",
                schema: || object(json!({ "dataset_id": { "type": "string" }, "song_ids": { "type": "array", "items": { "type": "string" } } }), &["dataset_id", "song_ids"]),
                call: |args| post(format!("/v1/training/datasets/{}/songs", segment(&text(args, "dataset_id")?)), body_without(args, &["dataset_id"])),
            },
            Tool {
                name: "dataset_update",
                description: "Rename a dataset or change its trigger word.",
                schema: || object(json!({ "dataset_id": { "type": "string" }, "name": { "type": "string" }, "trigger": { "type": "string" } }), &["dataset_id"]),
                call: |args| send(Method::PATCH, format!("/v1/training/datasets/{}", segment(&text(args, "dataset_id")?)), body_without(args, &["dataset_id"])),
            },
            Tool {
                name: "dataset_delete",
                description: "Remove a dataset and its audio.",
                schema: || id_only("dataset_id", "dataset id"),
                call: |args| send(Method::DELETE, format!("/v1/training/datasets/{}", segment(&text(args, "dataset_id")?)), json!({})),
            },
            Tool {
                name: "dataset_song_update",
                description: "Write a dataset song's title, artist, style, lyrics (sections tagged [Verse 1], [Chorus]...) or instrumental flag. What you write is final: preparation leaves it alone.",
                schema: || object(json!({ "dataset_id": { "type": "string" }, "song_id": { "type": "string" }, "title": { "type": "string" }, "artist": { "type": "string" }, "style": { "type": "string" }, "lyrics": { "type": "string" }, "instrumental": { "type": "boolean" } }), &["dataset_id", "song_id"]),
                call: |args| send(Method::PATCH, format!("/v1/training/datasets/{}/items/{}", segment(&text(args, "dataset_id")?), segment(&text(args, "song_id")?)), body_without(args, &["dataset_id", "song_id"])),
            },
            Tool {
                name: "dataset_song_delete",
                description: "Remove a song from a dataset.",
                schema: || object(json!({ "dataset_id": { "type": "string" }, "song_id": { "type": "string" } }), &["dataset_id", "song_id"]),
                call: |args| send(Method::DELETE, format!("/v1/training/datasets/{}/items/{}", segment(&text(args, "dataset_id")?), segment(&text(args, "song_id")?)), json!({})),
            },
            Tool {
                name: "lyrics_find",
                description: "Look a song's published lyrics up in LRCLIB, QQ Music and Kugou by artist, title and length; returns the plain and timed lyrics word for word, or that it is an instrumental, or nothing.",
                schema: || object(json!({ "artist": { "type": "string" }, "title": { "type": "string" }, "seconds": { "type": "number" } }), &["title"]),
                call: |args| post("/v1/lyrics/find".into(), args.clone()),
            },
            Tool {
                name: "dataset_prepare",
                description: "Fill a dataset in by itself: lyrics from the lyric databases, Whisper only for songs they miss, sections laid out by the assistant, styles written from what MOSS-Music hears. lyrics and style: missing (only what is not done yet), all (again, for the chosen songs) or none. items: song ids, all songs when left out. train: start this run once every song is ready. Watch training_status.",
                schema: || object(json!({
                    "dataset_id": { "type": "string" },
                    "items": { "type": "array", "items": { "type": "string" } },
                    "lyrics": { "type": "string", "enum": ["none", "missing", "all"] },
                    "style": { "type": "string", "enum": ["none", "missing", "all"] },
                    "language": { "type": "string", "description": "sung language code when known, e.g. ru" },
                    "train": { "type": "object", "description": "not with writer agent: start the training once you have written the songs", "properties": { "name": { "type": "string" }, "recipe": { "type": "object" } } },
                    "writer": { "type": "string", "enum": ["studio", "agent"], "description": "agent: the studio finds, recognises and listens, and leaves the lyric layout and the styles to you - songs stay lyrics_state 'found' (lyrics as found, lyrics_source says from where) and style_state 'heard' (heard: genre, caption, bpm); write them with dataset_song_update, following writing_guide" }
                }), &["dataset_id"]),
                call: |args| post(format!("/v1/training/datasets/{}/prepare", segment(&text(args, "dataset_id")?)), body_without(args, &["dataset_id"])),
            },
            Tool {
                name: "lora_export_comfyui",
                description: "Write a trained LoRA as one file for ComfyUI's native YuE2 (planner and sound halves together, the stock LoRA loader takes it). path: full path ending in .safetensors. ar, nar: strength of each half folded in, 1 when left out.",
                schema: || object(json!({ "lora_id": { "type": "string" }, "path": { "type": "string" }, "ar": { "type": "number" }, "nar": { "type": "number" } }), &["lora_id", "path"]),
                call: |args| post(format!("/v1/adapters/{}/comfyui", segment(&text(args, "lora_id")?)), body_without(args, &["lora_id"])),
            },
            Tool {
                name: "library_song_describe_style",
                description: "Describe a library song's style by ear, for a cover of a recording that came without one: MOSS-Music hears it and the tempo is measured; with a writing assistant the answer is a YuE2 style line, without one what MOSS heard. Needs auto-describe installed; holds the card for a minute.",
                schema: || object(json!({ "song_id": { "type": "string" } }), &["song_id"]),
                call: |args| post(format!("/v1/library/songs/{}/describe-style", segment(&text(args, "song_id")?)), json!({})),
            },
            Tool {
                name: "dataset_take_as_is",
                description: "Without a writing assistant: keep the found lyrics as they are (no section tags) and the heard style as MOSS-Music described it, and mark those songs ready. items: song ids, all songs when left out.",
                schema: || object(json!({ "dataset_id": { "type": "string" }, "items": { "type": "array", "items": { "type": "string" } } }), &["dataset_id"]),
                call: |args| post(format!("/v1/training/datasets/{}/take-as-is", segment(&text(args, "dataset_id")?)), body_without(args, &["dataset_id"])),
            },
            Tool {
                name: "dataset_prepare_cancel",
                description: "Stop the preparation at work; every song keeps what it has.",
                schema: nothing,
                call: |_| post("/v1/training/prepare/cancel".into(), json!({})),
            },
            Tool {
                name: "training_start",
                description: "Train a LoRA on a dataset. recipe: the recipe from training_status (recipe_defaults) with your changes. preset picks the size: 'fast' (50 steps of 4 songs), 'balanced' (100 of 4, the default), 'thorough' (200 of 8) - the base-matched method, trained the way the base model was; or 'tuned', the previous recipe, with stop 'kl' and target_kl 1.4 or stop 'epochs' with epochs. Watch training_status; one run at a time, and it holds the card.",
                schema: || object(json!({ "dataset_id": { "type": "string" }, "name": { "type": "string" }, "recipe": { "type": "object" } }), &["dataset_id", "recipe"]),
                call: |args| post("/v1/training/runs".into(), args.clone()),
            },
            Tool {
                name: "training_continue",
                description: "Train a finished or stopped run further, from its latest checkpoint up to steps in all (training_status shows resume_step, or resume_refused with the reason). Same recipe and songs; the loss chart and the checkpoints go on from there. Stops by steps only.",
                schema: || object(json!({ "run_id": { "type": "string" }, "steps": { "type": "integer", "description": "the total steps to reach, above resume_step" } }), &["run_id", "steps"]),
                call: |args| post(format!("/v1/training/runs/{}/continue", segment(&text(args, "run_id")?)), json!({ "steps": args.get("steps").cloned().unwrap_or(Value::Null) })),
            },
            Tool {
                name: "training_cancel",
                description: "Stop a training run; its saved checkpoints stay.",
                schema: || id_only("run_id", "training run id"),
                call: |args| post(format!("/v1/training/runs/{}/cancel", segment(&text(args, "run_id")?)), json!({})),
            },
            Tool {
                name: "training_checkpoint_install",
                description: "Put a run's checkpoint (its step) into the LoRA library, ready to use in song_create.",
                schema: || object(json!({ "run_id": { "type": "string" }, "step": { "type": "integer" }, "name": { "type": "string" } }), &["run_id", "step"]),
                call: |args| {
                    let step = args.get("step").and_then(Value::as_u64).ok_or("'step' is required")?;
                    post(format!("/v1/training/runs/{}/checkpoints/{step}/install", segment(&text(args, "run_id")?)), body_without(args, &["run_id", "step"]))
                },
            },
            Tool {
                name: "training_run_delete",
                description: "Remove a finished training run and its checkpoints.",
                schema: || id_only("run_id", "training run id"),
                call: |args| send(Method::DELETE, format!("/v1/training/runs/{}", segment(&text(args, "run_id")?)), json!({})),
            },
        ]
    })
}

/// Builds the route's request, calls it inside the process and returns what
/// it answered, cut to a size an agent reads.
async fn call_route(call: Call) -> Result<(StatusCode, String), String> {
    let api = API.get().ok_or("the studio is still starting")?.clone();
    let builder = Request::builder().method(call.method).uri(&call.path);
    let request = match call.payload {
        Payload::None | Payload::Window { .. } => builder.body(Body::empty()),
        Payload::Json(body) => builder.header(header::CONTENT_TYPE, "application/json").body(Body::from(body.to_string())),
        Payload::Form { fields, files } => {
            let boundary = format!("studio-mcp-{}", uuid::Uuid::now_v7().simple());
            let mut parts: Vec<Part> = Vec::new();
            for (name, value) in fields {
                parts.push(Part::Text(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")));
            }
            for (name, path, file) in files {
                if let Err(error) = tokio::fs::metadata(&path).await {
                    return Err(format!("read {}: {error}", path.display()));
                }
                parts.push(Part::Text(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{}\"\r\nContent-Type: application/octet-stream\r\n\r\n", file.replace('"', "'"))));
                parts.push(Part::File(path));
                parts.push(Part::Text("\r\n".into()));
            }
            parts.push(Part::Text(format!("--{boundary}--\r\n")));
            builder.header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}")).body(Body::from_stream(streamed(parts)))
        }
    }
    .map_err(|error| error.to_string())?;
    let response = api.oneshot(request).await.map_err(|error| error.to_string())?;
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024 * 1024).await.map_err(|error| error.to_string())?;
    let mut text = match serde_json::from_slice::<Value>(&bytes) {
        Ok(value) => serde_json::to_string(&value).unwrap_or_default(),
        Err(_) => String::from_utf8_lossy(&bytes).into_owned(),
    };
    if text.trim().is_empty() {
        text = if status.is_success() { "Done.".into() } else { status.to_string() };
    }
    Ok((status, text))
}

/// A piece of a multipart body: text, or a file read as it is sent.
enum Part {
    Text(String),
    File(PathBuf),
}

/// The parts as a stream, a file a megabyte at a time: a folder of albums is
/// sent without ever being held in memory whole.
fn streamed(parts: Vec<Part>) -> impl futures_util::Stream<Item = std::io::Result<axum::body::Bytes>> {
    futures_util::stream::unfold((parts.into_iter(), None::<tokio::fs::File>), |(mut parts, mut open)| async move {
        loop {
            if let Some(file) = open.as_mut() {
                let mut chunk = vec![0u8; 1 << 20];
                match tokio::io::AsyncReadExt::read(file, &mut chunk).await {
                    Ok(0) => open = None,
                    Ok(read) => {
                        chunk.truncate(read);
                        return Some((Ok(chunk.into()), (parts, open)));
                    }
                    Err(error) => return Some((Err(error), (parts, None))),
                }
                continue;
            }
            match parts.next()? {
                Part::Text(text) => return Some((Ok(text.into()), (parts, None))),
                Part::File(path) => match tokio::fs::File::open(&path).await {
                    Ok(file) => open = Some(file),
                    Err(error) => return Some((Err(error), (parts, None))),
                },
            }
        }
    })
}

/// Cuts an answer to what an agent reads in one go, and says how to get the rest.
fn cut(mut text: String) -> String {
    if text.len() > LIMIT {
        let mut end = LIMIT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n... (cut: ask for one item, e.g. library_song_get or dataset_get with song_id)");
    }
    text
}

/// The protocol revisions the studio speaks: the stateless one, where every
/// request carries its version, and the handshake ones older clients open
/// with `initialize`.
const MODERN: &[&str] = &["2026-07-28"];
const LEGACY: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];
const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
/// How long a client may keep the tool list, the prompts and the guides:
/// they change only with the studio, but an update should show within minutes.
const LIST_TTL_MS: u64 = 300_000;
const HEADER_MISMATCH: i64 = -32020;
const UNSUPPORTED_VERSION: i64 = -32022;

fn server_info() -> Value {
    json!({ "name": env!("CARGO_PKG_NAME"), "title": STUDIO, "version": crate::studio_version() })
}

fn supported_versions() -> Vec<&'static str> {
    MODERN.iter().chain(LEGACY).copied().collect()
}

/// A complete result, signed with the server's identity.
fn rpc(id: Value, mut result: Value) -> Response {
    if let Some(fields) = result.as_object_mut() {
        fields.entry("resultType").or_insert_with(|| "complete".into());
        let meta = fields.entry("_meta").or_insert_with(|| json!({}));
        meta["io.modelcontextprotocol/serverInfo"] = server_info();
    }
    Json(json!({ "jsonrpc": "2.0", "id": id, "result": result })).into_response()
}

fn rpc_error(id: Value, code: i64, message: String) -> Response {
    rpc_failure(StatusCode::OK, id, code, message, None)
}

fn rpc_failure(status: StatusCode, id: Value, code: i64, message: String, data: Option<Value>) -> Response {
    let mut error = json!({ "code": code, "message": message });
    if let Some(data) = data {
        error["data"] = data;
    }
    (status, Json(json!({ "jsonrpc": "2.0", "id": id, "error": error }))).into_response()
}

/// A list or a read a client may cache: the same for everyone, fresh for five minutes.
fn cacheable(mut result: Value) -> Value {
    result["ttlMs"] = LIST_TTL_MS.into();
    result["cacheScope"] = "public".into();
    result
}

/// A header value, with the Base64 sentinel form decoded.
fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    use base64::Engine;
    let raw = headers.get(name)?.to_str().ok()?;
    match raw.strip_prefix("=?base64?").and_then(|inner| inner.strip_suffix("?=")) {
        Some(encoded) => base64::engine::general_purpose::STANDARD.decode(encoded).ok().and_then(|bytes| String::from_utf8(bytes).ok()),
        None => Some(raw.to_string()),
    }
}

/// Why a stateless request is refused, if it is: a version the studio does
/// not speak, or headers that do not say what its body says.
fn refused(headers: &HeaderMap, method: &str, params: &Value, version: &str) -> Option<(i64, String, Option<Value>)> {
    if !MODERN.contains(&version) {
        return Some((UNSUPPORTED_VERSION, "Unsupported protocol version".into(), Some(json!({ "supported": supported_versions(), "requested": version }))));
    }
    let mismatch = |what: String| Some((HEADER_MISMATCH, format!("Header mismatch: {what}"), None));
    match header_value(headers, "mcp-protocol-version") {
        Some(value) if value == version => {}
        Some(value) => return mismatch(format!("MCP-Protocol-Version header value '{value}' does not match body value '{version}'")),
        None => return mismatch("the MCP-Protocol-Version header is missing".into()),
    }
    match header_value(headers, "mcp-method") {
        Some(value) if value == method => {}
        Some(value) => return mismatch(format!("Mcp-Method header value '{value}' does not match body value '{method}'")),
        None => return mismatch("the Mcp-Method header is missing".into()),
    }
    let named = match method {
        "tools/call" | "prompts/get" => params.get("name"),
        "resources/read" => params.get("uri"),
        _ => return None,
    }
    .and_then(Value::as_str)
    .unwrap_or_default();
    match header_value(headers, "mcp-name") {
        Some(value) if value == named => None,
        Some(value) => mismatch(format!("Mcp-Name header value '{value}' does not match body value '{named}'")),
        None => mismatch("the Mcp-Name header is missing".into()),
    }
}

/// A tool's answer: the text an agent reads, and the same data structured
/// when it is JSON and whole.
fn tool_result(id: Value, text: String, structured: Option<Value>, error: bool) -> Response {
    let mut result = json!({ "content": [{ "type": "text", "text": text }], "isError": error });
    if let Some(structured) = structured.filter(|value| value.is_object() || value.is_array()) {
        result["structuredContent"] = structured;
    }
    rpc(id, result)
}

fn tool_json(id: Value, value: Value) -> Response {
    let text = serde_json::to_string_pretty(&value).unwrap_or_default();
    if text.len() > LIMIT {
        tool_result(id, cut(text), None, false)
    } else {
        tool_result(id, text, Some(value), false)
    }
}

/// The name a tool shows the user: its words, the first capitalised.
fn tool_title(name: &str) -> String {
    let words = name.replace('_', " ");
    let mut letters = words.chars();
    letters.next().map(|first| first.to_uppercase().chain(letters).collect()).unwrap_or_default()
}

pub async fn handle(headers: HeaderMap, body: axum::body::Bytes) -> Response {
    if !local_origin(&headers) {
        return foreign_origin();
    }
    let Ok(message) = serde_json::from_slice::<Value>(&body) else {
        return rpc_failure(StatusCode::BAD_REQUEST, Value::Null, -32700, "Parse error".into(), None);
    };
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return rpc_failure(StatusCode::BAD_REQUEST, Value::Null, -32600, "Invalid request: one JSON-RPC request or notification per POST".into(), None);
    };
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    // a notification is only acknowledged
    if message.get("id").is_none() {
        return StatusCode::ACCEPTED.into_response();
    }
    // A request of the stateless revision says its version in _meta and is
    // checked against its headers; one without it is of the handshake era.
    if let Some(version) = params.get("_meta").and_then(|meta| meta.get(META_VERSION)).and_then(Value::as_str) {
        if let Some((code, text, data)) = refused(&headers, method, &params, version) {
            return rpc_failure(StatusCode::BAD_REQUEST, id, code, text, data);
        }
    }
    if method != "tools/call" {
        seen(method);
    }
    match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or_default();
            let protocol = LEGACY.iter().find(|version| **version == asked).copied().unwrap_or(LEGACY[0]);
            rpc(id, json!({
                "protocolVersion": protocol,
                "capabilities": { "tools": {}, "resources": {}, "prompts": {} },
                "serverInfo": server_info(),
                "instructions": INSTRUCTIONS,
            }))
        }
        "server/discover" => rpc(id, cacheable(json!({
            "supportedVersions": supported_versions(),
            "capabilities": { "tools": {}, "resources": {}, "prompts": {} },
            "instructions": INSTRUCTIONS,
        }))),
        "ping" => rpc(id, json!({})),
        "resources/list" => {
            let mut list = vec![json!({ "uri": SKILL_URI, "name": "studio skill", "title": "How to drive the studio", "description": "How to drive the studio: every tool, what the model reads, step-by-step recipes.", "mimeType": "text/markdown" })];
            for (topic, about) in crate::assistant::GUIDE_TOPICS {
                list.push(json!({ "uri": format!("{GUIDE_URI}{topic}"), "name": format!("writing guide: {topic}"), "title": format!("Writing guide: {topic}"), "description": about, "mimeType": "text/plain" }));
            }
            rpc(id, cacheable(json!({ "resources": list })))
        }
        "resources/templates/list" => rpc(id, cacheable(json!({ "resourceTemplates": [] }))),
        "resources/read" => {
            let uri = params.get("uri").and_then(Value::as_str).unwrap_or_default();
            let text = if uri == SKILL_URI { Some(SKILL.to_string()) } else { uri.strip_prefix(GUIDE_URI).and_then(crate::assistant::writing_guide) };
            match text {
                Some(text) => rpc(id, cacheable(json!({ "contents": [{ "uri": uri, "mimeType": if uri == SKILL_URI { "text/markdown" } else { "text/plain" }, "text": text }] }))),
                None => rpc_error(id, -32002, format!("Resource not found: {uri}")),
            }
        }
        "prompts/list" => rpc(id, cacheable(json!({ "prompts": [
            { "name": "studio", "title": "Studio skill", "description": "Load the studio's skill: the tools, what the model reads, and recipes." },
            { "name": "write_song", "title": "Write a song", "description": "Write a song for the model and make it.", "arguments": [{ "name": "idea", "description": "what the song is about, its genre and mood", "required": true }] },
        ] }))),
        "prompts/get" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let idea = params.get("arguments").and_then(|arguments| arguments.get("idea")).and_then(Value::as_str).unwrap_or_default();
            let text = match name {
                "studio" => Some(SKILL.to_string()),
                "write_song" => crate::assistant::writing_guide("song").map(|guide| format!("{guide}\n\nWrite a song about: {idea}\nFind the closest official examples with writing_examples, write the style and lyrics by the rules above, then song_create and studio_wait.")),
                _ => None,
            };
            match text {
                Some(text) => rpc(id, json!({ "messages": [{ "role": "user", "content": { "type": "text", "text": text } }] })),
                None => rpc_error(id, -32602, format!("Unknown prompt: {name}")),
            }
        }
        "tools/list" => {
            let list: Vec<Value> = tools().iter().map(|tool| json!({ "name": tool.name, "title": tool_title(tool.name), "description": tool.description, "inputSchema": schema_of(tool), "annotations": annotations(tool.name) })).collect();
            rpc(id, cacheable(json!({ "tools": list })))
        }
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            seen(name);
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let answer = |text: String, error: bool| tool_result(id.clone(), text, None, error);
            let Some(tool) = tools().iter().find(|tool| tool.name == name) else {
                return rpc_error(id, -32602, format!("Unknown tool: {name}; tools/list names them all."));
            };
            // a tool may read a file or walk a folder before its call: off the runtime
            let prepare = tool.call;
            let prepared = {
                let args = args.clone();
                tokio::task::spawn_blocking(move || prepare(&args)).await.unwrap_or_else(|error| Err(format!("{name} failed: {error}")))
            };
            // A track is always made inside the session it belongs to, so the makers
            // ask the service first: the agent hears the studio's own rule instead of
            // making a song that no session would hold.
            if makes_a_track(name) {
                // Where a track may be made, and how far the answer reaches, is the person's
                // call - never the agent's. A forbidden tool never runs; a session where the
                // person answered "in this session", or the studio's one switch for tracks,
                // lets the work go on; otherwise the window asks, and the answer is the same
                // five decisions the leash knows: once, this session, always, ask, never.
                if is_forbidden(name).await {
                    return answer(format!("The user forbade {name}; agent_settings lists what is forbidden."), true);
                }
                let session = open_session().await;
                if !session_allows(session.as_deref()).await && !tracks_without_asking().await {
                    let active = fetch("/v1/workspaces/active").await;
                    let asked = ask_window(
                        "session_confirm",
                        json!({
                            "tool": name,
                            "title": acted_on(&args, "{}"),
                            "session_id": session.clone().unwrap_or_default(),
                            "session_name": active.get("name").and_then(Value::as_str).unwrap_or_default(),
                            "allow_always": true,
                        }),
                        300,
                    )
                    .await;
                    let decided = asked
                        .as_ref()
                        .ok()
                        .and_then(|value| value.get("decision").and_then(Value::as_str))
                        .unwrap_or_default()
                        .to_string();
                    match decided.as_str() {
                        "once" | "opened" => {}
                        "session" => match session.as_deref() {
                            Some(id) => allow_in_session(id).await,
                            None => {
                                return answer("There is no open session to allow this in; open one first.".into(), true);
                            }
                        },
                        "always" => allow_tracks_always().await,
                        "deny_always" => {
                            forbid_for_good(name).await;
                            return answer(format!("The user forbade {name}; agent_settings lists what is forbidden."), true);
                        }
                        "ask" => {
                            ask_every_time_again(session.as_deref()).await;
                            return answer(format!("The user asked to be asked every time about {name}; nothing was made."), true);
                        }
                        _ => {
                            let why = match asked {
                                Ok(_) => "the person did not allow it".to_string(),
                                Err(problem) => problem,
                            };
                            let tail = if why.ends_with('.') { " Nothing was made." } else { "; nothing was made." };
                    return answer(format!("{why}{tail}"), true);
                        }
                    }
                }
            }
            // The user is asked before anything that cannot be undone (or before
            // everything, on the strict leash): the leash decides, not the agent.
            if let Err(refusal) = agent_may(name, &args).await {
                return answer(refusal, true);
            }
            // Where a track goes is the person's call, never the agent's guess: the call may name
            // the session (each, current, new:<name>), and otherwise the studio asks - once for
            // the pack of work at hand, the way the leash asks.
            let mut prepared = prepared;
            if takes_a_session(name) {
                match session_for_call(name, &args).await {
                    Ok(Some(id)) => {
                        if let Ok(call) = prepared.as_mut() {
                            give_the_session(call, &id);
                        }
                    }
                    Ok(None) => {}
                    Err(refusal) => return answer(refusal, true),
                }
            }
            match prepared {
                Err(problem) => answer(problem, true),
                Ok(Call { payload: Payload::Window { command, args, seconds }, .. }) => match ask_window(command, args, seconds).await {
                    Ok(result) => {
                        // a screenshot comes back as an image the agent sees
                        let mut content = Vec::new();
                        if let Some(image) = result.get("image").and_then(Value::as_str) {
                            content.push(json!({ "type": "image", "data": image, "mimeType": "image/png" }));
                        }
                        let text = result.get("text").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| if result.is_null() || result.get("image").is_some() { "Done.".into() } else { serde_json::to_string_pretty(&result).unwrap_or_default() });
                        content.push(json!({ "type": "text", "text": text }));
                        let mut answer = json!({ "content": content, "isError": false });
                        if result.get("image").is_none() && result.get("text").is_none() && (result.is_object() || result.is_array()) {
                            answer["structuredContent"] = result;
                        }
                        rpc(id.clone(), answer)
                    }
                    Err(problem) => answer(problem, true),
                },
                Ok(call) if call.path == "composite:equalizer_export" => match export_equalizer(&args).await {
                    Ok(text) => answer(text, false),
                    Err(problem) => answer(problem, true),
                },
                Ok(call) if call.path == "composite:winamp_skin_add" => match add_skin(&args) {
                    Ok(text) => answer(text, false),
                    Err(problem) => answer(problem, true),
                },
                Ok(call) if call.path == "composite:agent_settings" => {
                    let leash = fetch("/v1/agent/leash").await;
                    let tracks = fetch("/v1/agent/allowance").await;
                    let forbidden = fetch("/v1/agent/forbidden").await;
                    let sessions = fetch("/v1/workspaces").await;
                    let allowed: Vec<Value> = sessions
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|session| session.get("agent_allow").and_then(Value::as_bool) == Some(true))
                        .map(|session| json!({ "id": session.get("id"), "name": session.get("name") }))
                        .collect();
                    tool_json(
                        id,
                        json!({
                            "leash": leash.get("leash"),
                            "tracks_without_asking": tracks.get("allow"),
                            "forbidden": forbidden.get("forbidden"),
                            "sessions_that_allow_the_agent": allowed,
                        }),
                    )
                }
                Ok(call) if call.path == "composite:status" => tool_json(id, status_summary().await),
                Ok(call) if call.path == "composite:wait" => match wait_for(&args).await {
                    Ok(state) => tool_json(id, state),
                    Err(problem) => answer(problem, true),
                },
                Ok(call) if call.path == "composite:questions" => tool_json(id, wait_for_questions(&args).await),
                Ok(call) if call.path == "composite:answer" => match answer_question(&args) {
                    Ok(text) => answer(text, false),
                    Err(problem) => answer(problem, true),
                },
                Ok(call) => {
                    // A removal is named BEFORE it happens: afterwards there is nothing left to
                    // read the name from, and the log would say only "deleted · session".
                    let named_before = if is_noteworthy(name) && is_removal(name) {
                        removal_target(name, &args).await
                    } else {
                        String::new()
                    };
                    match call_route(call).await {
                    Ok((status, text)) if status.is_success() => {
                        announce_change(name);
                        if is_noteworthy(name) {
                            // The log keeps what the agent did to the studio - quietly, so the
                            // work never flashes a toast at the person. What travels is the act,
                            // the kind of thing it touched and that thing's own name - never the
                            // tool's name, which says nothing at all to the person reading it.
                            let (verb, kind) = journal_facts(name, &args);
                            let mut target = if named_before.is_empty() { acted_on(&args, &text) } else { named_before };
                            if target.is_empty() {
                                target = removal_target(name, &args).await;
                            }
                            let _ = ask_window("journal_note", json!({ "verb": verb, "kind": kind, "target": target }), 3).await;
                        }
                        match serde_json::from_str::<Value>(&text) {
                            Ok(value) => tool_json(id, shape(name, &args, value)),
                            Err(_) => answer(text, false),
                        }
                    }
                    Ok((_, text)) => answer(text, true),
                    Err(problem) => answer(problem, true),
                    }
                },
            }
        }
        _ => rpc_failure(StatusCode::NOT_FOUND, id, -32601, format!("Method not found: {method}"), None),
    }
}

/// The equalizer's curve as a .EQF file at the agent's path: the window writes the file's bytes, this puts them there.
async fn export_equalizer(args: &Value) -> Result<String, String> {
    use base64::Engine;
    let path = PathBuf::from(text(args, "path")?);
    let exported = ask_window("equalizer_export", json!({ "name": args.get("name").cloned().unwrap_or(Value::Null) }), 15).await?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(exported["eqf_base64"].as_str().ok_or("the window sent no .EQF")?)
        .map_err(|error| format!("the window's .EQF: {error}"))?;
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    std::fs::write(&path, bytes).map_err(|error| format!("write {}: {error}", path.display()))?;
    Ok(format!("Saved the preset {} to {}.", exported["name"].as_str().unwrap_or_default(), path.display()))
}

/// A .wsz from the agent's path into the studio's skins.
fn add_skin(args: &Value) -> Result<String, String> {
    let path = PathBuf::from(text(args, "path")?);
    let bytes = std::fs::read(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let name = match args.get("name").and_then(Value::as_str).filter(|name| !name.trim().is_empty()) {
        Some(name) => name.to_string(),
        None => path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default(),
    };
    let saved = crate::skins::save(&name, &bytes)?;
    Ok(format!("Added the skin {}; winamp_set skin puts it on.", saved["name"].as_str().unwrap_or_default()))
}

/// A moment an agent names, as unix seconds in this computer's time zone:
/// today, yesterday, a date (its start, or its end for an upper bound) or a date-time.
fn moment(text: &str, end: bool) -> Result<i64, String> {
    use chrono::{Duration as Days, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
    let text = text.trim();
    let day = match text.to_lowercase().as_str() {
        "today" => Some(Local::now().date_naive()),
        "yesterday" => Some(Local::now().date_naive() - Days::days(1)),
        _ => NaiveDate::parse_from_str(text, "%Y-%m-%d").ok(),
    };
    let at = match day {
        Some(day) => day.and_time(if end { NaiveTime::from_hms_opt(23, 59, 59).unwrap_or_default() } else { NaiveTime::MIN }),
        None => NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S")
            .or_else(|_| NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M"))
            .or_else(|_| NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M"))
            .map_err(|_| format!("'{text}' is not a moment: today, yesterday, 2026-09-26 or 2026-09-26T18:00."))?,
    };
    Local.from_local_datetime(&at).earliest().map(|at| at.timestamp()).ok_or_else(|| format!("'{text}' does not exist in this time zone."))
}

/// What an agent is told when it connects.
const INSTRUCTIONS: &str = "You drive YuE2 Studio on this computer. Every tool runs the same code as a button of the studio, and the user sees what you do in its window: every change you make is written into the studio's own message log, the bell in the top panel - what was done, to what kind of thing, and that thing's own name - while reads and ui_* leave no line. Start with studio_status. Every track is made inside a session, and you say where it goes: the makers take a `session` word (each | current | new:<name>); one session of the studio's own - marked kind: import - gathers tracks nobody claimed, and is not yours to rename or delete. Long work (songs, stems, karaoke, dataset preparation, training) is a job: start it, then studio_wait instead of polling. The graphics card runs one heavy job at a time; while a LoRA trains no song is made. Look ids up instead of guessing them: library_songs_list, training_status, dataset_get, lora_list, models_status. Before writing for the model yourself read writing_guide and writing_examples. When the user has made you the studio's writing assistant, answer its requests: assistant_requests_wait, then assistant_request_answer. The whole guide is the resource studio://skill (prompt 'studio').";

#[cfg(test)]
mod tests {
    use super::*;

    /// The log is written for a person, not for a log reader: it says what was done and to
    /// what kind of thing - and it never carries the tool's own name, which explains nothing.
    #[test]
    fn the_log_says_what_was_done_and_to_what() {
        let silent = json!({});
        assert_eq!(journal_facts("workspace_delete", &silent), ("deleted", "session"));
        assert_eq!(journal_facts("song_create", &silent), ("created", "song"));
        assert_eq!(journal_facts("playlist_update", &silent), ("updated", "playlist"));
        assert_eq!(journal_facts("stems_split", &silent), ("split", "stems"));
        assert_eq!(journal_facts("library_import_audio", &silent), ("imported", "song"));
        assert_eq!(journal_facts("library_song_like", &json!({ "liked": true })), ("liked", "song"));
        assert_eq!(journal_facts("library_song_like", &json!({ "liked": false })), ("unliked", "song"));
        // An action nobody taught us the words for is still work, not a tool name.
        assert_eq!(journal_facts("karaoke_make", &silent), ("ran", "other"));
        assert!(is_noteworthy("workspace_delete"));
        assert!(!is_noteworthy("library_songs_list"));
    }

    #[test]
    fn a_change_reaches_the_windows_and_a_read_does_not() {
        let mut windows = bridge().commands.subscribe();
        announce_change("library_songs_list");
        announce_change("song_create");
        let mut notices = Vec::new();
        while let Ok(message) = windows.try_recv() {
            notices.push(serde_json::from_str::<Value>(&message).unwrap());
        }
        assert!(notices.contains(&json!({ "changed": "song_create" })));
        assert!(!notices.contains(&json!({ "changed": "library_songs_list" })));
    }

    #[test]
    fn every_tool_has_a_unique_name_and_an_object_schema() {
        let mut names: Vec<&str> = tools().iter().map(|tool| tool.name).collect();
        let count = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), count, "two tools share a name");
        for tool in tools() {
            let schema = (tool.schema)();
            assert_eq!(schema["type"], "object", "{} has no object schema", tool.name);
            assert!(!tool.description.is_empty(), "{} has no description", tool.name);
        }
    }

    #[test]
    fn a_tool_turns_its_arguments_into_its_route() {
        let find = |name: &str| tools().iter().find(|tool| tool.name == name).expect("the tool");
        let call = (find("dataset_song_update").call)(&json!({ "dataset_id": "d 1", "song_id": "s", "lyrics": "[Verse 1]" })).unwrap();
        assert_eq!(call.method, Method::PATCH);
        assert_eq!(call.path, "/v1/training/datasets/d%201/items/s");
        match call.payload {
            Payload::Json(body) => assert_eq!(body, json!({ "lyrics": "[Verse 1]" })),
            _ => panic!("a JSON body"),
        }
        assert!((find("library_song_get").call)(&json!({})).is_err(), "a missing id is refused");
    }

    #[test]
    fn a_folder_becomes_its_songs_with_their_paths() {
        let root = tempfile::tempdir().unwrap();
        let album = root.path().join("Artist").join("2020 - Album");
        std::fs::create_dir_all(&album).unwrap();
        std::fs::write(album.join("01. Song.flac"), b"x").unwrap();
        std::fs::write(album.join("01. Song.lrc"), b"x").unwrap();
        std::fs::write(album.join("cover.jpg"), b"x").unwrap();
        let files = folder_files(&root.path().join("Artist"), song_file, "audio").unwrap();
        let names: Vec<&str> = files.iter().map(|(_, _, name)| name.as_str()).collect();
        assert_eq!(names, ["Artist/2020 - Album/01. Song.flac", "Artist/2020 - Album/01. Song.lrc"]);
    }

    #[test]
    fn an_agent_is_answered_what_it_acts_on() {
        let job = json!({
            "id": "j", "title": "t", "status": "completed", "phase": "done", "message": "m", "duration_seconds": 60,
            "lyrics": "[Verse 1]", "generation_settings": { "seed": 1, "lm_seed": 2, "lyrics": "[Verse 1]", "adapters": [{ "name": "a" }] },
            "songs": [{ "id": "s", "audio_url": "/x", "song": { "title": "Song", "audio_codes": "1,2,3" } }],
        });
        let short = compact_job(&job);
        assert_eq!(short["songs"], json!([{ "id": "s", "title": "Song" }]));
        assert_eq!(short["seed"], 1);
        assert!(!short.to_string().contains("[Verse 1]") && !short.to_string().contains("1,2,3"), "no lyrics, no codes");

        let song = compact_song(&json!({ "id": "s", "lyrics": "[Verse 1]", "audio_codes": "1,2,3", "replay_request": { "x": 1 } }));
        assert_eq!(song["has_audio_codes"], true);
        assert!(song.get("audio_codes").is_none() && song.get("replay_request").is_none());

        let loras = compact_loras(&json!({
            "installed": [{ "id": "l", "name": "Mine", "trigger": "t", "slots": ["ar"], "scales": {}, "bytes": 5 }],
            "catalog": [{ "id": "c", "name": { "en": "Pop", "ru": "Поп" }, "description": { "en": "Hooks", "ru": "Хуки" }, "installed": false, "slots": ["ar"] }],
        }));
        assert_eq!(loras["catalog"][0]["name"], "Pop");
        assert_eq!(loras["catalog"][0]["about"], "Hooks");
        assert!(loras["installed"][0].get("bytes").is_none());
    }

    #[test]
    fn a_tool_that_changes_something_is_not_read_only() {
        for name in ["lora_install_catalog", "lora_import_files", "separator_runtime_install", "openrouter_catalog_refresh", "dataset_create"] {
            assert_eq!(annotations(name)["readOnlyHint"], false, "{name}");
        }
        for name in ["lora_list", "training_status", "dataset_song_files", "ui_screenshot", "writing_guide", "create_form_get", "song_defaults"] {
            assert_eq!(annotations(name)["readOnlyHint"], true, "{name}");
        }
        for name in ["models_download", "lora_install_catalog", "training_pack_install", "openrouter_complete"] {
            assert_eq!(annotations(name)["openWorldHint"], true, "{name}");
        }
        assert_eq!(annotations("training_checkpoint_install")["openWorldHint"], false);
    }

    #[test]
    fn idle_waits_for_every_kind_of_work() {
        let idle = json!({ "song_jobs": [], "covers_and_karaoke": [], "stems": null, "processing": { "done": true }, "preparation": null, "training": null });
        assert!(busy(&idle).is_empty());
        let mut splitting = idle.clone();
        splitting["stems"] = json!({ "song_id": "s", "progress": 0.1, "done": false });
        assert_eq!(busy(&splitting), ["stems"]);
        let mut processing = idle.clone();
        processing["processing"] = json!({ "done": false });
        assert_eq!(busy(&processing), ["processing"]);
    }

    #[test]
    fn a_dataset_folder_brings_its_json_and_wavs() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("set");
        std::fs::create_dir_all(folder.join("audio")).unwrap();
        std::fs::write(folder.join("dataset.json"), b"{}").unwrap();
        std::fs::write(folder.join("audio").join("a.WAV"), b"x").unwrap();
        std::fs::write(folder.join("audio").join("a.mp3"), b"x").unwrap();
        let files = folder_files(&folder, dataset_file, "dataset.json or WAV files").unwrap();
        let names: Vec<&str> = files.iter().map(|(_, _, name)| name.as_str()).collect();
        assert_eq!(names, ["set/audio/a.WAV", "set/dataset.json"]);
        assert!(folder_files(&folder.join("audio"), |_| false, "x").is_err(), "an empty pick is refused");
    }

    #[test]
    fn only_the_makers_carry_the_word_about_sessions() {
        let schema = |name: &str| schema_of(tools().iter().find(|tool| tool.name == name).expect("the tool is in the catalogue"));
        let about = schema("song_create")["properties"]["session"]["description"].as_str().unwrap_or_default().to_string();
        assert!(about.contains("each") && about.contains("current") && about.contains("new:"), "the makers say where the tracks go");
        assert!(schema("stems_split")["properties"].get("session").is_some(), "separating makes parts, so it takes a session");
        assert!(schema("library_import_audio")["properties"].get("session").is_some(), "an import lands in a session too");
        assert!(schema("library_songs_list")["properties"].get("session").is_none(), "a reader is never asked where tracks go");
        assert!(takes_a_session("song_create") && !takes_a_session("studio_status"), "the list names the makers only");
    }

    /// What a route would receive, whoever built the call.
    fn body_of(call: &Call) -> Value {
        match &call.payload {
            Payload::Json(body) => body.clone(),
            Payload::Form { fields, .. } => Value::Object(fields.iter().map(|(name, value)| (name.clone(), Value::String(value.clone()))).collect()),
            Payload::None | Payload::Window { .. } => Value::Null,
        }
    }

    #[test]
    fn the_chosen_session_reaches_the_route_that_makes_the_track() {
        let mut with_body = post("/v1/music/jobs".into(), json!({ "style": "pop" })).unwrap();
        give_the_session(&mut with_body, "session-1");
        assert_eq!(body_of(&with_body)["workspace_id"], "session-1", "the session joins the request");
        assert_eq!(body_of(&with_body)["style"], "pop", "and nothing else about it changes");

        let mut without_body = post("/v1/library/songs/s1/stems".into(), Value::Null).unwrap();
        give_the_session(&mut without_body, "session-2");
        assert_eq!(body_of(&without_body)["workspace_id"], "session-2", "a POST without its own body carries the session");

        let mut form = Call {
            method: Method::POST,
            path: "/v1/library/import".into(),
            payload: Payload::Form { fields: vec![("title".into(), "T".into())], files: vec![] },
        };
        give_the_session(&mut form, "session-3");
        assert_eq!(body_of(&form)["workspace_id"], "session-3", "an upload carries it as a form field");
        assert_eq!(body_of(&form)["title"], "T");

        let mut reading = get("/v1/library/songs".into()).unwrap();
        give_the_session(&mut reading, "session-4");
        assert_eq!(body_of(&reading), Value::Null, "a reader sends no body and gets none");

        let mut windowed = window("ui_screenshot", &json!({}), 5).unwrap();
        give_the_session(&mut windowed, "session-5");
        assert!(matches!(windowed.payload, Payload::Window { .. }), "a window command is left alone");
    }

    #[test]
    fn a_session_made_for_one_track_is_named_after_that_track() {
        assert_eq!(track_name(&json!({ "title": "Night shift" })), "Night shift");
        assert_eq!(track_name(&json!({ "title": "   ", "style": "pop, sad" })), "pop, sad", "a nameless track falls back to its style");
        assert_eq!(track_name(&json!({})), "Track", "and a bare request still gets a name");
        assert_eq!(track_name(&json!({ "title": "x".repeat(90) })).chars().count(), 48, "a session name stays readable");
    }

    #[test]
    fn the_answer_stands_for_the_pack_and_is_forgotten_after_a_quiet_spell() {
        hold_pack("each", None);
        assert_eq!(pack_word().as_deref(), Some("each"), "while the pack goes on, its answer stands");
        assert_eq!(pack_new_workspace("Rain"), None, "a pack running under another word has made no session");

        hold_pack("new:Rain", Some("session-9".to_string()));
        assert_eq!(pack_new_workspace("Rain").as_deref(), Some("session-9"), "the session made for the pack is reused, not made twice");

        *pack_store().lock().unwrap() = Some(PackChoice {
            word: "current".into(),
            workspace: None,
            at: std::time::Instant::now() - std::time::Duration::from_secs(PACK_QUIET_SECONDS + 1),
        });
        assert_eq!(pack_word(), None, "after a quiet spell the next pack is asked about again");

        hold_pack("current", None);
        assert_eq!(pack_word().as_deref(), Some("current"), "and the answer that follows is kept");
        *pack_store().lock().unwrap() = None;
    }

    #[test]
    fn a_question_goes_to_the_window_the_person_works_in() {
        open_windows().clear();
        focused_windows().clear();
        open_windows().extend([1u64, 2u64]);
        assert_eq!(window_to_ask(), Some(2), "with nobody saying anything, the newest window is asked");
        focused_windows().push(1);
        assert_eq!(window_to_ask(), Some(1), "the window the person looked at is the one asked");
        focused_windows().push(2);
        assert_eq!(window_to_ask(), Some(2), "and the last one looked at wins");
        open_windows().retain(|window| *window != 2);
        assert_eq!(window_to_ask(), Some(1), "a window that is gone is not asked again");
        open_windows().clear();
        focused_windows().clear();
    }

    #[test]
    fn a_removal_is_one_plain_question() {
        assert!(is_removal("workspace_delete") && is_removal("library_song_delete") && is_removal("dataset_song_delete"), "every deletion is a removal");
        assert!(!is_removal("workspace_close") && !is_removal("stems_split") && !is_removal("library_song_update"), "closing, splitting and rewriting are other questions");
        assert!(is_destructive("workspace_delete") && is_destructive("library_song_delete"), "and a removal is still asked about on the risky leash");
    }

    #[test]
    fn deleting_a_session_is_a_question_the_person_answers() {
        let tool = tools().iter().find(|tool| tool.name == "workspace_delete").expect("workspace_delete is in the catalogue");
        assert!(tool.description.contains("stay in the library"), "the tracks outlive their session");
        assert!(is_destructive("workspace_delete"), "removing a session cannot be undone, so the leash asks first");
        assert!(!takes_a_session("workspace_delete"), "tidying a session is not making a track");
    }

    #[test]
    fn only_local_pages_and_agents_may_drive_the_studio() {
        let from = |origin: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(header::ORIGIN, origin.parse().unwrap());
            local_origin(&headers)
        };
        assert!(local_origin(&HeaderMap::new()), "an agent sends no origin");
        assert!(from("http://127.0.0.1:3791") && from("http://localhost") && from("http://tauri.localhost") && from("tauri://localhost") && from("http://[::1]:8791"));
        assert!(!from("https://example.com") && !from("http://127.0.0.1.evil.com") && !from("null"));
    }

    #[tokio::test]
    async fn the_agent_answers_what_the_studio_asks_its_assistant() {
        seen("studio_status");
        let asking = tokio::spawn(async { ask_agent("rules", "idea", Some(json!({ "type": "object" })), "all").await });
        let waiting = wait_for_questions(&json!({ "seconds": 5 })).await;
        let request = &waiting["requests"][0];
        assert_eq!(request["instructions"], "rules");
        assert_eq!(request["target"], "all");
        let id = request["id"].as_str().unwrap().to_string();
        answer_question(&json!({ "request_id": id, "answer": { "lyrics": "[Verse 1]" } })).unwrap();
        assert_eq!(asking.await.unwrap().unwrap(), r#"{"lyrics":"[Verse 1]"}"#);
        assert!(answer_question(&json!({ "request_id": id, "answer": "x" })).is_err(), "answered once only");
    }

    #[tokio::test]
    async fn a_multipart_body_streams_its_files() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("song.flac");
        std::fs::write(&path, vec![7u8; (1 << 20) + 5]).unwrap();
        let parts = vec![Part::Text("head".into()), Part::File(path), Part::Text("tail".into())];
        let chunks: Vec<axum::body::Bytes> = futures_util::StreamExt::collect::<Vec<_>>(streamed(parts)).await.into_iter().map(Result::unwrap).collect();
        let whole: Vec<u8> = chunks.concat();
        assert_eq!(whole.len(), 4 + (1 << 20) + 5 + 4);
        assert!(whole.starts_with(b"head") && whole.ends_with(b"tail"));
    }

    #[tokio::test]
    async fn a_stateless_request_is_checked_against_its_headers() {
        let call = |headers: &[(&str, &str)], body: Value| {
            let mut map = HeaderMap::new();
            for (name, value) in headers {
                map.insert(axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(), value.parse().unwrap());
            }
            async move {
                let response = handle(map, axum::body::Bytes::from(body.to_string())).await;
                let status = response.status();
                let bytes = axum::body::to_bytes(response.into_body(), 1 << 22).await.unwrap();
                (status, serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null))
            }
        };
        let meta = json!({ "io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientCapabilities": {} });
        let modern = [("mcp-protocol-version", "2026-07-28"), ("mcp-method", "server/discover")];
        let (status, found) = call(&modern, json!({ "jsonrpc": "2.0", "id": 1, "method": "server/discover", "params": { "_meta": meta } })).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(found["result"]["supportedVersions"][0], "2026-07-28");
        assert_eq!(found["result"]["resultType"], "complete");
        assert_eq!(found["result"]["cacheScope"], "public");
        assert!(found["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"].is_string());

        let (status, found) = call(&[("mcp-protocol-version", "2026-07-28"), ("mcp-method", "tools/call"), ("mcp-name", "studio_system")], json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "studio_status", "_meta": meta } })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "a name that differs from the body is refused");
        assert_eq!(found["error"]["code"], HEADER_MISMATCH);

        let (status, found) = call(&[("mcp-protocol-version", "2026-07-28")], json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list", "params": { "_meta": meta } })).await;
        assert_eq!((status, found["error"]["code"].clone()), (StatusCode::BAD_REQUEST, json!(HEADER_MISMATCH)), "a missing Mcp-Method is refused");

        let old = json!({ "io.modelcontextprotocol/protocolVersion": "1900-01-01" });
        let (status, found) = call(&[("mcp-protocol-version", "1900-01-01"), ("mcp-method", "tools/list")], json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/list", "params": { "_meta": old } })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(found["error"]["code"], UNSUPPORTED_VERSION);
        assert_eq!(found["error"]["data"]["requested"], "1900-01-01");

        let encoded = format!("=?base64?{}?=", { use base64::Engine; base64::engine::general_purpose::STANDARD.encode("studio://skill") });
        let (status, found) = call(&[("mcp-protocol-version", "2026-07-28"), ("mcp-method", "resources/read"), ("mcp-name", encoded.as_str())], json!({ "jsonrpc": "2.0", "id": 5, "method": "resources/read", "params": { "uri": "studio://skill", "_meta": meta } })).await;
        assert_eq!(status, StatusCode::OK, "a Base64 name is decoded before it is compared");
        assert!(found["result"]["contents"][0]["text"].as_str().unwrap().contains("MCP"));

        let (status, found) = call(&[("mcp-protocol-version", "2026-07-28"), ("mcp-method", "nope/nope")], json!({ "jsonrpc": "2.0", "id": 6, "method": "nope/nope", "params": { "_meta": meta } })).await;
        assert_eq!((status, found["error"]["code"].clone()), (StatusCode::NOT_FOUND, json!(-32601)));

        let (status, _) = call(&[], json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await;
        assert_eq!(status, StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn the_server_introduces_itself_and_lists_its_tools() {
        let reply = |body: Value| async move {
            let response = handle(HeaderMap::new(), axum::body::Bytes::from(body.to_string())).await;
            let bytes = axum::body::to_bytes(response.into_body(), 1 << 22).await.unwrap();
            serde_json::from_slice::<Value>(&bytes).unwrap()
        };
        let hello = reply(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } })).await;
        assert_eq!(hello["result"]["capabilities"], json!({ "tools": {}, "resources": {}, "prompts": {} }));
        let resources = reply(json!({ "jsonrpc": "2.0", "id": 4, "method": "resources/read", "params": { "uri": "studio://guide/lyrics" } })).await;
        assert!(resources["result"]["contents"][0]["text"].as_str().unwrap().contains("[Verse 1]"));
        let skill = reply(json!({ "jsonrpc": "2.0", "id": 5, "method": "prompts/get", "params": { "name": "studio" } })).await;
        assert!(skill["result"]["messages"][0]["content"]["text"].as_str().unwrap().contains("MCP"));
        let list = reply(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" })).await;
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), tools().len());
        let unknown = reply(json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "nope" } })).await;
        assert_eq!(unknown["error"]["code"], -32602);
    }
}
