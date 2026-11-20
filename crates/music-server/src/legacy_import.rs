use super::{manifest, open_source, source_root};
use crate::{
    api_error,
    library::{self, Library, Song},
    ApiError, AppState,
};
use anyhow::{Context, Result};
use axum::{extract::State, http::StatusCode, Json};
use rusqlite::{params, Transaction};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
pub(crate) struct ImportRequest {
    path: PathBuf,
}

pub(crate) async fn import(
    State(state): State<AppState>,
    Json(request): Json<ImportRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let result = tokio::task::spawn_blocking(move || transfer(&state.library, &request.path))
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("{e:#}")))?;
    crate::mcp::announce("library");
    Ok(Json(result))
}

struct Mapping {
    id: String,
    imported: bool,
}
type Ids = HashMap<String, Mapping>;

fn reserve(
    tx: &Transaction,
    source: &str,
    kind: &str,
    keys: impl Iterator<Item = String>,
) -> Result<Ids> {
    let mut ids = Ids::new();
    for key in keys {
        tx.execute("INSERT INTO legacy_ids(source,kind,old_id,new_id) VALUES(?,?,?,?) ON CONFLICT(source,kind,old_id) DO NOTHING",
            params![source,kind,key,uuid::Uuid::now_v7().to_string()])?;
        let mapping = tx.query_row(
            "SELECT new_id,imported FROM legacy_ids WHERE source=? AND kind=? AND old_id=?",
            params![source, kind, key],
            |r| {
                Ok(Mapping {
                    id: r.get(0)?,
                    imported: r.get(1)?,
                })
            },
        )?;
        ids.insert(key, mapping);
    }
    Ok(ids)
}

fn claim(tx: &Transaction, id: &str) -> Result<bool> {
    Ok(tx.execute(
        "UPDATE legacy_ids SET imported=1 WHERE new_id=? AND imported=0",
        [id],
    )? == 1)
}

use crate::install_record::sha256_of as digest;

// A retry can find a complete file from an interrupted import. A partially copied file never
// occupies the final name, and a user edit or a different file is never overwritten.
fn install(source: &Path, target: &Path) -> Result<()> {
    anyhow::ensure!(
        fs::symlink_metadata(source)?.is_file(),
        "Expected regular file {}",
        source.display()
    );
    if !target.exists() {
        let mut tmp = tempfile::NamedTempFile::new_in(
            target.parent().context("Missing destination folder")?,
        )?;
        io::copy(&mut fs::File::open(source)?, &mut tmp)?;
        tmp.as_file().sync_all()?;
        match tmp.persist_noclobber(target) {
            Ok(_) => return Ok(()),
            Err(e) if e.error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.error.into()),
        }
    }
    anyhow::ensure!(
        fs::symlink_metadata(target)?.is_file() && digest(source)? == digest(target)?,
        "Destination already contains different data: {}",
        target.display()
    );
    Ok(())
}

fn remap_reference(value: &mut Value, pointer: &str, ids: &Ids) -> Result<()> {
    if let Some(field) = value.pointer_mut(pointer) {
        if let Some(old) = field.as_str().filter(|s| !s.is_empty()) {
            if let Some(mapping) = ids.get(old) {
                *field = json!(mapping.id);
            } else {
                *field = Value::Null;
            } // The original may already have been removed from the source library.
        }
    }
    Ok(())
}

fn prepare_song(
    song: &mut Song,
    item: &Value,
    ids: &Ids,
    source_media: &Path,
    destination: &Path,
) -> Result<()> {
    let id = &ids[&song.id].id;
    let missing = item["missing"]
        .as_array()
        .context("Missing file inventory")?;
    anyhow::ensure!(
        missing.is_empty(),
        "{}: missing files {}",
        song.title,
        item["missing"]
    );
    let mut names = HashMap::new();
    for file in item["files"].as_array().context("Missing file inventory")? {
        let name = file["name"].as_str().context("Missing filename")?;
        let extension = Path::new(name)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        anyhow::ensure!(
            extension.chars().all(|c| c.is_ascii_alphanumeric()),
            "Unsupported extension: {name}"
        );
        let new_name = match file["kind"].as_str() {
            Some("stem" | "midi" | "midi_metadata" | "latent") => format!(
                "{}{}",
                id,
                name.strip_prefix(&song.id)
                    .context("Invalid sidecar name")?
            ),
            _ => {
                let hash: String = Sha256::digest(name.as_bytes())
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                format!("{id}-{hash}.{extension}")
            }
        };
        install(&source_media.join(name), &destination.join(&new_name))
            .with_context(|| format!("{}: {name}", song.title))?;
        names.insert(name.to_owned(), new_name);
    }
    let rename = |s: &str| -> Result<String> {
        let base = s.rsplit(['/', '\\']).next().unwrap_or("");
        names
            .get(base)
            .cloned()
            .with_context(|| format!("No local file for {s}"))
    };
    song.audio_path = song.audio_path.as_deref().map(rename).transpose()?;
    for field in ["cover_filename", "original_audio_path"] {
        if let Some(value) = song.metadata[field].as_str() {
            song.metadata[field] = json!(rename(value)?);
        }
    }
    if let Some(versions) = song
        .metadata
        .get_mut("audio_versions")
        .and_then(Value::as_array_mut)
    {
        for version in versions {
            if let Some(file) = version["file"].as_str() {
                version["file"] = json!(rename(file)?);
            }
        }
    }
    remap_reference(&mut song.metadata, "/derived/from", ids)?;
    remap_reference(&mut song.metadata, "/deleted_with", ids)?;
    remap_reference(&mut song.generation_settings, "/source_song_id", ids)?;
    if let Some(req) = &mut song.replay_request {
        remap_reference(req, "/source_song_id", ids)?;
    }
    song.id = id.clone();
    Ok(())
}

