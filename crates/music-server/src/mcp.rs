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
pub(crate) const STUDIO: &str = "YuE2 Studio";
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
    File(PathBuf),
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
        "steps_done": last["step"].as_u64().unwrap_or(steps.len() as u64), "step_limit": run["recipe"]["steps"], "preset": run["recipe"]["preset"], "songs_per_step": run["recipe"]["grad_accum"],
        "stop": if tuned { run["recipe"]["stop"].clone() } else { Value::Null }, "target_kl": if tuned { run["recipe"]["target_kl"].clone() } else { Value::Null },
        "last_step": last, "checkpoints": run["checkpoints"], "installed": run["installed"], "error": run["error"],
        "step_stage": run["step_stage"], "engine": run["engine"], "output_adapter": run["output_adapter"],
        "rvc_voice_id": run["rvc_voice_id"], "rvc_best_checkpoint": run["rvc_best_checkpoint"],
        "rvc_checkpoint_ready": run["rvc_checkpoint_ready"],
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
    let mut short = json!({
        "id": job["id"], "title": job["title"], "status": job["status"], "phase": job["phase"], "message": job["message"],
        "duration_seconds": job["duration_seconds"], "seed": settings["seed"], "lm_seed": settings["lm_seed"],
        "adapters": settings["adapters"], "songs": songs,
        "tracks": settings["lm_batch_size"], "tracks_asked": job["tracks_asked"], "card_memory_short": job["card_memory_short"].as_bool().unwrap_or(false),
    });
    if settings["family"] == crate::audio_cpp_midasheng::FAMILY {
        short["requested_duration_seconds"] = settings["duration_seconds"].clone();
        if job["status"] == "completed" {
            short["effective_duration_seconds"] = job["duration_seconds"].clone();
        }
    }
    short
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

/// Keep a short detailed MIDI response intact. Large transcriptions use offsets
/// so the protocol's text and structured results describe the same complete page.
fn midi_note_page(mut song: Value, offset: Option<u64>) -> Value {
    if offset.is_none() && serde_json::to_string_pretty(&song).is_ok_and(|text| text.len() <= LIMIT) {
        return song;
    }
    let Some(Value::Array(notes)) = song.as_object_mut().and_then(|fields| fields.remove("notes")) else { return song };
    let total = notes.len();
    let start = usize::try_from(offset.unwrap_or(0)).unwrap_or(usize::MAX).min(total);
    let mut count = (total - start).min(256);
    song["notes_count"] = total.into();
    song["offset"] = start.into();
    loop {
        let end = start + count;
        song["notes"] = Value::Array(notes[start..end].to_vec());
        song["next_offset"] = if end < total { end.into() } else { Value::Null };
        if count <= 1 || serde_json::to_string_pretty(&song).is_ok_and(|text| text.len() <= LIMIT) {
            return song;
        }
        count /= 2;
    }
}

fn rvc_catalog_match(voice: &Value, args: &Value) -> bool {
    let metadata = &voice["metadata"];
    if let Some(query) = args["query"].as_str().filter(|query| !query.trim().is_empty()) {
        let mut text = voice["name"].as_str().unwrap_or_default().to_lowercase();
        for label in metadata["source_labels"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            text.push(' ');
            text.push_str(&label.to_lowercase());
        }
        if !query.to_lowercase().split_whitespace().all(|term| text.contains(term)) {
            return false;
        }
    }
    for key in ["vocal_style", "rvc_version"] {
        if let Some(value) = args[key].as_str().filter(|value| !value.trim().is_empty()) {
            let matches = metadata[key].as_str().is_some_and(|actual| actual.to_lowercase() == value.trim().to_lowercase())
                || metadata[key].as_u64().is_some_and(|actual| actual.to_string() == value.trim());
            if !matches {
                return false;
            }
        }
    }
    if let Some(rate) = args["sample_rate_hz"].as_u64() {
        if metadata["sample_rate_hz"].as_u64() != Some(rate) {
            return false;
        }
    }
    let has_index = voice["has_index"].as_bool().unwrap_or(!voice["index"].is_null());
    args["has_index"].as_bool().is_none_or(|requested| requested == has_index)
        && args["has_sample"].as_bool().is_none_or(|requested| requested == voice["sample_audio_url"].is_string())
}

/// What an agent is answered: the parts it acts on, unless it asked for
/// every field with response_format detailed.
fn shape(name: &str, args: &Value, value: Value) -> Value {
    let detailed = args.get("response_format").and_then(Value::as_str) == Some("detailed");
    match name {
        "training_status" if !detailed => compact_training(&value),
        "rvc_catalog_list" => {
            let entries: Vec<_> = value.as_array().into_iter().flatten()
                .filter(|voice| rvc_catalog_match(voice, args)).collect();
            let offset = args["offset"].as_u64().unwrap_or(0).min(entries.len() as u64) as usize;
            let end = (offset + 20).min(entries.len());
            json!({
                "total": entries.len(), "offset": offset, "voices": &entries[offset..end],
                "next_offset": (end < entries.len()).then_some(end),
            })
        },
        "song_create" | "sound_effect_create" | "controlfoley_create" | "song_job_get" | "song_replay" if !detailed => compact_job(&value),
        "song_jobs_list" if !detailed => Value::Array(value.as_array().into_iter().flatten().map(compact_job).collect()),
        "library_song_get" if !detailed => compact_song(&value),
        "lora_list" if !detailed => compact_loras(&value),
        "midi_get" if detailed || args.get("offset").is_some() => midi_note_page(value, args.get("offset").and_then(Value::as_u64)),
        "midi_get" => {
            let mut value = value;
            let notes = value.as_object_mut().and_then(|fields| fields.remove("notes")).and_then(|notes| notes.as_array().map(Vec::len)).unwrap_or(0);
            value["notes_count"] = notes.into();
            value
        }
        "karaoke_settings_get" if !detailed => compact_karaoke(&value),
        "library_liked" if !detailed => json!({ "songs": value.as_array().into_iter().flatten().map(|song| json!({ "id": song["id"], "title": song["title"], "made": song["created_at"], "liked_at": song["metadata"]["liked_at"] })).collect::<Vec<_>>() }),
        // any other answer that is a whole library song
        _ if !detailed && value.get("audio_codes").is_some() && value.get("replay_request").is_some() => compact_song(&value),
        "training_checkpoint_install" | "lora_install_hf" | "lora_import_files" if value["slots"].as_array().is_some_and(Vec::is_empty) => {
            let mut value = value;
            value["slots"] = json!("not known yet: the engine reads them from the file; lora_list shows them");
            value
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

pub(crate) async fn status_summary() -> Value {
    let (jobs, activity, training, processing, separation) = tokio::join!(fetch("/v1/music/jobs"), fetch("/v1/activity"), fetch("/v1/training"), fetch("/v1/processing"), fetch("/v1/separation/status"));
    let (midi, engine, own, restore) = tokio::join!(fetch("/v1/midi"), fetch("/engine/options"), fetch("/setup/own-model"), fetch("/v1/restore"));
    let running_activity: Vec<Value> = activity["activity"].as_array().into_iter().flatten().filter(|entry| entry["state"] == "running").cloned().collect();
    let active_run = training["runs"].as_array().into_iter().flatten().find(|run| Some(run["id"].as_str().unwrap_or_default()) == training["active"].as_str()).map(compact_run);
    json!({
        "window_open": !open_windows().is_empty(),
        "assistant_requests_waiting": open_questions().len(),
        "song_jobs": Value::Array(jobs.as_array().into_iter().flatten().map(compact_job).collect()),
        "covers_and_karaoke": running_activity,
        "stems": separation["run"],
        "midi": midi["run"],
        "restore": restore["run"],
        "preparation": compact_preparation(&training["prepare"]),
        "training": active_run,
        "processing": processing["run"],
        // what the running engine started with: its KV cache and flash attention
        "engine": engine["in_effect"],
        // the person's own YuE2 file the engine runs in place of the set's backbone, none for the set's
        "own_model": own["selected"],
        // the community model that file is, by language; none for a file of the person's own
        "own_model_name": own["selected_name"],
    })
}

/// Whether anything in a status summary is still at work.
pub(crate) fn busy(summary: &Value) -> Vec<&'static str> {
    let unfinished = |run: &Value| run.is_object() && run["done"] != true;
    [
        ("song_jobs", summary["song_jobs"].as_array().is_some_and(|jobs| !jobs.is_empty())),
        ("covers_and_karaoke", summary["covers_and_karaoke"].as_array().is_some_and(|entries| !entries.is_empty())),
        ("stems", unfinished(&summary["stems"])),
        ("processing", unfinished(&summary["processing"])),
        ("restore", unfinished(&summary["restore"])),
        ("midi", unfinished(&summary["midi"]) || summary["midi"]["batch"]["queued"].as_array().is_some_and(|queued| !queued.is_empty())),
        ("preparation", summary["preparation"].is_object() && summary["preparation"]["finished"] != true),
        ("training", !summary["training"].is_null()),
    ]
    .into_iter()
    .filter_map(|(what, working)| working.then_some(what))
    .collect()
}

/// What is at work for the desktop shell's idle stop: what `studio_wait` waits on, and a
/// download. None while the routes are not built yet, when nothing can be read.
pub(crate) async fn at_work() -> Option<Vec<&'static str>> {
    API.get()?;
    let mut working = busy(&status_summary().await);
    if crate::chunked::downloading() {
        working.push("download");
    }
    Some(working)
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
    const READS: &[&str] = &["_get", "_status", "_list", "_catalog", "_profiles", "_files", "_logs", "_log", "_runtime", "_local_models", "_hf_files", "_search_hf", "_capabilities", "_system", "_state", "_read_page", "_screenshot"];
    // a verb that changes something outweighs a noun that reads
    const CHANGES: &[&str] = &["install", "import", "remove", "delete", "refresh", "create", "update", "start", "cancel", "select", "download", "apply", "restart"];
    // reads whose names the rules above miss: create_form names the create page
    const READ_NAMES: &[&str] = &["engine_hip_status", "music_package_updates_get", "music_package_progress", "lyrics_syllables", "assistant_cli_agents", "system_gpus", "cover_scenes", "cover_photos_search", "video_backgrounds_search", "network_proxy_test", "journal_read", "score_to_midi", "score_instrumental", "score_vocal_octave", "score_from_midi", "score_mark", "lyrics_find", "cover_prompt_render", "studio_wait", "engine_presets_get", "assistant_requests_wait", "ui_console", "song_defaults", "create_form_get", "library_liked", "score_chord_bed", "score_backing_tempo", "song_tokenize", "score_match_sections", "models_update_plan", "problem_preview", "library_migration_preview", "audio_cpp_music_models", "hotstep_cover_profiles", "hotstep_sa3_training_status", "hotstep_heartmula_training_status", "rvc_training_status"];
    // writes over what was stored, so the earlier content is gone: a client asks first
    const OVERWRITES: &[&str] = &["library_song_rename", "library_song_update", "playlist_update", "album_reorder", "dataset_update", "dataset_song_update", "dataset_song_describe_style", "dataset_prepare", "lora_update", "stems_split", "restore_start", "karaoke_make", "midi_transcribe", "cover_draw", "cover_set_from_file", "openrouter_set_key", "models_update_apply"];
    let changes = CHANGES.iter().any(|verb| name.split('_').any(|word| word == *verb));
    let read_only = READ_NAMES.contains(&name) || !changes && (READS.iter().any(|part| name.ends_with(part) || name.contains(&format!("{part}_"))) || name.starts_with("writing_"));
    let destructive = name.ends_with("_delete") || name.ends_with("_remove") || name == "processing_discard" || name.ends_with("_cancel") || name.contains("_cancel_") || OVERWRITES.contains(&name);
    // what reaches the internet: OpenRouter, Hugging Face, the lyric databases and every download
    let open_world = name.starts_with("openrouter_") || name == "problem_report" || name.contains("_hf") || name == "lyrics_find" || name == "models_download" || name == "models_update_apply" || name == "lora_install_catalog" || name == "rvc_training_install_runtime" || name.ends_with("_install") && name != "training_checkpoint_install";
    let title = name.replace('_', " ");
    json!({ "title": title, "readOnlyHint": read_only, "destructiveHint": destructive, "idempotentHint": read_only || name.ends_with("_set") || name.contains("_select") || name == "rvc_voice_remove", "openWorldHint": open_world })
}

/// What an agent's change goes into the journal as: what was done and to what kind
/// of thing, for the window to put into words of its language. A job is written
/// as started, since what it makes is not there yet. Reads, the window's own
/// controls, answers that store nothing and the service's upkeep leave no line;
/// a test holds every other tool to this list.
const JOURNAL: &[(&str, &str, &str)] = &[
    ("network_proxy_set", "updated", "settings"),
    ("network_access_set", "updated", "settings"),
    ("cover_look_set", "updated", "settings"),
    ("midi_set", "updated", "midi"),
    ("library_song_note_set", "updated", "song"),
    ("song_cover_photo_set", "updated", "cover"),
    ("song_cover_pattern_set", "updated", "cover"),
    ("song_job_dismiss", "discarded", "generation"),
    ("song_job_score_again", "started", "generation"),
    ("song_job_again", "started", "generation"),
    ("song_create", "started", "generation"),
    ("controlfoley_video_import", "imported", "video"),
    ("controlfoley_create", "started", "generation"),
    ("sound_effect_create", "started", "generation"),
    ("song_replay", "started", "generation"),
    ("audio_cpp_music_model_install", "started", "download"),
    ("song_job_cancel", "cancelled", "generation"),
    ("library_song_rename", "updated", "song"),
    ("library_song_update", "updated", "song"),
    ("library_song_delete", "deleted", "song"),
    ("library_trash_restore", "restored", "song"),
    ("library_trash_delete", "deleted", "song"),
    ("library_trash_empty", "deleted", "song"),
    ("library_import_audio", "imported", "song"),
    ("library_version_select", "updated", "song"),
    ("library_version_delete", "deleted", "version"),
    ("album_reorder", "updated", "library"),
    ("playlist_create", "created", "playlist"),
    ("playlist_update", "updated", "playlist"),
    ("playlist_select_preferred_take", "updated", "playlist"),
    ("playlist_delete", "deleted", "playlist"),
    ("stems_split", "started", "stems"),
    ("karaoke_make", "started", "karaoke"),
    ("karaoke_delete", "deleted", "karaoke"),
    ("karaoke_set", "updated", "karaoke"),
    ("midi_transcribe", "started", "midi"),
    ("restore_start", "started", "restore"),
    ("restore_cancel", "cancelled", "restore"),
    ("midi_cancel", "cancelled", "midi"),
    ("midi_delete", "deleted", "midi"),
    ("score_compose", "started", "score"),
    ("score_transcribe", "started", "score"),
    ("score_job_cancel", "cancelled", "score"),
    ("processing_start", "started", "processing"),
    ("processing_keep", "kept", "processing"),
    ("processing_discard", "discarded", "processing"),
    ("processing_set_reference", "updated", "processing"),
    ("seed_vc_apply", "created", "song"),
    ("cover_draw", "started", "cover"),
    ("cover_set_from_file", "updated", "cover"),
    ("dataset_create", "created", "dataset"),
    ("dataset_import_folder", "created", "dataset"),
    ("dataset_update", "updated", "dataset"),
    ("dataset_add_folder", "updated", "dataset"),
    ("dataset_add_library_songs", "updated", "dataset"),
    ("dataset_song_update", "updated", "dataset"),
    ("dataset_song_describe_style", "updated", "dataset"),
    ("dataset_song_delete", "updated", "dataset"),
    ("dataset_take_as_is", "updated", "dataset"),
    ("dataset_delete", "deleted", "dataset"),
    ("dataset_prepare", "started", "preparation"),
    ("dataset_prepare_train_after", "updated", "preparation"),
    ("dataset_prepare_cancel", "cancelled", "preparation"),
    ("training_start", "started", "training"),
    ("rvc_training_start", "started", "training"),
    ("rvc_training_install_voice", "installed", "rvc_voice"),
    ("rvc_training_install_runtime", "started", "download"),
    ("rvc_training_runtime_cancel", "cancelled", "download"),
    ("hotstep_sa3_training_install", "started", "download"),
    ("hotstep_sa3_training_start", "started", "training"),
    ("hotstep_heartmula_training_start", "started", "training"),
    ("hotstep_heartmula_training_install", "started", "download"),
    ("hotstep_heartmula_install", "started", "download"),
    ("training_continue", "started", "training"),
    ("training_checkpoints_listen", "started", "training"),
    ("training_cancel", "cancelled", "training"),
    ("training_run_delete", "deleted", "training"),
    ("training_checkpoint_install", "installed", "lora"),
    ("lora_install_catalog", "installed", "lora"),
    ("lora_install_hf", "installed", "lora"),
    ("lora_import_files", "installed", "lora"),
    ("rvc_voice_import", "installed", "rvc_voice"),
    ("rvc_voice_remove", "deleted", "rvc_voice"),
    ("music_package_rollback", "updated", "settings"),
    ("music_package_cancel", "cancelled", "download"),
    ("music_model_package_install", "installed", "model"),
    ("music_engine_package_install", "installed", "music_engine"),
    ("music_package_update_mode_set", "updated", "settings"),
    ("music_engine_catalog_refresh", "refreshed", "music_engine_catalog"),
    ("rvc_catalog_refresh", "refreshed", "rvc_catalog"),
    ("rvc_catalog_install", "installed", "rvc_voice"),
    ("rvc_apply_install", "installed", "model"),
    ("native_music_runtime_install", "started", "download"),
    ("native_music_runtime_cancel", "cancelled", "download"),
    ("rvc_native_runtime_install", "started", "download"),
    ("rvc_native_runtime_cancel", "cancelled", "download"),
    ("rvc_apply", "created", "song"),
    ("rvc_preview", "started", "processing"),
    ("rvc_voice_select", "updated", "rvc_voice"),
    ("rvc_quick_start", "started", "processing"),
    ("rvc_quick_cancel", "cancelled", "processing"),
    ("lora_import_folder", "installed", "lora"),
    ("lora_update", "updated", "lora"),
    ("lora_delete", "deleted", "lora"),
    ("lora_export_comfyui", "exported", "lora"),
    ("storage_set", "updated", "settings"),
    ("storage_cancel", "cancelled", "settings"),
    ("song_export", "exported", "song"),
    ("song_export_live", "exported", "song"),
    ("library_song_midi_part_write", "exported", "midi"),
    ("song_export_cancel", "cancelled", "song"),
    ("models_download", "started", "download"),
    ("models_update_apply", "started", "download"),
    ("training_pack_install", "started", "download"),
    ("training_listen_pack_install", "started", "download"),
    ("training_listen_pack_cancel", "cancelled", "download"),
    ("assistant_model_install", "started", "download"),
    ("separator_install", "started", "download"),
    ("separator_runtime_install", "started", "download"),
    ("hotstep_cover_install", "started", "download"),
    ("hotstep_minimax_install", "started", "download"),
    ("hotstep_sa3_install", "started", "download"),
    ("mulacover_runtime_install", "started", "download"),
    ("vevo2_model_directory_set", "updated", "settings"),
    ("recogniser_install", "started", "download"),
    ("midi_install", "started", "download"),
    ("restore_install", "started", "download"),
    ("models_cancel_download", "cancelled", "download"),
    ("training_pack_cancel", "cancelled", "download"),
    ("assistant_cancel_download", "cancelled", "download"),
    ("separator_cancel_download", "cancelled", "download"),
    ("recogniser_cancel_download", "cancelled", "download"),
    ("lora_cancel_download", "cancelled", "download"),
    ("library_migration_import", "imported", "library"),
    ("models_adopt", "installed", "model"),
    ("models_remove", "deleted", "model"),
    ("restore_remove", "deleted", "model"),
    ("assistant_model_remove", "deleted", "model"),
    ("separator_remove", "deleted", "model"),
    ("recogniser_remove", "deleted", "model"),
    ("midi_remove", "deleted", "model"),
    ("models_select", "updated", "settings"),
    ("music_pair_select", "updated", "settings"),
    ("engine_hip_install", "started", "download"),
    ("engine_hip_cancel", "cancelled", "download"),
    ("models_preset", "updated", "settings"),
    ("models_own_select", "updated", "settings"),
    ("models_own_cancel", "cancelled", "download"),
    ("models_own_discard", "deleted", "download"),
    ("models_own_remove", "deleted", "model"),
    ("settings_set", "updated", "settings"),
    ("engine_options_set", "updated", "settings"),
    ("engine_pause", "updated", "settings"),
    ("engine_resume", "updated", "settings"),
    ("engine_fitted_cache_reset", "updated", "settings"),
    ("karaoke_settings_set", "updated", "settings"),
    ("separator_settings_set", "updated", "settings"),
    ("cover_templates_set", "updated", "settings"),
    ("library_file_tags_set", "updated", "settings"),
    ("assistant_set", "updated", "settings"),
    ("openrouter_set_key", "updated", "settings"),
];

/// The facts of a change, when the tool makes one worth a line.
fn journal_facts(name: &str, args: &Value) -> Option<(&'static str, &'static str)> {
    if name == "library_song_like" {
        let taken_back = args.get("liked").and_then(Value::as_bool) == Some(false);
        return Some((if taken_back { "unliked" } else { "liked" }, "song"));
    }
    JOURNAL.iter().find(|(tool, _, _)| *tool == name).map(|(_, verb, kind)| (*verb, *kind))
}

/// The fields that name a thing of the kind in a call or an answer: a dataset's
/// songs carry titles of their own, and settings name nothing.
fn name_fields(kind: &str) -> &'static [&'static str] {
    match kind {
        "settings" => &[],
        "dataset" | "preparation" => &["name"],
        _ => &["title", "name"],
    }
}

