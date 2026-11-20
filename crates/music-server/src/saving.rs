//! "Save as" for everything the window hands the user: a song, its stems, a
//! MIDI file, a lyric sheet, a score, a video.
//!
//! The window asks for a place first; the desktop shell shows Windows' own
//! Save dialog, starting in the folder chosen last. The service then writes
//! the file itself - one it serves, read from its own address, or bytes the
//! window made - and tells the window how far it has got, so the files panel
//! can show each one with its progress and a way to it in Explorer.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use axum::body::Body;
use axum::extract::Path;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;

use crate::{api_error, mcp, ApiError};

/// Shows the Save dialog: the suggested file name and the folder to start in;
/// the chosen path, or None when the user cancels.
pub type SaveDialog = Box<dyn Fn(&str, Option<PathBuf>) -> Option<PathBuf> + Send + Sync>;

static DIALOG: OnceLock<SaveDialog> = OnceLock::new();

/// Set by the desktop shell, which owns the windows a dialog belongs to.
pub fn set_save_dialog(dialog: SaveDialog) {
    let _ = DIALOG.set(dialog);
}

#[derive(Default)]
struct Saving {
    /// Places chosen and not yet written, by the id handed to the window.
    chosen: HashMap<String, PathBuf>,
    /// The folder of the last file saved, where the next dialog starts.
    last_folder: Option<PathBuf>,
    /// Files written: the only ones the window may ask Explorer to show.
    written: Vec<PathBuf>,
}

fn saving() -> &'static Mutex<Saving> {
    static SAVING: OnceLock<Mutex<Saving>> = OnceLock::new();
    SAVING.get_or_init(|| Mutex::new(Saving::default()))
}

#[derive(Debug, Deserialize)]
pub struct Destination {
    name: String,
}

/// Asks where to save `name`. Cancelling is an answer, not an error.
pub async fn choose(Json(request): Json<Destination>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let name: String = request.name.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { '_' } else { c }).collect();
    let name = name.trim().trim_end_matches(['.', ' ']).to_string();
    if name.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "a file name is needed".into()));
    }
    let start = saving().lock().expect("saving").last_folder.clone();
    let asked = name.clone();
    let chosen = tokio::task::spawn_blocking(move || DIALOG.get().map(|dialog| dialog(&asked, start)))
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("the Save dialog failed: {error}")))?;
    let Some(chosen) = chosen else {
        return Err(api_error(StatusCode::SERVICE_UNAVAILABLE, "this studio has no window to show a Save dialog in".into()));
    };
    let Some(path) = chosen else {
        return Ok(Json(json!({ "cancelled": true })));
    };
    let id = uuid::Uuid::now_v7().to_string();
    let mut state = saving().lock().expect("saving");
    state.last_folder = path.parent().map(PathBuf::from);
    state.chosen.insert(id.clone(), path.clone());
    Ok(Json(json!({ "id": id, "path": path.display().to_string(), "name": path.file_name().map(|name| name.to_string_lossy().to_string()) })))
}

fn tell(id: &str, fields: Value) {
    let mut event = json!({ "id": id });
    if let (Some(event), Value::Object(fields)) = (event.as_object_mut(), fields) {
        event.extend(fields);
    }
    mcp::tell_windows(json!({ "saving": event }));
}

/// What the window sends to be written: an address of this service, or the
/// bytes themselves.
#[derive(Debug, Deserialize)]
struct Served {
    url: String,
}

/// A path on this service from what the window holds: `/v1/...`, or the same
/// with this machine's address in front.
fn own_path(url: &str) -> Option<String> {
    if url.starts_with('/') && !url.starts_with("//") {
        return Some(url.to_string());
    }
    let parsed = reqwest::Url::parse(url).ok()?;
    let local = matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    (local && parsed.port() == Some(crate::listen_port())).then(|| match parsed.query() {
        Some(query) => format!("{}?{query}", parsed.path()),
        None => parsed.path().to_string(),
    })
}

