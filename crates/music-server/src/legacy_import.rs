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

fn destination_name(old_id: &str, id: &str, name: &str, kind: Option<&str>) -> Result<String> {
    let extension = Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    anyhow::ensure!(
        extension.chars().all(|c| c.is_ascii_alphanumeric()),
        "Unsupported extension: {name}"
    );
    let new_name = match kind {
        Some("stem" | "midi" | "midi_metadata" | "latent") => format!(
            "{}{}",
            id,
            name.strip_prefix(old_id).context("Invalid sidecar name")?
        ),
        _ => {
            let hash: String = Sha256::digest(name.as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            format!("{id}-{hash}.{extension}")
        }
    };
    Ok(new_name)
}

#[derive(serde::Serialize)]
pub(super) struct MediaSpace {
    total_bytes: u64,
    required_bytes: u64,
    available_bytes: Option<u64>,
}

pub(super) fn media_space(
    view: &Value,
    mappings: &HashMap<String, (String, bool)>,
    destination: &Path,
) -> Result<MediaSpace> {
    let volume = destination.ancestors().find(|path| path.exists());
    let mut space = MediaSpace {
        total_bytes: 0,
        required_bytes: 0,
        available_bytes: volume.and_then(|path| fs2::available_space(path).ok()),
    };
    for song in view["songs"].as_array().context("Missing songs")? {
        let old = song["id"].as_str().context("Missing song ID")?;
        let mapping = mappings.get(old);
        let mut counted = std::collections::HashSet::new();
        for file in song["files"].as_array().context("Missing files")? {
            let name = file["name"].as_str().context("Missing filename")?;
            // The active take and the original may point to the same file.
            let key = destination_name(
                old,
                mapping.map_or("new", |(id, _)| id.as_str()),
                name,
                file["kind"].as_str(),
            )?;
            if !counted.insert(key.clone()) {
                continue;
            }
            let bytes = file["bytes"].as_u64().context("Missing file size")?;
            space.total_bytes += bytes;
            if !mapping.is_some_and(|(_, done)| *done || destination.join(&key).is_file()) {
                space.required_bytes += bytes;
            }
        }
    }
    Ok(space)
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
        let new_name = destination_name(&song.id, id, name, file["kind"].as_str())?;
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
    for field in ["/source_song_id", "/reference_song_id"] {
        remap_reference(&mut song.generation_settings, field, ids)?;
        if let Some(req) = &mut song.replay_request {
            remap_reference(req, field, ids)?;
        }
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
    let playlists = Library::read_playlists(&db)?;
    let source = root.to_str().context("Unsupported library path")?;
    let space = media_space(&view, &to.legacy_song_mappings(source)?, to.media_dir())?;
    if let Some(free) = space.available_bytes {
        anyhow::ensure!(
            space.required_bytes <= free,
            "Not enough disk space: {} bytes required, {} available",
            space.required_bytes,
            free
        );
    }
    let (song_ids, list_ids) = to.legacy_transaction(|tx| {
        Ok((
            reserve(tx, source, "song", songs.iter().map(|s| s.id.clone()))?,
            reserve(
                tx,
                source,
                "playlist",
                playlists.iter().map(|p| p.id.clone()),
            )?,
        ))
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
        let mut next_album_position = None;
        for list in playlists {
            let new_id=&list_ids[&list.id].id;
            if !claim(tx,new_id)? {continue;}
            let album = library::is_album_description(list.description.as_deref());
            if album && next_album_position.is_none() {
                let mut next: i64 = tx.query_row("SELECT COALESCE(MAX(position),-1)+1 FROM album_order", [], |row| row.get(0))?;
                for existing in Library::read_playlists(tx)? {
                    if library::is_album_description(existing.description.as_deref()) {
                        next += tx.execute("INSERT OR IGNORE INTO album_order(playlist_id,position) VALUES(?,?)", params![existing.id,next])? as i64;
                    }
                }
                next_album_position = Some(next);
            }
            tx.execute("INSERT INTO playlists(id,name,description,created_at,updated_at) VALUES(?,?,?,?,?)",params![new_id,list.name,list.description,list.created_at,list.updated_at])?;
            if album {
                if let Some(position) = next_album_position.as_mut() {
                    tx.execute("INSERT INTO album_order(playlist_id,position) VALUES(?,?)", params![new_id,*position])?;
                    *position += 1;
                }
            }
            for (position,old_id) in list.song_ids.iter().enumerate(){
                let song=song_ids.get(old_id).context("Playlist refers to a missing song")?;
                tx.execute("INSERT INTO playlist_songs(playlist_id,song_id,position) VALUES(?,?,?)",params![new_id,song.id,i64::try_from(position)?])?;
            }
            added_lists+=1;
        }
        Ok(json!({"added":added,"already_imported":song_ids.len()-added,"playlists_added":added_lists,"playlists_already_imported":list_ids.len()-added_lists}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn album_import_appends_in_source_order_and_preserves_subsequent_edits() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let source = temp.path().join("old");
        let dest = temp.path().join("new");
        let old = Library::open_at(source.join("library.sqlite"), source.join("media"))?;
        let new = Library::open_at(dest.join("library.sqlite"), dest.join("media"))?;
        let create = |db: &Library, name: &str| db.create_playlist(library::PlaylistInput {
            name: name.into(), description: Some("[YuE2-Studio:album:v1]\n".into()), song_ids: vec![],
        });
        let original = create(&new, "Existing")?;
        let a = create(&old, "A")?;
        let b = create(&old, "B")?;
        old.reorder_albums(&[a.id, b.id])?;
        transfer(&new, &source)?;
        let albums = new.list_playlists()?;
        assert_eq!(albums.iter().map(|p|p.name.as_str()).collect::<Vec<_>>(), ["Existing", "A", "B"]);
        let edited = vec![albums[2].id.clone(), original.id, albums[1].id.clone()];
        new.reorder_albums(&edited)?;
        transfer(&new, &source)?;
        assert_eq!(new.list_playlists()?.into_iter().map(|p|p.id).collect::<Vec<_>>(), edited);
        Ok(())
    }

    #[test]
    fn retry_publishes_all_rows_together_and_keeps_user_edits() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let source = temp.path().join("old");
        let destination = temp.path().join("new");
        let old = Library::open_at(source.join("library.sqlite"), source.join("media"))?;
        let new = Library::open_at(
            destination.join("library.sqlite"),
            destination.join("media"),
        )?;
        fs::create_dir_all(old.media_dir())?;
        old.legacy_transaction(|tx| {
            for (id, created) in [("a", "2"), ("b", "1")] {
                tx.execute("INSERT INTO songs VALUES(?,?,?,'','','{}','{}','acestep-cpp',NULL,NULL,NULL,'native',?,?)",
                    params![id,id,format!("{id}.wav"),created,created])?;
            }
            Ok(())
        })?;
        fs::write(old.media_dir().join("a.wav"), b"first")?;
        assert!(transfer(&new, &source)
            .unwrap_err()
            .to_string()
            .contains("b.wav"));
        assert!(new.list_songs()?.is_empty());
        fs::write(old.media_dir().join("b.wav"), b"second")?;
        assert_eq!(transfer(&new, &source)?["added"], 2);
        let song = new.list_songs()?.remove(0);
        new.rename_song(&song.id, "Edited in YuE2")?;
        let preview = super::super::inspect(&source, &new)?;
        assert_eq!(preview["space"]["required_bytes"], 0);
        assert_eq!(preview["space"]["total_bytes"], 11);
        let repeat = transfer(&new, &source)?;
        assert_eq!(repeat["already_imported"], 2);
        assert_eq!(new.get_song(&song.id)?.unwrap().title, "Edited in YuE2");
        assert_eq!(fs::read_dir(new.media_dir())?.count(), 2);
        assert!(transfer(&new, &destination).is_err());
        Ok(())
    }
}
