//! Native song-completion notifications for the Windows *portable* studio.
//! The Tauri notification plugin requires an installed Windows application.
//! A Shell notification-area balloon is supported without a Start Menu shortcut.
#[tauri::command]
pub fn notify_song_ready(app: tauri::AppHandle, message: String) -> Result<(), String> {
    if message.trim().is_empty() {
        return Err("the notification message is empty".into());
    }
    #[cfg(windows)]
    {
        show_windows_balloon(&app, &message)
    }
    #[cfg(not(windows))]
    {
        let _ = (app, message);
        Err("Windows desktop notifications are unavailable on this platform".into())
    }
}

/// A terminating NUL and a length limit are required by NOTIFYICONDATAW.
/// Truncate at Unicode scalar boundaries, never in the middle of a surrogate pair.
#[cfg(windows)]
fn wide_field<const N: usize>(value: &str) -> [u16; N] {
    let mut output = [0u16; N];
    let mut cursor = 0;
    for ch in value.chars() {
        let mut pair = [0u16; 2];
        let encoded = ch.encode_utf16(&mut pair);
        if cursor + encoded.len() >= N {
            break;
        }
        output[cursor..cursor + encoded.len()].copy_from_slice(encoded);
        cursor += encoded.len();
    }
    output
}

#[cfg(windows)]
fn show_windows_balloon(app: &tauri::AppHandle, message: &str) -> Result<(), String> {
    use std::{sync::Mutex, time::Duration};
    use tauri::Manager;
    use windows_sys::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD,
        NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{LoadIconW, IDI_APPLICATION, WM_APP};

    const ICON_ID: u32 = 0x59554532; // "YUE2"; separate from the optional keep-running tray icon
    static NOTIFICATION_GENERATION: Mutex<u64> = Mutex::new(0);
    let mut generation = NOTIFICATION_GENERATION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let window = app
        .get_webview_window("main")
        .ok_or("the main studio window is not available")?;
    let hwnd = window.hwnd().map_err(|error| error.to_string())?.0;

    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd as _;
    data.uID = ICON_ID;
    data.uFlags = NIF_ICON | NIF_TIP | NIF_MESSAGE;
    data.uCallbackMessage = WM_APP + 0x4A2;
    data.hIcon = unsafe { LoadIconW(std::ptr::null_mut(), IDI_APPLICATION) };
    data.szTip = wide_field("YuE2 Studio song notifications");
    if data.hIcon.is_null() {
        return Err("could not load the system notification icon".into());
    }

    // Adding an already-present icon can fail harmlessly; NIM_MODIFY still
    // reaches it. Trying NIM_ADD each time also heals an Explorer restart.
    unsafe { Shell_NotifyIconW(NIM_ADD, &data) };
    data.uFlags = NIF_INFO;
    data.szInfoTitle = wide_field("YuE2 Studio");
    data.szInfo = wide_field(message);
    data.dwInfoFlags = NIIF_INFO;
    if unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) } == 0 {
        unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
        return Err("Windows declined the notification".into());
    }
    *generation = generation.wrapping_add(1);
    let current = *generation;
    // Keep the icon alive while Windows displays the message. A newer
    // notification extends its lifetime and supersedes this cleanup.
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        let generation = NOTIFICATION_GENERATION
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if *generation == current {
            if let Ok(hwnd) = window.hwnd() {
                let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
                data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                data.hWnd = hwnd.0 as _;
                data.uID = ICON_ID;
                unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
            }
        }
    });
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::wide_field;
    #[test]
    fn notification_fields_are_terminated_and_preserve_unicode_characters() {
        let field = wide_field::<8>("ab🎵cd");
        assert_eq!(String::from_utf16_lossy(&field[..6]), "ab🎵cd");
        assert_eq!(field[6], 0);
        let truncated = wide_field::<5>("abcd🎵");
        assert_eq!(String::from_utf16_lossy(&truncated[..4]), "abcd");
    }
}