/// Writes the file chosen as `id`: from the address in a JSON body, or from
/// the body's bytes.
pub async fn write(Path(id): Path<String>, headers: HeaderMap, body: Body) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let path = saving()
        .lock()
        .expect("saving")
        .chosen
        .remove(&id)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "no place was chosen for this file".into()))?;
    let json = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"));
    tell(&id, json!({ "path": path.display().to_string(), "state": "saving", "written": 0 }));
    let result = if json {
        let bytes = axum::body::to_bytes(body, 64 * 1024)
            .await
            .map_err(|error| api_error(StatusCode::BAD_REQUEST, format!("read the request: {error}")))?;
        let served: Served = serde_json::from_slice(&bytes).map_err(|error| api_error(StatusCode::BAD_REQUEST, format!("the request is not an address: {error}")))?;
        let Some(own) = own_path(&served.url) else {
            return Err(api_error(StatusCode::BAD_REQUEST, format!("{} is not a file of this studio", served.url)));
        };
        let response = crate::net::client()
            .get(format!("http://127.0.0.1:{}{own}", crate::listen_port()))
            .send()
            .await
            .and_then(|response| response.error_for_status());
        match response {
            Ok(response) => {
                let total = response.content_length();
                copy(&id, &path, response.bytes_stream().map(|chunk| chunk.map_err(std::io::Error::other)), total).await
            }
            Err(error) => Err(format!("the studio could not read {own}: {error}")),
        }
    } else {
        let total = headers
            .get(axum::http::header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok());
        copy(&id, &path, body.into_data_stream().map(|chunk| chunk.map_err(std::io::Error::other)), total).await
    };
    match result {
        Ok(written) => {
            {
                let mut state = saving().lock().expect("saving");
                state.written.retain(|file| file != &path);
                state.written.push(path.clone());
                let excess = state.written.len().saturating_sub(100);
                state.written.drain(..excess);
            }
            tell(&id, json!({ "state": "done", "written": written, "total": written }));
            Ok(Json(json!({ "path": path.display().to_string(), "written": written })))
        }
        Err(error) => {
            tell(&id, json!({ "state": "error", "error": error }));
            Err(api_error(StatusCode::INTERNAL_SERVER_ERROR, error))
        }
    }
}

/// Publish a complete export in one rename. Each save owns its temporary file,
/// even when two dialogs picked the same destination.
async fn copy<S>(id: &str, path: &std::path::Path, mut chunks: S, total: Option<u64>) -> Result<u64, String>
where
    S: futures_util::Stream<Item = std::io::Result<axum::body::Bytes>> + Unpin,
{
    let folder = path.parent().filter(|folder| !folder.as_os_str().is_empty()).unwrap_or_else(|| std::path::Path::new("."));
    let write_error = |error: std::io::Error| format!("{} cannot be written: {error}", path.display());
    let (file, part) = tempfile::NamedTempFile::new_in(folder).map_err(write_error)?.into_parts();
    let mut file = tokio::fs::File::from_std(file);
    let mut written = 0u64;
    let mut told = std::time::Instant::now();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|error| format!("the file stopped arriving: {error}"))?;
        file.write_all(&chunk).await.map_err(write_error)?;
        written += chunk.len() as u64;
        if told.elapsed() >= std::time::Duration::from_millis(250) {
            told = std::time::Instant::now();
            tell(id, json!({ "written": written, "total": total }));
        }
    }
    if let Some(expected) = total {
        if written != expected {
            return Err(format!("received {written} bytes of the expected {expected}"));
        }
    }
    file.flush().await.map_err(write_error)?;
    drop(file);
    part.persist(path).map_err(|error| format!("{} cannot be put in place: {error}", path.display()))?;
    Ok(written)
}

#[derive(Debug, Deserialize)]
pub struct Written {
    path: String,
}

/// The file manager, open on a file saved here, with the file selected.
pub async fn reveal(Json(file): Json<Written>) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    let path = PathBuf::from(&file.path);
    if !saving().lock().expect("saving").written.contains(&path) {
        return Err(api_error(StatusCode::NOT_FOUND, format!("{} is not a file saved here", path.display())));
    }
    tokio::task::spawn_blocking(move || show_in_file_manager(&path))
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("The file manager did not open: {error}")))?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(windows)]
fn show_in_file_manager(path: &std::path::Path) -> std::io::Result<()> {
    let mut explorer = std::process::Command::new("explorer.exe");
    // Explorer reads /select,"path" as one switch; quoting the whole of it
    // breaks the switch.
    std::os::windows::process::CommandExt::raw_arg(&mut explorer, format!("/select,\"{}\"", path.display()));
    music_engine::process_group::detach_from_group(&mut explorer).spawn().map(drop)
}

