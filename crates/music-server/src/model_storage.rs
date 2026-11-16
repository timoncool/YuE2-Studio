//! Where the studio's downloads are kept: the model sets, the LoRAs, the stem
//! separator, karaoke, the audio.cpp runtime, the assistant's models, the
//! trainer and Audio to MIDI. They live in the studio's data folder; someone
//! whose disk is small moves them to another one.
//!
//! A moved folder leaves a link where it was (a junction on Windows, a symlink
//! elsewhere), so every path of the studio stays what it was: the engine, the
//! install records, the update screen and the other programs that take models
//! from this one (CONTRACT.md) read through the link and need to know nothing.
//! What a person made - the library, datasets, training runs - is never moved.
//!
//! A folder is copied whole before anything is taken away: on the same disk it
//! is renamed, on another it is copied beside its new name, checked file by
//! file and only then published; the old copy goes last. A move that is
//! stopped or fails leaves the folder where it was.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{anyhow, bail, Context, Result};
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{api_error, ApiError, AppState};

/// The folders of the data folder that hold downloads, in the order they are shown and moved.
pub const FOLDERS: &[&str] = &["models", "adapters", "separation", "karaoke", "audiocpp", "assistant", "hotstep", "midi", "training/models", "training/runtime"];

/// The folder made inside the one a person picks, so the studio's folders never mix with what is there.
const HOME: &str = "YuE2 Studio models";

const STAGED: &str = ".arriving";
const LEAVING: &str = ".leaving";

/// A download writes its `.part` every few seconds; one touched this recently is still running.
const PART_FRESH: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FolderUse {
    pub name: &'static str,
    pub bytes: u64,
    /// Kept outside the data folder, behind a link.
    pub elsewhere: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Storage {
    /// Where the downloads are now: the folder chosen, or the data folder.
    pub directory: String,
    /// The folder a person chose; none while everything is in the data folder.
    pub chosen: Option<String>,
    pub default: String,
    pub free_bytes: u64,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub folders: Vec<FolderUse>,
}

/// What one folder's move came to.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Moved {
    pub name: &'static str,
    pub bytes: u64,
    /// Why it stayed where it was; none when it moved.
    pub error: Option<String>,
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().map(|name| name.to_os_string()).unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

/// Where a link leads, when `path` is one.
pub fn link_target(path: &Path) -> Option<PathBuf> {
    let meta = fs::symlink_metadata(path).ok()?;
    #[cfg(windows)]
    {
        if junction::exists(path).unwrap_or(false) {
            return junction::get_target(path).ok();
        }
    }
    meta.file_type().is_symlink().then(|| fs::read_link(path).ok()).flatten()
}

fn make_link(target: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        junction::create(target, link)
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(target, link)
    }
}

fn remove_link(link: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        fs::remove_dir(link)
    }
    #[cfg(not(windows))]
    {
        fs::remove_file(link)
    }
}

