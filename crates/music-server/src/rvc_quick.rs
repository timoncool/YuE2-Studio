use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    AppState, MusicJobStatus, api_error, card_free_of_training, htdemucs_installed, refuse_trashed,
    roformer_on_card,
    rvc_apply::{self, ApplyRequest, Backend},
    rvc_voices, separate_song_audio, separation, stem_path,
};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct Options {
    pub(super) voice_id: String,
    #[serde(default)]
    pub(super) backend: Backend,
}

#[derive(Deserialize)]
pub(super) struct Request {
    song_id: String,
    voice_id: Option<String>,
    backend: Option<Backend>,
}

fn selection_path(state: &AppState) -> std::path::PathBuf {
    crate::update_data_root(state).join("rvc/selection.json")
}

fn selection(state: &AppState) -> Option<Options> {
    serde_json::from_slice(&std::fs::read(selection_path(state)).ok()?).ok()
}

pub(super) async fn select(
    State(state): State<AppState>,
    Json(options): Json<Options>,
) -> Result<Json<Options>, Failure> {
    check_voice(&state, &options.voice_id)?;
    let path = selection_path(&state);
    let parent = path.parent().expect("RVC settings are in the rvc folder");
    std::fs::create_dir_all(parent).map_err(rvc_voices::internal)?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(rvc_voices::internal)?;
    serde_json::to_writer(file.as_file_mut(), &options).map_err(rvc_voices::internal)?;
    file.persist(path).map_err(rvc_voices::internal)?;
    Ok(Json(options))
}

pub(super) async fn selected(State(state): State<AppState>) -> Json<Option<Options>> {
    Json(selection(&state).filter(|options| check_voice(&state, &options.voice_id).is_ok()))
}

fn check_voice(state: &AppState, voice_id: &str) -> Result<(), Failure> {
    if voice_id.len() != 64 || !voice_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(rvc_voices::invalid("Use an installed RVC voice ID."));
    }
    let voice_root = crate::update_data_root(state).join("rvc/voices");
    if rvc_voices::read_voice(&voice_root, voice_id).is_none() {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            "The installed RVC voice was not found.".into(),
        ));
    }
    Ok(())
}

#[derive(Default)]
pub(super) struct Queue {
    jobs: Mutex<HashMap<String, Arc<Run>>>,
    convert: tokio::sync::Mutex<()>,
    pub(super) separation: Arc<tokio::sync::Mutex<()>>,
}

pub(super) struct Run(Mutex<Progress>);

#[derive(Debug, Clone, Serialize)]
pub(super) struct Progress {
    id: String,
    song_id: String,
    voice_id: String,
    backend: Backend,
    status: Stage,
    take: Option<Value>,
    error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Stage {
    Queued,
    Separating,
    Converting,
    Completed,
    Cancelled,
    Failed,
}

impl Stage {
    fn active(self) -> bool {
        matches!(self, Self::Queued | Self::Separating | Self::Converting)
    }
}

impl Progress {
    fn cancel(&mut self) {
        if self.status.active() {
            self.status = Stage::Cancelled;
        }
    }

    pub(super) fn complete(&mut self, take: Value) {
        self.take = Some(take);
        self.status = Stage::Completed;
    }
}

impl Run {
    pub(super) fn commit(&self) -> anyhow::Result<MutexGuard<'_, Progress>> {
        let progress = self.0.lock().unwrap();
        anyhow::ensure!(progress.status.active(), "RVC conversion cancelled.");
        Ok(progress)
    }

    pub(super) fn cancelled(&self) -> bool {
        self.0.lock().unwrap().status == Stage::Cancelled
    }
}

type Failure = rvc_voices::ApiFailure;

pub(super) async fn start(
    State(state): State<AppState>,
    Json(request): Json<Request>,
) -> Result<(StatusCode, Json<Progress>), Failure> {
    let stored = selection(&state);
    let voice_id = request
        .voice_id
        .or_else(|| stored.as_ref().map(|options| options.voice_id.clone()))
        .ok_or_else(|| rvc_voices::invalid("Select an installed RVC voice first."))?;
    let backend = request
        .backend
        .or_else(|| stored.map(|options| options.backend))
        .unwrap_or_default();
    enqueue(state, request.song_id, Options { voice_id, backend })
        .map(|job| (StatusCode::ACCEPTED, Json(job)))
}