fn named_in(kind: &str, value: &Value) -> Option<String> {
    name_fields(kind).iter().find_map(|field| value.get(*field).and_then(Value::as_str).filter(|name| !name.trim().is_empty())).map(|name| name.trim().chars().take(80).collect())
}

/// Where the name of the thing a change touches is read before the change.
#[derive(Debug, PartialEq)]
struct NameSource {
    /// the studio's read that holds it
    path: String,
    /// the list in that answer it is found in by its id; none when the read is
    /// of the thing itself
    list: Option<(&'static str, String)>,
    /// the field holding the name
    field: &'static str,
}

/// Where a change's thing is named, by its id in the call: only the thing of
/// the change's kind is looked up, not every id the call holds.
fn name_source(kind: &str, args: &Value) -> Option<NameSource> {
    let id = |key: &str| args.get(key).and_then(Value::as_str).filter(|id| !id.trim().is_empty());
    let itself = |path: String, field| NameSource { path, list: None, field };
    let listed = |path: &str, list, id: &str| NameSource { path: path.into(), list: Some((list, id.to_string())), field: "name" };
    match kind {
        "playlist" => id("playlist_id").map(|id| itself(format!("/v1/library/playlists/{}", segment(id)), "name")),
        "song" | "generation" | "version" | "stems" | "karaoke" | "midi" | "score" | "restore" | "processing" | "cover" => {
            id("song_id").map(|id| itself(format!("/v1/library/songs/{}", segment(id)), "title"))
        }
        "lora" => id("lora_id").map(|id| listed("/v1/adapters", "installed", id)),
        "dataset" | "preparation" => id("dataset_id").map(|id| listed("/v1/training", "datasets", id)),
        "training" => id("run_id").map(|id| listed("/v1/training", "runs", id)),
        _ => None,
    }
}

/// The name of what a change touches when the call does not carry it, read
/// before the change, since a deletion leaves nothing to read it from. A list
/// is read whole, so it is read only for a deletion; any other change is
/// named by the studio's answer.
async fn name_before_change(verb: &str, kind: &str, args: &Value) -> String {
    if named_in(kind, args).is_some() {
        return String::new();
    }
    let Some(source) = name_source(kind, args) else { return String::new() };
    if source.list.is_some() && verb != "deleted" {
        return String::new();
    }
    let answer = fetch(&source.path).await;
    let thing = match &source.list {
        Some((list, id)) => answer[*list].as_array().into_iter().flatten().find(|entry| entry["id"].as_str() == Some(id.as_str())).cloned().unwrap_or_default(),
        None => answer,
    };
    english(&thing[source.field]).as_str().unwrap_or_default().to_string()
}

/// The name a journal line gives what a change touched: the one the call gave,
/// the one read before the change, the one the studio answered with, or what
/// the call named instead - a model, a file.
fn change_target(kind: &str, args: &Value, before: &str, reply: &str) -> String {
    if let Some(name) = named_in(kind, args) {
        return name;
    }
    if !before.trim().is_empty() {
        return before.trim().chars().take(80).collect();
    }
    let answer = serde_json::from_str::<Value>(reply).unwrap_or(Value::Null);
    if let Some(name) = named_in(kind, &answer).or_else(|| named_in(kind, &answer["song"])) {
        return name;
    }
    if !matches!(kind, "download" | "model" | "processing") {
        return String::new();
    }
    let id = |key: &str| args.get(key).and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty());
    let ids = args.get("ids").and_then(Value::as_array).map(|ids| ids.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).filter(|ids| !ids.is_empty());
    let file = id("path").map(|path| Path::new(path).file_name().map_or_else(|| path.to_string(), |name| name.to_string_lossy().into_owned()));
    ids.or_else(|| ["model_id", "asset_id", "profile_id", "size"].into_iter().find_map(id).map(str::to_string)).or(file).map(|name| name.chars().take(80).collect()).unwrap_or_default()
}

/// Writes an agent's change into the studio's journal, where the person reads it.
async fn journal_change(name: &str, args: &Value, before: &str, reply: &str) {
    let outcome_field = match name {
        "rvc_voice_remove" => Some("removed"),
        "playlist_select_preferred_take" => Some("changed"),
        _ => None,
    };
    if outcome_field.is_some_and(|field| serde_json::from_str::<Value>(reply).is_ok_and(|result| result[field] == false)) {
        return;
    }
    let Some((verb, kind)) = journal_facts(name, args) else { return };
    let target = match name {
        "engine_pause" => "YuE2 engine paused".into(),
        "engine_resume" => "YuE2 engine resumed".into(),
        _ => change_target(kind, args, before, reply),
    };
    let line = json!({ "source": "agent", "verb": verb, "kind": kind, "target": target });
    let written = match send(Method::POST, "/v1/journal".into(), line) {
        Ok(call) => call_route(call).await,
        Err(problem) => Err(problem),
    };
    match written {
        Ok((status, _)) if status.is_success() => {}
        Ok((status, text)) => eprintln!("[ERROR] the journal refused an agent's change to {name}: {status} {text}"),
        Err(problem) => eprintln!("[ERROR] the journal did not take an agent's change to {name}: {problem}"),
    }
}

struct Bridge {
    commands: tokio::sync::broadcast::Sender<String>,
    pending: Mutex<HashMap<String, (u64, tokio::sync::oneshot::Sender<Result<Value, String>>)>>,
    /// The open windows, the one the person turned to last at the end: a
    /// command goes to that one only, so a second window never runs it again.
    windows: Mutex<Vec<u64>>,
    sequence: AtomicU64,
}

fn bridge() -> &'static Bridge {
    static BRIDGE: OnceLock<Bridge> = OnceLock::new();
    BRIDGE.get_or_init(|| Bridge { commands: tokio::sync::broadcast::channel(64).0, pending: Mutex::new(HashMap::new()), windows: Mutex::new(Vec::new()), sequence: AtomicU64::new(0) })
}

fn open_windows() -> std::sync::MutexGuard<'static, Vec<u64>> {
    bridge().windows.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Keeps the window on the list while its stream is open.
struct Listening(u64);

/// What a command gets when the window it went to closes before answering.
const WINDOW_CLOSED: &str = "The studio's window closed before it answered. Call the tool again: it goes to another open window, or says that none is open.";

impl Drop for Listening {
    fn drop(&mut self) {
        open_windows().retain(|window| *window != self.0);
        // a window that went away answers nothing: its commands fail now, not at their timeout
        let mut pending = bridge().pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let gone: Vec<String> = pending.iter().filter(|(_, (window, _))| *window == self.0).map(|(id, _)| id.clone()).collect();
        for id in gone {
            if let Some((_, sender)) = pending.remove(&id) {
                let _ = sender.send(Err(WINDOW_CLOSED.into()));
            }
        }
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
        Some((_, sender)) => {
            let _ = sender.send(match answer.error {
                Some(error) => Err(error),
                None => Ok(answer.result),
            });
            StatusCode::NO_CONTENT
        }
        None => StatusCode::NOT_FOUND,
    }
}

#[derive(serde::Deserialize)]
pub struct WindowFocus {
    window: u64,
}

/// The page the person turned to: an agent's command goes there, not to the
/// window that happened to open last.
pub async fn window_focus(headers: HeaderMap, Json(focus): Json<WindowFocus>) -> StatusCode {
    if !local_origin(&headers) {
        return StatusCode::FORBIDDEN;
    }
    if focus_window(focus.window) { StatusCode::NO_CONTENT } else { StatusCode::NOT_FOUND }
}

