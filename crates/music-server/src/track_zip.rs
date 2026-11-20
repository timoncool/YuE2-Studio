use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result};
use axum::extract::{Path as UrlPath, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::{Json, response::Response};
use serde::Deserialize;
use serde_json::{Value, json};
use tempfile::NamedTempFile;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use crate::{
    ApiError, AppState, api_error,
    library::{Library, Song},
};

static ARCHIVES: OnceLock<Mutex<HashMap<String, Arc<NamedTempFile>>>> = OnceLock::new();

fn archives() -> &'static Mutex<HashMap<String, Arc<NamedTempFile>>> {
    ARCHIVES.get_or_init(Default::default)
}

#[derive(Deserialize)]
pub struct Selection {
    song_ids: Vec<String>,
}

pub async fn start(
    State(state): State<AppState>,
    Json(selection): Json<Selection>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    if selection.song_ids.is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "Choose at least one song to export.".into(),
        ));
    }
    let mut seen = HashSet::new();
    let songs = selection
        .song_ids
        .into_iter()
        .filter(move |id| seen.insert(id.clone()));
    let archive = tokio::task::spawn_blocking(move || build(&state.library, songs))
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(|error| api_error(StatusCode::UNPROCESSABLE_ENTITY, format!("{error:#}")))?;
    let id = uuid::Uuid::now_v7().to_string();
    archives()
        .lock()
        .expect("track ZIPs")
        .insert(id.clone(), Arc::new(archive));
    let expires = id.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60 * 60)).await;
        archives().lock().expect("track ZIPs").remove(&expires);
    });
    Ok(Json(
        json!({"url": format!("/v1/exports/tracks/{id}/file"), "file_name": "tracks.zip"}),
    ))
}

pub async fn download(
    UrlPath(id): UrlPath<String>,
    request: Request,
) -> Result<Response, (StatusCode, Json<ApiError>)> {
    let archive = archives()
        .lock()
        .expect("track ZIPs")
        .get(&id)
        .cloned()
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Track export has expired.".into()))?;
    let mut response = crate::serve_audio_file(archive.path(), request).await;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"tracks.zip\""),
    );
    Ok(response)
}

fn build(library: &Library, ids: impl IntoIterator<Item = String>) -> Result<NamedTempFile> {
    let mut archive = NamedTempFile::new()?;
    {
        let mut zip = ZipWriter::new(BufWriter::new(archive.as_file_mut()));
        for (index, id) in ids.into_iter().enumerate() {
            let song = library
                .get_song(&id)
                .with_context(|| format!("Read song {id}"))?
                .with_context(|| format!("Song {id} is no longer in the library"))?;
            add_song(&mut zip, library, &song, index + 1)
                .with_context(|| format!("Could not export \"{}\" ({id})", song.title))?;
        }
        zip.finish()?.flush()?;
    }
    Ok(archive)
}

fn add_song(
    zip: &mut ZipWriter<BufWriter<&mut File>>,
    library: &Library,
    song: &Song,
    index: usize,
) -> Result<()> {
    let title: String = song
        .title
        .trim()
        .chars()
        .take(60)
        .map(|ch| {
            if ch.is_control() || "\\/:*?\"<>|".contains(ch) {
                '_'
            } else {
                ch
            }
        })
        .collect();
    let title = title.trim_end_matches([' ', '.']);
    let folder = format!(
        "{index:03} - {}",
        if title.is_empty() { "Untitled" } else { title }
    );
    let original = library
        .media_path_for_song(song)
        .context("original audio is missing or cannot be read")?;
    let ext = original
        .extension()
        .and_then(|ext| ext.to_str())
        .filter(|ext| !ext.is_empty() && ext.chars().all(|ch| ch.is_ascii_alphanumeric()));
    let audio = ext.map_or_else(|| "audio".into(), |ext| format!("audio.{ext}"));
    add_file(zip, &folder, &audio, &original).context("read original audio")?;

    let mut midi_names = Vec::new();
    let sidecar = format!("{}.mid", song.id);
    if let Some(path) = library
        .media_file(&sidecar)
        .and_then(|_| library.resolve_media(&sidecar))
    {
        add_file(zip, &folder, "transcription.mid", &path).context("read saved MIDI")?;
        midi_names.push("transcription.mid");
    }
    if let Some(score) =
        crate::song_midi::song_score(&song.generation_settings, song.replay_request.as_ref())
    {
        add_text(zip, &folder, "score.abc", score.as_bytes())?;
        if let Ok(midi) = crate::song_midi::part_midi(&score, "score") {
            add_text(zip, &folder, "score.mid", &midi)?;
            midi_names.push("score.mid");
        }
    }

    let settings = serde_json::to_string_pretty(&json!({
        "song_id": song.id, "title": song.title, "engine": song.engine_id, "profile": song.profile_id,
        "generation_settings": song.generation_settings, "midi": midi_names,
    }))?;
    let notes = format!(
        "{}\n\nPrompt\n{}\n\nLyrics\n{}\n\nSettings\n{settings}\n",
        song.title, song.caption, song.lyrics
    );
    add_text(zip, &folder, "settings.txt", notes.as_bytes())
}

