use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use axum::{http::StatusCode, Json};
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{api_error, ApiError};

#[derive(Deserialize)]
pub(super) struct Source {
    path: PathBuf,
}

pub(super) async fn preview(
    Json(source): Json<Source>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    tokio::task::spawn_blocking(move || inspect(&source.path))
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map(Json)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, format!("{error:#}")))
}

fn inspect(folder: &Path) -> Result<Value> {
    anyhow::ensure!(folder.is_absolute(), "Choose an absolute Studio folder");
    let root = [folder.to_owned(), folder.join("data")]
        .into_iter()
        .find(|path| path.join("library.sqlite").is_file())
        .context("No library.sqlite in the selected folder or its data folder")?;
    let db = Connection::open_with_flags(
        root.join("library.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    db.execute_batch("PRAGMA trusted_schema=OFF; BEGIN")?;
    let media = root.join("media");
    let mut files = HashSet::new();
    match std::fs::read_dir(media) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    files.insert(entry.file_name().to_string_lossy().into_owned());
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let mut stmt = db.prepare(
        "SELECT id,title,engine_id,audio_path,metadata_json FROM songs ORDER BY created_at DESC,id",
    )?;
    let mut rows = stmt.query([])?;
    let mut songs = Vec::new();
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let metadata: Value = serde_json::from_str(&row.get::<_, String>(4)?)
            .with_context(|| format!("Invalid metadata for song {id}"))?;
        let mut available = Vec::new();
        let mut missing = Vec::new();
        let mut check = |kind: &str, name: &str, required: bool| {
            if files.contains(name) {
                available.push(json!({"kind": kind, "name": name}));
            } else if required {
                missing.push(json!({"kind": kind, "name": name}));
            }
        };
        if let Some(path) = row.get::<_, Option<String>>(3)? {
            check("audio", path.rsplit(['/', '\\']).next().unwrap_or(""), true);
        }
        if let Some(name) = metadata["cover_filename"].as_str() {
            check("cover", name, true);
        }
        for (kind, suffix) in [
            ("latent", "latent"),
            ("midi", "mid"),
            ("midi_metadata", "mid.json"),
        ] {
            check(kind, &format!("{id}.{suffix}"), false);
        }
        for stem in crate::separation::STEMS
            .iter()
            .chain(["instrumental"].iter())
        {
            check("stem", &format!("{id}-{stem}.wav"), false);
        }
        songs.push(json!({"id": id, "title": row.get::<_, String>(1)?, "engine_id": row.get::<_, String>(2)?,
            "trashed": metadata.get("deleted_at").is_some(), "files": available, "missing": missing}));
    }
    let mut stmt = db.prepare("SELECT id,name FROM playlists ORDER BY created_at DESC,id")?;
    let mut memberships =
        db.prepare("SELECT song_id FROM playlist_songs WHERE playlist_id=? ORDER BY position")?;
    let mut playlists = Vec::new();
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let song_ids = memberships
            .query_map([&id], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        playlists.push(json!({"id": id, "name": row.get::<_, String>(1)?, "song_ids": song_ids}));
    }
    Ok(json!({"songs": songs, "playlists": playlists}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_portable_media_and_order_without_touching_source() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        let media = root.join("media");
        std::fs::create_dir_all(&media).unwrap();
        let path = root.join("library.sqlite");
        let db = Connection::open(&path).unwrap();
        db.execute_batch(r#"CREATE TABLE songs(id,title,engine_id,audio_path,metadata_json,created_at);
            INSERT INTO songs VALUES ('p','Parent','minimax','C:\old\media\p.mp3','{"cover_filename":"p.jpg"}',2),
            ('s','Drums','stems','/old/media/s.wav','{}',1);
            CREATE TABLE playlists(id,name,created_at);
            INSERT INTO playlists VALUES ('list','Mix',1);
            CREATE TABLE playlist_songs(playlist_id,song_id,position);
            INSERT INTO playlist_songs VALUES ('list','p',2),('list','s',1);"#).unwrap();
        drop(db);
        for file in ["p.mp3", "p-drums.wav", "s.wav"] {
            std::fs::write(media.join(file), file).unwrap();
        }
        let original = std::fs::read(&path).unwrap();
        let result = inspect(temp.path()).unwrap();
        assert_eq!(result, inspect(&root).unwrap());
        assert_eq!(result["playlists"][0]["song_ids"], json!(["s", "p"]));
        assert_eq!(result["songs"][0]["missing"][0]["name"], "p.jpg");
        assert_eq!(result["songs"][0]["files"][1]["name"], "p-drums.wav");
        assert_eq!(result["songs"][1]["files"][0]["name"], "s.wav");
        assert_eq!(original, std::fs::read(&path).unwrap());
    }
}