fn focus_window(window: u64) -> bool {
    let mut windows = open_windows();
    let Some(place) = windows.iter().position(|open| *open == window) else { return false };
    windows.remove(place);
    windows.push(window);
    true
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

/// An agent's call that changes something reaches the windows. The window
/// never calls the MCP server itself, so each such notice is an agent's doing.
fn announce_change(tool: &str) {
    if annotations(tool)["readOnlyHint"].as_bool() == Some(false) {
        announce(tool);
    }
}

async fn ask_window(command: &str, args: Value, seconds: u64) -> Result<Value, String> {
    let Some(window) = open_windows().last().copied() else {
        return Err("The studio's window is not open. Open YuE2 Studio and call the tool again; everything else works without it.".into());
    };
    let id = format!("w{}", bridge().sequence.fetch_add(1, Ordering::Relaxed));
    let (sender, receiver) = tokio::sync::oneshot::channel();
    bridge().pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(id.clone(), (window, sender));
    // the window may have closed after it was chosen: its Listening went before this entry was there to fail
    if !open_windows().contains(&window) {
        bridge().pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&id);
        return Err(WINDOW_CLOSED.into());
    }
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

fn native_music_runtime_schema() -> Value {
    object(json!({ "engine": { "type": "string", "enum": ["acestep-cpp", "minimaxmusic-cpp"] } }), &["engine"])
}

fn native_music_runtime_route(args: &Value) -> Result<String, String> {
    let engine = text(args, "engine")?;
    if !matches!(engine.as_str(), "acestep-cpp" | "minimaxmusic-cpp") {
        return Err("Choose acestep-cpp or minimaxmusic-cpp.".into());
    }
    Ok(format!("/v1/music/native/{engine}/runtime"))
}

fn rvc_runtime_schema() -> Value {
    object(json!({ "metal": { "type": "boolean", "description": "Choose the experimental Apple Metal projections runtime." } }), &[])
}

fn rvc_runtime_route(args: &Value) -> String {
    if args.get("metal").and_then(Value::as_bool) == Some(true) {
        "/v1/rvc/native/runtime?metal=true".into()
    } else {
        "/v1/rvc/native/runtime".into()
    }
}

fn song_create_schema(mut properties: Value) -> Value {
    properties["minimax"] = json!({
        "type": "object", "additionalProperties": false,
        "description": "Optional sampling controls for native MiniMax Music3.",
        "properties": {
            "lm_cfg": { "type": "number", "minimum": 0 },
            "lm_top_k": { "type": "integer", "minimum": 1, "maximum": i32::MAX },
            "dit_cfg": { "type": "number", "minimum": 0 }
        }
    });
    properties["backing_song_id"] = json!({"type":"string","description":"Instrumental library song to use unchanged as the backing. Requires native YuE2, a matching Chord bed in abc, lyrics, and a ready vocal separator. Returns a mixed WAV."});
    properties["openrouter"] = json!({
        "type": "object", "additionalProperties": false,
        "description": "Optional music settings advertised by the selected OpenRouter model. Omit fields to use provider defaults.",
        "properties": {
            "seed": { "type": "integer" },
            "temperature": { "type": "number", "minimum": 0, "maximum": 2 },
            "top_p": { "type": "number", "minimum": 0, "maximum": 1 },
            "max_tokens": { "type": "integer", "minimum": 1, "maximum": u32::MAX }
        }
    });
    for stage in ["abc_sampling", "semantic_sampling"] {
        let names: Vec<_> = crate::sampling_choices()[stage].as_object().unwrap().keys().collect();
        let knobs = properties[stage].take();
        properties[stage] = json!({"oneOf": [knobs, {"type": "string", "enum": names}]});
    }
    properties["rvc_after"] = json!({
        "type": "object",
        "description": "After successful generation, save a separate RVC take with this voice and backend. Use rvc_quick_list for progress.",
        "properties": {
            "voice_id": { "type": "string", "description": "An installed RVC voice ID" },
            "backend": { "type": "string", "enum": ["auto", "vocal-fast-rs", "vocal-fast-metal", "burn-cuda", "audio.cpp"] }
        },
        "required": ["voice_id"]
    });
    properties["heartmula"] = json!({"type":"object","description":"Native HeartMuLa sampling and codec settings.","properties":{"temperature":{"type":"number"},"top_k":{"type":"integer"},"codec_duration_sec":{"type":"number"},"codec_guidance_scale":{"type":"number"}}});
    properties["hotstep_heartmula"] = json!({"type":"object","description":"HOT-Step HeartMuLa 3B backed by native HeartCodec.","properties":{"adapter":{"type":"string"},"adapter_scale":{"type":"number","minimum":0,"maximum":2}}});
    properties["hotstep_sa3"] = json!({"type":"object","description":"HOT-Step Stable Audio 3 Medium. Use an instrumental description as style.","properties":{"sampler":{"enum":["pingpong","euler"]},"adapter":{"type":"string","description":"Installed adapter name from hotstep_cover_profiles."},"adapter_scale":{"type":"number","minimum":0,"maximum":2}}});
    properties["stable_audio"] = json!({"type":"object","description":"Native Stable Audio 3 controls. Send the music prompt as style and leave lyrics empty.","properties":{"negative_prompt":{"type":"string"},"sampler_mode":{"enum":["pingpong","euler"]},"apg_scale":{"type":"number"},"duration_padding_sec":{"type":"number","minimum":0}}});
    properties["midasheng"] = json!({"type":"object","description":"MiDashengLM-Gen scene and sound layers.","properties":{"caption":{"type":"string"},"music":{"type":"string"},"speech_text":{"type":"string"},"speech_style":{"type":"string"},"effects":{"type":"string"},"environment":{"type":"string"}}});
    properties["magenta"] = json!({
        "type": "object", "description": "Magenta RealTime 2 instrumental style reference from the library.",
        "properties": { "reference_song_id": { "type": "string", "minLength": 1 } },
        "additionalProperties": false
    });
    properties["vevo2"] = json!({"type":"object","description":"Vevo2 singing references from the song library.","properties":{"voice_song_id":{"type":"string"},"melody_song_id":{"type":"string"}},"required":["voice_song_id"]});
    properties["soulx"] = json!({
        "type": "object", "description": "SoulX-Singer singing synthesis (soulx_singer) or voice conversion (soulx_singer_svc).",
        "properties": {
            "reference_song_id": {"type": "string", "description": "Library recording of the target singing voice."},
            "source_song_id": {"type": "string", "description": "Library recording of the source vocals for conversion."},
            "reference_metadata": {"type": "string", "description": "For synthesis, the aligned metadata JSON text for the reference singing.", "maxLength": 262144},
            "target_metadata": {"type": "string", "description": "For synthesis, the aligned metadata JSON text for the output singing.", "maxLength": 262144},
            "control": {"enum": ["score", "melody"], "description": "Default score uses notes. Melody requires 50 Hz F0 contours."}
        },
        "required": ["reference_song_id"]
    });
    let mut schema = object(properties, &[]);
    schema["anyOf"] = json!([
        { "required": ["style"] },
        {
            "required": ["music_source", "soulx"],
            "properties": { "music_source": { "required": ["kind", "model"], "properties": {
                "kind": { "const": "audio_cpp" }, "model": { "enum": ["soulx_singer", "soulx_singer_svc"] }
            } } }
        },
        {
            "required": ["music_source", "midasheng"],
            "properties": { "music_source": { "properties": {
                "kind": { "const": "audio_cpp" }, "model": { "const": "midashenglm_gen" }
            } } }
        },
        {
            "required": ["music_source", "vevo2", "lyrics"],
            "properties": { "music_source": { "properties": {
                "kind": { "const": "audio_cpp" }, "model": { "const": "vevo2" }
            } } }
        }
    ]);
    schema["anyOf"].as_array_mut().expect("song_create anyOf").push(json!({
        "required": ["music_source", "magenta"],
        "properties": {
            "music_source": { "properties": {
                "kind": { "const": "audio_cpp" }, "model": { "const": "magenta_realtime_2" }
            } },
            "magenta": { "required": ["reference_song_id"] }
        }
    }));
    schema
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
    crate::audio_pcm::reads_audio(&extension) || ["txt", "lrc", "cue"].contains(&extension.as_str())
}

/// A dataset folder of the studio family: its dataset.json and WAV files.
fn dataset_file(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "dataset.json") || path.extension().is_some_and(|value| value.eq_ignore_ascii_case("wav"))
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| "audio".into())
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
                description: "Wait for work to finish instead of polling: a song or score job (job_id), or until one kind of work is over - song_jobs, covers_and_karaoke, stems, midi, restore, processing, preparation, training - or everything (until: idle, the default). Returns when it is done or after seconds (30 by default, at most 55, under the minute clients allow a call) with how far it got; call it again to keep waiting.",
                schema: || object(json!({ "job_id": { "type": "string" }, "until": { "type": "string", "enum": ["idle", "song_jobs", "covers_and_karaoke", "stems", "midi", "restore", "processing", "preparation", "training"] }, "seconds": { "type": "integer" } }), &[]),
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
                description: "Every model set and file the studio can download, with sizes and the video memory each needs; partial_bytes of a file is what a stopped download of it left on disk (models_download resumes it, models_remove takes it away).",
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
                name: "library_migration_import",
                description: "Copy the songs, related files and playlists from the selected old Studio folder into this library. Safe to retry after a preview.",
                schema: || id_only("path", "Old Studio installation or data folder"),
                call: |args| post("/v1/library/migration/import".into(), json!({ "path": text(args, "path")? })),
            },
            Tool {
                name: "library_migration_sources_list",
                description: "List locally registered ACE-Step and MiniMax Studio libraries available for migration.",
                schema: nothing,
                call: |_| get("/v1/library/migration/sources".into()),
            },
            Tool {
                name: "library_migration_preview",
                description: "Inspect an ACE-Step or MiniMax installation or data folder for songs, playlists and local media available for migration.",
                schema: || id_only("path", "Old Studio installation or data folder"),
                call: |args| post("/v1/library/migration/preview".into(), json!({ "path": text(args, "path")? })),
            },
            Tool {
                name: "models_adopt",
                description: "Take model files already on this computer instead of downloading them: every file of the catalogue found in the folder (by name, else by exact size) is linked into the studio's models. models_status shows the result.",
                schema: || id_only("path", "folder with the model files"),
                call: |args| post("/setup/adopt".into(), json!({ "path": text(args, "path")? })),
            },
            Tool {
                name: "engine_hip_status",
                description: "Read the optional Windows YuE2 HIP component, matched AMD gfx target, driver and installation progress.",
                schema: nothing,
                call: |_| get("/engine/hip".into()),
            },
            Tool {
                name: "engine_hip_install",
                description: "Download and verify the optional HIP component for the matching AMD GPU. This does not change the engine choice.",
                schema: nothing,
                call: |_| send(Method::POST, "/engine/hip".into(), json!({})),
            },
            Tool {
                name: "engine_hip_cancel",
                description: "Cancel the active YuE2 HIP component download.",
                schema: nothing,
                call: |_| send(Method::DELETE, "/engine/hip".into(), json!({})),
            },
            Tool {
                name: "music_pair_status",
                description: "Read the server-side music pair transition, including requested and active source, exact stage and elapsed time. Does not install files or start a song.",
                schema: nothing,
                call: |_| get("/v1/music/pair".into()),
            },
            Tool {
                name: "music_pair_select",
                description: "Prepare a music source using the same music_source object as song_create. Waits for active generation and does not download missing files. Read music_pair_status for progress.",
                schema: || object(json!({"music_source":{"type":"object"},"client_id":{"type":"string"},"sequence":{"type":"integer","minimum":0}}), &["music_source"]),
                call: |args| post("/v1/music/pair".into(), args.clone()),
            },
            Tool {
                name: "models_update_plan",
                description: "The update screen's plan of this release: what is updated (from, to, bytes to download, reason), what is new, what former releases left that is no longer needed (paths, bytes freed, reason), the totals, show (whether the window shows the screen at start), and job, the screen's download while it runs and after. models_update_apply carries it out.",
                schema: nothing,
                call: |_| get("/setup/update".into()),
            },
            Tool {
                name: "models_update_apply",
                description: "The update screen's button: downloads what the plan updates and adds in the background (models_update_plan's job shows it) and, once all of it is in place, removes what former releases left; with nothing to download it removes them at once. Answers the plan and the job.",
                schema: nothing,
                call: |_| post("/setup/update/apply".into(), json!({})),
            },
            Tool {
                name: "models_update_later",
                description: "The update screen's «Later»: this release's plan is not shown at start again; models_update_plan and Settings - Models still have it.",
                schema: nothing,
                call: |_| post("/setup/update/seen".into(), json!({})),
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
                name: "models_preset",
                description: "Set the studio up the way others made it, in one step. id ruach: Ruach Studio's fine-tune (TW 2.0x0.75, the quantisation the card takes) sings in place of the set's model and their YuE2-VAE-RS x3 decoder turns the song into sound; the set chosen now keeps its VAE, transcriber and the rest. What is not on disk is downloaded (models_status and models_own_list show the progress) and the engine starts on each part as it arrives. The decoder takes about 4 GB of the card while it decodes, the VAE's own 2.5 GB. To go back: models_select with a set's profile_id, and models_own_select with no file.",
                schema: || object(json!({ "id": { "type": "string", "enum": ["ruach"] } }), &["id"]),
                call: |args| post("/setup/preset".into(), args.clone()),
            },
            Tool {
                name: "models_own_list",
                description: "Your own YuE2 models: the GGUF files in models/yue2-cpp/own (folder), each by name and size, the one the engine runs in place of the set's backbone (selected, none for the set's own), and missing when that file is gone; community, the catalog of community fine-tunes (Ruach Studio, CC BY-NC-SA 4.0) with each quantisation's file, size and whether it is downloaded; card_quant, the quantisation this card takes; download, the one under way. The VAE and the rest come from the chosen set.",
                schema: nothing,
                call: |_| get("/setup/own-model".into()),
            },
            Tool {
                name: "models_own_select",
                description: "Run the engine on one of your own YuE2 files (file, a name from models_own_list) in place of the set's backbone, or on the set's again (file empty or left out). A community model (community, its id from models_own_list; quant Q8_0 or Q6_K, else the card's) not on disk yet is downloaded into the folder first - models_own_list shows the download - and chosen once its digest matches the pin. The engine starts again. With your own model the decoder companion is off unless a song asks for it, and LoRA training still runs on the set's backbone.",
                schema: || object(json!({ "file": { "type": "string" }, "community": { "type": "string" }, "quant": { "type": "string", "enum": ["Q8_0", "Q6_K"] } }), &[]),
                call: |args| send(Method::PUT, "/setup/own-model".into(), args.clone()),
            },
            Tool {
                name: "models_own_cancel",
                description: "Stop the download of a community model; the part fetched resumes on the next models_own_select, and models_own_list shows it as partial_bytes until then.",
                schema: nothing,
                call: |_| post("/setup/own-model/cancel".into(), json!({})),
            },
            Tool {
                name: "models_own_discard",
                description: "Take away what a stopped download of a community model left in the folder (community, its id; quant, else the card's): its part file and the record of its pieces, partial_bytes in models_own_list. The next models_own_select downloads it from the start. A file that arrived whole is removed with models_own_remove; one downloading now is stopped with models_own_cancel first.",
                schema: || object(json!({ "community": { "type": "string" }, "quant": { "type": "string", "enum": ["Q8_0", "Q6_K"] } }), &["community"]),
                call: |args| post("/setup/own-model/discard".into(), args.clone()),
            },
            Tool {
                name: "models_own_remove",
                description: "Move one of your own YuE2 files (file, a name from models_own_list) to the system's recycle bin; the engine runs the set's model again when it was the chosen one.",
                schema: || object(json!({ "file": { "type": "string" } }), &["file"]),
                call: |args| post("/setup/own-model/remove".into(), args.clone()),
            },
            Tool {
                name: "models_own_reveal",
                description: "Open the folder of your own YuE2 models (models/yue2-cpp/own) in the file manager, making it first when it is not there.",
                schema: nothing,
                call: |_| post("/setup/own-model/reveal".into(), json!({})),
            },
            Tool {
                name: "engine_options_get",
                description: "The engine's launch options: backend (auto, cuda, vulkan, cpu), keep_loaded (auto, yes, no: Auto keeps the models on the card between songs when the set kept whole, a full-context cache for each track and a reserve fit its free memory at the engine's start; in_effect says whether the running engine keeps them and why; kept models are unloaded before other work on the card and after 10 minutes without a song), kv_cache (auto, f16 or q8: Auto keeps F16 while the set and a full-context cache for each track fit the card's free memory and takes Q8, half the memory, when they do not; Q8 only with flash attention on an NVIDIA card through CUDA, kv_q8_off_reason says why not - processor, vulkan or flash_attention_off; the same seed gives another song on another type of cache), kv_cache_now (the running engine's cache: q8, reason - set, fits, does_not_fit, unmeasured or why Q8 does not apply - need_gib and free_gib), and the rest - including extra_args, any other command-line flag the engine takes, and engine_bin, a hand-built engine executable to run instead of the bundled one.",
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
                name: "engine_fitted_cache_reset",
                description: "Forget the KV cache the studio stepped down to after the card ran out of memory on a song (engine_options_get: in_effect.max_seq_reason fitted), so the engine starts on the model's full context again; restarts a running engine.",
                schema: nothing,
                call: |_| send(Method::DELETE, "/engine/options/fitted".into(), json!({})),
            },
            Tool {
                name: "engine_restart",
                description: "Restart the music engine, for example after changing its options or models.",
                schema: nothing,
                call: |_| post("/engine/restart".into(), json!({})),
            },
            Tool {
                name: "engine_pause",
                description: "Stop the Studio-managed YuE2 engine until explicitly resumed. Finish or cancel any active local song or score first. Saved songs remain playable.",
                schema: nothing,
                call: |_| post("/engine/pause".into(), json!({})),
            },
            Tool {
                name: "engine_resume",
                description: "Resume the Studio-managed YuE2 engine with its selected models and launch options after a manual pause.",
                schema: nothing,
                call: |_| post("/engine/resume".into(), json!({})),
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
                description: "Visible messages and controls of the window, one per line. Each control includes its ref (e12), kind, label and value. Refs are what ui_click, ui_type and ui_select take; read the page again after it changes.",
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
                description: "Fill the create page's form in the window, as if typed - the user sees every field change; fields not given stay. fields: title, style, lyrics, abc, cot (full|melody|off), duration_seconds (seconds or 'auto'), lm_batch_size, synth_batch_size, steps, cfg_scale, companion_scale, lm_seed, seed, randomize_seed, chord_strength, chord_identity (root|spelling), chord_window, chord_hold_limit, outside_bonus, section_strength, follow_sections, cover_prompt, output_format, mp3_bitrate, adapters, mode (studio|simple|cover). Use it when the user wants to see and adjust the song before it is made; song_create makes one directly.",
                schema: || object(json!({ "fields": { "type": "object", "description": "field -> value" } }), &["fields"]),
                call: |args| window("create_set", args, 15),
            },
            Tool {
                name: "create_form_submit",
                description: "Press Create on the create page: the song is made from the form as it stands. studio_wait until idle waits for it; create_form_get shows why the form refused, if it did.",
                schema: || object(json!({ "variations": { "type": "boolean", "description": "Use the local YuE2 score and the same seeds for the alternate performance presets." } }), &[]),
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
                name: "player_queue_add",
                description: "Queue library songs in the studio's player without stopping what plays: next=true puts them right after the song playing (Play next), otherwise at the end of the queue (Add to queue). With nothing loaded the first of them starts. In the Winamp mode they go to the end of Winamp's list.",
                schema: || object(json!({ "song_ids": { "type": "array", "items": { "type": "string" } }, "next": { "type": "boolean" } }), &["song_ids"]),
                call: |args| window("player_queue_add", args, 15),
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
                description: "The visualiser: where it is (closed, panel over the studio, window of its own), fullscreen, the engine (milkdrop, spectrum, blackhole or tunnel), the MilkDrop preset on screen, the spectrum look and the looks there are, the tunnel's jump threshold, held or cycling, random or in order, the preset name shown, seconds per preset, the panel's size.",
                schema: nothing,
                call: |args| window("visualizer_get", args, 15),
            },
            Tool {
                name: "visualizer_set",
                description: "Work the visualiser as its buttons and keys do; what is not given stays. place: closed | panel (floating over the studio, resizable) | window (a window of its own). fullscreen. engine: milkdrop | spectrum | blackhole | tunnel. jump_at: how loud a note makes the tunnel jump, 0 to 1. preset: a MilkDrop preset by name or part of one (visualizer_presets lists them). look: a spectrum look (visualizer_get lists them). step: next | previous. locked: stay on this preset. random: random or in order. show_name. cycle_seconds: how long MilkDrop stays on a preset. panel_size: {width, height} and panel_position: {x, y} in pixels. It hears the player after the equalizer; press play for it to move.",
                schema: || object(json!({
                    "place": { "type": "string", "enum": ["closed", "panel", "window"] },
                    "fullscreen": { "type": "boolean" },
                    "engine": { "type": "string", "enum": ["milkdrop", "spectrum", "blackhole", "tunnel"] },
                    "jump_at": { "type": "number", "minimum": 0, "maximum": 1 },
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
                description: "Generate a song with YuE2, native ACE-Step or MiniMax, an installed audio.cpp music family, or an explicitly selected OpenRouter music model. YuE2 uses the ABC score. Native engines use their own profile and supported controls. style: one English sentence (language, genre, vocal, instruments, mood, production, 'N BPM'). lyrics: sections tagged [Verse 1], [Chorus], [Bridge]... one per line, blank line between sections; empty for an instrumental. abc: a score to sing (from score_compose or an edited one), else the model writes its own. cot: full (melody and chords), melody, or off. adapters: installed LoRA ids with strengths per slot (see lora_list). Returns a job; poll song_job_get until completed, the song then is in the library.",
                schema: || song_create_schema(json!({
                    "ace": { "type": "object", "description": "ACE-Step generation settings, including guidance_scale, think, source_song_id and reference_song_id." },
                    "music_source": {
                        "description": "Select local YuE2, a native profile, an audio.cpp model, an installed signed music adapter and its model, or an OpenRouter music model. OpenRouter submits a paid request to the selected model.",
                        "oneOf": [
                            { "type": "object", "properties": { "kind": { "const": "local" } }, "required": ["kind"] },
                            { "type": "object", "properties": { "kind": { "const": "native" }, "engine": { "enum": ["acestep-cpp", "minimaxmusic-cpp"] }, "model": { "type": "string", "minLength": 1 } }, "required": ["kind", "engine", "model"] },
                            { "type": "object", "properties": { "kind": { "const": "signed" }, "engine": { "type": "string", "minLength": 1 }, "model": { "type": "string", "minLength": 1 }, "generation": { "type": "string", "description": "Optional verified engine generation. Omit to use the active engine." }, "model_generation": { "type": "string", "description": "Optional verified model generation. Omit to use the active model." } }, "required": ["kind", "engine", "model"] },
                            { "type": "object", "properties": { "kind": { "const": "audio_cpp" }, "model": { "type": "string", "minLength": 1 } }, "required": ["kind", "model"] },
                            { "type": "object", "properties": { "kind": { "const": "openrouter" }, "model": { "type": "string", "minLength": 1 } }, "required": ["kind", "model"] }
                        ]
                    },
                    "style": { "type": "string" },
                    "lyrics": { "type": "string" },
                    "title": { "type": "string" },
                    "abc": { "type": "string" },
                    "solver": { "type": "string", "enum": ["midpoint", "ab2"], "description": "acoustic ODE solver: midpoint is the reference; ab2 spends one network evaluation a step instead of two, about twice as fast at the same steps, and sounds the same" },
                    "abc_continue": { "type": "boolean", "description": "abc is only the opening (a hummed or played seed, from score_transcribe or the MIDI editor): the model writes the rest of the song on from it in its key and meter, and the opening comes back as a hook" },
                    "transpose": { "type": "integer", "minimum": -24, "maximum": 24, "description": "move a supplied abc score before singing; requires full or melody cot and no semantic_tokens" },
                    "lyric_timing": { "type": "boolean", "description": "with a supplied abc score, each section's words wait until the score reaches it, so the voice keeps to the band; on unless false" },
                    "vocals_only": { "type": "boolean", "description": "extract vocals after synthesis using the installed separator; output_format must be mp3 or wav32" },
                    "fade_in": { "type": "number", "minimum": 0, "description": "seconds the studio fades the made song in over; 0 or absent leaves the start as sung" },
                    "fade_out": { "type": "number", "minimum": 0, "description": "seconds the studio fades the made song out over at its end; 0 or absent leaves the end as sung" },
                    "playlist_id": { "type": "string", "description": "a playlist (see playlist_list) the made songs are added to" },
                    "cot": { "type": "string", "enum": ["full", "melody", "off"] },
                    "duration_seconds": { "type": "number", "description": "In seconds. For local YuE2, this caps generation. For MiDashengLM-Gen, it sets the budget and the model can finish earlier. Omit it to use the checkpoint budget." },
                    "seed": { "type": "integer" },
                    "lm_seed": { "type": "integer" },
                    "steps": { "type": "integer" },
                    "cfg_scale": { "type": "number" },
                    "companion_scale": { "type": "number", "minimum": 0, "maximum": 1, "description": "strength of the realaudio decoder adapter under the song; left out, 0 (the YuE2 checkpoint alone, as audio.cpp and ComfyUI render it), or 1 under a LoRA trained in the studio" },
                    "lm_batch_size": { "type": "integer", "description": "compositions written from the request (1 by default)" },
                    "synth_batch_size": { "type": "integer", "description": "performances rendered of each composition (1 by default)" },
                    "semantic_tokens": { "type": "string", "description": "audio codes of a song already sung (library_song_get audio_codes): renders that take again" },
                    "harmony": { "type": "object", "description": "chord variety and section order of the score the model plans (also after an abc opening with abc_continue; ignored with a whole abc score): strength 0-64 lowers chords heard among the recent changes (about 6-10 breaks a looping progression), identity root (C, Cmaj7, C/E one chord) or spelling, window 1-512 recent changes, hold_limit 0-64 symbols one root holds for free, outside_bonus 0-20 favours roots outside the key (root identity) while fewer than outside_limit 0-1 of the changes are outside, section_strength 0-64 keeps a section from opening like the one before, section_open 1-16 chords compared, follow_lyrics true holds the plan to the lyrics' sections in order", "properties": { "identity": { "type": "string", "enum": ["root", "spelling"] }, "strength": { "type": "number" }, "window": { "type": "integer" }, "hold_limit": { "type": "integer" }, "outside_bonus": { "type": "number" }, "outside_limit": { "type": "number" }, "section_strength": { "type": "number" }, "section_open": { "type": "integer" }, "follow_lyrics": { "type": "boolean" } } },
                    "abc_sampling": { "type": "object", "description": "temperature, top_p, top_k, repetition_penalty, penalty_window, min_tokens, max_tokens", "properties": { "temperature": { "type": "number" }, "top_p": { "type": "number" }, "top_k": { "type": "integer" }, "repetition_penalty": { "type": "number" }, "penalty_window": { "type": "integer" }, "min_tokens": { "type": "integer" }, "max_tokens": { "type": "integer" } } },
                    "semantic_sampling": { "type": "object", "description": "temperature, top_p, top_k, repetition_penalty, penalty_window, min_tokens, max_tokens", "properties": { "temperature": { "type": "number" }, "top_p": { "type": "number" }, "top_k": { "type": "integer" }, "repetition_penalty": { "type": "number" }, "penalty_window": { "type": "integer" }, "min_tokens": { "type": "integer" }, "max_tokens": { "type": "integer" } } },
                    "mp3_bitrate": { "type": "integer" },
                    "output_format": { "type": "string", "enum": ["flac", "mp3", "wav32"], "description": "how the song is kept: lossless FLAC, 32-bit WAV, or MP3 at mp3_bitrate" },
                    "cover_prompt": { "type": "string", "description": "what the cover should show; it is drawn only when an image model is set up (settings_get, covers), else the song has no cover" },
                    "cover_of": { "type": "string", "description": "Library original song ID. Required by MuLaCover, which reads the song audio directly. For YuE2 covers, pass the source of the ABC melody." },
                    "adapters": { "type": "array", "items": { "type": "object", "properties": { "id": { "type": "string" }, "scales": { "type": "object", "description": "slot -> strength, e.g. {\"ar\": 1, \"nar\": 1}; left out, the LoRA's own strengths, else 1 on each slot it touches" } }, "required": ["id"] } }
                })),
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
                description: "Render a library song again. HOT-Step Stable Audio 3 and MiniMax Music3 use their saved generation parameters, with optional steps, seed or format overrides. Engines with saved audio codes can resynthesize those codes directly.",
                schema: || object(json!({ "song_id": { "type": "string" }, "steps": { "type": "integer" }, "solver": { "type": "string", "enum": ["midpoint", "ab2"] }, "seed": { "type": "integer" }, "synth_batch_size": { "type": "integer" }, "output_format": { "type": "string", "enum": ["flac", "mp3", "wav32"] }, "title": { "type": "string" } }), &["song_id"]),
                call: |args| post("/v1/music/replay".into(), args.clone()),
            },
            Tool {
                name: "song_tokenize",
                description: "Exact checkpoint token IDs, Unicode cut marks and torn characters for style and lyrics in the whole music prompt. Unavailable until a tokenizer-capable music engine is running; never estimates counts.",
                schema: || object(json!({ "style": { "type": "string" }, "lyrics": { "type": "string" }, "cot": { "type": "string", "enum": ["full", "melody", "off"] } }), &["lyrics"]),
                call: |args| post("/v1/song/tokenize".into(), args.clone()),
            },
            Tool {
                name: "score_compose",
                description: "Write score plans (ABC notation) for a style and lyrics. lm_batch_size chooses the number up to the configured engine song limit. Returns a job; poll score_job_get for plans, each with its abc and lm_seed. Read and choose a plan, then pass its abc to song_create.",
                schema: || object(json!({ "style": { "type": "string" }, "lyrics": { "type": "string" }, "cot": { "type": "string", "enum": ["full", "melody"] }, "lm_seed": { "type": "integer" }, "lm_batch_size": { "type": "integer", "minimum": 1, "maximum": 8 }, "abc": { "type": "string", "description": "an opening the plan goes on from; the plans begin with it" }, "harmony": { "type": "object", "description": "chord variety and section order of the score the model plans (with an abc opening, of the rest it writes): strength 0-64 lowers chords heard among the recent changes (about 6-10 breaks a looping progression), identity root (C, Cmaj7, C/E one chord) or spelling, window 1-512 recent changes, hold_limit 0-64 symbols one root holds for free, outside_bonus 0-20 favours roots outside the key (root identity) while fewer than outside_limit 0-1 of the changes are outside, section_strength 0-64 keeps a section from opening like the one before, section_open 1-16 chords compared, follow_lyrics true holds the plan to the lyrics' sections in order", "properties": { "identity": { "type": "string", "enum": ["root", "spelling"] }, "strength": { "type": "number" }, "window": { "type": "integer" }, "hold_limit": { "type": "integer" }, "outside_bonus": { "type": "number" }, "outside_limit": { "type": "number" }, "section_strength": { "type": "number" }, "section_open": { "type": "integer" }, "follow_lyrics": { "type": "boolean" } } } }), &["style"]),
                call: |args| post("/v1/scores".into(), args.clone()),
            },
            Tool {
                name: "score_chord_bed",
                description: "Write a chord bed at once, without the model planning: chords over rests in the Vocal voice and an empty Ins voice, one section per lyrics tag (bars from its lines), in the tempo, key and meter given; verses, choruses and a bridge each get a common progression, no chord repeats its neighbour; a section plays as many of its chords as its bars take, and the answer's sections name each section once with the chords it plays. Pass its abc to song_create (cot full): with words to sing the model plans the melody over the bed's chords, section by section, and the song's score is that plan; without words the bed is played as it stands. The same seed writes the same bed.",
                schema: || object(json!({ "bpm": { "type": "integer", "minimum": 40, "maximum": 240 }, "key": { "type": "string", "description": "tonic and m for minor: Em, F#m, Bb" }, "meter": { "type": "string", "enum": ["4/4", "3/4", "6/8", "2/4"] }, "lyrics": { "type": "string" }, "seed": { "type": "integer" } }), &["bpm", "key", "meter"]),
                call: |args| post("/v1/score/chord-bed".into(), args.clone()),
            },
            Tool {
                name: "score_backing_tempo",
                description: "Measure a library recording's tempo with Beat This. Requires the listening pack. Returns a tempo estimate for review.",
                schema: || object(json!({ "song_id": { "type": "string" } }), &["song_id"]),
                call: |args| post("/v1/score/backing-tempo".into(), args.clone()),
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
            Tool {
                name: "score_match_sections",
                description: "For a cover: retag the lyric blocks with the score sections they are sung in, found by recognising the source recording (the karaoke recogniser). Words never change; a chorus written once is copied into later chorus sections, sections without voice get an empty tag, blocks not heard keep their place and come back as unsure. Pass the cover's abc, its lyrics and the source as song_id or path. Returns the proposed lyrics and a per-block report; nothing is applied.",
                schema: || object(json!({ "abc": { "type": "string" }, "lyrics": { "type": "string" }, "song_id": { "type": "string" }, "path": { "type": "string" }, "language": { "type": "string" } }), &["abc", "lyrics"]),
                call: |args| {
                    let mut fields = vec![("abc".to_string(), text(args, "abc")?), ("lyrics".to_string(), text(args, "lyrics")?)];
                    let mut files = Vec::new();
                    if let Some(path) = args.get("path").and_then(Value::as_str).filter(|path| !path.trim().is_empty()) {
                        let path = PathBuf::from(path.trim());
                        files.push(("audio".to_string(), path.clone(), file_name(&path)));
                    } else {
                        fields.push(("song_id".to_string(), text(args, "song_id").map_err(|_| "song_id or path is required".to_string())?));
                    }
                    if let Some(language) = args.get("language").and_then(Value::as_str).filter(|value| !value.trim().is_empty()) {
                        fields.push(("language".into(), language.trim().to_string()));
                    }
                    Ok(Call { method: Method::POST, path: "/v1/score/sections".into(), payload: Payload::Form { fields, files } })
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
                description: "The songs the user liked (the thumbs-up in the library), the latest like first: the ones they count as the best. With library_songs_list since today it gives today's best. The like is kept with the song, so no window has to be open.",
                schema: nothing,
                call: |_| get("/v1/library/liked".into()),
            },
            Tool {
                name: "library_song_like",
                description: "Like a library song (liked true, the default) or take the like back (liked false), as the thumbs-up in the library does.",
                schema: || object(json!({ "song_id": { "type": "string" }, "liked": { "type": "boolean" } }), &["song_id"]),
                call: |args| send(Method::PUT, format!("/v1/library/songs/{}/liked", segment(&text(args, "song_id")?)), json!({ "liked": args.get("liked").and_then(Value::as_bool).unwrap_or(true) })),
            },
            Tool {
                name: "library_song_get",
                description: "One library song with its style, lyrics, score, settings and metadata (derived: what it was made from). Its audio codes and replay request only with response_format detailed.",
                schema: || object(json!({ "song_id": { "type": "string", "description": "library song id" }, "response_format": { "type": "string", "enum": ["concise", "detailed"], "description": "detailed gives every field" } }), &["song_id"]),
                call: |args| get(format!("/v1/library/songs/{}", segment(&text(args, "song_id")?))),
            },
            Tool {
                name: "library_song_rename",
                description: "Set the title of one library song, preserving its recording, generation settings and origin.",
                schema: || object(json!({ "song_id": { "type": "string" }, "title": { "type": "string" } }), &["song_id", "title"]),
                call: |args| send(Method::PUT, format!("/v1/library/songs/{}/title", segment(&text(args, "song_id")?)), json!({"title": text(args, "title")?})),
            },
            Tool {
                name: "library_song_update",
                description: "Change a library song: send the song as library_song_get returns it with your changes (title, caption, lyrics, metadata).",
                schema: || object(json!({ "song_id": { "type": "string" }, "song": { "type": "object" } }), &["song_id", "song"]),
                call: |args| send(Method::PUT, format!("/v1/library/songs/{}", segment(&text(args, "song_id")?)), args.get("song").cloned().unwrap_or_default()),
            },
            Tool {
                name: "library_song_delete",
                description: "Put a song in the library's bin, with the songs made from it (its stems). The bin keeps them for the days library_trash_list names; library_trash_restore brings them back.",
                schema: || id_only("song_id", "library song id"),
                call: |args| send(Method::DELETE, format!("/v1/library/songs/{}", segment(&text(args, "song_id")?)), json!({})),
            },
            Tool {
                name: "library_trash_list",
                description: "The songs in the library's bin: when each went there, when it goes for good, how many songs went with it, and how many days the bin keeps a song (days).",
                schema: nothing,
                call: |_| get("/v1/library/trash".into()),
            },
            Tool {
                name: "library_trash_restore",
                description: "Bring a song back from the library's bin, with the songs that went there with it.",
                schema: || id_only("song_id", "song id in the bin"),
                call: |args| post(format!("/v1/library/trash/{}/restore", segment(&text(args, "song_id")?)), json!({})),
            },
            Tool {
                name: "library_trash_delete",
                description: "Delete a song of the bin for good, with the songs that went there with it; its files go to the system's recycle bin.",
                schema: || id_only("song_id", "song id in the bin"),
                call: |args| send(Method::DELETE, format!("/v1/library/trash/{}", segment(&text(args, "song_id")?)), json!({})),
            },
            Tool {
                name: "library_trash_empty",
                description: "Delete every song of the library's bin for good; their files go to the system's recycle bin.",
                schema: nothing,
                call: |_| send(Method::DELETE, "/v1/library/trash".into(), json!({})),
            },
            Tool {
                name: "library_import_audio",
                description: "Add an audio file from this computer to the library. Without a title it takes the file's own title tag, else its name; the file's lyrics come with it, and a track one of the studios made brings its style and note, and from this studio also what it was made with, so it can be made again.",
                schema: || object(json!({ "path": { "type": "string" }, "title": { "type": "string" } }), &["path"]),
                call: |args| {
                    let path = PathBuf::from(text(args, "path")?);
                    let fields = args.get("title").and_then(Value::as_str).map(|title| vec![("title".to_string(), title.to_string())]).unwrap_or_default();
                    let name = file_name(&path);
                    Ok(Call { method: Method::POST, path: "/v1/library/import".into(), payload: Payload::Form { fields, files: vec![("audio".into(), path, name)] } })
                },
            },
            Tool {
                name: "library_file_tags_get",
                description: "Whether the studio writes each track's note, style and generation settings into its file (settings: true, the default) besides the title, lyrics, cover, genre and tempo it always writes.",
                schema: nothing,
                call: |_| get("/v1/library/file-tags".into()),
            },
            Tool {
                name: "library_file_tags_set",
                description: "Turn writing each track's note, style and generation settings into its file on (settings: true) or off; every stored track is tagged again in the background.",
                schema: || object(json!({ "settings": { "type": "boolean" } }), &["settings"]),
                call: |args| send(Method::PUT, "/v1/library/file-tags".into(), json!({ "settings": args.get("settings").and_then(Value::as_bool).unwrap_or(true) })),
            },
            Tool {
                name: "playlist_list",
                description: "The library's playlists.",
                schema: nothing,
                call: |_| get("/v1/library/playlists".into()),
            },
            Tool {
                name: "album_reorder",
                description: "Save the display order of all library albums. Supply every album ID exactly once.",
                schema: || object(json!({ "ids": { "type": "array", "items": { "type": "string" }, "uniqueItems": true } }), &["ids"]),
                call: |args| send(Method::PUT, "/v1/library/albums/order".into(), args.clone()),
            },
            Tool {
                name: "playlist_create",
                description: "Make a playlist of library songs.",
                schema: || object(json!({ "name": { "type": "string" }, "description": { "type": "string" }, "song_ids": { "type": "array", "items": { "type": "string" } } }), &["name"]),
                call: |args| post("/v1/library/playlists".into(), args.clone()),
            },
            Tool {
                name: "playlist_update",
                description: "Rename a playlist, change its description, or set its songs: song_ids is the whole list in its order, so it also reorders, adds and removes.",
                schema: || object(json!({ "playlist_id": { "type": "string" }, "name": { "type": "string" }, "description": { "type": "string" }, "song_ids": { "type": "array", "items": { "type": "string" } } }), &["playlist_id", "name"]),
                call: |args| send(Method::PUT, format!("/v1/library/playlists/{}", segment(&text(args, "playlist_id")?)), body_without(args, &["playlist_id"])),
            },
            Tool {
                name: "playlist_select_preferred_take",
                description: "Mark a playable song already in this playlist as its preferred take. This keeps every take and the current playlist order. Read the saved choice with playlist_get.",
                schema: || object(json!({
                    "playlist_id": { "type": "string" },
                    "song_id": { "type": "string" }
                }), &["playlist_id", "song_id"]),
                call: |args| send(Method::PUT, format!("/v1/library/playlists/{}/preferred-take", segment(&text(args, "playlist_id")?)), json!({ "song_id": text(args, "song_id")? })),
            },
            Tool {
                name: "playlist_delete",
                description: "Remove a playlist; its songs stay in the library.",
                schema: || id_only("playlist_id", "playlist id"),
                call: |args| send(Method::DELETE, format!("/v1/library/playlists/{}", segment(&text(args, "playlist_id")?)), json!({})),
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
                description: "Split a library song into stems with the chosen separator: six (drums, bass, other, vocals, guitar, piano) with HT-Demucs, or vocals and instrumental with Mel-Band RoFormer (separator_settings_get names it). Optionally select engine for just this split. Each stem becomes a track of the library made from the song (made_from, made_by stems), replacing the stems of an earlier split; stems_get names them. A stem is not split again: split the song it came from. Wait with studio_wait until stems.",
                schema: || object(json!({ "song_id": { "type": "string" }, "engine": { "type": "string", "enum": ["htdemucs", "roformer"] } }), &["song_id"]),
                call: |args| post(format!("/v1/library/songs/{}/stems", segment(&text(args, "song_id")?)), body_without(args, &["song_id"])),
            },
            Tool {
                name: "stems_get",
                description: "The stems of a library song and the progress of a split.",
                schema: || id_only("song_id", "library song id"),
                call: |args| get(format!("/v1/library/songs/{}/stems", segment(&text(args, "song_id")?))),
            },
            // ---------------------------------------------------------------- restore after compression
            Tool {
                name: "restore_status",
                description: "Restore after compression (Apollo, weights CC BY-SA 4.0): whether Apollo and audio.cpp's server (on the stems' device) are installed, what is still to download, the download at work, and the restoration at work or the last one (run: song_id, title, stage, error, library_song).",
                schema: nothing,
                call: |_| get("/v1/restore".into()),
            },
            Tool {
                name: "restore_start",
                description: "Remove the artefacts of MP3 compression from a library track (song_id) or an audio file on this computer (path): the result is 44.1 kHz in the channels of the source and becomes a new library track beside it, titled '<title> · restored' (made_from, made_by restore). Restoring a track again replaces the track its last restoration made; a restored track is not restored again. Apollo must be installed first (restore_install). Wait with studio_wait until restore; restore_status run.library_song names the new track.",
                schema: || object(json!({ "song_id": { "type": "string" }, "path": { "type": "string", "description": "an audio file on this computer, instead of song_id" } }), &[]),
                call: |args| post("/v1/restore/start".into(), args.clone()),
            },
            Tool {
                name: "restore_install",
                description: "Download Apollo (66 MB) and audio.cpp's server of the stems' device, whichever is missing. restore_status shows the progress.",
                schema: nothing,
                call: |_| post("/v1/restore/install".into(), json!({})),
            },
            Tool {
                name: "restore_remove",
                description: "Delete Apollo's weights; the restored tracks stay in the library.",
                schema: nothing,
                call: |_| post("/v1/restore/remove".into(), json!({})),
            },
            Tool {
                name: "restore_cancel",
                description: "Stop the restoration at work, or the download for it.",
                schema: nothing,
                call: |_| post("/v1/restore/cancel".into(), json!({})),
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
                description: "Turn a library track - a song, a stem, a processed take - or any audio file on this computer into multi-instrument MIDI (34 instrument groups and drums). Give song_id or path, or song_ids for several library tracks one after another (midi_status run.batch: done, total, queued, failed); size small, medium (default) or large. What the transcriber needs is downloaded first when it is missing. Wait with studio_wait until midi; a track's MIDI is then kept beside it (midi_get, library_song_files), a file's in the studio's midi folder (midi_status run.file).",
                schema: || object(json!({ "song_id": { "type": "string" }, "song_ids": { "type": "array", "items": { "type": "string" }, "description": "several library tracks, turned into MIDI one after another, instead of song_id" }, "path": { "type": "string", "description": "an audio file on this computer, instead of song_id" }, "size": { "type": "string", "enum": ["small", "medium", "large"] } }), &[]),
                call: |args| post("/v1/midi/transcribe".into(), args.clone()),
            },
            Tool {
                name: "midi_get",
                description: "A library track's MIDI: the file on this computer, the model size, when it was made, its instruments and how many notes. response_format detailed returns notes with pitch, start, end and instrument. For a long track, pass next_offset as offset to read the next page.",
                schema: || object(json!({ "song_id": { "type": "string" }, "response_format": { "type": "string", "enum": ["concise", "detailed"], "description": "include notes" }, "offset": { "type": "integer", "minimum": 0, "description": "next_offset from the previous page of notes" } }), &["song_id"]),
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
                description: "Run audio processing on a library song: any of denoise, lifter (Spectral Lifter), naturalize (vocal naturaliser), vst (plugin chain), master (to a reference), normalize (peak normalization). The optional region in seconds applies the effects only to that part. Each stage is an object of its settings; processing_get shows the result, then processing_keep or processing_discard.",
                schema: || object(json!({ "song_id": { "type": "string" }, "denoise": { "type": "object" }, "lifter": { "type": "object" }, "naturalize": { "type": "object" }, "vst": { "type": "array" }, "master": { "type": "object" }, "normalize": { "type": "object" },
                    "region": { "type": "object", "properties": { "start_seconds": { "type": "number", "minimum": 0 }, "end_seconds": { "type": "number", "exclusiveMinimum": 0 } }, "required": ["start_seconds", "end_seconds"] } }), &["song_id"]),
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
                description: "Delete downloaded model files by component id (see models_catalog), and what a stopped download of them left (partial_bytes in models_catalog): the next download of that component starts from the beginning.",
                schema: || object(json!({ "ids": { "type": "array", "items": { "type": "string" } } }), &["ids"]),
                call: |args| post("/setup/remove".into(), args.clone()),
            },
            Tool {
                name: "ace_music_profiles",
                description: "List native ACE-Step profiles, installed weights, and missing model files.",
                schema: nothing,
                call: |_| get("/v1/music/native/acestep-cpp/profiles".into()),
            },
            Tool {
                name: "minimax_music_profiles",
                description: "List native MiniMax Music 3 profiles, installed weights, and missing model files.",
                schema: nothing,
                call: |_| get("/v1/music/native/minimaxmusic-cpp/profiles".into()),
            },
            Tool {
                name: "controlfoley_model_get",
                description: "Read ControlFoley's Q8 package and FFmpeg and audio.cpp readiness.",
                schema: nothing,
                call: |_| get("/v1/tools/controlfoley/model".into()),
            },
            Tool {
                name: "controlfoley_video_import",
                description: "Import a local MP4, MOV, WebM or MKV into Studio's video folder. The original file is preserved. Pass the returned name to controlfoley_create.",
                schema: || object(json!({"video_path":{"type":"string"}}), &["video_path"]),
                call: |args| {
                    let path = PathBuf::from(text(args, "video_path")?);
                    let name = path.file_name().and_then(|name| name.to_str()).ok_or("Choose a video file.")?;
                    crate::audio_cpp_controlfoley::video_name(name).map_err(|error| error.to_string())?;
                    let extension = path.extension().and_then(|ext| ext.to_str()).ok_or("Choose a video file.")?;
                    let target = format!("foley-{}.{extension}", uuid::Uuid::now_v7().simple());
                    Ok(Call { method: Method::POST, path: format!("/v1/videos?name={target}"), payload: Payload::File(path) })
                },
            },
            Tool {
                name: "controlfoley_create",
                description: "Generate audio for an imported video with ControlFoley, optionally with a text description. The result goes to the library. Use song_job_get or song_job_cancel to follow it.",
                schema: || object(json!({
                    "video_name":{"type":"string"}, "description":{"type":"string"}, "title":{"type":"string"},
                    "duration_seconds":{"type":"number", "minimum":0.2, "maximum":8},
                    "steps":{"type":"integer", "minimum":1}, "seed":{"type":"integer", "minimum":0, "maximum":4294967295_u64}
                }), &["video_name"]),
                call: |args| {
                    post("/v1/music/jobs".into(), json!({
                        "music_source": {"kind":"audio_cpp", "model":crate::audio_cpp_controlfoley::FAMILY},
                        "controlfoley_video": text(args, "video_name")?,
                        "style": args["description"].as_str().unwrap_or_default(), "title": args["title"],
                        "duration_seconds": args["duration_seconds"], "steps": args["steps"], "seed": args["seed"],
                    }))
                },
            },
            Tool {
                name: "native_music_runtime_status",
                description: "Read the pinned native music runtime revision, platform support, missing bytes, and installation progress.",
                schema: native_music_runtime_schema,
                call: |args| get(native_music_runtime_route(args)?),
            },
            Tool {
                name: "native_music_runtime_install",
                description: "Start installing the verified native ACE-Step or MiniMax runtime. Model weights are installed separately.",
                schema: native_music_runtime_schema,
                call: |args| post(native_music_runtime_route(args)?, json!({})),
            },
            Tool {
                name: "native_music_runtime_cancel",
                description: "Cancel this native runtime download without removing any installed runtime.",
                schema: native_music_runtime_schema,
                call: |args| send(Method::DELETE, native_music_runtime_route(args)?, json!({})),
            },
            Tool {
                name: "sound_effect_model_get",
                description: "Read the local MOSS-SoundEffect model and runtime readiness, file source, terms and destination.",
                schema: nothing,
                call: |_| get("/v1/tools/sound-effects/model".into()),
            },
            Tool {
                name: "sound_effect_create",
                description: "Generate a sound effect from an English or Chinese description with local MOSS-SoundEffect. The result goes to the library. Follow with song_job_get or cancel with song_job_cancel.",
                schema: || object(json!({
                    "description": {"type":"string", "minLength":1}, "title": {"type":"string"},
                    "duration_seconds": {"type":"number", "minimum":0.1, "maximum":30},
                    "steps": {"type":"integer", "minimum":1}, "seed": {"type":"integer", "minimum":0, "maximum":4294967295_u64}
                }), &["description"]),
                call: |args| {
                    let description = args["description"].as_str().ok_or("A sound description is required.")?;
                    post("/v1/music/jobs".into(), json!({
                        "music_source": {"kind":"audio_cpp", "model":crate::audio_cpp_sound_effects::FAMILY},
                        "style": description, "title": args["title"], "duration_seconds": args["duration_seconds"],
                        "steps": args["steps"], "seed": args["seed"],
                    }))
                },
            },
            Tool {
                name: "audio_cpp_music_models",
                description: "List audio.cpp music models with experimental status, runtime readiness and installation progress. Manual profiles include the weight source, license, data-root-relative directory and missing bytes.",
                schema: nothing,
                call: |_| get("/v1/music/audio-cpp/models".into()),
            },
            Tool {
                name: "hotstep_cover_profiles",
                description: "List HOT-Step profiles, installed native runtime, adapters and required weights.",
                schema: nothing,
                call: |_| get("/v1/music/native/hotstep/profiles".into()),
            },
            Tool {
                name: "hotstep_cover_install",
                description: "Install the pinned HOT-Step runtime and MuLaCover weights. Its codec and tokenizer files are shared with HeartMuLa.",
                schema: nothing,
                call: |_| post("/v1/music/native/hotstep/install".into(), json!({})),
            },
            Tool {
                name: "hotstep_heartmula_install",
                description: "Install the HOT-Step HeartMuLa 3B Q8_0 generation profile. Read progress in hotstep_cover_profiles.",
                schema: nothing,
                call: |_| post("/v1/music/native/hotstep/heartmula/install".into(), json!({})),
            },
            Tool {
                name: "hotstep_minimax_install",
                description: "Install the published HOT-Step runtime and experimental compact MiniMax Music3 weights. Read progress in hotstep_cover_profiles.",
                schema: nothing,
                call: |_| post("/v1/music/native/hotstep/minimax-music3/install".into(), json!({})),
            },
            Tool {
                name: "hotstep_sa3_install",
                description: "Start the managed HOT-Step Stable Audio 3 Medium installation. Read progress in hotstep_cover_profiles.",
                schema: nothing,
                call: |_| post("/v1/music/native/hotstep/stable-audio-3/install".into(), json!({})),
            },
            Tool {
                name: "vevo2_model_directory_set",
                description: "Use a local folder containing your existing vevo2-q8_0.gguf. Use null to return to the Studio data folder. Read audio_cpp_music_models for readiness and original model terms.",
                schema: || object(json!({"directory": {"type": ["string", "null"]}}), &["directory"]),
                call: |args| post("/v1/music/audio-cpp/vevo2/model-directory".into(), args.clone()),
            },
            Tool {
                name: "mulacover_runtime_install",
                description: "Install the pinned SOXR runtime for MuLaCover on Windows. The local model catalog reports readiness afterwards. On Linux and macOS, use the system libsoxr package.",
                schema: nothing,
                call: |_| post("/v1/music/audio-cpp/mulacover/runtime".into(), json!({})),
            },
            Tool {
                name: "audio_cpp_music_model_install",
                description: "Start installation of a managed audio.cpp music model. Poll audio_cpp_music_models for progress.",
                schema: || object(json!({"model": {"type": "string", "enum": ["heartmula", "stable_audio", "soulx_singer", "soulx_singer_svc"]}}), &["model"]),
                call: |args| post("/v1/music/audio-cpp/models/install".into(), args.clone()),
            },
            Tool {
                name: "engine_logs",
                description: "The music engine's recent log, for when a song fails or the engine will not start.",
                schema: nothing,
                call: |_| get("/v1/engine/logs".into()),
            },
            Tool {
                name: "diagnostics_get",
                description: "The studio's diagnostics, as Settings - Local engine - System diagnostics shows them: version, system, graphics card and CUDA build, memory, the music models and optional parts on disk, the own YuE2 model, DLLs the engine misses, and the latest log lines; `text` is what to paste into an issue.",
                schema: nothing,
                call: |_| get("/v1/diagnostics".into()),
            },
            Tool {
                name: "diagnostics_report",
                description: "Packs a bug report into the data folder's reports: the diagnostics, the journal and the logs, keys masked. Answers the zip's path; nothing is sent anywhere.",
                schema: nothing,
                call: |_| post("/v1/diagnostics/report".into(), json!({})),
            },
            Tool {
                name: "problem_preview",
                description: "Exactly what problem_report would send to Studio Hub now (the words, version, system, card, model set and the cleaned engine log), as Report a problem shows it under \"what will be sent\". kind is bug (default), idea or question; what needs 10 characters at least; attach_log is true unless set false.",
                schema: || object(json!({ "kind": { "type": "string", "enum": ["bug", "idea", "question"] }, "what": { "type": "string" }, "steps": { "type": "string" }, "expected": { "type": "string" }, "attach_log": { "type": "boolean" } }), &["what"]),
                call: |args| post("/v1/problem/preview".into(), args.clone()),
            },
            Tool {
                name: "problem_report",
                description: "Reports a problem without a GitHub account, as Report a problem does: Studio Hub's bot opens a public GitHub issue with the words and the facts problem_preview shows, and answers its number and url. A refusal comes back by code: field, rate (retry_after_s), not_configured, busy or unreachable - then the GitHub form is the way.",
                schema: || object(json!({ "kind": { "type": "string", "enum": ["bug", "idea", "question"] }, "what": { "type": "string" }, "steps": { "type": "string" }, "expected": { "type": "string" }, "attach_log": { "type": "boolean" } }), &["what"]),
                call: |args| post("/v1/problem/report".into(), args.clone()),
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
                description: "Choose the writing assistant: provider none, local (an OpenAI-compatible server at local_base_url with local_model), managed (a model the studio runs: managed_model, or a GGUF at managed_path), openrouter (openrouter_model), agent (the connected MCP agent) or cli (an agent CLI on this machine: cli_agent, cli_model, cli_path); reasoning_effort off, minimal, low, medium or high. own_rules {song, style, lyrics}: the person's own rules for each task, after the studio's guide; avoid_words: words the assistant must not write, one a line or comma-separated.",
                schema: || object(json!({ "provider": { "type": "string", "enum": ["none", "local", "managed", "openrouter", "agent", "cli"] }, "local_base_url": { "type": "string" }, "local_model": { "type": "string" }, "managed_model": { "type": "string" }, "managed_path": { "type": "string" }, "openrouter_model": { "type": "string" }, "cli_agent": { "type": "string" }, "cli_model": { "type": "string" }, "cli_path": { "type": "string" }, "reasoning_effort": { "type": "string" }, "own_rules": { "type": "object", "properties": { "song": { "type": "string" }, "style": { "type": "string" }, "lyrics": { "type": "string" } } }, "avoid_words": { "type": "string" } }), &[]),
                call: |args| send(Method::PUT, "/v1/assistant/status".into(), args.clone()),
            },
            Tool {
                name: "assistant_local_models",
                description: "The models an OpenAI-compatible local server (LM Studio, Ollama...) at base offers.",
                schema: || id_only("base", "the server's base URL, e.g. http://127.0.0.1:1234/v1"),
                call: |args| get(format!("/v1/assistant/local-models?base={}", segment(&text(args, "base")?))),
            },
            Tool {
                name: "assistant_cli_agents",
                description: "The agent CLIs the cli assistant can run (Claude Code, Codex, opencode, Antigravity, Muse Code): which are installed, where, their version and default model, and how to install and sign in to the others.",
                schema: || object(json!({}), &[]),
                call: |_| get("/v1/assistant/cli-agents".into()),
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
                description: "Choose the recogniser used for karaoke and for dataset lyrics: enabled, provider (parakeet, whisper - CrisperWhisper 2.0 turbo in audio.cpp's server -, openrouter), whisper_model (for parakeet: parakeet-tdt-int8 is Parakeet v3, parakeet-ultra-int8 is Ultra, both on transcribe.cpp), openrouter_model, runtime (cpu, or cuda for the card: transcribe.cpp and the audio.cpp server run CUDA on an NVIDIA card, Vulkan on any other).",
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
                name: "karaoke_set",
                description: "Set a library song's karaoke from a subtitle file on this computer (path: .srt, .ass, .ssa or .lrc) or from LRC text (lrc: one [mm:ss.cc] line each), in place of the recognised timings.",
                schema: || object(json!({ "song_id": { "type": "string" }, "path": { "type": "string" }, "lrc": { "type": "string" } }), &["song_id"]),
                call: |args| {
                    let path = format!("/v1/library/songs/{}/karaoke", segment(&text(args, "song_id")?));
                    if let Some(lrc) = args.get("lrc").and_then(Value::as_str) {
                        return send(Method::PUT, path, json!({ "lrc": lrc }));
                    }
                    let file = PathBuf::from(text(args, "path")?);
                    use base64::Engine;
                    let bytes = std::fs::read(&file).map_err(|error| format!("read {}: {error}", file.display()))?;
                    let content = base64::engine::general_purpose::STANDARD.encode(bytes);
                    send(Method::PUT, path, json!({ "content_base64": content, "file_name": file_name(&file) }))
                },
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
                description: "Download for the separator: an engine whole on the settings' device (htdemucs: its model and audio.cpp's server; roformer: Mel-Band RoFormer and its BSRoformer.cpp build), the chosen engine on a device (auto, cuda, cpu), or a single asset_id from separator_runtime.",
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
                description: "Delete a separator's files: asset_id roformer deletes Mel-Band RoFormer and its engine, otherwise the HT-Demucs model.",
                schema: || object(json!({ "asset_id": { "type": "string", "enum": ["htdemucs", "roformer"] } }), &[]),
                call: |args| post("/v1/separation/remove".into(), json!({ "asset_id": args.get("asset_id").and_then(Value::as_str).unwrap_or("htdemucs") })),
            },
            Tool {
                name: "separator_settings_get",
                description: "Separator settings: runtime (auto, the card or the processor), the stems to keep and the engine.",
                schema: nothing,
                call: |_| get("/v1/separation/settings".into()),
            },
            Tool {
                name: "separator_settings_set",
                description: "Save separator settings: the object as separator_settings_get returns it, with your changes. engine htdemucs splits six stems; roformer (Mel-Band RoFormer, Dub Studio's separator) splits the vocals and the instrumental, more cleanly, and also separates the vocals of vocals-only songs and of training; install it with separator_install after choosing it.",
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
                name: "storage_get",
                description: "Where the studio's downloads are kept - the model sets, LoRAs, the stem separator, karaoke, the audio.cpp runtime, the assistant's models, the trainer and Audio to MIDI: the folder (the studio's data folder, or one a person chose on another disk), the room free and in all on its disk, what each folder takes and whether it is kept elsewhere behind a link; and a move in progress - the folder at work and the bytes copied of it - or what the last move came to, folder by folder.",
                schema: nothing,
                call: |_| get("/v1/storage".into()),
            },
            Tool {
                name: "storage_set",
                description: "Move the studio's downloads to another disk, or back. directory: a folder (full path) - the studio makes its own folder inside it, moves each download folder there and leaves a link where it was, so nothing else changes; null brings them back into the studio's data folder. The library, datasets and training runs never move. The engine and the writing assistant stop for the move and the engine starts again after it; refused while a song, a download or any other work is running. On the same disk it is instant; to another disk the files are copied and checked first. storage_get tells how far it is; a folder that could not move says why and stays where it was.",
                schema: || object(json!({ "directory": { "type": ["string", "null"] } }), &["directory"]),
                call: |args| send(Method::PUT, "/v1/storage".into(), args.clone()),
            },
            Tool {
                name: "storage_cancel",
                description: "Stop a move started by storage_set after the file being copied: the folder at work stays where it was, those already moved stay moved.",
                schema: nothing,
                call: |_| post("/v1/storage/cancel".into(), json!({})),
            },
            Tool {
                name: "song_export",
                description: "Write a library song out as a new file; the song in the library is not changed. format: wav (bit_depth 16, 24 or 32f), flac (16 or 24) or mp3 (mp3_kbps 320, 256, 224, 192, 160 or 128). sample_rate: 44100, 48000, 88200 or 96000; left out, the song's own. loudness: {target_lufs, ceiling_dbtp} brings the song to a loudness target by one gain held under the true-peak ceiling (-14 and -1 is the common streaming reference, -23 and -1 EBU R 128); there is no limiter, and the report says when the ceiling stopped the gain short. Dither is added only when bits are taken away. folder: the folder on this computer to write into, a full path; the file never replaces another. Answers the export's id: song_export_get tells its stage and, when done, the file's path and what was done to the sound.",
                schema: || object(json!({
                    "song_id": { "type": "string" },
                    "folder": { "type": "string" },
                    "format": { "type": "string", "enum": ["wav", "flac", "mp3"] },
                    "sample_rate": { "type": "integer", "enum": [44100, 48000, 88200, 96000] },
                    "bit_depth": { "type": "string", "enum": ["16", "24", "32f"] },
                    "mp3_kbps": { "type": "integer", "enum": [320, 256, 224, 192, 160, 128] },
                    "loudness": { "type": "object", "properties": { "target_lufs": { "type": "number", "minimum": -40, "maximum": -5 }, "ceiling_dbtp": { "type": "number", "minimum": -9, "maximum": 0 } }, "required": ["target_lufs", "ceiling_dbtp"] }
                }), &["song_id", "folder", "format"]),
                call: |args| post("/v1/exports".into(), args.clone()),
            },
            Tool {
                name: "song_export_get",
                description: "An export started by song_export: its stage and fraction while it runs; when done, the file's path and the report - the format written, the loudness and true peak before and after, the gain, and a note for each thing done to the sound; or the error.",
                schema: || id_only("export_id", "the id song_export answered"),
                call: |args| get(format!("/v1/exports/{}", segment(&text(args, "export_id")?))),
            },
            Tool {
                name: "song_export_cancel",
                description: "Stop a running export; nothing is written.",
                schema: || id_only("export_id", "the id song_export answered"),
                call: |args| post(format!("/v1/exports/{}/cancel", segment(&text(args, "export_id")?)), json!({})),
            },
            Tool {
                name: "song_export_live",
                description: "Write a library song as an Ableton Live 12 project, a zip in the folder named (a full path): a Live set with the song on a track that plays, each stem the song has on a track that is switched off, and the parts of its score and its own MIDI on MIDI tracks, at the score's tempo and meter (Live's default when the song has no score), with the audio and MIDI files beside the set and a read-me. It is made from what the song has now: stems_split and midi_transcribe first, to have stems and MIDI in it. Answers the zip's path, the tracks, and the files left off a track (a set holds eight, as Live Lite does).",
                schema: || object(json!({ "song_id": { "type": "string" }, "folder": { "type": "string" } }), &["song_id", "folder"]),
                call: |args| post("/v1/exports/live".into(), args.clone()),
            },
            Tool {
                name: "song_export_zip",
                description: "Prepare a ZIP for selected library tracks with original audio, lyrics, prompts, settings and available ABC and MIDI. Answers with the file name and the local HTTP download path.",
                schema: || object(json!({ "song_ids": { "type": "array", "items": { "type": "string" }, "minItems": 1 } }), &["song_ids"]),
                call: |args| post("/v1/exports/tracks".into(), args.clone()),
            },
            Tool {
                name: "library_song_midi_files",
                description: "The MIDI a library song has for a DAW, each file named for where its notes come from. score: the score the song was generated with - its tempo, meter and key, and the parts with notes: score (the whole of it), lead, voice, chords; a song made without a score has none. own: the track's own MIDI, transcribed from the recording (an estimate) or edited in the MIDI editor.",
                schema: || id_only("song_id", "library song id"),
                call: |args| get(format!("/v1/library/songs/{}/midi/parts", segment(&text(args, "song_id")?))),
            },
            Tool {
                name: "library_song_midi_part_write",
                description: "Write one of a song's MIDI files into a folder on this computer: part is score, lead, voice or chords (from the score the song was generated with, at its tempo, meter and key, with its section markers) or own (the track's own MIDI). folder: a full path. Answers the file's path; the file never replaces another.",
                schema: || object(json!({ "song_id": { "type": "string" }, "part": { "type": "string", "enum": ["score", "lead", "voice", "chords", "own"] }, "folder": { "type": "string" } }), &["song_id", "part", "folder"]),
                call: |args| post(format!("/v1/library/songs/{}/midi/parts/{}", segment(&text(args, "song_id")?), segment(&text(args, "part")?)), body_without(args, &["song_id", "part"])),
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
                name: "lyrics_recognise",
                description: "The lyrics a library song or an audio file on this computer sings, heard by the recogniser chosen for karaoke (Settings - Karaoke) and laid out in lines. Give song_id or path.",
                schema: || object(json!({ "song_id": { "type": "string" }, "path": { "type": "string" }, "language": { "type": "string" } }), &[]),
                call: |args| {
                    use base64::Engine;
                    if let Some(song_id) = args.get("song_id").and_then(Value::as_str) {
                        return post("/v1/lyrics/recognise".into(), json!({ "song_id": song_id, "language": args.get("language") }));
                    }
                    let path = PathBuf::from(text(args, "path")?);
                    let bytes = std::fs::read(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
                    let format = path.extension().and_then(|value| value.to_str()).unwrap_or("wav").to_lowercase();
                    post("/v1/lyrics/recognise".into(), json!({ "audio_base64": base64::engine::general_purpose::STANDARD.encode(bytes), "audio_format": format, "audio_name": file_name(&path), "language": args.get("language") }))
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
                name: "rvc_catalog_list",
                description: "List up to 20 curated RVC voices, license terms, demo WAV/MP3 URLs when published, and installed local IDs. Pass next_offset as offset for the next page.",
                schema: || object(json!({
                    "offset": { "type": "integer", "minimum": 0 },
                    "query": { "type": "string", "description": "Case-insensitive words in the name or source labels" },
                    "vocal_style": { "type": "string", "description": "Exact style tag such as high" },
                    "rvc_version": { "type": "string", "description": "Exact version such as v2" },
                    "sample_rate_hz": { "type": "integer", "minimum": 1 },
                    "has_index": { "type": "boolean" },
                    "has_sample": { "type": "boolean" },
                }), &[]),
                call: |_| get("/v1/rvc/catalog".into()),
            },
            Tool {
                name: "rvc_catalog_progress_get",
                description: "Read preparation, file download and verification progress for a catalog voice being installed.",
                schema: || id_only("id", "the ID from rvc_catalog_list"),
                call: |args| get(format!("/v1/rvc/catalog/progress/{}", segment(&text(args, "id")?))),
            },
            Tool {
                name: "music_package_updates_get",
                description: "Read the installed packages with independent updates available and the background update policy.",
                schema: nothing,
                call: |_| get("/v1/components/updates".into()),
            },
            Tool {
                name: "music_package_update_mode_set",
                description: "Set the policy for installed packages. Manual checks on request, notify refreshes the catalogue, auto updates installed packages.",
                schema: || object(json!({"mode":{"type":"string","enum":["manual","notify","auto"]}}), &["mode"]),
                call: |args| post("/v1/components/updates/mode".into(), args.clone()),
            },
            Tool {
                name: "music_package_progress",
                description: "Read the current file, transfer stage, reused and downloaded bytes for one signed package.",
                schema: || object(json!({"kind":{"type":"string","enum":["music-engines","music-models"]},"id":{"type":"string"}}), &["kind","id"]),
                call: |args| get(format!("/v1/components/packages/{}/{}/progress", segment(&text(args,"kind")?),segment(&text(args,"id")?))),
            },
            Tool {
                name: "music_installed_package_list",
                description: "Read installed music packages and their retained generations from local signed metadata, including packages removed from the online catalogue.",
                schema: nothing,
                call: |_| get("/v1/components/packages/installed".into()),
            },
            Tool {
                name: "music_package_rollback",
                description: "Restore the previous installed generation after a full offline integrity check. Pass the active and previous generation IDs from package status.",
                schema: || object(json!({"kind":{"type":"string","enum":["music-engines","music-models"]},"id":{"type":"string"},"from_generation":{"type":"string"},"to_generation":{"type":"string"}}), &["kind","id","from_generation","to_generation"]),
                call: |args| post(format!("/v1/components/packages/{}/{}/rollback",segment(&text(args,"kind")?),segment(&text(args,"id")?)),args.clone()),
            },
            Tool {
                name: "music_package_cancel",
                description: "Request cancellation of an active signed package installation.",
                schema: || object(json!({"kind":{"type":"string","enum":["music-engines","music-models"]},"id":{"type":"string"}}), &["kind","id"]),
                call: |args| post(format!("/v1/components/packages/{}/{}/cancel", segment(&text(args,"kind")?),segment(&text(args,"id")?)),json!({})),
            },
            Tool {
                name: "music_model_package_install",
                description: "Install an independently published music model, reusing verified weights already present.",
                schema: || id_only("id", "model ID in the signed music catalogue"),
                call: |args| post(format!("/v1/components/music-models/{}/install", segment(&text(args, "id")?)), json!({})),
            },
            Tool {
                name: "music_model_package_status",
                description: "Verify the installed model and its compatibility with the named engine.",
                schema: || object(json!({"engine":{"type":"string"},"model":{"type":"string"}}), &["engine","model"]),
                call: |args| get(format!("/v1/components/music-engines/{}/models/{}/status", segment(&text(args, "engine")?),segment(&text(args, "model")?))),
            },
            Tool {
                name: "music_engine_package_install",
                description: "Verify and activate the signed engine package for this platform. A failed update preserves the active generation.",
                schema: || id_only("id", "the engine ID from music_engine_catalog_list"),
                call: |args| post(format!("/v1/components/music-engines/{}/install", segment(&text(args, "id")?)), json!({})),
            },
            Tool {
                name: "music_engine_package_status",
                description: "Check the active engine generation, its signature, and the contents of its runtime directory.",
                schema: || id_only("id", "the installed engine ID"),
                call: |args| get(format!("/v1/components/music-engines/{}/status", segment(&text(args, "id")?))),
            },
            Tool {
                name: "music_engine_catalog_list",
                description: "List engine packages in the accepted signed catalogue without network access.",
                schema: nothing,
                call: |_| get("/v1/components/music-engines".into()),
            },
            Tool {
                name: "music_engine_catalog_refresh",
                description: "Refresh signed music engine package manifests. Installed versions remain available.",
                schema: nothing,
                call: |_| post("/v1/components/music-engines/refresh".into(), json!({})),
            },
            Tool {
                name: "rvc_catalog_refresh",
                description: "Update the curated RVC voices from the installed signed component channel. A failed update preserves the last accepted catalogue.",
                schema: nothing,
                call: |_| post("/v1/rvc/catalog/refresh".into(), json!({})),
            },
            Tool {
                name: "rvc_catalog_install",
                description: "Download and verify a voice from the curated RVC catalog by its ID.",
                schema: || id_only("id", "the ID from rvc_catalog_list"),
                call: |args| post("/v1/rvc/catalog/install".into(), json!({ "id": text(args, "id")? })),
            },
            Tool {
                name: "rvc_voices_list",
                description: "List local RVC voices and their optional retrieval indexes.",
                schema: nothing,
                call: |_| get("/v1/rvc/voices".into()),
            },
            Tool {
                name: "rvc_backend_status",
                description: "Check RVC backend readiness without starting a native worker. Metal is experimental and its device is checked on start.",
                schema: nothing,
                call: |_| get("/v1/rvc/apply/setup".into()),
            },
            Tool {
                name: "rvc_apply_install",
                description: "Install the audio.cpp conversion runtime and its RVC support weights.",
                schema: nothing,
                call: |_| post("/v1/rvc/apply/setup".into(), json!({})),
            },
            Tool {
                name: "rvc_native_runtime_status",
                description: "Check host support, selected backend, remaining bytes and download progress for vocal-fast-rs without starting the worker.",
                schema: rvc_runtime_schema,
                call: |args| get(rvc_runtime_route(args)),
            },
            Tool {
                name: "rvc_native_runtime_install",
                description: "Start downloading the verified vocal-fast-rs runtime for this host and its acoustic weights. Supported runtimes include CPU on Apple Silicon with macOS 13+ and TensorRT-RTX on compatible Windows and Linux hardware. Read rvc_native_runtime_status for progress.",
                schema: rvc_runtime_schema,
                call: |args| post(rvc_runtime_route(args), json!({})),
            },
            Tool {
                name: "rvc_native_runtime_cancel",
                description: "Cancel the active vocal-fast-rs runtime download. Keeps installed components.",
                schema: rvc_runtime_schema,
                call: |args| send(Method::DELETE, rvc_runtime_route(args), json!({})),
            },
            Tool {
                name: "rvc_preview",
                description: "Prepare time-aligned before and after WAVs of an isolated vocal fragment with the chosen installed RVC voice. Returns both URLs and the actual bounds; leaves the library untouched.",
                schema: || object(json!({
                    "song_id": { "type": "string" },
                    "voice_id": { "type": "string", "description": "The installed ID from rvc_voices_list" },
                    "start_seconds": { "type": "number", "minimum": 0 },
                    "end_seconds": { "type": "number", "description": "Between 0.5 and 15 seconds after start; shortened to the vocal end if necessary" },
                    "semitone_shift": { "type": "integer", "minimum": -24, "maximum": 24 },
                    "retrieval_blend": { "type": "number", "minimum": 0, "maximum": 1 },
                    "backend": { "type": "string", "enum": ["auto", "vocal-fast-rs", "vocal-fast-metal", "burn-cuda", "audio.cpp"] },
                }), &["song_id", "voice_id", "start_seconds", "end_seconds"]),
                call: |args| post("/v1/rvc/preview".into(), args.clone()),
            },
            Tool {
                name: "seed_vc_status",
                description: "Read Seed-VC model and audio.cpp runtime readiness for singing conversion.",
                schema: nothing,
                call: |_| get("/v1/seed-vc".into()),
            },
            Tool {
                name: "seed_vc_apply",
                description: "Replace a separated song's vocal timbre with Seed-VC using a 1–25 second voice sample. Upload it with processing_set_reference first and pass its upload_id. The original song stays and the result is a new take.",
                schema: || object(json!({"song_id":{"type":"string"}, "upload_id":{"type":"string"}}), &["song_id","upload_id"]),
                call: |args| post("/v1/seed-vc".into(), json!({
                    "song_id": text(args, "song_id")?, "upload_id": text(args, "upload_id")?
                })),
            },
            Tool {
                name: "rvc_apply",
                description: "Convert the separated vocal of a library song with an installed RVC voice, mix it with the existing accompaniment, and save a new song.",
                schema: || object(json!({
                    "song_id": { "type": "string" },
                    "voice_id": { "type": "string", "description": "The installed ID from rvc_voices_list" },
                    "semitone_shift": { "type": "integer", "minimum": -24, "maximum": 24 },
                    "retrieval_blend": { "type": "number", "minimum": 0, "maximum": 1 },
                    "backend": { "type": "string", "enum": ["auto", "vocal-fast-rs", "vocal-fast-metal", "burn-cuda", "audio.cpp"] },
                }), &["song_id", "voice_id"]),
                call: |args| post("/v1/rvc/apply".into(), args.clone()),
            },
            Tool {
                name: "rvc_voice_selection_get",
                description: "Read the installed voice and backend remembered for the quick RVC library action.",
                schema: nothing,
                call: |_| get("/v1/rvc/selection".into()),
            },
            Tool {
                name: "rvc_voice_select",
                description: "Remember the installed RVC voice and backend used by the quick library action.",
                schema: || object(json!({
                    "voice_id": { "type": "string" },
                    "backend": { "enum": ["auto", "vocal-fast-rs", "vocal-fast-metal", "burn-cuda", "audio.cpp"] }
                }), &["voice_id"]),
                call: |args| send(Method::PUT, "/v1/rvc/selection".into(), args.clone()),
            },
            Tool {
                name: "rvc_quick_start",
                description: "Save a separate RVC take of a library song. Uses the remembered voice by default and separates missing stems. Repeating the same song, voice and backend reuses the job or the saved take.",
                schema: || object(json!({
                    "song_id": { "type": "string" },
                    "voice_id": { "type": "string" },
                    "backend": { "enum": ["auto", "vocal-fast-rs", "vocal-fast-metal", "burn-cuda", "audio.cpp"] }
                }), &["song_id"]),
                call: |args| post("/v1/rvc/quick".into(), args.clone()),
            },
            Tool {
                name: "rvc_quick_list",
                description: "List quick and automatic RVC take jobs, including their current stage and saved take.",
                schema: nothing,
                call: |_| get("/v1/rvc/quick".into()),
            },
            Tool {
                name: "rvc_quick_get",
                description: "Read a quick RVC take job by ID.",
                schema: || id_only("id", "the job ID from rvc_quick_start or rvc_quick_list"),
                call: |args| get(format!("/v1/rvc/quick/{}", segment(&text(args, "id")?))),
            },
            Tool {
                name: "rvc_quick_cancel",
                description: "Stop a queued or active RVC take before it is saved. A completed take stays in the library.",
                schema: || id_only("id", "the job ID from rvc_quick_start or rvc_quick_list"),
                call: |args| send(Method::DELETE, format!("/v1/rvc/quick/{}", segment(&text(args, "id")?)), json!({})),
            },
            Tool {
                name: "rvc_voice_remove",
                description: "Remove the installed RVC checkpoint and its retrieval index. Keeps existing songs and rendered audio.",
                schema: || id_only("id", "the installed ID from rvc_voices_list"),
                call: |args| send(Method::DELETE, format!("/v1/rvc/voices/{}", segment(&text(args, "id")?)), json!({})),
            },
            Tool {
                name: "rvc_voice_import",
                description: "Import a local RVC inference checkpoint and an optional FAISS retrieval index into the voice catalog.",
                schema: || object(json!({
                    "model_path": { "type": "string", "description": "The exported inference .pth or .pt file" },
                    "index_path": { "type": "string", "description": "Optional filled FAISS .index file" },
                }), &["model_path"]),
                call: |args| {
                    let model = PathBuf::from(text(args, "model_path")?);
                    let mut files = vec![("model".into(), model.clone(), file_name(&model))];
                    if let Some(path) = args.get("index_path").and_then(Value::as_str) {
                        let index = PathBuf::from(path);
                        files.push(("index".into(), index.clone(), file_name(&index)));
                    }
                    Ok(Call { method: Method::POST, path: "/v1/rvc/voices/import".into(), payload: Payload::Form { fields: Vec::new(), files } })
                },
            },
            Tool {
                name: "lora_import_folder",
                description: "Install every LoRA of a folder on this computer and its subfolders without copying it: a folder with adapter_config.json is one LoRA, elsewhere each .safetensors is one; files are hard-linked, and copied only from another drive (copied: true says which).",
                schema: || object(json!({ "path": { "type": "string", "description": "the folder, e.g. the LoRA folder ComfyUI or a trainer uses" } }), &["path"]),
                call: |args| post("/v1/adapters/import-folder".into(), json!({ "path": text(args, "path")? })),
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
                description: "Stop the trainer download. Set training_base true for a separately selected base.",
                schema: || object(json!({ "training_base": { "type": "boolean" } }), &[]),
                call: |args| {
                    let path = if args.get("training_base").and_then(Value::as_bool) == Some(true) {
                        "/v1/training/bases/cancel"
                    } else {
                        "/v1/training/pack/cancel"
                    };
                    post(path.into(), json!({}))
                },
            },
            Tool {
                name: "dataset_prepare_train_after",
                description: "For the preparation at work: start this training run once every song is ready (train: {name, recipe, optional base_id}), or disable auto-start (train: null).",
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
                description: "Read a dataset. Set page or page_size for a short page of titles, lengths and readiness, with total and has_more. Set response_format detailed for full rows on that page, or give song_id to read one complete song.",
                schema: || object(json!({
                    "dataset_id": { "type": "string" },
                    "song_id": { "type": "string" },
                    "page": { "type": "integer", "minimum": 1 },
                    "page_size": { "type": "integer", "minimum": 1, "maximum": 100 },
                    "response_format": { "type": "string", "enum": ["concise", "detailed"] },
                }), &["dataset_id"]),
                call: |args| {
                    let mut path = format!("/v1/training/datasets/{}", segment(&text(args, "dataset_id")?));
                    if let Some(song_id) = args.get("song_id").and_then(Value::as_str) {
                        path.push_str(&format!("?song_id={}", segment(song_id)));
                    } else if args.get("page").is_some() || args.get("page_size").is_some() {
                        let page = args.get("page").and_then(Value::as_u64).unwrap_or(1).max(1);
                        let size = args.get("page_size").and_then(Value::as_u64).unwrap_or(25).clamp(1, 100);
                        let detailed = args["response_format"] == "detailed";
                        path.push_str(&format!("?page={page}&page_size={size}&detailed={detailed}"));
                    }
                    get(path)
                },
            },
            Tool {
                name: "training_pack_install",
                description: "Download the standard trainer and its weights. With base_id, install a separate base listed by detailed training_status.",
                schema: || object(json!({ "base_id": { "type": "string" } }), &[]),
                call: |args| match args.get("base_id").and_then(Value::as_str) {
                    Some(id) => post(format!("/v1/training/bases/{}/install", segment(id)), json!({})),
                    None => post("/v1/training/pack/install".into(), json!({})),
                },
            },
            Tool {
                name: "training_listen_pack_cancel",
                description: "Stop the listening pack download, including its ONNX runtime.",
                schema: nothing,
                call: |_| post("/v1/training/listen/cancel".into(), json!({})),
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
                name: "dataset_song_describe_style",
                description: "Describe a dataset recording by ear and save its style and the measured tempo. Waits for a song already rendering. Requires the listening pack.",
                schema: || object(json!({ "dataset_id": { "type": "string" }, "song_id": { "type": "string" } }), &["dataset_id", "song_id"]),
                call: |args| post(format!("/v1/training/datasets/{}/items/{}/describe", segment(&text(args, "dataset_id")?), segment(&text(args, "song_id")?)), json!({})),
            },
            Tool {
                name: "dataset_song_delete",
                description: "Remove a song from a dataset.",
                schema: || object(json!({ "dataset_id": { "type": "string" }, "song_id": { "type": "string" } }), &["dataset_id", "song_id"]),
                call: |args| send(Method::DELETE, format!("/v1/training/datasets/{}/items/{}", segment(&text(args, "dataset_id")?), segment(&text(args, "song_id")?)), json!({})),
            },
            Tool {
                name: "lyrics_syllables",
                description: "Each sung line of the lyrics with its syllable count as the score lays the words out: new words with the same counts keep a score's melody, a line of another count moves it.",
                schema: || object(json!({ "lyrics": { "type": "string" } }), &["lyrics"]),
                call: |args| post("/v1/lyrics/syllables".into(), json!({ "lyrics": text(args, "lyrics")? })),
            },
            Tool {
                name: "lyrics_find",
                description: "Look a song's published lyrics up in LRCLIB, QQ Music and Kugou by artist, title and length; returns the plain and timed lyrics word for word, or that it is an instrumental, or nothing.",
                schema: || object(json!({ "artist": { "type": "string" }, "title": { "type": "string" }, "seconds": { "type": "number" } }), &["title"]),
                call: |args| post("/v1/lyrics/find".into(), args.clone()),
            },
            Tool {
                name: "dataset_prepare",
                description: "Fill a dataset in by itself: lyrics from the lyric databases, CrisperWhisper only for songs they miss, sections laid out by the assistant, styles written from what MOSS-Music hears. lyrics and style: missing (only what is not done yet), all (again, for the chosen songs) or none. items: song ids, all songs when left out. train: start this run once every song is ready. Watch training_status.",
                schema: || object(json!({
                    "dataset_id": { "type": "string" },
                    "items": { "type": "array", "items": { "type": "string" } },
                    "lyrics": { "type": "string", "enum": ["none", "missing", "all"] },
                    "skip_asr": { "type": "boolean", "description": "Look up missing lyrics and keep existing text. Skip vocal separation and speech recognition for songs the databases miss." },
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
                name: "recording_describe_style",
                description: "Describe the style of an audio file on this computer by ear, as library_song_describe_style does for a library song: the cover form's recording taken from disk. path: the file; lyrics: the words the style is written for (an empty one is an instrumental). Needs auto-describe installed; holds the card for a minute.",
                schema: || object(json!({ "path": { "type": "string" }, "lyrics": { "type": "string" } }), &["path"]),
                call: |args| {
                    let path = PathBuf::from(text(args, "path")?.trim());
                    let mut fields = Vec::new();
                    if let Some(lyrics) = args.get("lyrics").and_then(Value::as_str) {
                        fields.push(("lyrics".to_string(), lyrics.to_string()));
                    }
                    let files = vec![("audio".to_string(), path.clone(), file_name(&path))];
                    Ok(Call { method: Method::POST, path: "/v1/describe-style".into(), payload: Payload::Form { fields, files } })
                },
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
                name: "hotstep_heartmula_training_status",
                description: "Report CUDA readiness and asset progress for HOT-Step HeartMuLa 3B.",
                schema: nothing,
                call: |_| get("/v1/training/hotstep-heartmula".into()),
            },
            Tool {
                name: "hotstep_heartmula_training_install",
                description: "Install the shared pinned HOT-Step CUDA trainer and HeartCodec encoder weights.",
                schema: nothing,
                call: |_| post("/v1/training/hotstep-heartmula/install".into(), json!({})),
            },
            Tool {
                name: "hotstep_heartmula_training_start",
                description: "Prepare HeartCodec samples and train a HeartMuLa 3B LoRA (set style for all samples and lyrics for vocal samples). Read the resulting adapter in training_status.",
                schema: || object(json!({"dataset_id":{"type":"string"},"name":{"type":"string"},"settings":{"type":"object","properties":{"steps":{"type":"integer","minimum":1},"rank":{"type":"integer","minimum":1},"alpha":{"type":"number"},"learning_rate":{"type":"number"},"duration_seconds":{"type":"number","minimum":1,"maximum":384},"seed":{"type":"integer","minimum":0}}}}), &["dataset_id"]),
                call: |args| post("/v1/training/hotstep-heartmula".into(), args.clone()),
            },
            Tool {
                name: "hotstep_sa3_training_status",
                description: "Report CUDA readiness and download progress for HOT-Step Stable Audio 3 Medium training.",
                schema: nothing,
                call: |_| get("/v1/training/hotstep-sa3".into()),
            },
            Tool {
                name: "hotstep_sa3_training_install",
                description: "Download the pinned Stable Audio 3 Medium training base and native HOT-Step CUDA components.",
                schema: nothing,
                call: |_| post("/v1/training/hotstep-sa3/install".into(), json!({})),
            },
            Tool {
                name: "hotstep_sa3_training_start",
                description: "Train a native HOT-Step Stable Audio 3 Medium DoRA adapter from a prepared instrumental dataset. A finished adapter is available in the HOT-Step Create profile.",
                schema: || object(json!({"dataset_id":{"type":"string"},"name":{"type":"string"},"settings":{"type":"object","properties":{"steps":{"type":"integer","minimum":1},"rank":{"type":"integer","minimum":1},"alpha":{"type":"number"},"learning_rate":{"type":"number"},"duration_seconds":{"type":"number","minimum":1,"maximum":384},"seed":{"type":"integer","minimum":0}}}}), &["dataset_id"]),
                call: |args| post("/v1/training/hotstep-sa3".into(), args.clone()),
            },
            Tool {
                name: "rvc_training_status",
                description: "Check the native Burn RVC training pack, accelerator and missing files.",
                schema: nothing,
                call: |_| get("/v1/training/rvc-burn".into()),
            },
            Tool {
                name: "rvc_training_start",
                description: "Train an RVC v2 Safetensors voice on isolated voice recordings in a Studio dataset, using native Burn CUDA.",
                schema: || object(json!({"dataset_id":{"type":"string"},"name":{"type":"string"},"settings":{"type":"object","properties":{"epochs":{"type":"integer","minimum":1},"batch_size":{"type":"integer","minimum":1},"learning_rate":{"type":"number"}}}}), &["dataset_id"]),
                call: |args| post("/v1/training/rvc-burn".into(), args.clone()),
            },
            Tool {
                name: "rvc_training_install_runtime",
                description: "Download and verify the pinned RVC Burn CUDA trainer and its resources. Follow the transfer in rvc_training_status.",
                schema: nothing,
                call: |_| post("/v1/training/rvc-burn/setup".into(), json!({})),
            },
            Tool {
                name: "rvc_training_runtime_cancel",
                description: "Cancel the current RVC Burn component download.",
                schema: nothing,
                call: |_| Ok(Call { method: Method::DELETE, path: "/v1/training/rvc-burn/setup".into(), payload: Payload::None }),
            },
            Tool {
                name: "rvc_training_install_voice",
                description: "Install a saved native RVC Burn training checkpoint in the voice catalogue. Use the returned ID with rvc_apply.",
                schema: || id_only("run_id", "RVC Burn run ID"),
                call: |args| post(format!("/v1/training/rvc-burn/{}/install", segment(&text(args, "run_id")?)), json!({})),
            },
            Tool {
                name: "training_start",
                description: "Train a LoRA on a dataset. recipe: the recipe from training_status (recipe_defaults) with your changes. preset picks the size: 'fast' (100 steps of 4 songs), 'balanced' (200 of 4, the default), 'thorough' (300 of 8) - the base-matched method, trained the way the base model was; or 'tuned', the previous recipe, with stop 'kl' and target_kl 1.4 or stop 'epochs' with epochs. Watch training_status; one run at a time, and it holds the card.",
                schema: || object(json!({ "dataset_id": { "type": "string" }, "name": { "type": "string" }, "recipe": { "type": "object" }, "trainer_id": { "type": "string" }, "base_id": { "type": "string" } }), &["dataset_id", "recipe"]),
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
                name: "training_checkpoints_listen",
                description: "Queue a 35-second YuE2 base take and one take per saved LoRA checkpoint, using one score and seed. Reuses previous takes. Returns the comparison playlist and job IDs.",
                schema: || id_only("run_id", "YuE2 training run id"),
                call: |args| post(format!("/v1/training/runs/{}/checkpoints/listen", segment(&text(args, "run_id")?)), json!({})),
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
            Tool {
                name: "network_proxy_get",
                description: "How the studio reaches the internet: no proxy, the system's, or a proxy address of a kind (http, https, socks5, socks4), and whether models come from the HF-Mirror instead of Hugging Face; with the last problem the proxy had.",
                schema: nothing,
                call: |_| get("/v1/network/proxy".into()),
            },
            Tool {
                name: "network_proxy_set",
                description: "Set how the studio reaches the internet, as Settings - Network does: mode off, system or custom (with address and kind), and huggingface_mirror to take models from the HF-Mirror. Test it first with network_proxy_test.",
                schema: || object(json!({ "mode": { "type": "string", "enum": ["off", "system", "custom"] }, "address": { "type": "string", "description": "host:port of a custom proxy" }, "kind": { "type": "string", "enum": ["http", "https", "socks5", "socks4"] }, "huggingface_mirror": { "type": "boolean" } }), &["mode", "kind", "huggingface_mirror"]),
                call: |args| send(Method::PUT, "/v1/network/proxy".into(), args.clone()),
            },
            Tool {
                name: "network_proxy_test",
                description: "Try a proxy setting without keeping it: says whether Hugging Face and the HF-Mirror answer through it.",
                schema: || object(json!({ "mode": { "type": "string", "enum": ["off", "system", "custom"] }, "address": { "type": "string" }, "kind": { "type": "string", "enum": ["http", "https", "socks5", "socks4"] }, "huggingface_mirror": { "type": "boolean" } }), &["mode", "kind", "huggingface_mirror"]),
                call: |args| post("/v1/network/proxy/test".into(), args.clone()),
            },
            Tool {
                name: "network_access_get",
                description: "Whether the studio can be opened from other computers and phones, its address there and the access key, as Settings - Network shows.",
                schema: nothing,
                call: |_| get("/v1/network".into()),
            },
            Tool {
                name: "network_access_set",
                description: "Turn access from the network on or off (enabled), or make a new access key (new_key: every browser signed in with the old one is out). It takes effect when the studio is opened again.",
                schema: || object(json!({ "enabled": { "type": "boolean" }, "new_key": { "type": "boolean" } }), &[]),
                call: |args| send(Method::PUT, "/v1/network".into(), args.clone()),
            },
            Tool {
                name: "runtime_status",
                description: "Whether a downloadable part is installed and what it would download: midi (the Audio to MIDI transcriber), restore (Apollo, Restore after compression), training_pack (the trainer and its models) or training_listen (the recognisers that hear a dataset).",
                schema: || object(json!({ "part": { "type": "string", "enum": ["midi", "restore", "training_pack", "training_listen"] } }), &["part"]),
                call: |args| get(match text(args, "part")?.as_str() {
                    "midi" => "/v1/midi/runtime".into(),
                    "restore" => "/v1/restore/runtime".into(),
                    "training_pack" => "/v1/training/pack/runtime".into(),
                    "training_listen" => "/v1/training/listen/runtime".into(),
                    other => return Err(format!("no part '{other}': midi, restore, training_pack or training_listen")),
                }),
            },
            Tool {
                name: "midi_set",
                description: "Give a library song the MIDI file at path, in place of the one Audio to MIDI made, as the MIDI editor saves it.",
                schema: || object(json!({ "song_id": { "type": "string" }, "path": { "type": "string", "description": "a .mid file on this computer" } }), &["song_id", "path"]),
                call: |args| {
                    use base64::Engine;
                    let path = text(args, "path")?;
                    let data = std::fs::read(&path).map_err(|error| format!("read {path}: {error}"))?;
                    send(Method::PUT, format!("/v1/library/songs/{}/midi", segment(&text(args, "song_id")?)), json!({ "data": base64::engine::general_purpose::STANDARD.encode(data) }))
                },
            },
            Tool {
                name: "system_gpus",
                description: "The NVIDIA cards of this computer and the one the studio computes on, as Settings - Local engine lists them.",
                schema: nothing,
                call: |_| get("/v1/system/gpus".into()),
            },
            Tool {
                name: "score_to_midi",
                description: "A score (ABC, as the create form holds it) written as a MIDI file: the answer carries the file as base64 in data.",
                schema: || object(json!({ "abc": { "type": "string" } }), &["abc"]),
                call: |args| post("/v1/score/midi".into(), json!({ "abc": text(args, "abc")? })),
            },
            Tool {
                name: "score_instrumental",
                description: "The score made instrumental, as the score card's Instrumental button does: the voice's melody moves to the instrument and Vocal keeps only its rests and chords; a score without Vocal notes comes back as it is (moved 0).",
                schema: || object(json!({ "abc": { "type": "string" } }), &["abc"]),
                call: |args| post("/v1/score/instrumental".into(), json!({ "abc": text(args, "abc")? })),
            },
            Tool {
                name: "score_vocal_octave",
                description: "The score with the voice moved by whole octaves (octaves -2..2), apart from the band, as the score card's octave buttons do; warns when its middle leaves the singable range.",
                schema: || object(json!({ "abc": { "type": "string" }, "octaves": { "type": "integer" } }), &["abc", "octaves"]),
                call: |args| post("/v1/score/vocal-octave".into(), args.clone()),
            },
            Tool {
                name: "score_from_midi",
                description: "A score read from the MIDI file at path, as the score card's MIDI import does: mode, which track is the voice and which the instrument, sections, grid and octave moves are optional.",
                schema: || object(json!({ "path": { "type": "string", "description": "a .mid file on this computer" }, "mode": { "type": "string" }, "vocal": { "type": "string" }, "instrument": { "type": "string" }, "sections": { "type": "boolean" }, "grid": { "type": "integer" }, "vocal_octaves": { "type": "integer" }, "instrument_octaves": { "type": "integer" } }), &["path"]),
                call: |args| {
                    use base64::Engine;
                    let path = text(args, "path")?;
                    let data = std::fs::read(&path).map_err(|error| format!("read {path}: {error}"))?;
                    let mut body = body_without(args, &["path"]);
                    body["data"] = Value::String(base64::engine::general_purpose::STANDARD.encode(data));
                    post("/v1/score/from-midi".into(), body)
                },
            },
            Tool {
                name: "score_mark",
                description: "The score with its sections marked from the style and lyrics, as the create form does before singing (cot: full, melody or off; keep leaves marks already there).",
                schema: || object(json!({ "abc": { "type": "string" }, "style": { "type": "string" }, "lyrics": { "type": "string" }, "cot": { "type": "string", "enum": ["full", "melody", "off"] }, "keep": { "type": "boolean" } }), &["abc", "style", "lyrics", "cot"]),
                call: |args| post("/v1/score/mark".into(), args.clone()),
            },
            Tool {
                name: "library_song_note_set",
                description: "Write a library song's note (empty clears it), as the note field of its details does; the note goes into its file as the comment.",
                schema: || object(json!({ "song_id": { "type": "string" }, "note": { "type": "string" } }), &["song_id", "note"]),
                call: |args| send(Method::PUT, format!("/v1/library/songs/{}/note", segment(&text(args, "song_id")?)), json!({ "note": args.get("note").and_then(Value::as_str).unwrap_or_default() })),
            },
            Tool {
                name: "journal_read",
                description: "The Activity page: what agents changed and what the studio said, newest first.",
                schema: nothing,
                call: |_| get("/v1/journal".into()),
            },
            Tool {
                name: "journal_clear",
                description: "Clear the Activity page, as its Clear button does.",
                schema: nothing,
                call: |_| send(Method::DELETE, "/v1/journal".into(), json!({})),
            },
            Tool {
                name: "journal_remove",
                description: "Remove one line of the Activity page by its id (from journal_read).",
                schema: || object(json!({ "id": { "type": "integer" } }), &["id"]),
                call: |args| send(Method::DELETE, format!("/v1/journal/{}", args.get("id").and_then(Value::as_i64).ok_or("'id' is required")?), json!({})),
            },
            Tool {
                name: "cover_look_get",
                description: "What a track without a cover of its own wears (a photo for its style or a pattern, the pattern's style) and whether that picture is saved into the track, as Settings - Cover art shows.",
                schema: nothing,
                call: |_| get("/v1/covers/look".into()),
            },
            Tool {
                name: "cover_look_set",
                description: "Choose what tracks without a cover of their own wear: photo true for a photo for the style, false for a pattern; pattern names the pattern style; unique draws each track's pattern from its own words (the shapes) and music style (the colours) instead of its id; keep saves the picture into the track's file.",
                schema: || object(json!({ "photo": { "type": "boolean" }, "pattern": { "type": "string" }, "unique": { "type": "boolean" }, "keep": { "type": "boolean" } }), &[]),
                call: |args| send(Method::PUT, "/v1/covers/look".into(), args.clone()),
            },
            Tool {
                name: "cover_scenes",
                description: "The photo scenes the studio would search for a style (genre, mood, instruments), as the cover picker suggests them.",
                schema: || object(json!({ "style": { "type": "string" } }), &["style"]),
                call: |args| get(format!("/v1/media/scenes?style={}", segment(&text(args, "style")?))),
            },
            Tool {
                name: "cover_photos_search",
                description: "Free CC0 photos for a cover found by words (q), from Wikimedia Commons; from pages through the results. song_cover_photo_set puts one on a song.",
                schema: || object(json!({ "q": { "type": "string" }, "from": { "type": "integer" } }), &["q"]),
                call: |args| get(format!("/v1/media/photos?q={}&from={}", segment(&text(args, "q")?), args.get("from").and_then(Value::as_u64).unwrap_or(0))),
            },
            Tool {
                name: "video_backgrounds_search",
                description: "Free video backgrounds for a clip found by words (q); from pages through the results. video_set puts one in the video editor.",
                schema: || object(json!({ "q": { "type": "string" }, "from": { "type": "integer" } }), &["q"]),
                call: |args| get(format!("/v1/media/videos?q={}&from={}", segment(&text(args, "q")?), args.get("from").and_then(Value::as_u64).unwrap_or(0))),
            },
            Tool {
                name: "song_cover_photo_set",
                description: "Put a photo from cover_photos_search on a library song as its cover (image: the photo's address, page: its page).",
                schema: || object(json!({ "song_id": { "type": "string" }, "image": { "type": "string" }, "page": { "type": "string" } }), &["song_id", "image"]),
                call: |args| post(format!("/v1/library/songs/{}/cover/photo", segment(&text(args, "song_id")?)), body_without(args, &["song_id"])),
            },
            Tool {
                name: "song_cover_pattern_set",
                description: "Put a pattern on a library song as its cover: pattern names the style (cover_look_get lists them), seed makes it this song's own.",
                schema: || object(json!({ "song_id": { "type": "string" }, "pattern": { "type": "string" }, "seed": { "type": "string" } }), &["song_id", "pattern", "seed"]),
                call: |args| post(format!("/v1/library/songs/{}/cover/pattern", segment(&text(args, "song_id")?)), body_without(args, &["song_id"])),
            },
            Tool {
                name: "playlist_get",
                description: "One playlist with its songs in their order.",
                schema: || id_only("playlist_id", "playlist id"),
                call: |args| get(format!("/v1/library/playlists/{}", segment(&text(args, "playlist_id")?))),
            },
            Tool {
                name: "song_job_dismiss",
                description: "Take an ended song job's card off the create page (a failed or cancelled one, as its Remove does); a running job is cancelled with song_job_cancel.",
                schema: || id_only("job_id", "song job id"),
                call: |args| send(Method::DELETE, format!("/v1/music/jobs/{}", segment(&text(args, "job_id")?)), json!({})),
            },
            Tool {
                name: "song_job_score_again",
                description: "Make a failed song again with its LoRA brought to 1.0 on the score half (two LoRA share it, 0.5 each), as the button of the error 'score_unsingable_strength' does; only this request changes, the sound half keeps its strengths. Returns the new job.",
                schema: || id_only("job_id", "the failed song job's id"),
                call: |args| send(Method::POST, format!("/v1/music/jobs/{}/score-strength", segment(&text(args, "job_id")?)), json!({})),
            },
            Tool {
                name: "song_job_again",
                description: "Make a failed song again with the same request, as the button of the error 'not_audio' does (the card computed something other than sound). Returns the new job.",
                schema: || id_only("job_id", "the failed song job's id"),
                call: |args| send(Method::POST, format!("/v1/music/jobs/{}/again", segment(&text(args, "job_id")?)), json!({})),
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
        Payload::File(path) => builder.body(Body::from_stream(streamed(vec![Part::File(path)]))),
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
            let list: Vec<Value> = tools().iter().map(|tool| json!({ "name": tool.name, "title": tool_title(tool.name), "description": tool.description, "inputSchema": (tool.schema)(), "annotations": annotations(tool.name) })).collect();
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
                    let before = match journal_facts(name, &args) {
                        Some((verb, kind)) => name_before_change(verb, kind, &args).await,
                        None => String::new(),
                    };
                    match call_route(call).await {
                        Ok((status, text)) if status.is_success() => {
                            announce_change(name);
                            journal_change(name, &args, &before, &text).await;
                            match serde_json::from_str::<Value>(&text) {
                                Ok(value) => tool_json(id, shape(name, &args, value)),
                                Err(_) => answer(text, false),
                            }
                        }
                        Ok((_, text)) => answer(text, true),
                        Err(problem) => answer(problem, true),
                    }
                }
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
const INSTRUCTIONS: &str = "You drive YuE2 Studio on this computer. Every tool runs the same code as a button of the studio, the user sees what you do in its window, and every change a tool makes - all but the window's own controls - is written into its Activity journal. Start with studio_status. Long work (songs, stems, karaoke, dataset preparation, training) is a job: start it, then studio_wait instead of polling. The graphics card runs one heavy job at a time; while a LoRA trains no song is made. Look ids up instead of guessing them: library_songs_list, training_status, dataset_get, lora_list, models_status. Before writing for the model yourself read writing_guide and writing_examples. When the user has made you the studio's writing assistant, answer its requests: assistant_requests_wait, then assistant_request_answer. The whole guide is the resource studio://skill (prompt 'studio').";

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn long_midi_transcriptions_round_trip_through_structured_and_text_results() {
        let notes: Vec<_> = (0..4179).map(|index| json!({"start": index, "end": index + 1, "pitch": 60, "instrument": "acoustic_piano"})).collect();
        let song = json!({"song_id": "recording", "notes": notes});
        let mut args = json!({"response_format": "detailed"});
        let mut collected = Vec::new();
        while collected.len() < notes.len() {
            let page = shape("midi_get", &args, song.clone());
            let bytes = axum::body::to_bytes(tool_json(json!(1), page).into_body(), usize::MAX).await.unwrap();
            let reply: Value = serde_json::from_slice(&bytes).unwrap();
            let data = &reply["result"]["structuredContent"];
            assert_eq!(serde_json::from_str::<Value>(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap(), *data);
            assert_eq!(data["offset"], collected.len());
            assert_eq!(data["notes_count"], notes.len());
            let batch = data["notes"].as_array().unwrap();
            assert!(!batch.is_empty());
            collected.extend_from_slice(batch);
            if collected.len() < notes.len() { assert_eq!(data["next_offset"], collected.len()); }
            else { assert!(data["next_offset"].is_null()); }
            args = json!({"offset": data["next_offset"]});
        }
        assert_eq!(collected, notes);
        let short = json!({"notes":[{"pitch": 72}]});
        assert_eq!(shape("midi_get", &json!({"response_format":"detailed"}), short.clone()), short);
        assert_eq!(shape("midi_get", &json!({"offset":u64::MAX}), short)["notes"], json!([]));
    }

    #[tokio::test]
    async fn a_command_to_a_window_that_closed_fails_at_once() {
        // a command is sent only while some window listens: without a receiver it would fail as "not open"
        let _commands = bridge().commands.subscribe();
        let window = bridge().sequence.fetch_add(1, Ordering::Relaxed);
        open_windows().push(window);
        let listening = Listening(window);
        let asked = tokio::spawn(ask_window("read_page", json!({}), 300));
        while !bridge().pending.lock().unwrap().values().any(|(to, _)| *to == window) {
            tokio::task::yield_now().await;
        }
        drop(listening);
        let answer = tokio::time::timeout(Duration::from_secs(5), asked).await.expect("an answer before the timeout").unwrap();
        assert!(answer.unwrap_err().contains("closed"));
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

    /// How long the bin keeps a song is the service's to say (`days` of library_trash_list): no tool writes it in.
    #[test]
    fn no_tool_writes_in_how_long_the_bin_keeps_a_song() {
        for tool in tools().iter().filter(|tool| tool.name.starts_with("library_")) {
            assert!(!tool.description.contains("30 days"), "{} writes in the bin's length", tool.name);
        }
        let list = tools().iter().find(|tool| tool.name == "library_trash_list").expect("the bin's list");
        assert!(list.description.contains("(days)"), "the list names the field that says it");
    }

    #[test]
    fn a_command_goes_to_the_window_the_person_turned_to() {
        let (older, newer) = (u64::MAX - 20, u64::MAX - 21);
        open_windows().extend([older, newer]);
        let place = |window: u64| open_windows().iter().position(|open| *open == window).unwrap();
        assert!(place(newer) > place(older), "the window opened last is asked first");
        assert!(focus_window(older));
        assert!(place(older) > place(newer), "the one the person turned to is asked now");
        assert!(!focus_window(u64::MAX - 22), "a window that is not open is not taken");
        open_windows().retain(|open| *open != older && *open != newer);
    }

    #[test]
    fn every_tool_is_in_the_skill() {
        let missing: Vec<&str> = tools().iter().map(|tool| tool.name).filter(|name| !SKILL.contains(&format!("`{name}`"))).collect();
        assert!(missing.is_empty(), "docs/mcp-skill.md does not name {missing:?}");
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
    fn catalog_filters_radio_tapok_before_paging() {
        let source: Value = serde_json::from_str(include_str!("../../../config/rvc-voice-catalog.json")).unwrap();
        let args = json!({"query": "Radio Tapok", "vocal_style": "high", "rvc_version": "v2",
            "sample_rate_hz": 40000, "has_index": true});
        let filtered = shape("rvc_catalog_list", &args, source["voices"].clone());
        assert_eq!(filtered["total"], 1);
        assert_eq!(filtered["next_offset"], Value::Null);
        assert_eq!(filtered["voices"][0]["id"], "rvc-50ea3d733073550ef6f7");
        let legacy = json!({"name":"Legacy", "metadata":null, "index":null});
        assert!(rvc_catalog_match(&legacy, &json!({})));
        assert!(rvc_catalog_match(&legacy, &json!({"has_index":false})));
        assert!(!rvc_catalog_match(&legacy, &json!({"vocal_style":"high"})));
    }

    #[test]
    fn curated_voice_pages_keep_total_and_unicode_names() {
        let list = json!((0..103).map(|id| json!({
            "id": id, "name": format!("Голос 音楽 {id}"),
        })).collect::<Vec<_>>());
        let page = shape("rvc_catalog_list", &json!({"offset": 80}), list.clone());
        assert_eq!(page["total"], 103);
        assert_eq!(page["next_offset"], 100);
        assert_eq!(page["voices"][0]["name"], "Голос 音楽 80");
        let last = shape("rvc_catalog_list", &json!({"offset": 100}), list);
        assert_eq!(last["voices"].as_array().unwrap().len(), 3);
        assert_eq!(last["next_offset"], Value::Null);
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
    fn midasheng_job_distinguishes_its_requested_budget_from_audio_duration() {
        let mut job = json!({
            "status": "running",
            "duration_seconds": 5.0,
            "generation_settings": { "family": "midashenglm_gen", "duration_seconds": 5.0 },
        });
        let running = compact_job(&job);
        assert_eq!(running["requested_duration_seconds"], 5.0);
        assert!(running.get("effective_duration_seconds").is_none());

        job["status"] = json!("completed");
        job["duration_seconds"] = json!(1.0);
        let completed = compact_job(&job);
        assert_eq!(completed["requested_duration_seconds"], 5.0);
        assert_eq!(completed["effective_duration_seconds"], 1.0);
        assert_eq!(completed["duration_seconds"], 1.0);
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
    fn a_tool_that_writes_over_what_was_stored_is_destructive() {
        for name in ["library_song_update", "playlist_update", "stems_split", "karaoke_make", "midi_transcribe", "dataset_prepare", "cover_set_from_file", "library_song_delete"] {
            assert_eq!(annotations(name)["destructiveHint"], true, "{name}");
        }
        for name in ["song_create", "playlist_create", "library_song_like", "library_version_select", "settings_set"] {
            assert_eq!(annotations(name)["destructiveHint"], false, "{name}");
        }
        assert_eq!(annotations("library_liked")["readOnlyHint"], true, "reading the liked songs changes nothing");
    }

    #[test]
    fn a_change_is_written_as_what_was_done_and_to_what() {
        for (tool, _, _) in JOURNAL {
            assert!(tools().iter().any(|entry| entry.name == *tool), "{tool} is a tool of the studio");
        }
        assert_eq!(journal_facts("playlist_delete", &json!({})), Some(("deleted", "playlist")));
        assert_eq!(journal_facts("song_create", &json!({})), Some(("started", "generation")), "the song is made later: the line says the job started");
        assert_eq!(journal_facts("library_song_like", &json!({ "liked": false })), Some(("unliked", "song")));
        assert_eq!(journal_facts("library_song_like", &json!({})), Some(("liked", "song")));
        assert_eq!(journal_facts("library_songs_list", &json!({})), None, "a read leaves no line");
        assert_eq!(journal_facts("ui_click", &json!({})), None, "nor does moving the window around");
        assert_eq!(change_target("song", &json!({ "title": " Night " }), "", "{}"), "Night");
        assert_eq!(change_target("song", &json!({ "song_id": "s1" }), "Old name", "{}"), "Old name", "a deletion keeps the name it had");
        assert_eq!(change_target("playlist", &json!({}), "", r#"{"name":"Rock"}"#), "Rock", "or the one the studio answered with");
        assert_eq!(change_target("song", &json!({ "path": "C:/music/take.wav" }), "", r#"{"song":{"title":"Take"},"audio_filename":"x.wav"}"#), "Take", "an imported song by its title");
        assert_eq!(change_target("dataset", &json!({ "dataset_id": "d", "song_id": "s", "title": "A song" }), "", r#"{"name":"Voices"}"#), "Voices", "a dataset by its own name, not its song's");
        assert_eq!(change_target("model", &json!({ "ids": ["yue-s1", "yue-s2"] }), "", "{}"), "yue-s1, yue-s2", "a model by what the call named");
        assert_eq!(change_target("processing", &json!({ "path": "C:/refs/master.wav" }), "", "{}"), "master.wav");
        assert_eq!(change_target("settings", &json!({ "name": "x" }), "", r#"{"title":"y"}"#), "", "settings name nothing");
        assert!(name_source("generation", &json!({ "playlist_id": "p" })).is_none(), "a new song is not named after the playlist it goes into");
        assert_eq!(name_source("dataset", &json!({ "dataset_id": "d", "song_id": "s" })), Some(NameSource { path: "/v1/training".into(), list: Some(("datasets", "d".into())), field: "name" }), "a dataset by its own name, not a library song's");
        assert_eq!(name_source("playlist", &json!({ "playlist_id": "p 1" })), Some(NameSource { path: "/v1/library/playlists/p%201".into(), list: None, field: "name" }));
        assert_eq!(name_source("stems", &json!({ "song_id": "s" })), Some(NameSource { path: "/v1/library/songs/s".into(), list: None, field: "title" }));
        assert_eq!(name_source("lora", &json!({ "lora_id": "l" })), Some(NameSource { path: "/v1/adapters".into(), list: Some(("installed", "l".into())), field: "name" }), "a LoRA deleted keeps its name");
        assert_eq!(name_source("training", &json!({ "run_id": "r" })), Some(NameSource { path: "/v1/training".into(), list: Some(("runs", "r".into())), field: "name" }));
    }

    #[test]
    fn every_change_an_agent_can_make_is_journaled() {
        // the window's own controls: the person watches them happen, and what
        // they make the studio reports as it happens
        const WINDOW: &[&str] = &["ui_", "player_", "video_", "visualizer_", "winamp_", "equalizer_"];
        const UNJOURNALED: &[&str] = &[
            "create_form_set",
            "create_form_submit",
            "dataset_reveal",
            "models_own_reveal",
            "studio_open_data_folder",
            "vst_open_editor",
            // answers that store nothing
            "openrouter_complete",
            "openrouter_cover",
            "openrouter_transcribe",
            "lyrics_recognise",
            "assistant_write",
            "assistant_sections",
            "assistant_request_answer",
            "library_song_describe_style",
            "recording_describe_style",
            // A temporary download is staged without changing the library.
            "song_export_zip",
            // the journal itself
            "journal_clear",
            "journal_remove",
            // the service's upkeep
            "engine_restart",
            "vst_scan",
            "openrouter_catalog_refresh",
            "assistant_start",
            "assistant_stop",
            "diagnostics_report",
            // a message to GitHub through the hub: the issue is its record
            "problem_report",
            // the update screen put off: nothing of the person's changes
            "models_update_later",
        ];
        let unwritten: Vec<&str> = tools()
            .iter()
            .map(|tool| tool.name)
            .filter(|name| annotations(name)["readOnlyHint"] != true && !WINDOW.iter().any(|prefix| name.starts_with(prefix)) && !UNJOURNALED.contains(name))
            .filter(|name| journal_facts(name, &json!({})).is_none())
            .collect();
        assert!(unwritten.is_empty(), "these change something: each goes into JOURNAL, or into this test's list with why it leaves no line: {unwritten:?}");
        for name in UNJOURNALED {
            assert!(tools().iter().any(|entry| entry.name == *name), "{name} is a tool of the studio");
        }
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

    // ---------------------------------------------------------------- the router, read from its source

    /// The studio's routes: (METHOD, path pattern), the hub's own router aside.
    fn router_routes() -> Vec<(String, String)> {
        let source = include_str!("lib.rs").replace("\r\n", "\n");
        let body = source.split("let app = Router::new()").nth(1).expect("the router").split(".merge(state.hub").next().unwrap();
        let code: String = body.lines().map(|line| line.split("//").next().unwrap_or_default()).collect::<Vec<_>>().join("\n");
        let mut routes = Vec::new();
        for chunk in code.split(".route(").skip(1) {
            let path = chunk.trim_start().strip_prefix('"').and_then(|rest| rest.split('"').next()).expect("a route's path").to_string();
            let handlers = chunk.split(".layer(").next().unwrap();
            for method in ["get", "post", "patch", "put", "delete"] {
                let call = format!("{method}(");
                let called = handlers.match_indices(&call).any(|(at, _)| !handlers[..at].chars().last().is_some_and(|c| c.is_alphanumeric() || c == '_'));
                if called {
                    routes.push((method.to_uppercase(), path.clone()));
                }
            }
        }
        routes
    }

    fn route_of(pattern: &str, path: &str) -> bool {
        let path = path.split('?').next().unwrap_or_default();
        let (wanted, given): (Vec<&str>, Vec<&str>) = (pattern.split('/').collect(), path.split('/').collect());
        wanted.len() == given.len() && wanted.iter().zip(&given).all(|(part, value)| part.starts_with('{') && !value.is_empty() || part == value)
    }

    /// Routes an agent is not given, and why.
    const NOT_TOOLS: &[(&str, &str, &str)] = &[
        ("GET", "/v1/rvc/preview/{id}/{side}", "binary audio from the before_url and after_url answered by rvc_preview"),
        ("GET", "/v1/rvc/catalog/sample/{id}", "binary verified demo audio addressed by sample_audio_url in rvc_catalog_list"),
        ("GET", "/health", "the service's identity for the desktop shell: initialize names the server and its version"),
        ("POST", "/engine/pick-executable", "a file dialog for the person: engine_options_set takes the executable's path"),
        ("POST", "/setup/kv-cache-notice/seen", "the window's note that it told the person: engine_options_get says the cache and why"),
        ("POST", "/v1/diagnostics/report/reveal", "shows a report in the person's file manager: diagnostics_report answers its path"),
        ("POST", "/v1/diagnostics/logs/reveal", "shows the log folder in the person's file manager: diagnostics_get carries the latest lines"),
        ("GET", "/v1/engine/progress", "the starting page's progress stream: studio_status and engine_logs"),
        ("GET", "/v1/proxy/image", "pictures from the internet relayed for the page"),
        ("POST", "/v1/files/save", "a save dialog for the person: the tools write to the path the agent names"),
        ("POST", "/v1/files/save/{id}", "a save dialog for the person: the tools write to the path the agent names"),
        ("POST", "/v1/files/reveal", "shows a file in the person's file manager"),
        ("POST", "/v1/storage/pick", "a folder dialog for the person: storage_set takes the folder's path"),
        ("GET", "/v1/exports/{id}/file", "the window's Save dialog reads an export from here: song_export writes into the folder the agent names and song_export_get answers the path"),
        ("GET", "/v1/library/songs/{id}/midi/parts/{part}", "the window's Save dialog reads a part's MIDI from here: library_song_midi_part_write writes it into the folder the agent names"),
        ("GET", "/v1/exports/live/{id}/file", "the window's Save dialog reads a Live project from here: song_export_live writes it into the folder the agent names"),
        ("GET", "/v1/exports/tracks/{id}/file", "binary ZIP download; song_export_zip prepares it and answers its URL"),
        ("DELETE", "/v1/exports/live/{id}", "the window lets go of a Live project it did not save: an agent's project is a file in its own folder"),
        ("DELETE", "/v1/exports/{id}", "the window lets go of an export it did not save: an agent's export is a file in its own folder"),
        ("GET", "/v1/skins", "the page's Winamp reads its skins here: winamp_skins asks the window for the same list"),
        ("POST", "/v1/skins", "the page's upload of a .wsz: winamp_skin_add adds one from a path"),
        ("GET", "/v1/skins/file/{name}", "a skin's file for the page's Winamp"),
        ("POST", "/v1/assistant/local-key", "a key the person types for their own server; agents are not given keys"),
        ("POST", "/v1/assistant/write/stream", "the page's streamed answer: assistant_write gives the whole one"),
        ("GET", "/editor", "the MIDI editor's page: the window shows it"),
        ("GET", "/editor/{*path}", "the MIDI editor's files: the window shows it"),
        ("GET", "/v1/processing/preview", "audio for the page's preview player: processing_get names the result"),
        ("GET", "/v1/training/datasets/{id}/items/{item}/audio", "audio for the page's player: dataset_song_files names the files"),
        ("GET", "/v1/midi/notes", "the page's live piano roll while Audio to MIDI runs: studio_wait follows the job"),
        ("GET", "/v1/library/songs/{id}/midi/file", "the .mid as a download for the page: midi_get gives the song's MIDI"),
        ("GET", "/v1/adapters/{id}/comfyui", "the page's download of the file lora_export_comfyui writes to a path"),
        ("GET", "/v1/library/songs/{id}/stems/{stem}", "a stem's audio for the page's player: stems_get names the files"),
        ("POST", "/v1/library/songs", "a bare record without audio, kept for the old page: library_import_audio brings songs"),
        ("POST", "/v1/journal", "the window's own messages; the lines of an agent's changes are written by the MCP server itself"),
        ("GET", "/v1/library/media/{song_id}", "a song's audio for the page's player: library_song_files names the file"),
        ("GET", "/v1/library/songs/{id}/cover", "a cover picture for the page: library_song_files names the file"),
        ("GET", "/v1/library/songs/{id}/cover/placeholder", "the picture the page shows while a song has no cover"),
        ("GET", "/v1/transcriptions/{job_id}", "the same score job score_job_get reads"),
        ("POST", "/v1/transcriptions/{job_id}", "the same cancel as score_job_cancel"),
        ("GET", "/v1/music/jobs/ended", "the page's cards after a reload: song_jobs_list and song_job_get"),
    ];

    /// What the composite tools read.
    const COMPOSITE_ROUTES: &[(&str, &[(&str, &str)])] = &[
        ("composite:status", &[("GET", "/v1/music/jobs"), ("GET", "/v1/activity"), ("GET", "/v1/training"), ("GET", "/v1/processing"), ("GET", "/v1/separation/status"), ("GET", "/v1/midi"), ("GET", "/v1/restore")]),
        ("composite:wait", &[("GET", "/v1/music/jobs/x1"), ("GET", "/v1/scores/x1"), ("GET", "/v1/music/jobs"), ("GET", "/v1/activity"), ("GET", "/v1/training"), ("GET", "/v1/processing"), ("GET", "/v1/separation/status"), ("GET", "/v1/midi"), ("GET", "/v1/restore")]),
        ("composite:questions", &[]),
        ("composite:answer", &[]),
        ("composite:equalizer_export", &[]),
        ("composite:winamp_skin_add", &[]),
    ];

    /// Arguments for every property of a tool, and one set more for each other choice of an enum.
    fn samples(tool: &Tool, file: &std::path::Path, image: &std::path::Path) -> Vec<Value> {
        let schema = (tool.schema)();
        let properties = schema["properties"].as_object().cloned().unwrap_or_default();
        let sample = |name: &str, property: &Value| -> Value {
            if let Some(first) = property["enum"].as_array().and_then(|choices| choices.first()) {
                return first.clone();
            }
            let kind = match &property["type"] {
                Value::Array(kinds) => kinds[0].as_str().unwrap_or("string").to_string(),
                other => other.as_str().unwrap_or("string").to_string(),
            };
            match (kind.as_str(), name) {
                ("integer", _) => json!(1),
                ("number", _) => json!(1.5),
                ("boolean", _) => json!(true),
                ("array", _) if property["items"]["type"] == "integer" => json!([1]),
                ("array", _) => json!(["x1"]),
                ("object", _) => json!({}),
                (_, name) if tool.name.contains("folder") && name.contains("path") => json!(file.parent().unwrap_or(file).to_string_lossy()),
                (_, "video_path") => json!(file.with_file_name("clip.mp4").to_string_lossy()),
                (_, name) if (name.contains("art") || name.contains("image") || name.contains("cover")) && name.contains("path") => json!(image.to_string_lossy()),
                (_, name) if name.contains("path") || name == "file" || name == "folder" => json!(file.to_string_lossy()),
                _ => json!("x1"),
            }
        };
        let base: serde_json::Map<String, Value> = properties.iter().map(|(name, property)| (name.clone(), sample(name, property))).collect();
        let mut all = vec![Value::Object(base.clone())];
        if matches!(tool.name, "training_pack_install" | "training_pack_cancel") {
            // Their optional arguments select the separate training-base routes.
            all.push(json!({}));
        }
        for (name, property) in &properties {
            for choice in property["enum"].as_array().into_iter().flatten().skip(1) {
                let mut other = base.clone();
                other.insert(name.clone(), choice.clone());
                all.push(Value::Object(other));
            }
        }
        all
    }

    /// The routes a tool reaches: (METHOD, path); the window's commands reach none.
    fn reached_by(tool: &Tool, file: &std::path::Path, image: &std::path::Path, refused: &mut Vec<String>) -> Vec<(String, String)> {
        let mut reached = Vec::new();
        for args in samples(tool, file, image) {
            let call = match (tool.call)(&args) {
                Ok(call) => call,
                Err(problem) => {
                    refused.push(format!("{} refused {args}: {problem}", tool.name));
                    continue;
                }
            };
            if matches!(call.payload, Payload::Window { .. }) {
                continue;
            }
            match COMPOSITE_ROUTES.iter().find(|(path, _)| *path == call.path) {
                Some((_, reads)) => reached.extend(reads.iter().map(|(method, path)| (method.to_string(), path.to_string()))),
                None => {
                    assert!(!call.path.starts_with("composite:"), "{} is a composite this test does not know", tool.name);
                    reached.push((call.method.to_string(), call.path.clone()));
                }
            }
        }
        reached
    }

    #[test]
    fn every_route_is_a_tool() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("song.wav");
        std::fs::write(&file, b"x").unwrap();
        let image = folder.path().join("cover.png");
        std::fs::write(&image, b"x").unwrap();
        let mut refused = Vec::new();
        let routes = router_routes();
        assert!(routes.len() > 100, "the router read: {routes:?}");
        let mut covered = vec![false; routes.len()];
        for tool in tools() {
            for (method, path) in reached_by(tool, &file, &image, &mut refused) {
                let found: Vec<usize> = routes.iter().enumerate().filter(|(_, (m, p))| *m == method && route_of(p, &path)).map(|(at, _)| at).collect();
                assert!(!found.is_empty(), "{} calls {method} {path}, a route the studio does not have", tool.name);
                for at in found {
                    covered[at] = true;
                }
            }
        }
        assert!(refused.is_empty(), "tools that refused the test's arguments, so their routes are not seen: {refused:#?}");
        let by_tool = covered.clone();
        let mut stale = Vec::new();
        for (method, path) in NOT_TOOLS.iter().map(|(method, path, _)| (method, path)) {
            let at = routes.iter().position(|(m, p)| m == method && p == path).unwrap_or_else(|| panic!("{method} {path} of NOT_TOOLS is not a route of the studio"));
            if by_tool[at] {
                stale.push(format!("{method} {path}"));
            }
            covered[at] = true;
        }
        assert!(stale.is_empty(), "these routes of NOT_TOOLS are reached by a tool: drop them from NOT_TOOLS: {stale:?}");
        let missing: Vec<String> = routes.iter().zip(&covered).filter(|(_, done)| !**done).map(|((method, path), _)| format!("{method} {path}")).collect();
        assert!(missing.is_empty(), "these routes have no tool: give each one, or put it into NOT_TOOLS with why: {missing:?}");
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
