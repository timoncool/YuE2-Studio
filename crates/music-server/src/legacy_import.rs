use super::{manifest, open_source, source_root};
use crate::{
    api_error,
    library::{self, Library, Song},
    ApiError, AppState,
};
use anyhow::{Context, Result};
use axum::{extract::State, http::StatusCode, Json};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
pub(crate) struct ImportRequest {
    path: PathBuf,
    #[serde(default)]
    exclude: HashSet<Category>,
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
enum Category {
    Audio,
    Lyrics,
    Karaoke,
    Generation,
    Covers,
    Takes,
    Stems,
    Midi,
    Latents,
    Playlists,
}

const SONG_CATEGORIES: [Category; 9] = [Category::Audio, Category::Lyrics, Category::Generation,
    Category::Covers, Category::Karaoke, Category::Takes, Category::Stems, Category::Midi, Category::Latents];

type Content = HashSet<Category>;

fn included(kind: &str, exclude: &HashSet<Category>) -> bool {
    let category = match kind {
        "audio" => Category::Audio,
        "cover" => Category::Covers,
        "original" | "version" => {
            if exclude.contains(&Category::Audio) {
                return false;
            }
            Category::Takes
        }
        "stem" => Category::Stems,
        "midi" | "midi_metadata" => Category::Midi,
        "latent" => Category::Latents,
        _ => return true,
    };
    !exclude.contains(&category)
}

pub(crate) async fn import(
    State(state): State<AppState>,
    Json(request): Json<ImportRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let result = tokio::task::spawn_blocking(move || transfer_selected(&state.library, &request.path, &request.exclude))
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

fn filter_files(song: &mut Value, exclude: &Content) -> Result<()> {
    for key in ["files", "missing"] {
        song[key].as_array_mut().context("Missing file inventory")?
            .retain(|file| included(file["kind"].as_str().unwrap_or_default(), exclude));
    }
    Ok(())
}

const STAGING_FOLDER: &str = ".legacy-import";
const PARTIAL_PREFIX: &str = "copy-";

fn lock_staging(media: &Path) -> Result<(fs::File, PathBuf)> {
    let staging = media.join(STAGING_FOLDER);
    fs::create_dir_all(&staging)?;
    anyhow::ensure!(
        fs::symlink_metadata(&staging)?.file_type().is_dir(),
        "Import staging path is not a directory"
    );
    let lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(staging.join("lock"))?;
    fs2::FileExt::lock_exclusive(&lock)?;
    // A process exit between copy and persist can leave a file from the last import.
    for entry in fs::read_dir(&staging)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with(PARTIAL_PREFIX)
            && entry.file_type()?.is_file()
        {
            fs::remove_file(entry.path())?;
        }
    }
    Ok((lock, staging))
}


// A retry can find a complete file from an interrupted import. A partially copied file never
// occupies the final name, and a user edit or a different file is never overwritten.
fn install(source: &Path, target: &Path, staging: &Path) -> Result<bool> {
    anyhow::ensure!(
        fs::symlink_metadata(source)?.is_file(),
        "Expected regular file {}",
        source.display()
    );
    if !target.exists() {
        let mut tmp = tempfile::Builder::new()
            .prefix(PARTIAL_PREFIX)
            .tempfile_in(staging)?;
        io::copy(&mut fs::File::open(source)?, &mut tmp)?;
        tmp.as_file().sync_all()?;
        match tmp.persist_noclobber(target) {
            Ok(_) => return Ok(true),
            Err(e) if e.error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.error.into()),
        }
    }
    anyhow::ensure!(
        fs::symlink_metadata(target)?.is_file() && digest(source)? == digest(target)?,
        "Destination already contains different data: {}",
        target.display()
    );
    Ok(false)
}

fn pending_content(tx: &Transaction, id: &str) -> Result<Option<Content>> {
    let raw: Option<String> = tx.query_row(
        "SELECT pending FROM legacy_import_content WHERE song_id=?", [id], |row| row.get(0)
    ).optional()?;
    raw.map(|raw| serde_json::from_str(&raw).context("Read pending legacy content")).transpose()
}

fn save_pending(tx: &Transaction, song_id: &str, pending: &Content) -> Result<()> {
    tx.execute("INSERT INTO legacy_import_content(song_id,pending) VALUES(?,?) ON CONFLICT(song_id) DO UPDATE SET pending=excluded.pending",
        params![song_id, serde_json::to_string(pending)?])?;
    Ok(())
}

