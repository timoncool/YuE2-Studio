//! Optional location of the music library (SQLite catalogue + audio/cover media).
//! Changing it is staged while the service is running; the copy takes place
//! before SQLite opens on the next launch, never while music is being written.
//! The old directory remains authoritative until all bytes and the preference
//! have been committed. Models/training data are deliberately unaffected.
use std::{fs, path::{Path, PathBuf}};
use anyhow::{bail, Context, Result};
use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::{api_error, ApiError};

const SETTINGS: &str = "music-location.json";

#[derive(Default, Clone, Serialize, Deserialize)]
struct Choice {
    #[serde(default)] directory: Option<PathBuf>,
    #[serde(default)] pending_directory: Option<PathBuf>,
    #[serde(default)] pending_reset: bool,
    #[serde(default)] error: Option<String>,
}

fn settings_file(root: &Path) -> PathBuf { root.join(SETTINGS) }

fn read_choice(root: &Path) -> Result<Choice> {
    let file = settings_file(root);
    if !file.exists() { return Ok(Choice::default()); }
    serde_json::from_slice(&fs::read(&file).with_context(|| format!("read {}", file.display()))?)
        .with_context(|| format!("decode {}", file.display()))
}

fn write_choice(root: &Path, choice: &Choice) -> Result<()> {
    fs::create_dir_all(root)?;
    let temp = root.join(format!(".{SETTINGS}.tmp"));
    fs::write(&temp, serde_json::to_vec_pretty(choice)?)?;
    fs::rename(&temp, settings_file(root)).context("save music location")?;
    Ok(())
}

fn chosen(root: &Path, directory: Option<&PathBuf>) -> PathBuf {
    directory.cloned().unwrap_or_else(|| root.to_path_buf())
}

fn has_pending(choice: &Choice) -> bool {
    choice.pending_reset || choice.pending_directory.is_some()
}

fn total_bytes(path: &Path) -> Result<u64> {
    if !path.exists() { return Ok(0); }
    let mut size = 0;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() { bail!("music contains a symbolic link: {}", entry.path().display()); }
        if kind.is_dir() { size += total_bytes(&entry.path())?; }
        else if kind.is_file() { size += entry.metadata()?.len(); }
    }
    Ok(size)
}

fn copy_media(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)?;
    if !source.exists() { return Ok(()); }
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let from = entry.path();
        let to = target.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_symlink() { bail!("music contains a symbolic link: {}", from.display()); }
        if kind.is_dir() { copy_media(&from, &to)?; }
        else if kind.is_file() {
            let copied = fs::copy(&from, &to).with_context(|| format!("copy {}", from.display()))?;
            if copied != entry.metadata()?.len() { bail!("the copied file is incomplete: {}", from.display()); }
        }
    }
    Ok(())
}

fn validate_directory(root: &Path, requested: &Path, current: &Path) -> Result<PathBuf> {
    if !requested.is_absolute() { bail!("choose a full folder path"); }
    let requested = fs::canonicalize(requested).with_context(|| format!("the folder {} does not exist", requested.display()))?;
    if !requested.is_dir() { bail!("music location must be a folder"); }
    let root = fs::canonicalize(root)?;
    let current = fs::canonicalize(current).unwrap_or_else(|_| current.to_path_buf());
    if requested.starts_with(&root) || root.starts_with(&requested) {
        bail!("choose a dedicated music folder outside the studio data directory");
    }
    if requested != current && (requested.starts_with(&current) || current.starts_with(&requested)) {
        bail!("the new folder cannot overlap the existing music library");
    }
    let probe = requested.join(format!(".yue2-write-probe-{}", uuid::Uuid::now_v7()));
    fs::write(&probe, b"write test").with_context(|| format!("cannot write into {}", requested.display()))?;
    let _ = fs::remove_file(probe);
    Ok(requested)
}