/// Every file under `root`, added up; a link inside is not followed.
pub fn tree_bytes(root: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(root) else { return 0 };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => tree_bytes(&entry.path()),
            Ok(kind) if kind.is_file() => entry.metadata().map(|meta| meta.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

fn space(path: &Path) -> (u64, u64) {
    let existing = path.ancestors().find(|candidate| candidate.exists()).unwrap_or(path);
    (fs2::available_space(existing).unwrap_or(0), fs2::total_space(existing).unwrap_or(0))
}

/// The folder the downloads go into when a person picks `picked`: a folder of the studio's own inside it.
pub fn home_in(picked: &Path) -> PathBuf {
    if picked.file_name().is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(HOME)) {
        picked.to_path_buf()
    } else {
        picked.join(HOME)
    }
}

/// The folder a moved `name` sits in, from where its link leads: `<chosen>/training/models` gives `<chosen>`.
fn chosen_of(name: &str, target: &Path) -> Option<PathBuf> {
    let mut chosen = target;
    for _ in name.split('/') {
        chosen = chosen.parent()?;
    }
    Some(chosen.to_path_buf())
}

pub fn storage(data_root: &Path) -> Storage {
    let mut chosen = None;
    let folders: Vec<FolderUse> = FOLDERS
        .iter()
        .map(|name| {
            let path = data_root.join(name);
            let target = link_target(&path);
            if chosen.is_none() {
                chosen = target.as_deref().and_then(|target| chosen_of(name, target));
            }
            FolderUse { name, bytes: tree_bytes(target.as_deref().unwrap_or(&path)), elsewhere: target.is_some() }
        })
        .collect();
    let directory = chosen.clone().unwrap_or_else(|| data_root.to_path_buf());
    let (free_bytes, total_bytes) = space(&directory);
    Storage {
        directory: directory.display().to_string(),
        chosen: chosen.map(|path| path.display().to_string()),
        default: data_root.display().to_string(),
        free_bytes,
        total_bytes,
        used_bytes: folders.iter().map(|folder| folder.bytes).sum(),
        folders,
    }
}

/// A download is being written into one of the folders right now.
pub fn download_running(data_root: &Path) -> bool {
    fn fresh_part(dir: &Path, depth: usize) -> bool {
        let Ok(entries) = fs::read_dir(dir) else { return false };
        entries.flatten().any(|entry| {
            let path = entry.path();
            if path.is_dir() {
                return depth > 0 && fresh_part(&path, depth - 1);
            }
            path.extension().is_some_and(|extension| extension == "part")
                && entry.metadata().ok().and_then(|meta| meta.modified().ok()).and_then(|at| at.elapsed().ok()).is_some_and(|age| age < PART_FRESH)
        })
    }
    FOLDERS.iter().any(|name| fresh_part(&data_root.join(name), 6))
}

/// What is asked of a folder picked for the downloads, before anything moves: it is a full path outside the data
/// folder, it can be written to, and its disk has room for what will be copied there.
pub fn check_place(data_root: &Path, picked: &Path) -> Result<PathBuf> {
    if !picked.is_absolute() {
        bail!("the folder must be a full path");
    }
    let home = home_in(picked);
    let inside = |path: &Path, root: &Path| fs::canonicalize(root).ok().is_some_and(|root| fs::canonicalize(path.ancestors().find(|p| p.exists()).unwrap_or(path)).is_ok_and(|path| path.starts_with(root)));
    if inside(&home, data_root) {
        bail!("that folder is inside the studio's own data folder");
    }
    fs::create_dir_all(&home).with_context(|| format!("create {}", home.display()))?;
    let probe = home.join(".studio-write-probe");
    fs::write(&probe, b"probe").with_context(|| format!("write into {}", home.display()))?;
    let _ = fs::remove_file(&probe);
    Ok(home)
}

/// Copies the tree at `source` to `staging`, file by file, telling how far it is; a link inside refuses the whole tree.
fn copy_tree(source: &Path, staging: &Path, cancel: &AtomicBool, done: &mut u64, tell: &mut dyn FnMut(u64)) -> Result<()> {
    fs::create_dir_all(staging).with_context(|| format!("create {}", staging.display()))?;
    for entry in fs::read_dir(source).with_context(|| format!("read {}", source.display()))? {
        let entry = entry?;
        let (from, to) = (entry.path(), staging.join(entry.file_name()));
        let kind = entry.file_type()?;
        if kind.is_symlink() || link_target(&from).is_some() {
            bail!("{} is a link to another place; a folder with one is not moved", from.display());
        }
        if kind.is_dir() {
            copy_tree(&from, &to, cancel, done, tell)?;
            continue;
        }
        let mut reader = fs::File::open(&from).with_context(|| format!("open {}", from.display()))?;
        let mut writer = fs::File::create(&to).with_context(|| format!("create {}", to.display()))?;
        let mut buffer = vec![0u8; 4 << 20];
        loop {
            if cancel.load(Ordering::Relaxed) {
                bail!("cancelled");
            }
            let read = reader.read(&mut buffer).with_context(|| format!("read {}", from.display()))?;
            if read == 0 {
                break;
            }
            writer.write_all(&buffer[..read]).with_context(|| format!("write {}", to.display()))?;
            *done += read as u64;
            tell(*done);
        }
        writer.sync_all().with_context(|| format!("write {}", to.display()))?;
        let (written, wanted) = (fs::metadata(&to)?.len(), entry.metadata()?.len());
        if written != wanted {
            bail!("{} was copied as {written} bytes, not {wanted}", to.display());
        }
    }
    Ok(())
}

/// Puts the tree at `source` at `target`. Answers whether `source` is still there: it is gone when a rename was
/// enough (the same disk), and left untouched when the tree had to be copied.
fn carry(source: &Path, target: &Path, cancel: &AtomicBool, tell: &mut dyn FnMut(u64)) -> Result<bool> {
    if target.exists() {
        if fs::read_dir(target).map(|mut entries| entries.next().is_some()).unwrap_or(true) {
            bail!("{} is already there and is not empty: a move that was cut off left it, or another studio keeps its models there. Remove it or pick another folder", target.display());
        }
        fs::remove_dir(target).with_context(|| format!("replace the empty {}", target.display()))?;
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    if fs::rename(source, target).is_ok() {
        return Ok(false);
    }
    let staging = sibling(target, STAGED);
    if staging.exists() {
        fs::remove_dir_all(&staging).with_context(|| format!("clear what an earlier move left at {}", staging.display()))?;
    }
    let mut done = 0;
    if let Err(error) = copy_tree(source, &staging, cancel, &mut done, tell) {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    fs::rename(&staging, target).with_context(|| format!("publish {}", target.display()))?;
    Ok(true)
}

/// Moves one folder of the data folder into `home` and leaves a link where it was.
fn move_out(data_root: &Path, name: &str, home: &Path, cancel: &AtomicBool, tell: &mut dyn FnMut(u64)) -> Result<()> {
    let place = data_root.join(name);
    let target = home.join(name);
    let copied = carry(&place, &target, cancel, tell)?;
    let aside = sibling(&place, LEAVING);
    let linked = (|| -> Result<()> {
        if copied {
            fs::rename(&place, &aside).with_context(|| format!("a file in {} is in use: close what uses it, or restart the studio, and move again", place.display()))?;
        }
        make_link(&target, &place).with_context(|| format!("link {} to {}", place.display(), target.display()))
    })();
    if let Err(error) = linked {
        // back to how it was: the folder in the data folder, nothing at the new place
        if copied {
            if aside.exists() && !place.exists() {
                let _ = fs::rename(&aside, &place);
            }
            let _ = fs::remove_dir_all(&target);
        } else {
            let _ = fs::rename(&target, &place);
        }
        return Err(error);
    }
    if copied {
        fs::remove_dir_all(&aside).with_context(|| format!("the folder moved, but its old copy at {} could not be removed; it is removed when the studio starts", aside.display()))?;
    }
    Ok(())
}

/// Brings a moved folder back into the data folder, where its link was. The link stays until the folder is whole
/// beside it, so a move that is cut off leaves the studio reading the folder where it has been.
fn move_back(data_root: &Path, name: &str, target: &Path, cancel: &AtomicBool, tell: &mut dyn FnMut(u64)) -> Result<()> {
    let place = data_root.join(name);
    let arriving = sibling(&place, STAGED);
    if arriving.exists() {
        fs::remove_dir_all(&arriving).with_context(|| format!("clear what an earlier move left at {}", arriving.display()))?;
    }
    let renamed = fs::rename(target, &arriving).is_ok();
    if !renamed {
        let mut done = 0;
        if let Err(error) = copy_tree(target, &arriving, cancel, &mut done, tell) {
            let _ = fs::remove_dir_all(&arriving);
            return Err(error);
        }
    }
    let swapped = remove_link(&place)
        .with_context(|| format!("remove the link at {}", place.display()))
        .and_then(|()| fs::rename(&arriving, &place).with_context(|| format!("put the folder back at {}", place.display())));
    if let Err(error) = swapped {
        if renamed {
            let _ = fs::rename(&arriving, target);
        } else {
            let _ = fs::remove_dir_all(&arriving);
        }
        if link_target(&place).is_none() && !place.exists() {
            let _ = make_link(target, &place);
        }
        return Err(error);
    }
    if !renamed {
        fs::remove_dir_all(target).with_context(|| format!("the folder is back, but its copy at {} could not be removed", target.display()))?;
    }
    Ok(())
}


/// Moves a folder kept at `old` straight to `home` and points its link there, without a copy through the data
/// folder's disk. The link leads to `old` until the folder is whole at the new place, so a move that is cut off or
/// fails leaves the studio reading the folder where it has been.
fn move_across(data_root: &Path, name: &str, old: &Path, home: &Path, cancel: &AtomicBool, tell: &mut dyn FnMut(u64)) -> Result<()> {
    let place = data_root.join(name);
    let target = home.join(name);
    let copied = carry(old, &target, cancel, tell)?;
    let relinked = remove_link(&place)
        .with_context(|| format!("remove the link at {}", place.display()))
        .and_then(|()| make_link(&target, &place).with_context(|| format!("link {} to {}", place.display(), target.display())));
    if let Err(error) = relinked {
        if copied {
            let _ = fs::remove_dir_all(&target);
        } else {
            let _ = fs::rename(&target, old);
        }
        if link_target(&place).is_none() && !place.exists() {
            let _ = make_link(old, &place);
        }
        return Err(error);
    }
    if copied {
        fs::remove_dir_all(old).with_context(|| format!("the folder moved, but its old copy at {} could not be removed", old.display()))?;
    }
    Ok(())
}

/// Moves the download folders into `home`, or back into the data folder when `home` is none. A folder that cannot
/// move stays where it is and says why; the others still move. `tell` hears the folder at work, the bytes copied of
/// it and its size.
pub fn relocate(data_root: &Path, home: Option<&Path>, cancel: &AtomicBool, mut tell: impl FnMut(&'static str, u64, u64)) -> Vec<Moved> {
    let mut moved = Vec::new();
    for name in FOLDERS {
        let place = data_root.join(name);
        let linked = link_target(&place);
        let real = linked.clone().unwrap_or_else(|| place.clone());
        let already = match home {
            Some(home) => linked.as_deref() == Some(home.join(name).as_path()),
            None => linked.is_none(),
        };
        if already || !real.is_dir() {
            continue;
        }
        if cancel.load(Ordering::Relaxed) {
            moved.push(Moved { name, bytes: 0, error: Some("cancelled".into()) });
            continue;
        }
        let bytes = tree_bytes(&real);
        tell(name, 0, bytes);
        let mut progress = |done: u64| tell(name, done, bytes);
        let outcome = match (home, &linked) {
            (Some(home), None) => move_out(data_root, name, home, cancel, &mut progress),
            (Some(home), Some(target)) => move_across(data_root, name, target, home, cancel, &mut progress),
            (None, Some(target)) => move_back(data_root, name, target, cancel, &mut progress),
            (None, None) => Ok(()),
        };
        moved.push(Moved { name, bytes, error: outcome.err().map(|error| format!("{error:#}")) });
    }
    moved
}

/// Sets right what an interrupted move left in the data folder: the old copy of a folder that had already moved is
/// removed, an unfinished copy of one coming back is removed, and a folder that was itself on its way back when the
/// move was cut off comes home.
pub fn sweep(data_root: &Path) {
    for name in FOLDERS {
        let place = data_root.join(name);
        let aside = sibling(&place, LEAVING);
        if aside.is_dir() && link_target(&place).is_some() {
            let _ = fs::remove_dir_all(&aside);
        } else if aside.is_dir() && !place.exists() {
            // cut off after the folder was set aside and before its link was made: the folder comes back
            let _ = fs::rename(&aside, &place);
        }
        let arriving = sibling(&place, STAGED);
        if !arriving.is_dir() {
            continue;
        }
        if place.exists() {
            let _ = fs::remove_dir_all(&arriving);
        } else {
            if link_target(&place).is_some() {
                let _ = remove_link(&place);
            }
            let _ = fs::rename(&arriving, &place);
        }
    }
}

/// How much of the folders would have to be copied to `home`: nothing for those on the same disk.
pub fn bytes_to_copy(data_root: &Path, home: &Path) -> u64 {
    let same_disk = |a: &Path, b: &Path| {
        let root = |path: &Path| fs::canonicalize(path.ancestors().find(|p| p.exists()).unwrap_or(path)).ok().and_then(|path| path.components().next().map(|c| c.as_os_str().to_os_string()));
        cfg!(windows) && root(a).is_some() && root(a) == root(b)
    };
    FOLDERS
        .iter()
        .map(|name| {
            let place = data_root.join(name);
            let real = link_target(&place).unwrap_or(place);
            if same_disk(&real, home) { 0 } else { tree_bytes(&real) }
        })
        .sum()
}

pub fn room_for(data_root: &Path, home: &Path) -> Result<()> {
    let need = bytes_to_copy(data_root, home);
    let (free, _) = space(home);
    if free > 0 && need > free {
        return Err(anyhow!("not enough room there: {} MB to copy, {} MB free", need >> 20, free >> 20));
    }
    Ok(())
}

// ---- the move, asked for by the window or an agent ----

/// Where a move stands, for the window to poll.
#[derive(Debug, Clone, Serialize, Default)]
pub struct Move {
    pub running: bool,
    /// The folder at work.
    pub folder: Option<&'static str>,
    pub done_bytes: u64,
    pub total_bytes: u64,
    /// What each folder came to, once the move has ended.
    pub moved: Vec<Moved>,
    /// Why the engine did not start again afterwards.
    pub engine_error: Option<String>,
}

fn job() -> &'static std::sync::Mutex<(Move, std::sync::Arc<AtomicBool>)> {
    static JOB: std::sync::OnceLock<std::sync::Mutex<(Move, std::sync::Arc<AtomicBool>)>> = std::sync::OnceLock::new();
    JOB.get_or_init(Default::default)
}

fn job_now() -> Move {
    job().lock().unwrap_or_else(std::sync::PoisonError::into_inner).0.clone()
}

fn data_root() -> Result<PathBuf, (StatusCode, Json<ApiError>)> {
    crate::studio_data_root().ok_or_else(|| api_error(StatusCode::INTERNAL_SERVER_ERROR, "the studio has no data folder".into()))
}

/// Where the downloads are, how much room is there, and a move in progress.
pub async fn status() -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let root = data_root()?;
    let storage = tokio::task::spawn_blocking(move || storage(&root)).await.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(json!({ "storage": storage, "move": job_now() })))
}

#[derive(Debug, Deserialize, Default)]
pub struct Named {
    /// A folder an agent names; the window leaves it out and the person picks one in a dialog.
    #[serde(default)]
    pub path: Option<String>,
}

/// A folder for the downloads, picked in a dialog or named: where the studio's folders would go inside it, the room
/// on its disk and how much would be copied there. Nothing moves.
pub async fn pick(body: axum::body::Bytes) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let root = data_root()?;
    let named = serde_json::from_slice::<Named>(&body).unwrap_or_default().path.map(PathBuf::from);
    let picked = match named {
        Some(folder) => Some(folder),
        None => tokio::task::spawn_blocking(|| rfd::FileDialog::new().set_title("Folder for the studio's models").pick_folder())
            .await
            .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?,
    };
    let Some(picked) = picked else { return Ok(Json(json!({ "picked": false }))) };
    if !picked.is_absolute() {
        return Err(api_error(StatusCode::BAD_REQUEST, "the folder must be a full path".into()));
    }
    let home = home_in(&picked);
    let (free_bytes, total_bytes) = space(&home);
    let copy_bytes = { let (root, home) = (root.clone(), home.clone()); tokio::task::spawn_blocking(move || bytes_to_copy(&root, &home)).await.unwrap_or(0) };
    Ok(Json(json!({ "picked": true, "path": picked.display().to_string(), "directory": home.display().to_string(), "free_bytes": free_bytes, "total_bytes": total_bytes, "copy_bytes": copy_bytes })))
}

