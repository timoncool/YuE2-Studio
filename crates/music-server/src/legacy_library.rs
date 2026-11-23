use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use axum::{http::StatusCode, Json};
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{api_error, ApiError};

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum Source {
    Folder { path: PathBuf },
    Picker { pick: bool },
}

pub(super) async fn sources(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let own = state
        .library
        .media_dir()
        .parent()
        .map(Path::to_owned)
        .unwrap_or_default();
    tokio::task::spawn_blocking(move || {
        let mut seen = std::collections::HashSet::new();
        if let Ok(root) = own.canonicalize() {
            seen.insert(root);
        }
        let mut sources = Vec::new();
        for n in studio_hub_client::neighbors::others(&own) {
            let title = match n.app.as_str() {
                "ace" => "ACE-Step Studio",
                "minimax" => "MiniMax Music3 Studio",
                _ => continue,
            };
            if n.data_dir.join("library.sqlite").is_file()
                && n.data_dir
                    .canonicalize()
                    .is_ok_and(|root| seen.insert(root))
            {
                sources.push(json!({"studio": title, "path": n.data_dir, "version": n.version}));
            }
        }
        for (title, folder) in standard_folders() {
            if folder.join("library.sqlite").is_file() && folder.canonicalize().is_ok_and(|root| seen.insert(root)) {
                sources.push(json!({"studio": title, "path": folder, "version": Value::Null}));
            }
        }
        Json(json!({"sources": sources}))
    })
    .await
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}


/// Where ACE-Step Studio and MiniMax Music3 Studio keep their data when nothing registered them in the shared list
/// of programs: the profile folder each one falls back to, and on Windows the `data` folder of an installation.
fn standard_folders() -> Vec<(&'static str, std::path::PathBuf)> {
    let mut found = Vec::new();
    for (title, slug) in [("ACE-Step Studio", "ace-step-studio"), ("MiniMax Music3 Studio", "minimax-music3-studio")] {
        #[cfg(windows)]
        {
            let _ = slug;
            for variable in ["LOCALAPPDATA", "APPDATA"] {
                if let Some(root) = std::env::var_os(variable) {
                    found.push((title, std::path::PathBuf::from(root).join(title)));
                }
            }
            if let Some(root) = std::env::var_os("LOCALAPPDATA") {
                found.push((title, std::path::PathBuf::from(root).join("Programs").join(title).join("data")));
            }
            found.extend(install_locations(title).into_iter().map(|folder| (title, folder.join("data"))));
        }
        #[cfg(target_os = "macos")]
        {
            let _ = slug;
            if let Some(home) = std::env::var_os("HOME") {
                found.push((title, std::path::PathBuf::from(home).join("Library").join("Application Support").join(title)));
            }
        }
        #[cfg(all(not(windows), not(target_os = "macos")))]
        {
            if let Some(root) = std::env::var_os("XDG_DATA_HOME") {
                found.push((title, std::path::PathBuf::from(root).join(slug)));
            }
            if let Some(home) = std::env::var_os("HOME") {
                found.push((title, std::path::PathBuf::from(home).join(".local").join("share").join(slug)));
            }
        }
    }
    found
}

/// The folders an installer recorded for a program under its Uninstall key, the user's and the machine's.
#[cfg(windows)]
fn install_locations(program: &str) -> Vec<std::path::PathBuf> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    let key: Vec<u16> = format!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{program}").encode_utf16().chain([0]).collect();
    let value: Vec<u16> = "InstallLocation".encode_utf16().chain([0]).collect();
    let mut found = Vec::new();
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let mut size: u32 = 0;
        // SAFETY: key and value are NUL-terminated; a null buffer asks for the size in bytes.
        let asked = unsafe { RegGetValueW(root, key.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), std::ptr::null_mut(), &mut size) };
        if asked != 0 || size < 4 {
            continue;
        }
        let mut buffer = vec![0u16; size as usize / 2];
        // SAFETY: the buffer holds exactly `size` bytes.
        let read = unsafe { RegGetValueW(root, key.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buffer.as_mut_ptr().cast(), &mut size) };
        if read != 0 {
            continue;
        }
        let text = String::from_utf16_lossy(&buffer[..(size as usize / 2).saturating_sub(1)]);
        let text = text.trim().trim_matches('"');
        if !text.is_empty() {
            found.push(std::path::PathBuf::from(text));
        }
    }
    found
}

pub(super) async fn preview(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
    Json(source): Json<Source>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    tokio::task::spawn_blocking(move || {
        let folder = match source {
            Source::Folder { path } => path,
            Source::Picker { pick: true } => match rfd::FileDialog::new().pick_folder() {
                Some(folder) => folder,
                None => return Ok(json!({"cancelled": true})),
            },
            Source::Picker { pick: false } => anyhow::bail!("Choose a Studio folder"),
        };
        let mut result = inspect(&folder, &state.library)?;
        result["path"] = json!(folder);
        Ok::<_, anyhow::Error>(result)
    })
    .await
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map(Json)
    .map_err(|error| api_error(StatusCode::BAD_REQUEST, format!("{error:#}")))
}