fn add_file(
    zip: &mut ZipWriter<BufWriter<&mut File>>,
    folder: &str,
    name: &str,
    path: &Path,
) -> Result<()> {
    let mut file = File::open(path)?;
    zip.start_file(
        format!("{folder}/{name}"),
        SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .large_file(true),
    )?;
    io::copy(&mut file, zip)?;
    Ok(())
}

fn add_text(
    zip: &mut ZipWriter<BufWriter<&mut File>>,
    folder: &str,
    name: &str,
    bytes: &[u8],
) -> Result<()> {
    zip.start_file(
        format!("{folder}/{name}"),
        SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
    )?;
    zip.write_all(bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn same_title_keeps_each_original_and_available_sidecar() {
        let root = tempfile::tempdir().unwrap();
        let media = root.path().join("media");
        std::fs::create_dir(&media).unwrap();
        let library = Library::open_at(root.path().join("library.sqlite"), media.clone()).unwrap();
        let sheet = "X:1\nT:Song\nM:4/4\nL:1/4\nK:C\nC D E F|]";
        let make_song = |file: &str, score: Value| {
            library.create_song(serde_json::from_value(json!({
            "title": "../Same Name", "audio_path": file, "caption": "quiet folk", "lyrics": "the words",
            "engine_id": "minimaxmusic-cpp",
            "generation_settings": {"abc": score, "seed": 12345},
        })).unwrap()).unwrap()
        };
        let one = make_song("one.flac", json!(sheet));
        let two = make_song("two.mp3", Value::Null);
        std::fs::write(media.join("one.flac"), b"first original").unwrap();
        std::fs::write(media.join("two.mp3"), b"second original").unwrap();
        std::fs::write(media.join(format!("{}.mid", one.id)), b"saved midi").unwrap();

        let archive = build(&library, [one.id.clone(), two.id.clone()]).unwrap();
        let mut zip = zip::ZipArchive::new(File::open(archive.path()).unwrap()).unwrap();
        let mut bytes = |name: &str| {
            let mut contents = Vec::new();
            zip.by_name(name)
                .unwrap()
                .read_to_end(&mut contents)
                .unwrap();
            contents
        };
        assert_eq!(bytes("001 - .._Same Name/audio.flac"), b"first original");
        assert_eq!(bytes("002 - .._Same Name/audio.mp3"), b"second original");
        assert_eq!(bytes("001 - .._Same Name/transcription.mid"), b"saved midi");
        assert!(
            String::from_utf8(bytes("001 - .._Same Name/score.abc"))
                .unwrap()
                .split_whitespace()
                .collect::<String>()
                .contains("CDEF")
        );
        let notes = String::from_utf8(bytes("001 - .._Same Name/settings.txt")).unwrap();
        for text in ["quiet folk", "the words", "12345", &one.id] {
            assert!(notes.contains(text));
        }
        assert_eq!(
            std::fs::read(media.join("one.flac")).unwrap(),
            b"first original"
        );

        std::fs::remove_file(media.join("two.mp3")).unwrap();
        let error = build(&library, [one.id, two.id.clone()]).unwrap_err();
        let error = format!("{error:#}");
        assert!(
            error.contains(&two.title)
                && error.contains(&two.id)
                && error.contains("original audio")
        );
    }
}