#[cfg(target_os = "macos")]
fn show_in_file_manager(path: &std::path::Path) -> std::io::Result<()> {
    std::process::Command::new("open").arg("-R").arg(path).spawn().map(drop)
}

/// The freedesktop file manager interface selects the file, as Explorer does;
/// where no file manager offers it, the folder opens without the selection.
#[cfg(all(not(windows), not(target_os = "macos")))]
fn show_in_file_manager(path: &std::path::Path) -> std::io::Result<()> {
    let shown = dub_core::proc::host_command("dbus-send")
        .args(["--session", "--print-reply", "--reply-timeout=3000", "--dest=org.freedesktop.FileManager1", "/org/freedesktop/FileManager1", "org.freedesktop.FileManager1.ShowItems"])
        .arg(format!("array:string:{}", file_uri(path)))
        .arg("string:")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if shown {
        return Ok(());
    }
    let folder = path.parent().unwrap_or(path);
    music_engine::process_group::detach_from_group(&mut dub_core::proc::host_command("xdg-open")).arg(folder).spawn().map(drop)
}

/// `file://` and the path with every byte outside the unreserved set escaped.
#[cfg(all(not(windows), not(target_os = "macos")))]
fn file_uri(path: &std::path::Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn concurrent_exports_to_one_path_each_publish_their_own_bytes() {
        use axum::body::Bytes;
        use futures_util::stream;
        let folder = tempfile::tempdir().unwrap();
        let target = folder.path().join("track.wav");
        let (started, first_chunk) = tokio::sync::oneshot::channel();
        let (resume, second_done) = tokio::sync::oneshot::channel();
        let slow = stream::iter([Ok(Bytes::from_static(b"first "))]).chain(stream::once(async {
            started.send(()).unwrap();
            second_done.await.unwrap();
            Ok(Bytes::from_static(b"take"))
        })).boxed();
        let fast = async {
            first_chunk.await.unwrap();
            let data = stream::iter([Ok(Bytes::from_static(b"second"))]);
            let result = copy("fast", &target, data, None).await;
            let visible = std::fs::read(&target);
            resume.send(()).unwrap();
            assert_eq!(result.unwrap(), 6);
            assert_eq!(visible.unwrap(), b"second");
        };
        let (written, ()) = tokio::join!(copy("slow", &target, slow, None), fast);
        assert_eq!(written.unwrap(), 10);
        assert_eq!(std::fs::read(&target).unwrap(), b"first take");
    }

    #[tokio::test]
    async fn an_incomplete_export_preserves_the_previous_file_and_its_neighbours() {
        use axum::body::Bytes;
        let folder = tempfile::tempdir().unwrap();
        let target = folder.path().join("дубль.wav");
        let neighbour = folder.path().join("дубль.wav.part");
        std::fs::write(&target, b"original").unwrap();
        std::fs::write(&neighbour, b"another file").unwrap();
        let chunks = || futures_util::stream::iter([Ok(Bytes::from_static(b"take"))]);
        assert!(copy("short", &target, chunks(), Some(8)).await.is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"original");
        assert_eq!(copy("complete", &target, chunks(), Some(4)).await.unwrap(), 4);
        assert_eq!(std::fs::read(&target).unwrap(), b"take");
        assert_eq!(std::fs::read(&neighbour).unwrap(), b"another file");
        assert_eq!(folder.path().read_dir().unwrap().count(), 2);
    }

    #[test]
    fn only_this_studios_own_files_are_read() {
        let port = crate::listen_port();
        assert_eq!(own_path("/v1/library/songs/a/midi/file").as_deref(), Some("/v1/library/songs/a/midi/file"));
        assert_eq!(own_path(&format!("http://127.0.0.1:{port}/media/a.mp3?v=2")).as_deref(), Some("/media/a.mp3?v=2"));
        assert_eq!(own_path("https://example.com/a.mp3"), None);
        assert_eq!(own_path(&format!("http://127.0.0.1:{}/a", port.wrapping_add(1))), None);
        assert_eq!(own_path("//example.com/a"), None);
    }
}