fn absent(song: &Song, category: Category) -> bool {
    let no = |key| song.metadata.get(key).is_none_or(Value::is_null);
    match category {
        Category::Audio => song.audio_path.is_none(),
        Category::Lyrics => song.lyrics.is_empty(),
        Category::Generation => song.replay_request.is_none() && song.audio_codes.is_none() &&
            (song.generation_settings.is_null() || song.generation_settings.as_object().is_some_and(|obj| obj.is_empty())),
        Category::Covers => no("cover_filename"),
        Category::Karaoke => no("lrc") && no("karaoke"),
        Category::Takes => no("audio_versions") && no("original_audio_path") && no("active_version"),
        _ => true,
    }
}

fn source_has(song: &Song, item: &Value, category: Category, include_missing: bool) -> bool {
    let kinds: &[&str] = match category {
        Category::Audio => &["audio"],
        Category::Covers => &["cover"],
        Category::Takes => &["original", "version"],
        Category::Stems => &["stem"],
        Category::Midi => &["midi", "midi_metadata"],
        Category::Latents => &["latent"],
        _ => return !absent(song, category),
    };
    let matches = |key| item[key].as_array().into_iter().flatten()
        .any(|file| file["kind"].as_str().is_some_and(|kind| kinds.contains(&kind)));
    matches("files") || (include_missing && matches("missing"))
}

// Old imports already selected all optional content. Missing attachments or
// text can be a later user edit. The original main audio may be supplied for a
// metadata-only row since there is no command to detach it from an existing song.
fn old_pending(song: &Song) -> Content {
    [Category::Audio]
        .into_iter().filter(|kind| absent(song, *kind)).collect()
}