fn source_root(folder: &Path) -> Result<PathBuf> {
    anyhow::ensure!(folder.is_absolute(), "Choose an absolute Studio folder");
    [folder.to_owned(), folder.join("data")]
        .into_iter()
        .find(|path| path.join("library.sqlite").is_file())
        .context("No library.sqlite in the selected folder or its data folder")?
        .canonicalize()
        .context("Resolve Studio data directory")
}

#[path = "legacy_import.rs"]
mod write;
pub(super) use write::import;

fn open_source(root: &Path) -> Result<Connection> {
    let db = Connection::open_with_flags(
        root.join("library.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    db.execute_batch("PRAGMA trusted_schema=OFF; BEGIN")?;
    Ok(db)
}

fn inspect(folder: &Path, destination: &crate::library::Library) -> Result<Value> {
    let root = source_root(folder)?;
    let db = open_source(&root)?;
    let mut view = manifest(&root, &db)?;
    if let Some(install) = root.parent().filter(|_| root.ends_with("data")) {
        let ace = install.join("ACE-Step-Studio.exe").is_file();
        let minimax = install.join("MiniMax-Music3-Studio.exe").is_file();
        match (ace, minimax) {
            (true, false) => view["studio"] = json!("ACE-Step Studio"),
            (false, true) => view["studio"] = json!("MiniMax Music3 Studio"),
            _ => {}
        }
    }
    let source = root.to_str().context("Unsupported library path")?;
    let space = write::media_space(&view, destination, source)?;
    view["space"] = serde_json::to_value(space)?;
    Ok(view)
}

fn manifest(root: &Path, db: &Connection) -> Result<Value> {
    let media = root.join("media");
    let mut files = HashMap::new();
    match std::fs::read_dir(media) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    files.insert(
                        entry.file_name().to_string_lossy().into_owned(),
                        entry.metadata()?.len(),
                    );
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
            if let Some(bytes) = files.get(name) {
                available.push(json!({"kind": kind, "name": name, "bytes": bytes}));
            } else if required {
                missing.push(json!({"kind": kind, "name": name}));
            }
        };
        if let Some(path) = row.get::<_, Option<String>>(3)? {
            check("audio", path.rsplit(['/', '\\']).next().unwrap_or(""), true);
        }
        if let Some(original) = metadata["original_audio_path"].as_str() {
            check(
                "original",
                original.rsplit(['/', '\\']).next().unwrap_or(""),
                true,
            );
        }
        if let Some(versions) = metadata["audio_versions"].as_array() {
            for version in versions {
                if let Some(name) = version["file"].as_str() {
                    check("version", name, true);
                }
            }
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
            ('s','Drums','stems','/old/media/s.wav','{"original_audio_path":"C:\\old\\media\\orig.wav","audio_versions":[{"file":"absent.wav"}]}',1);
            CREATE TABLE playlists(id,name,created_at);
            INSERT INTO playlists VALUES ('list','Mix',1);
            CREATE TABLE playlist_songs(playlist_id,song_id,position);
            INSERT INTO playlist_songs VALUES ('list','p',2),('list','s',1);"#).unwrap();
        drop(db);
        for file in ["p.mp3", "p-drums.wav", "s.wav", "orig.wav"] {
            std::fs::write(media.join(file), file).unwrap();
        }
        let original = std::fs::read(&path).unwrap();
        let dest = temp.path().join("destination");
        let library =
            crate::library::Library::open_at(dest.join("library.sqlite"), dest.join("media"))
                .unwrap();
        assert!(inspect(&root, &library).unwrap().get("studio").is_none());
        std::fs::write(temp.path().join("MiniMax-Music3-Studio.exe"), []).unwrap();
        let result = inspect(temp.path(), &library).unwrap();
        assert_eq!(result["studio"], "MiniMax Music3 Studio");
        assert_eq!(result["songs"], inspect(&root, &library).unwrap()["songs"]);
        assert_eq!(result["space"]["required_bytes"], 5 + 11 + 5 + 8);
        assert_eq!(result["playlists"][0]["song_ids"], json!(["s", "p"]));
        assert_eq!(result["songs"][0]["missing"][0]["name"], "p.jpg");
        assert_eq!(result["songs"][0]["files"][1]["name"], "p-drums.wav");
        assert_eq!(result["songs"][1]["files"][0]["name"], "s.wav");
        assert_eq!(
            result["songs"][1]["files"][1],
            json!({"kind": "original", "name": "orig.wav", "bytes": 8})
        );
        assert_eq!(result["songs"][1]["missing"][0]["name"], "absent.wav");
        assert_eq!(original, std::fs::read(&path).unwrap());
    }
}