fn transfer(to: &Library, folder: &Path) -> Result<Value> {
    let root = source_root(folder)?;
    let source_db = root.join("library.sqlite");
    let dest_db = to
        .media_dir()
        .parent()
        .context("Missing library folder")?
        .join("library.sqlite");
    if dest_db.is_file() {
        anyhow::ensure!(
            !same_file::is_same_file(&source_db, &dest_db)?,
            "This is the current Studio library"
        );
    }
    let db = open_source(&root)?;
    let view = manifest(&root, &db)?;
    let items: HashMap<_, _> = view["songs"]
        .as_array()
        .context("Missing songs")?
        .iter()
        .map(|s| (s["id"].as_str().unwrap(), s))
        .collect();
    let mut songs = Library::read_songs(&db)?;
    let mut stmt =
        db.prepare("SELECT id,name,description,created_at,updated_at FROM playlists ORDER BY id")?;
    let playlists = stmt
        .query_map([], |row| library::playlist(&db, row))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let source = root.to_str().context("Unsupported library path")?;
    let (song_ids,list_ids)=to.legacy_transaction(|tx|{
        tx.execute_batch("CREATE TABLE IF NOT EXISTS legacy_ids(source TEXT NOT NULL,kind TEXT NOT NULL,old_id TEXT NOT NULL,new_id TEXT NOT NULL UNIQUE,imported INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(source,kind,old_id))")?;
        Ok((reserve(tx,source,"song",songs.iter().map(|s|s.id.clone()))?,
            reserve(tx,source,"playlist",playlists.iter().map(|p|p.id.clone()))?))
    })?;
    fs::create_dir_all(to.media_dir())?;
    let source_media = root.join("media");
    if source_media.is_dir() {
        anyhow::ensure!(
            !same_file::is_same_file(&source_media, to.media_dir())?,
            "Source and destination share a media folder"
        );
    }
    songs.retain(|s| !song_ids[&s.id].imported);
    for song in &mut songs {
        let item = items[song.id.as_str()];
        prepare_song(song, item, &song_ids, &source_media, to.media_dir())?;
    }
    to.legacy_transaction(|tx|{
        let mut added=0;
        for s in songs {
            if !claim(tx,&s.id)? {continue;}
            tx.execute("INSERT INTO songs(id,title,audio_path,caption,lyrics,metadata_json,generation_settings_json,engine_id,profile_id,replay_request_json,audio_codes_json,source,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                params![s.id,s.title,s.audio_path,s.caption,s.lyrics,s.metadata.to_string(),s.generation_settings.to_string(),s.engine_id,s.profile_id,s.replay_request.map(|v|v.to_string()),s.audio_codes.map(|v|v.to_string()),s.source,s.created_at,s.updated_at])?;
            added+=1;
        }
        let mut added_lists=0;
        for list in playlists {
            let new_id=&list_ids[&list.id].id;
            if !claim(tx,new_id)? {continue;}
            tx.execute("INSERT INTO playlists(id,name,description,created_at,updated_at) VALUES(?,?,?,?,?)",params![new_id,list.name,list.description,list.created_at,list.updated_at])?;
            for (position,old_id) in list.song_ids.iter().enumerate(){
                let song=song_ids.get(old_id).context("Playlist refers to a missing song")?;
                tx.execute("INSERT INTO playlist_songs(playlist_id,song_id,position) VALUES(?,?,?)",params![new_id,song.id,i64::try_from(position)?])?;
            }
            added_lists+=1;
        }
        Ok(json!({"added":added,"already_imported":song_ids.len()-added,"playlists_added":added_lists,"playlists_already_imported":list_ids.len()-added_lists}))
    })
}