#[derive(Debug, Deserialize)]
pub struct Change {
    /// The folder to keep the downloads in; null brings them back into the studio's data folder.
    pub directory: Option<String>,
}

/// Starts the move. The engine and the writing assistant are stopped for it, since they hold the files open, and the
/// engine starts again when it ends; nothing else may be at work.
pub async fn change(State(state): State<AppState>, Json(change): Json<Change>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let root = data_root()?;
    if job_now().running {
        return Err(api_error(StatusCode::CONFLICT, "the models are being moved already".into()));
    }
    let at_work = crate::mcp::busy(&crate::mcp::status_summary().await);
    if !at_work.is_empty() {
        return Err(api_error(StatusCode::CONFLICT, format!("the studio is at work ({}); move the models when it is done", at_work.join(", "))));
    }
    if crate::chunked::downloading() || download_running(&root) {
        return Err(api_error(StatusCode::CONFLICT, "a download is running; move the models when it is done or cancelled".into()));
    }
    let home = match change.directory.as_deref().map(str::trim).filter(|directory| !directory.is_empty()) {
        Some(directory) => {
            let home = check_place(&root, Path::new(directory)).map_err(|error| api_error(StatusCode::BAD_REQUEST, format!("{error:#}")))?;
            room_for(&root, &home).map_err(|error| api_error(StatusCode::BAD_REQUEST, format!("{error:#}")))?;
            Some(home)
        }
        None => None,
    };
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    {
        let mut slot = job().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if slot.0.running {
            return Err(api_error(StatusCode::CONFLICT, "the models are being moved already".into()));
        }
        *slot = (Move { running: true, ..Move::default() }, cancel.clone());
    }
    tokio::spawn(async move {
        let _held = state.engine_use.write().await;
        let engine_was_up = crate::stop_engine_for_files(&state).await;
        let work = tokio::task::spawn_blocking(move || {
            relocate(&root, home.as_deref(), &cancel, |folder, done, total| {
                let mut job = job().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                job.0.folder = Some(folder);
                job.0.done_bytes = done;
                job.0.total_bytes = total;
            })
        })
        .await;
        let engine_error = if engine_was_up { crate::restart_engine(&state).await.err() } else { None };
        let mut job = job().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        job.0 = Move { running: false, moved: work.unwrap_or_default(), engine_error, ..Move::default() };
    });
    Ok(Json(json!({ "move": job_now() })))
}