fn merge_content(target: &mut Song, patch: &Song, chosen: &Content) {
    for &category in chosen {
        if !absent(target, category) {
            continue;
        }
        match category {
            Category::Audio => target.audio_path.clone_from(&patch.audio_path),
            Category::Lyrics => target.lyrics.clone_from(&patch.lyrics),
            Category::Generation => {
                target.generation_settings.clone_from(&patch.generation_settings);
                target.replay_request.clone_from(&patch.replay_request);
                target.audio_codes.clone_from(&patch.audio_codes);
            }
            Category::Covers | Category::Karaoke | Category::Takes => {
                let fields: &[&str] = match category {
                    Category::Covers => &["cover_filename"],
                    Category::Karaoke => &["lrc", "karaoke"],
                    _ => &["audio_versions", "original_audio_path", "active_version"],
                };
                for key in fields {
                    if let (Some(value), Some(metadata)) = (patch.metadata.get(*key), target.metadata.as_object_mut()) {
                        metadata.insert((*key).to_owned(), value.clone());
                    }
                }
            }
            _ => {}
        }
    }
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
            let Ok(key) = destination_name(
                old,
                mapping.map_or("new", |(id, _)| id.as_str()),
                name,
                file["kind"].as_str(),
            ) else { continue };
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
    staging: &Path,
    exclude: &HashSet<Category>,
    errors: &mut Vec<Value>,
    backfill: bool,
) -> Result<bool> {
    let id = &ids[&song.id].id;
    let missing = item["missing"]
        .as_array()
        .context("Missing file inventory")?;
    let start = errors.len();
    for file in missing {
        errors.push(json!({"song_id": song.id, "title": song.title, "kind": file["kind"],
            "file": file["name"], "code": "missing"}));
    }
    if errors.len() != start {
        return Ok(false);
    }
    let mut names = HashMap::new();
    let mut created = Vec::new();
    for file in item["files"].as_array().context("Missing file inventory")? {
        let name = file["name"].as_str().context("Missing filename")?;
        let kind = file["kind"].as_str();
        match destination_name(&song.id, id, name, kind).and_then(|new_name| {
            let target = destination.join(&new_name);
            let existing_sidecar = backfill && target.is_file() &&
                matches!(kind, Some("stem" | "midi" | "midi_metadata" | "latent"));
            if !existing_sidecar && install(&source_media.join(name), &target, staging)? {
                created.push(target);
            }
            Ok(new_name)
        }) {
            Ok(new_name) => { names.insert(name.to_owned(), new_name); }
            Err(error) => errors.push(json!({"song_id": song.id, "title": song.title, "kind": file["kind"],
                "file": name, "code": "copy_failed", "error": format!("{error:#}")})),
        }
    }
    if errors.len() != start {
        for target in created {
            if let Err(error) = fs::remove_file(&target) {
                errors.push(json!({"song_id": song.id, "title": song.title, "kind": "cleanup",
                    "file": target.file_name().and_then(|name| name.to_str()), "code": "copy_failed", "error": error.to_string()}));
            }
        }
        return Ok(false);
    }
    if let Some(metadata) = song.metadata.as_object_mut() {
        if exclude.contains(&Category::Covers) {
            metadata.remove("cover_filename");
        }
        if exclude.contains(&Category::Takes) || exclude.contains(&Category::Audio) {
            metadata.remove("original_audio_path");
            metadata.remove("audio_versions");
            metadata.remove("active_version");
        }
        if exclude.contains(&Category::Karaoke) {
            metadata.remove("karaoke");
            metadata.remove("lrc");
        }
    }
    if exclude.contains(&Category::Lyrics) {
        song.lyrics.clear();
    }
    if exclude.contains(&Category::Audio) {
        song.audio_path = None;
    }
    if exclude.contains(&Category::Generation) {
        song.generation_settings = json!({});
        song.replay_request = None;
        song.audio_codes = None;
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
    for field in ["/source_song_id", "/reference_song_id", "/cover_of"] {
        remap_reference(&mut song.generation_settings, field, ids)?;
        if let Some(req) = &mut song.replay_request {
            remap_reference(req, field, ids)?;
        }
    }
    song.id = id.clone();
    Ok(true)
}

#[cfg(test)]
fn transfer(to: &Library, folder: &Path) -> Result<Value> {
    transfer_selected(to, folder, &HashSet::new())
}

fn transfer_selected(to: &Library, folder: &Path, exclude: &HashSet<Category>) -> Result<Value> {
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
    let mut view = manifest(&root, &db)?;
    for song in view["songs"].as_array_mut().context("Missing songs")? {
        filter_files(song, exclude)?;
    }
    let items: HashMap<_, _> = view["songs"]
        .as_array()
        .context("Missing songs")?
        .iter()
        .map(|s| (s["id"].as_str().unwrap(), s))
        .collect();
    let mut songs = Library::read_songs(&db)?;
    let playlists = if exclude.contains(&Category::Playlists) { Vec::new() } else { Library::read_playlists(&db)? };
    let source = root.to_str().context("Unsupported library path")?;
    fs::create_dir_all(to.media_dir())?;
    let source_media = root.join("media");
    if source_media.is_dir() {
        anyhow::ensure!(
            !same_file::is_same_file(&source_media, to.media_dir())?,
            "Source and destination share a media folder"
        );
    }
    let (_lock, staging) = lock_staging(to.media_dir())?;
    let space = media_space(&view, &to.legacy_song_mappings(source)?, to.media_dir())?;
    if let Some(free) = space.available_bytes {
        anyhow::ensure!(
            space.required_bytes <= free,
            "Not enough disk space: {} bytes required, {} available",
            space.required_bytes,
            free
        );
    }
    let (song_ids, list_ids, pending) = to.legacy_transaction(|tx| {
        let ids = reserve(tx, source, "song", songs.iter().map(|s| s.id.clone()))?;
        let mut pending = HashMap::new();
        for (_, mapping) in ids.iter().filter(|(_, mapping)| mapping.imported) {
            if let Some(song) = Library::read_song(tx, &mapping.id)? {
                let content = pending_content(tx, &song.id)?;
                pending.insert(song.id.clone(), (song, content));
            }
        }
        Ok((
            ids,
            reserve(
                tx,
                source,
                "playlist",
                playlists.iter().map(|p| p.id.clone()),
            )?,
            pending,
        ))
    })?;
    let already_imported = song_ids.values().filter(|mapping| mapping.imported).count();
    let already_imported_lists = list_ids.values().filter(|mapping| mapping.imported).count();
    let mut errors = Vec::new();
    let mut available: HashSet<&str> = song_ids.iter().filter(|(_, mapping)| pending.contains_key(&mapping.id))
        .map(|(id, _)| id.as_str()).collect();
    let mut prepared = Vec::new();
    let mut backfills = Vec::new();
    let mut protected = Vec::new();
    let mut omitted = exclude.clone();
    omitted.remove(&Category::Playlists);
    if omitted.contains(&Category::Audio) {
        omitted.insert(Category::Takes);
    }
    for mut song in songs {
        let old_id = song.id.clone();
        let (old_id, mapping) = song_ids.get_key_value(&old_id).context("Missing song ID")?;
        let item = items[old_id.as_str()];
        if !mapping.imported {
            if prepare_song(&mut song, item, &song_ids, &source_media,
                to.media_dir(), &staging, &omitted, &mut errors, false)? {
                available.insert(old_id.as_str());
                prepared.push(song);
            }
        } else if let Some((current, recorded)) = pending.get(&mapping.id) {
            let mut remaining = recorded.clone().unwrap_or_else(|| old_pending(current));
            let count = remaining.len();
            remaining.retain(|kind| omitted.contains(kind) || absent(current, *kind));
            if remaining.len() != count {
                protected.push((current.id.clone(), remaining.clone()));
            }
            let chosen = remaining.difference(&omitted).copied()
                .filter(|kind| absent(current, *kind) && source_has(&song, item, *kind, recorded.is_some()))
                .collect::<Content>();
            if chosen.is_empty() { continue; }
            let mut skip = SONG_CATEGORIES.into_iter().filter(|kind| !chosen.contains(kind)).collect::<Content>();
            let mut selected_item = item.clone();
            // A backfill can attach additional takes to an audio file already present.
            if current.audio_path.is_some() && chosen.contains(&Category::Takes) {
                skip.remove(&Category::Audio);
                selected_item["files"].as_array_mut().unwrap().retain(|file| file["kind"] != "audio");
                selected_item["missing"].as_array_mut().unwrap().retain(|file| file["kind"] != "audio");
                song.audio_path = None;
            }
            filter_files(&mut selected_item, &skip)?;
            if prepare_song(&mut song, &selected_item, &song_ids, &source_media,
                to.media_dir(), &staging, &skip, &mut errors, true)? {
                backfills.push((song, chosen, remaining));
            }
        }
    }
    to.legacy_transaction(|tx|{
        let mut added=0;
        for s in prepared {
            if !claim(tx,&s.id)? {continue;}
            tx.execute("INSERT INTO songs(id,title,audio_path,caption,lyrics,metadata_json,generation_settings_json,engine_id,profile_id,replay_request_json,audio_codes_json,source,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                params![s.id,s.title,s.audio_path,s.caption,s.lyrics,s.metadata.to_string(),s.generation_settings.to_string(),s.engine_id,s.profile_id,s.replay_request.map(|v|v.to_string()),s.audio_codes.map(|v|v.to_string()),s.source,s.created_at,s.updated_at])?;
            save_pending(tx, &s.id, &omitted)?;
            added+=1;
        }
        let mut updated = 0;
        for (id, remaining) in protected {
            if Library::read_song(tx, &id)?.is_some() {
                save_pending(tx, &id, &remaining)?;
            }
        }
        for (patch, chosen, remaining) in backfills {
            if let Some(mut current) = Library::read_song(tx, &patch.id)? {
                merge_content(&mut current, &patch, &chosen);
                tx.execute("UPDATE songs SET audio_path=?,lyrics=?,metadata_json=?,generation_settings_json=?,replay_request_json=?,audio_codes_json=? WHERE id=?",
                    params![current.audio_path,current.lyrics,current.metadata.to_string(),current.generation_settings.to_string(),
                        current.replay_request.map(|v|v.to_string()),current.audio_codes.map(|v|v.to_string()),current.id])?;
                let pending = remaining.difference(&chosen).copied().collect();
                save_pending(tx, &current.id, &pending)?;
                updated += 1;
            }
        }
        let mut added_lists=0;
        let mut next_album_position = None;
        for list in playlists {
            if list_ids[&list.id].imported { continue; }
            if list.song_ids.iter().any(|song| !available.contains(song.as_str())) {
                errors.push(json!({"title": list.name, "kind": "playlist", "file": list.name, "code": "playlist_incomplete"}));
                continue;
            }
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
        Ok(json!({"added":added,"already_imported":already_imported,"playlists_added":added_lists,
            "playlists_already_imported":already_imported_lists,"updated":updated,"errors":errors}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_discards_only_the_interrupted_import_staging_files() -> Result<()> {
        let media = tempfile::tempdir()?;
        let staging = media.path().join(STAGING_FOLDER);
        fs::create_dir(&staging)?;
        fs::write(staging.join("copy-abandoned"), b"partial import")?;
        fs::write(media.path().join("imported.wav"), b"completed audio")?;
        fs::write(media.path().join(".other-temp"), b"another tool's file")?;
        let (_lock, found) = lock_staging(media.path())?;
        assert_eq!(found, staging);
        assert!(!staging.join("copy-abandoned").exists());
        assert_eq!(fs::read(media.path().join("imported.wav"))?, b"completed audio");
        assert_eq!(fs::read(media.path().join(".other-temp"))?, b"another tool's file");
        Ok(())
    }

    #[test]
    fn mulacover_import_rewrites_the_original_for_saved_settings() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let source = temp.path().join("old");
        let dest = temp.path().join("new");
        let old = Library::open_at(source.join("library.sqlite"), source.join("media"))?;
        let new = Library::open_at(dest.join("library.sqlite"), dest.join("media"))?;
        old.legacy_transaction(|tx| {
            tx.execute("INSERT INTO songs VALUES('old-origin','Original',NULL,'','','{}','{}','imported-audio',NULL,NULL,NULL,'audio_import','1','1')",[])?;
            tx.execute("INSERT INTO songs VALUES('old-cover','Cover',NULL,'folk','','{\"derived\":{\"from\":\"old-origin\",\"tool\":\"cover\"}}','{\"family\":\"mulacover\",\"cover_of\":\"old-origin\",\"seed\":42}','hotstep','heartmula-mulacover',NULL,NULL,'local_generation','2','2')",[])?;
            Ok(())
        })?;
        transfer(&new, &source)?;
        let imported = new.list_songs()?;
        let original = imported.iter().find(|song| song.title == "Original").unwrap();
        let cover = imported.iter().find(|song| song.title == "Cover").unwrap();
        assert_ne!(original.id, "old-origin");
        assert_eq!(cover.generation_settings["cover_of"], original.id);
        assert_eq!(cover.generation_settings["seed"], 42);
        new.legacy_transaction(|tx| {
            tx.execute("UPDATE songs SET generation_settings_json = ? WHERE id = ?", params![json!({"family":"mulacover","cover_of":"old-origin"}).to_string(), cover.id])?;
            Ok(())
        })?;
        let previously_migrated = new.get_song(&cover.id)?.unwrap();
        assert_eq!(previously_migrated.generation_settings["cover_of"], original.id);
        Ok(())
    }

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
    fn mixed_import_reports_missing_files_and_retries_without_replacing_user_edits() -> Result<()> {
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
            tx.execute("UPDATE songs SET lyrics='source lyrics', metadata_json=? WHERE id='a'",
                [json!({"lrc": "[00:01.00] word", "cover_filename": "a.jpg",
                    "original_audio_path": "old.wav", "audio_versions": [{"file": "a.wav"}],
                    "active_version": "take"}).to_string()])?;
            Ok(())
        })?;
        fs::write(old.media_dir().join("a.wav"), b"first")?;
        fs::write(old.media_dir().join("a-drums.wav"), b"stem")?;
        old.create_playlist(library::PlaylistInput {
            name: "Both".into(), description: None, song_ids: vec!["b".into(), "a".into()],
        })?;
        let exclude = HashSet::from([
            Category::Stems, Category::Karaoke, Category::Covers, Category::Takes,
        ]);
        let first = transfer_selected(&new, &source, &exclude)?;
        assert_eq!(first["added"], 1);
        assert_eq!(first["already_imported"], 0);
        assert_eq!(first["playlists_added"], 0);
        assert_eq!(first["errors"][0]["file"], "b.wav");
        assert_eq!(first["errors"][1]["code"], "playlist_incomplete");
        let song = new.list_songs()?.remove(0);
        assert!(!new.media_dir().join(format!("{}-drums.wav", song.id)).exists());
        assert_eq!(song.lyrics, "source lyrics");
        for key in ["lrc", "cover_filename", "original_audio_path", "audio_versions", "active_version"] {
            assert!(song.metadata.get(key).is_none(), "{key}");
        }
        assert!(song.audio_path.is_some());
        new.rename_song(&song.id, "Edited in YuE2")?;
        fs::write(old.media_dir().join("b.wav"), b"second")?;
        let repaired = transfer_selected(&new, &source, &exclude)?;
        assert_eq!(repaired["added"], 1);
        assert_eq!(repaired["playlists_added"], 1);
        assert_eq!(repaired["errors"], json!([]));
        let playlist = new.list_playlists()?.remove(0);
        assert_eq!(playlist.song_ids[1], song.id);
        let preview = super::super::inspect(&source, &new)?;
        assert_eq!(preview["space"]["required_bytes"], 0);
        assert_eq!(preview["space"]["total_bytes"], 15);
        let repeat = transfer_selected(&new, &source, &exclude)?;
        assert_eq!(repeat["already_imported"], 2);
        assert_eq!(new.get_song(&song.id)?.unwrap().title, "Edited in YuE2");
        new.legacy_transaction(|tx| {
            tx.execute("DELETE FROM legacy_import_content WHERE song_id=?", [&song.id])?;
            tx.execute("UPDATE songs SET audio_path=NULL, lyrics='' WHERE id=?", [&song.id])?;
            Ok(())
        })?;
        fs::write(old.media_dir().join("a.mid"), b"previously removed MIDI")?;
        let recovered = transfer_selected(&new, &source, &exclude)?;
        assert_eq!(recovered["added"], 0);
        assert_eq!(recovered["updated"], 1);
        let existing = new.get_song(&song.id)?.unwrap();
        assert!(existing.audio_path.is_some());
        assert!(existing.lyrics.is_empty(), "legacy rows must keep cleared lyrics");
        assert!(!new.media_dir().join(format!("{}.mid", song.id)).exists());
        assert_eq!(existing.title, "Edited in YuE2");
        let entries = fs::read_dir(new.media_dir())?.collect::<io::Result<Vec<_>>>()?;
        assert_eq!(entries.iter().filter(|entry| entry.path().is_file()).count(), 2);
        let staged = fs::read_dir(new.media_dir().join(STAGING_FOLDER))?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<io::Result<Vec<_>>>()?;
        assert_eq!(staged, [std::ffi::OsString::from("lock")]);
        assert!(transfer(&new, &destination).is_err());
        Ok(())
    }

    #[test]
    fn copy_conflicts_report_the_file_and_leave_the_existing_bytes_intact() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let source = temp.path().join("old");
        let destination = temp.path().join("new");
        let old = Library::open_at(source.join("library.sqlite"), source.join("media"))?;
        let new = Library::open_at(destination.join("library.sqlite"), destination.join("media"))?;
        let song = json!({"files": [
            {"kind":"stem","name":"old-drums.wav"}, {"kind":"audio","name":"old.wav"}
        ], "missing":[]});
        fs::create_dir_all(old.media_dir())?;
        fs::create_dir_all(new.media_dir())?;
        fs::write(old.media_dir().join("old.wav"), b"source")?;
        fs::write(old.media_dir().join("old-drums.wav"), b"part of a failed copy")?;
        let target = new.media_dir().join(destination_name("old", "reserved", "old.wav", Some("audio"))?);
        fs::write(&target, b"user edit")?;
        old.legacy_transaction(|tx| {
            tx.execute("INSERT INTO songs VALUES('old','Take','old.wav','','','{}','{}','acestep-cpp',NULL,NULL,NULL,'native','1','1')", [])?;
            Ok(())
        })?;
        let db = open_source(&source)?;
        let mut original = Library::read_songs(&db)?.remove(0);
        let ids = HashMap::from([("old".to_owned(), Mapping { id: "reserved".into(), imported: false })]);
        let (_lock, staging) = lock_staging(new.media_dir())?;
        let mut errors = Vec::new();
        assert!(!prepare_song(&mut original, &song, &ids, old.media_dir(), new.media_dir(),
            &staging, &HashSet::new(), &mut errors, false)?);
        assert_eq!(errors[0]["file"], "old.wav");
        assert_eq!(errors[0]["code"], "copy_failed");
        assert_eq!(fs::read(&target)?, b"user edit");
        assert!(!new.media_dir().join("reserved-drums.wav").exists());
        Ok(())
    }
}