fn enqueue(state: AppState, song_id: String, options: Options) -> Result<Progress, Failure> {
    let song = state
        .library
        .get_song(&song_id)
        .map_err(rvc_voices::internal)?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found.".into()))?;
    refuse_trashed(&song)?;
    if song.metadata["derived"]["tool"] == "stems" {
        return Err(rvc_voices::invalid(
            "Choose the original song rather than a stem.",
        ));
    }
    let Options { voice_id, backend } = options;
    check_voice(&state, &voice_id)?;
    let mut jobs = state.rvc_quick.jobs.lock().unwrap();
    if let Some(running) = jobs.values().find_map(|run| {
        let progress = run.0.lock().unwrap();
        (progress.status.active()
            && progress.song_id == song.id
            && progress.voice_id == voice_id
            && progress.backend == backend)
            .then(|| progress.clone())
    }) {
        return Ok(running);
    }
    let mut progress = Progress {
        id: uuid::Uuid::now_v7().to_string(),
        song_id: song.id,
        voice_id,
        backend,
        status: Stage::Queued,
        take: None,
        error: None,
    };
    if let Some(take) = saved_take(&state.library, &progress).map_err(rvc_voices::internal)? {
        if let Some(saved) = jobs.values().find_map(|run| {
            let job = run.0.lock().unwrap();
            (job.take.as_ref() == Some(&take)).then(|| job.clone())
        }) {
            return Ok(saved);
        }
        progress.complete(take);
    }
    let run = Arc::new(Run(Mutex::new(progress.clone())));
    jobs.insert(progress.id.clone(), run.clone());
    drop(jobs);
    if progress.status.active() {
        tokio::spawn(process(state, run));
    }
    Ok(progress)
}

fn saved_take(
    library: &crate::library::Library,
    progress: &Progress,
) -> anyhow::Result<Option<Value>> {
    let requested = serde_json::to_value(progress.backend)?;
    Ok(library.list_songs()?.into_iter().find_map(|song| {
        let details = &song.metadata["derived"];
        let settings = &details["settings"];
        (details["tool"] == "rvc"
            && details["from"] == progress.song_id
            && settings["quick"] == true
            && settings["voice_id"] == progress.voice_id
            && settings["requested_backend"] == requested
            && library
                .media_path_for_song(&song)
                .is_some_and(|path| path.is_file()))
        .then(|| {
            json!({
                "song_id": song.id, "audio_url": format!("/v1/library/media/{}", song.id),
                "requested_backend": requested, "effective_backend": settings["effective_backend"],
                "fallback_reason": settings["fallback_reason"],
            })
        })
    }))
}

fn read(state: &AppState, id: &str) -> Result<Arc<Run>, Failure> {
    state
        .rvc_quick
        .jobs
        .lock()
        .unwrap()
        .get(id)
        .cloned()
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "RVC job not found.".into()))
}

pub(super) async fn list(State(state): State<AppState>) -> Json<Vec<Progress>> {
    let jobs = state.rvc_quick.jobs.lock().unwrap();
    let mut entries: Vec<_> = jobs
        .values()
        .map(|run| run.0.lock().unwrap().clone())
        .collect();
    entries.sort_by(|a, b| b.id.cmp(&a.id));
    Json(entries)
}

pub(super) async fn status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Progress>, Failure> {
    Ok(Json(read(&state, &id)?.0.lock().unwrap().clone()))
}