/// Stops the move after the file being copied: the folder at work stays where it was, those already moved stay moved.
pub async fn cancel() -> Json<Value> {
    job().lock().unwrap_or_else(std::sync::PoisonError::into_inner).1.store(true, Ordering::Relaxed);
    Json(json!({ "move": job_now() }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(label: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("model-storage-{label}-{}", uuid::Uuid::now_v7()));
        let data = root.join("data");
        fs::create_dir_all(data.join("models/yue2-cpp")).unwrap();
        fs::write(data.join("models/yue2-cpp/backbone.gguf"), b"weights").unwrap();
        fs::create_dir_all(data.join("training/models")).unwrap();
        fs::write(data.join("training/models/trainer.bin"), b"pack").unwrap();
        fs::create_dir_all(data.join("training/datasets/mine")).unwrap();
        fs::write(data.join("training/datasets/mine/song.wav"), b"the person's own").unwrap();
        fs::create_dir_all(data.join("media")).unwrap();
        fs::write(data.join("media/song.flac"), b"a song").unwrap();
        (root, data)
    }

    fn quiet() -> AtomicBool {
        AtomicBool::new(false)
    }

    #[test]
    fn the_downloads_move_out_behind_links_and_come_back() {
        let (root, data) = data("move");
        let home = check_place(&data, &root.join("big disk")).unwrap();
        assert_eq!(home, root.join("big disk").join(HOME));

        let moved = relocate(&data, Some(&home), &quiet(), |_, _, _| {});
        assert_eq!(moved, vec![Moved { name: "models", bytes: 7, error: None }, Moved { name: "training/models", bytes: 4, error: None }]);
        assert_eq!(link_target(&data.join("models")), Some(home.join("models")));
        assert_eq!(fs::read(home.join("models/yue2-cpp/backbone.gguf")).unwrap(), b"weights");
        // every path of the studio still reads the file
        assert_eq!(fs::read(data.join("models/yue2-cpp/backbone.gguf")).unwrap(), b"weights");
        assert_eq!(fs::read(data.join("training/models/trainer.bin")).unwrap(), b"pack");
        // what the person made stayed
        assert!(link_target(&data.join("training")).is_none());
        assert!(data.join("training/datasets/mine/song.wav").is_file() && data.join("media/song.flac").is_file());

        let now = storage(&data);
        assert_eq!(now.chosen.as_deref(), Some(home.display().to_string().as_str()));
        assert_eq!(now.used_bytes, 11);
        assert!(now.folders.iter().filter(|folder| folder.elsewhere).map(|folder| folder.name).eq(["models", "training/models"]));
        assert!(relocate(&data, Some(&home), &quiet(), |_, _, _| {}).is_empty(), "already there: nothing to do");

        let back = relocate(&data, None, &quiet(), |_, _, _| {});
        assert!(back.iter().all(|folder| folder.error.is_none()), "{back:?}");
        assert!(link_target(&data.join("models")).is_none());
        assert_eq!(fs::read(data.join("models/yue2-cpp/backbone.gguf")).unwrap(), b"weights");
        assert!(!home.join("models").exists());
        assert_eq!(storage(&data).chosen, None);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_tree_copied_to_another_disk_is_whole_or_not_there() {
        let (root, data) = data("copy");
        let staging = root.join("staging");
        let (mut done, mut heard) = (0, Vec::new());
        copy_tree(&data.join("models"), &staging, &quiet(), &mut done, &mut |bytes| heard.push(bytes)).unwrap();
        assert_eq!((done, heard.last().copied()), (7, Some(7)));
        assert_eq!(fs::read(staging.join("yue2-cpp/backbone.gguf")).unwrap(), b"weights");

        // a stop in the middle is an error, and the caller clears what was staged
        let stopped = AtomicBool::new(true);
        assert_eq!(copy_tree(&data.join("models"), &root.join("other"), &stopped, &mut 0, &mut |_| {}).unwrap_err().to_string(), "cancelled");

        // a link inside refuses the tree: its target is somebody else's to move
        make_link(&data.join("media"), &data.join("models/linked")).unwrap();
        let error = copy_tree(&data.join("models"), &root.join("third"), &quiet(), &mut 0, &mut |_| {}).unwrap_err().to_string();
        assert!(error.contains("is a link"), "{error}");
        remove_link(&data.join("models/linked")).unwrap();
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_folder_that_cannot_move_stays_and_the_rest_go() {
        let (root, data) = data("refused");
        let home = check_place(&data, &root.join("elsewhere")).unwrap();
        // something of another program's is already under that name
        fs::create_dir_all(home.join("models")).unwrap();
        fs::write(home.join("models/theirs.bin"), b"x").unwrap();
        let moved = relocate(&data, Some(&home), &quiet(), |_, _, _| {});
        assert!(moved[0].error.as_deref().is_some_and(|error| error.contains("already there")), "{moved:?}");
        assert!(link_target(&data.join("models")).is_none(), "the folder stayed");
        assert_eq!(fs::read(data.join("models/yue2-cpp/backbone.gguf")).unwrap(), b"weights");
        assert_eq!(moved[1], Moved { name: "training/models", bytes: 4, error: None });
        fs::remove_dir_all(root).ok();
    }


    #[test]
    fn a_return_cut_off_leaves_the_folder_where_it_is_read_and_the_next_start_sets_it_right() {
        let (root, data) = data("cut");
        let home = check_place(&data, &root.join("big disk")).unwrap();
        relocate(&data, Some(&home), &quiet(), |_, _, _| {});
        let arriving = sibling(&data.join("models"), STAGED);

        // cut off while copying back: the link still leads to the folder, an unfinished copy lies beside it
        fs::create_dir_all(arriving.join("yue2-cpp")).unwrap();
        assert_eq!(fs::read(data.join("models/yue2-cpp/backbone.gguf")).unwrap(), b"weights");
        sweep(&data);
        assert!(!arriving.exists());
        assert_eq!(link_target(&data.join("models")), Some(home.join("models")));

        // cut off after the folder itself was taken from the new place: it comes home
        fs::rename(home.join("models"), &arriving).unwrap();
        sweep(&data);
        assert!(link_target(&data.join("models")).is_none() && !arriving.exists());
        assert_eq!(fs::read(data.join("models/yue2-cpp/backbone.gguf")).unwrap(), b"weights");
        fs::remove_dir_all(root).ok();
    }


    #[test]
    fn a_folder_goes_from_one_chosen_place_straight_to_another() {
        let (root, data) = data("across");
        let first = check_place(&data, &root.join("disk one")).unwrap();
        let second = check_place(&data, &root.join("disk two")).unwrap();
        relocate(&data, Some(&first), &quiet(), |_, _, _| {});

        let moved = relocate(&data, Some(&second), &quiet(), |_, _, _| {});
        assert!(moved.iter().all(|folder| folder.error.is_none()), "{moved:?}");
        assert_eq!(link_target(&data.join("models")), Some(second.join("models")));
        assert_eq!(fs::read(data.join("models/yue2-cpp/backbone.gguf")).unwrap(), b"weights");
        assert!(!first.join("models").exists(), "nothing of it stays at the place it left");
        assert!(!sibling(&data.join("models"), STAGED).exists(), "and nothing passed through the data folder");
        assert_eq!(storage(&data).chosen.as_deref(), Some(second.display().to_string().as_str()));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_folder_set_aside_when_the_move_was_cut_off_comes_back_at_the_next_start() {
        let (root, data) = data("aside");
        let place = data.join("models");
        fs::rename(&place, sibling(&place, LEAVING)).unwrap();
        assert!(!place.exists());
        sweep(&data);
        assert!(link_target(&place).is_none());
        assert_eq!(fs::read(place.join("yue2-cpp/backbone.gguf")).unwrap(), b"weights");
        assert!(!sibling(&place, LEAVING).exists());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_place_is_checked_before_anything_moves() {
        let (root, data) = data("place");
        assert!(check_place(&data, Path::new("relative")).unwrap_err().to_string().contains("full path"));
        assert!(check_place(&data, &data.join("inside")).unwrap_err().to_string().contains("inside the studio"));
        // a folder already named for the studio is used as it is
        assert_eq!(check_place(&data, &root.join(HOME)).unwrap(), root.join(HOME));
        assert_eq!(chosen_of("training/models", &root.join("x/training/models")), Some(root.join("x")));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_download_in_progress_is_seen_by_its_part_file() {
        let (root, data) = data("download");
        assert!(!download_running(&data));
        fs::write(data.join("models/yue2-cpp/vae.gguf.part"), b"half").unwrap();
        assert!(download_running(&data));
        fs::remove_dir_all(root).ok();
    }
}