/// Copy a closed library into the chosen directory. Never touch the original
/// unless every file was copied, promoted and the choice was durably recorded.
fn migrate(source: &Path, target: &Path, root: &Path, choice: &mut Choice, next: Option<PathBuf>) -> Result<()> {
    if source == target {
        choice.directory = next;
        choice.pending_directory = None;
        choice.pending_reset = false;
        choice.error = None;
        write_choice(root, choice)?;
        return Ok(());
    }
    if !source.join("library.sqlite").is_file() {
        // A fresh installation has no catalogue to move yet.
        if source != root { bail!("the existing music catalogue is missing: {}", source.display()); }
    }
    if target.join("library.sqlite").exists() || target.join("media").exists() {
        bail!("{} already contains a music catalogue or media folder; choose an empty destination", target.display());
    }
    fs::create_dir_all(target)?;
    let sqlite = source.join("library.sqlite");
    if sqlite.exists() {
        // The service is not running yet, so this also folds any old SQLite WAL
        // into the main database before we copy it to a new volume.
        let db = rusqlite::Connection::open(&sqlite)?;
        let busy: i64 = db.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))?;
        if busy != 0 { bail!("the music catalogue is busy; close other studio instances and restart"); }
        drop(db);
    }
    let required = if sqlite.exists() { fs::metadata(&sqlite)?.len() } else { 0 }
        + total_bytes(&source.join("media"))?;
    let free = fs2::available_space(target)?;
    if free < required { bail!("not enough room for the music library ({} bytes needed, {} free)", required, free); }

    let stage = target.join(format!(".yue2-music-stage-{}", uuid::Uuid::now_v7()));
    fs::create_dir(&stage)?;
    let copied = (|| -> Result<()> {
        if sqlite.exists() {
            let copied = fs::copy(&sqlite, stage.join("library.sqlite"))?;
            if copied != fs::metadata(&sqlite)?.len() { bail!("the catalogue copy is incomplete"); }
            let staged = rusqlite::Connection::open_with_flags(
                stage.join("library.sqlite"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            let integrity: String = staged.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
            if integrity != "ok" { bail!("copied music catalogue failed integrity check: {integrity}"); }
        }
        copy_media(&source.join("media"), &stage.join("media"))?;
        Ok(())
    })();
    if let Err(error) = copied {
        let _ = fs::remove_dir_all(&stage);
        return Err(error);
    }

    let db_target = target.join("library.sqlite");
    let media_target = target.join("media");
    let staged_db = stage.join("library.sqlite");
    let promoted = (|| -> Result<()> {
        if staged_db.exists() { fs::rename(&staged_db, &db_target)?; }
        if let Err(error) = fs::rename(stage.join("media"), &media_target) {
            if db_target.exists() { let _ = fs::remove_file(&db_target); }
            return Err(error.into());
        }
        choice.directory = next;
        choice.pending_directory = None;
        choice.pending_reset = false;
        choice.error = None;
        if let Err(error) = write_choice(root, choice) {
            let _ = fs::remove_file(&db_target);
            let _ = fs::remove_dir_all(&media_target);
            return Err(error);
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(&stage);
    promoted?;

    // From this moment the new directory is authoritative. Cleanup failures
    // only leave recoverable duplicates, never an empty current library.
    if sqlite.exists() {
        if let Err(error) = fs::remove_file(&sqlite) {
            eprintln!("[music location] old catalogue remains: {error}");
        }
        for suffix in ["-wal", "-shm"] {
            let journal = source.join(format!("library.sqlite{suffix}"));
            if journal.exists() {
                if let Err(error) = fs::remove_file(journal) {
                    eprintln!("[music location] old SQLite journal remains: {error}");
                }
            }
        }
    }
    if source.join("media").exists() {
        if let Err(error) = fs::remove_dir_all(source.join("media")) {
            eprintln!("[music location] old media remains: {error}");
        }
    }
    Ok(())
}

/// Called *before* the library is opened and while no songs can be written.
pub fn activate(root: &Path) -> Result<PathBuf> {
    fs::create_dir_all(root)?;
    let mut choice = read_choice(root)?;
    let old = chosen(root, choice.directory.as_ref());
    if has_pending(&choice) {
        let next = if choice.pending_reset { None } else { choice.pending_directory.clone() };
        let target = chosen(root, next.as_ref());
        if let Err(error) = migrate(&old, &target, root, &mut choice, next) {
            let reason = format!("{error:#}");
            eprintln!("[music location] move cancelled, original library kept: {reason}");
            choice = read_choice(root)?;
            choice.error = Some(reason);
            write_choice(root, &choice)?;
        }
    }
    let directory = chosen(root, choice.directory.as_ref());
    if choice.directory.is_some() && !directory.join("library.sqlite").is_file() {
        bail!("the chosen music catalogue is unavailable: {}", directory.display());
    }
    Ok(directory)
}

fn error(status: StatusCode, problem: impl ToString) -> (StatusCode, Json<ApiError>) {
    api_error(status, problem.to_string())
}

fn data_root() -> Result<PathBuf, (StatusCode, Json<ApiError>)> {
    crate::studio_data_root().ok_or_else(|| error(StatusCode::INTERNAL_SERVER_ERROR, "studio data directory is unavailable"))
}

pub async fn status() -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let root = data_root()?;
    let choice = read_choice(&root).map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let selected = chosen(&root, choice.directory.as_ref());
    let pending = if has_pending(&choice) {
        Some(chosen(&root, if choice.pending_reset { None } else { choice.pending_directory.as_ref() }))
    } else { None };
    Ok(Json(json!({
        "directory": selected.display().to_string(),
        "default_directory": root.display().to_string(),
        "is_default": choice.directory.is_none(),
        "pending_directory": pending.map(|p| p.display().to_string()),
        "restart_required": has_pending(&choice),
        "error": choice.error,
    })))
}

/// Windows native folder picker, matching the models' existing folder chooser.
pub async fn pick() -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let picked = tokio::task::spawn_blocking(|| rfd::FileDialog::new()
        .set_title("Music library location").pick_folder())
        .await.map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let Some(path) = picked else { return Ok(Json(json!({ "picked": false }))); };
    let root = data_root()?;
    let choice = read_choice(&root).map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let current = chosen(&root, choice.directory.as_ref());
    let validated = validate_directory(&root, &path, &current).map_err(|e| error(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!({ "picked": true, "directory": validated.display().to_string() })))
}

#[derive(Deserialize)]
pub struct Change { pub directory: Option<String> }

/// Only stages a change. On restart the service copies the closed database and
/// files; no mid-playback move, no silent switch to a blank music library.
pub async fn change(Json(input): Json<Change>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let root = data_root()?;
    fs::create_dir_all(&root).map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let mut choice = read_choice(&root).map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let current = chosen(&root, choice.directory.as_ref());
    let next = match input.directory.as_deref().map(str::trim).filter(|path| !path.is_empty()) {
        Some(path) => Some(validate_directory(&root, Path::new(path), &current).map_err(|e| error(StatusCode::BAD_REQUEST, e))?),
        None => None,
    };
    let target = chosen(&root, next.as_ref());
    if target != current && (target.join("library.sqlite").exists() || target.join("media").exists()) {
        return Err(error(StatusCode::CONFLICT, "destination already has a music catalogue or media directory"));
    }
    choice.pending_directory = next.clone();
    choice.pending_reset = next.is_none() && target != current;
    if target == current {
        choice.pending_directory = None;
        choice.pending_reset = false;
    }
    choice.error = None;
    write_choice(&root, &choice).map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    status().await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tmp() -> PathBuf { std::env::temp_dir().join(format!("music-location-test-{}", uuid::Uuid::now_v7())) }
    fn sample(root: &Path) {
        fs::create_dir_all(root.join("media")).unwrap();
        let conn = rusqlite::Connection::open(root.join("library.sqlite")).unwrap();
        conn.execute_batch("CREATE TABLE playlists(id TEXT PRIMARY KEY, name TEXT); INSERT INTO playlists VALUES('album-1','Rainy Days');").unwrap();
        fs::write(root.join("media/song.wav"), b"recorded music").unwrap();
    }
    #[test]
    fn migrates_catalogue_album_records_and_media_on_restart() {
        let root = tmp();
        let dst = tmp();
        sample(&root);
        fs::create_dir_all(&dst).unwrap();
        let choice = Choice { pending_directory: Some(dst.clone()), ..Choice::default() };
        write_choice(&root, &choice).unwrap();
        assert_eq!(activate(&root).unwrap(), dst);
        let db = rusqlite::Connection::open(dst.join("library.sqlite")).unwrap();
        let album: String = db.query_row("SELECT name FROM playlists WHERE id='album-1'", [], |r| r.get(0)).unwrap();
        assert_eq!(album, "Rainy Days");
        assert_eq!(fs::read(dst.join("media/song.wav")).unwrap(), b"recorded music");
        assert!(!root.join("library.sqlite").exists());
        drop(db);
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(dst);
    }
    #[test]
    fn never_overwrites_an_existing_target_or_discards_the_original() {
        let root = tmp();
        let dst = tmp();
        sample(&root);
        fs::create_dir_all(dst.join("media")).unwrap();
        fs::write(dst.join("media/other.wav"), b"someone else's music").unwrap();
        write_choice(&root, &Choice { pending_directory: Some(dst.clone()), ..Choice::default() }).unwrap();
        assert_eq!(activate(&root).unwrap(), root);
        assert!(root.join("library.sqlite").exists());
        assert!(read_choice(&root).unwrap().error.is_some());
        assert_eq!(fs::read(dst.join("media/other.wav")).unwrap(), b"someone else's music");
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(dst);
    }
    #[test]
    fn reset_to_default_copies_existing_music_home() {
        let root = tmp();
        let dst = tmp();
        fs::create_dir_all(&root).unwrap();
        sample(&dst);
        write_choice(&root, &Choice { directory: Some(dst.clone()), pending_reset: true, ..Choice::default() }).unwrap();
        assert_eq!(activate(&root).unwrap(), root);
        assert_eq!(fs::read(root.join("media/song.wav")).unwrap(), b"recorded music");
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(dst);
    }
}
