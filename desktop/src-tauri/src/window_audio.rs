use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
pub struct WindowAudio(Mutex<Option<Arc<AtomicBool>>>);

fn main_window(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("Window audio capture is available only from the main Studio window".into())
    }
}

#[tauri::command]
pub fn audio_capture_windows(
    window: tauri::WebviewWindow,
) -> Result<Vec<serde_json::Value>, String> {
    main_window(&window)?;
    #[cfg(windows)]
    {
        Ok(native::windows()
            .into_iter()
            .map(|(id, _, title)| serde_json::json!({ "id": id.to_string(), "title": title }))
            .collect())
    }
    #[cfg(not(windows))]
    Err("Window audio capture is available on Windows only".into())
}

#[tauri::command]
pub async fn record_window_audio(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, WindowAudio>,
    window_id: String,
) -> Result<tauri::ipc::Response, String> {
    main_window(&window)?;
    let stopped = Arc::new(AtomicBool::new(false));
    {
        let mut active = state.0.lock().map_err(|error| error.to_string())?;
        if active.is_some() {
            return Err("A window is already being recorded".into());
        }
        *active = Some(stopped.clone());
    }
    let result = tokio::task::spawn_blocking(move || native_record(window_id, &stopped)).await;
    *state.0.lock().map_err(|error| error.to_string())? = None;
    result
        .map_err(|error| error.to_string())?
        .map(tauri::ipc::Response::new)
}

#[tauri::command]
pub fn stop_window_audio(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, WindowAudio>,
) -> Result<(), String> {
    main_window(&window)?;
    if let Some(stopped) = &*state.0.lock().map_err(|error| error.to_string())? {
        stopped.store(true, Ordering::Release);
    }
    Ok(())
}

#[cfg(not(windows))]
fn native_record(_: String, _: &AtomicBool) -> Result<Vec<u8>, String> {
    Err("Window audio capture is available on Windows only".into())
}

#[cfg(windows)]
fn native_record(window_id: String, stopped: &AtomicBool) -> Result<Vec<u8>, String> {
    let handle = window_id
        .parse::<usize>()
        .map_err(|_| "Invalid window identifier".to_owned())?;
    let pid = native::windows()
        .into_iter()
        .find_map(|(id, pid, _)| (id == handle).then_some(pid))
        .ok_or_else(|| "The selected window is no longer available".to_owned())?;
    crate::process_audio::capture(
        pid,
        stopped,
        std::time::Duration::from_secs(crate::process_audio::MAX_SECONDS),
    )
}

#[cfg(windows)]
mod native {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    };

    pub fn windows() -> Vec<(usize, u32, String)> {
        let mut windows: Vec<(usize, u32, String)> = Vec::new();
        // EnumWindows is synchronous. The pointer refers to our list only until it returns.
        unsafe {
            EnumWindows(
                Some(collect_window),
                (&mut windows as *mut Vec<(usize, u32, String)>) as LPARAM,
            )
        };
        windows.sort_by(|(_, _, first), (_, _, second)| {
            first.to_lowercase().cmp(&second.to_lowercase())
        });
        windows
    }

    unsafe extern "system" fn collect_window(window: HWND, output: LPARAM) -> i32 {
        let mut title = [0_u16; 512];
        let length = unsafe {
            if IsWindowVisible(window) == 0 {
                return 1;
            }
            GetWindowTextW(window, title.as_mut_ptr(), title.len() as i32)
        };
        if length <= 0 {
            return 1;
        }
        let title = String::from_utf16_lossy(&title[..length as usize]);
        if title.trim().is_empty() {
            return 1;
        }
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(window, &mut pid);
            if pid != 0 {
                (*(output as *mut Vec<(usize, u32, String)>)).push((window as usize, pid, title));
            }
        }
        1
    }
}