pub(super) async fn cancel(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Progress>, Failure> {
    let run = read(&state, &id)?;
    let mut progress = run.0.lock().unwrap();
    progress.cancel();
    Ok(Json(progress.clone()))
}

async fn process(state: AppState, run: Arc<Run>) {
    let _guard = state.rvc_quick.convert.lock().await;
    if run.cancelled() {
        return;
    }
    if let Err((_, Json(error))) = process_take(&state, &run).await {
        let mut progress = run.0.lock().unwrap();
        if progress.status.active() {
            progress.status = Stage::Failed;
            progress.error = Some(error.error);
        }
    }
}

async fn process_take(state: &AppState, run: &Arc<Run>) -> Result<(), Failure> {
    let progress = run.0.lock().unwrap().clone();
    card_free_of_training(state, "convert vocals").await?;
    prepare_stems(state, run.clone(), &progress.song_id).await?;
    run.commit().map_err(rvc_voices::internal)?.status = Stage::Converting;
    rvc_apply::apply_with(
        state.clone(),
        ApplyRequest {
            song_id: progress.song_id,
            voice_id: progress.voice_id,
            backend: progress.backend,
            semitone_shift: 0,
            retrieval_blend: 0.0,
        },
        Some(run.clone()),
    )
    .await?;
    Ok(())
}

async fn prepare_stems(state: &AppState, run: Arc<Run>, id: &str) -> Result<(), Failure> {
    let _guard = state.rvc_quick.separation.lock().await;
    let has_backing = stem_path(state, id, "instrumental").is_file()
        || separation::STEMS
            .iter()
            .filter(|name| **name != "vocals")
            .all(|name| stem_path(state, id, name).is_file());
    if stem_path(state, id, "vocals").is_file() && has_backing {
        return Ok(());
    }
    run.commit().map_err(rvc_voices::internal)?.status = Stage::Separating;
    let song = state
        .library
        .get_song(id)
        .map_err(rvc_voices::internal)?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found.".into()))?;
    refuse_trashed(&song)?;
    let source = state
        .library
        .media_path_for_song(&song)
        .ok_or_else(|| rvc_voices::invalid("The original audio is unavailable."))?;
    let config = state.separation_config.read().await.clone();
    let runtime = config.runtime;
    let engine = config.engine;
    let gpu = roformer_on_card(state, runtime);
    let mut wanted: Vec<String> = if engine == separation::Engine::Roformer {
        if !state.separator.roformer_installed(gpu) {
            return Err(rvc_voices::invalid(
                "Install Mel-Band RoFormer in Settings → Models → Stems.",
            ));
        }
        ["vocals", "instrumental"]
            .into_iter()
            .map(String::from)
            .collect()
    } else {
        if !htdemucs_installed(state, runtime) {
            return Err(rvc_voices::invalid(
                "Install HT-Demucs in Settings → Models → Stems.",
            ));
        }
        separation::STEMS
            .iter()
            .map(|name| (*name).to_owned())
            .collect()
    };
    wanted.retain(|name| !stem_path(state, id, name).is_file());
    let state = state.clone();
    let id = id.to_owned();
    tokio::task::spawn_blocking(move || {
        separate_song_audio(&state, &id, &source, engine, runtime, gpu, &wanted, &|| {
            run.cancelled()
        })
    })
    .await
    .map_err(rvc_voices::internal)?
    .map_err(rvc_voices::internal)?;
    Ok(())
}

pub(super) fn after_generation(state: AppState, id: String, options: Options) {
    tokio::spawn(async move {
        loop {
            let job = state.jobs.read().await.get(&id).cloned();
            let Some(job) = job else { return };
            match job.status {
                MusicJobStatus::Completed => {
                    let songs = if job.songs.is_empty() {
                        job.song.into_iter().collect()
                    } else {
                        job.songs
                    };
                    for song in songs {
                        if let Err((_, Json(error))) =
                            enqueue(state.clone(), song.id, options.clone())
                        {
                            eprintln!("Could not queue RVC for generation {id}: {}", error.error);
                        }
                    }
                    return;
                }
                MusicJobStatus::Cancelled | MusicJobStatus::Failed => return,
                _ => tokio::time::sleep(Duration::from_millis(250)).await,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{AudioImportInput, Library};

    fn progress() -> Progress {
        Progress {
            id: "job".into(),
            song_id: "original".into(),
            voice_id: "voice".into(),
            backend: Backend::Auto,
            status: Stage::Converting,
            take: None,
            error: None,
        }
    }

    #[test]
    fn cancellation_and_save_have_one_commit_boundary() {
        let stopped = Run(Mutex::new(progress()));
        stopped.0.lock().unwrap().cancel();
        assert!(stopped.commit().is_err());

        let saved = Run(Mutex::new(progress()));
        saved.commit().unwrap().complete(json!({"song_id":"take"}));
        let mut saved = saved.0.lock().unwrap();
        saved.cancel();
        assert_eq!(saved.status, Stage::Completed);
        assert_eq!(saved.take.as_ref().unwrap()["song_id"], "take");
    }

    #[test]
    fn reuses_only_the_saved_quick_take_for_the_same_source_voice_and_backend() {
        let scratch = tempfile::tempdir().unwrap();
        let library = Library::open_at(
            scratch.path().join("library.sqlite"),
            scratch.path().join("media"),
        )
        .unwrap();
        let wav = crate::audio_pcm::wav_f32_bytes(&crate::audio_post::Stereo::new(
            vec![0.25; 100],
            vec![0.5; 100],
            44_100,
        ))
        .unwrap();
        let import = |metadata| {
            library
                .import_audio_song(AudioImportInput {
                    title: "Fixture".into(),
                    caption: String::new(),
                    lyrics: String::new(),
                    metadata,
                    generation_settings: Value::Null,
                    engine_id: "rvc".into(),
                    profile_id: None,
                    source: "rvc".into(),
                    audio_extension: "wav".into(),
                    audio: wav.clone(),
                })
                .unwrap()
                .song
        };
        let source = import(Value::Null);
        let mut same = progress();
        same.song_id = source.id.clone();
        let take = import(
            json!({"derived": {"from": source.id, "tool": "rvc", "settings": {
                "voice_id": same.voice_id, "quick": true, "requested_backend": "auto", "effective_backend": "audio.cpp"
            }}}),
        );
        assert_eq!(
            saved_take(&library, &same).unwrap().unwrap()["song_id"],
            take.id
        );
        let different_voice = Progress {
            voice_id: "different".into(),
            ..same.clone()
        };
        assert!(saved_take(&library, &different_voice).unwrap().is_none());
        let different_backend = Progress {
            backend: Backend::VocalFastRs,
            ..same
        };
        assert!(saved_take(&library, &different_backend).unwrap().is_none());
        assert_eq!(library.list_songs().unwrap().len(), 2);
        assert_eq!(
            std::fs::read(library.media_path_for_song(&source).unwrap()).unwrap(),
            wav
        );
    }
}
