//! Desktop shell for YuE2 Studio.
//!
//! The whole studio is this one executable: the window, and the native service
//! hosted inside it. The service in turn supervises the `yue2.cpp`
//! engine, so there is exactly one owner of that process and no launcher
//! script in the release layout. If a compatible service is already listening
//! on loopback — a developer running it separately — the shell uses it instead
//! of starting a second one.

use std::{
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const SERVER_PORT: u16 = 8791;
const RELEASES_URL: &str = "https://github.com/timoncool/YuE2-Studio/releases/latest";
const STUDIO_DATA_DIRECTORY: &str = "YuE2 Studio";

/// Mutable studio data must not be derived from the process working directory
/// or from the installed executable directory: a Start Menu shortcut is free
/// to choose either of those. Environment overrides remain authoritative for
/// portable/development installations.
fn studio_data_directory() -> PathBuf {
    // A portable copy keeps everything it owns beside itself: models, the
    // library, media, logs and settings. Nothing is written into AppData, so
    // deleting the folder deletes the studio, and carrying the folder to
    // another machine carries the whole studio with it.
    if is_portable() {
        return executable_directory().join("data");
    }

    // An installation is treated the same way whenever it can be: someone who
    // installs into F:\AI expects the twenty-five gigabytes of weights to land
    // in F:\AI, not in their profile on C:. Only when the install directory
    // cannot be written to - Program Files, a read-only share - does the studio
    // fall back to AppData, because then it has nowhere else to go.
    // On macOS the executable sits inside a signed .app bundle that must not
    // be written to, so an installation always uses Application Support.
    let beside_the_executable = executable_directory().join("data");
    if !cfg!(target_os = "macos") && directory_is_writable(&beside_the_executable) {
        return beside_the_executable;
    }

    #[cfg(windows)]
    {
        if let Some(root) = std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("APPDATA")) {
            return PathBuf::from(root).join(STUDIO_DATA_DIRECTORY);
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join("Library").join("Application Support").join(STUDIO_DATA_DIRECTORY);
        }
    }

    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        if let Some(root) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(root).join("yue2-studio");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("yue2-studio");
        }
    }

    std::env::temp_dir().join("yue2-studio")
}

/// A portable copy, or an installation into a folder it can write to: then the
/// data, the temporary files and the WebView2 profile all live beside the
/// executable, and deleting the folder deletes the studio.
fn keeps_everything_beside_itself() -> bool {
    studio_data_directory().starts_with(executable_directory())
}

/// Sets the same deterministic roots before the Axum bridge is spawned. Its
/// environment is inherited by the bridge and the subsequently started native
/// engine, so both always refer to one model library.
fn configure_studio_runtime_paths() {
    let data_root = studio_data_directory();

    // Temporary files count as leaving traces too: the engine, the downloader
    // and ffmpeg all write through the system temporary directory, and a
    // studio that keeps its data beside itself has no business filling the
    // system drive with them.
    if keeps_everything_beside_itself() {
        let temporary = executable_directory().join("temp");
        let _ = std::fs::create_dir_all(&temporary);
        for variable in ["TEMP", "TMP"] {
            unsafe {
                std::env::set_var(variable, &temporary);
            }
        }
    }
    if std::env::var_os("YUE_MODELS_ROOT").is_none() {
        unsafe {
            std::env::set_var(
                "YUE_MODELS_ROOT",
                data_root.join("models").join("yue2-cpp"),
            );
        }
    }
    if std::env::var_os("YUE_STUDIO_SETTINGS_PATH").is_none() {
        unsafe {
            std::env::set_var("YUE_STUDIO_SETTINGS_PATH", data_root.join("studio-settings.json"));
        }
    }
    // Without this the service falls back to `<working directory>/data` for the
    // library and media files. A Start Menu shortcut does not control the
    // working directory, so an installed build would scatter or lose the user's
    // library depending on how it was launched.
    if std::env::var_os("YUE_STUDIO_DATA_ROOT").is_none() {
        unsafe {
            std::env::set_var("YUE_STUDIO_DATA_ROOT", &data_root);
        }
    }

    // The service resolves and supervises the engine; the shell only needs the
    // loopback address they agree on.
    let host = std::env::var("YUE_ENGINE_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = music_engine::yue_server::engine_port();
    if std::env::var_os("YUE_ENGINE_BASE_URL").is_none() {
        unsafe {
            std::env::set_var("YUE_ENGINE_BASE_URL", format!("http://{host}:{port}"));
        }
    }
}

fn service_is_ready() -> bool {
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, SERVER_PORT);
    TcpStream::connect_timeout(&address.into(), Duration::from_millis(150)).is_ok()
}

fn wait_until_ready(timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if service_is_ready() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(120));
    }
    false
}

fn executable_directory() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn is_portable() -> bool {
    executable_directory().join("portable.flag").is_file()
}

/// Whether the studio may keep its data here.
///
/// Asked by creating the directory and writing to it, because that is the only
/// answer that matters: permissions on Windows are not something to reason
/// about from a path.
fn directory_is_writable(path: &std::path::Path) -> bool {
    if std::fs::create_dir_all(path).is_err() {
        return false;
    }
    let probe = path.join(".write-probe");
    let written = std::fs::write(&probe, b"1").is_ok();
    let _ = std::fs::remove_file(&probe);
    written
}

/// Hosts the studio service inside this process.
///
/// A release is one executable: there is no second binary to locate, no
/// launcher script, and nothing that can be left running if the window is
/// closed. The service is started on its own runtime thread and the window
/// only opens once it answers on loopback.
fn start_service() -> Result<(), String> {
    if service_is_ready() {
        return Ok(());
    }

    std::thread::Builder::new()
        .name("music-server".into())
        .spawn(|| {
            let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
                Ok(runtime) => runtime,
                Err(error) => {
                    eprintln!("could not create the studio service runtime: {error}");
                    return;
                }
            };
            // Closing the studio and opening it again leaves the previous
            // process holding the port for a moment. Giving up on the first
            // refusal left the window on a browser error page with no way back
            // except restarting the application.
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                match runtime.block_on(music_server::serve()) {
                    Ok(()) => return,
                    Err(error) if Instant::now() < deadline => {
                        eprintln!("studio service could not start yet: {error}");
                        std::thread::sleep(Duration::from_millis(400));
                    }
                    Err(error) => {
                        eprintln!("studio service stopped: {error}");
                        return;
                    }
                }
            }
        })
        .map_err(|error| format!("could not start the studio service thread: {error}"))?;

    if wait_until_ready(Duration::from_secs(30)) {
        Ok(())
    } else {
        Err(format!("the studio service did not become ready on 127.0.0.1:{SERVER_PORT}"))
    }
}

/// One request to the studio's own service: its body; `Err(true)` when it took
/// the request and gave nothing readable back, `Err(false)` when nothing listens.
fn service_call(method: &str, path: &str, wait: Duration) -> Result<String, bool> {
    use std::io::{Read, Write};
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, SERVER_PORT);
    let Ok(mut stream) = TcpStream::connect_timeout(&address.into(), Duration::from_millis(500)) else {
        return Err(false);
    };
    let _ = stream.set_read_timeout(Some(wait));
    let request = format!("{method} {path} HTTP/1.0\r\nHost: 127.0.0.1:{SERVER_PORT}\r\nContent-Length: 0\r\n\r\n");
    let mut response = String::new();
    if stream.write_all(request.as_bytes()).is_err() || stream.read_to_string(&mut response).is_err() {
        return Err(true);
    }
    response.split("\r\n\r\n").nth(1).map(str::to_owned).ok_or(true)
}

/// The songs queued or being made, by job id; `None` when the service could not say in `wait`.
fn songs_in_progress(wait: Duration) -> Option<Vec<String>> {
    match service_call("GET", "/v1/music/jobs", wait) {
        Ok(body) => serde_json::from_str::<Vec<serde_json::Value>>(&body)
            .ok()
            .map(|jobs| jobs.iter().filter_map(|job| job.get("id").and_then(serde_json::Value::as_str).map(str::to_owned)).collect()),
        Err(false) => Some(Vec::new()),
        Err(true) => None,
    }
}

/// What the shell asks the window and is still waiting on: the question about
/// quitting, and an update found but not answered. The window draws both in the
/// studio's own language and answers through the commands below.
#[derive(Default)]
struct Prompts {
    update: std::sync::Mutex<Option<tauri_plugin_updater::Update>>,
    quit_asked: std::sync::atomic::AtomicBool,
    quit_seen: std::sync::atomic::AtomicBool,
}

/// A portable copy is updated by hand: the window offers the download page instead.
fn update_offer(version: &str) -> serde_json::Value {
    serde_json::json!({ "version": version, "portable": is_portable(), "page": RELEASES_URL })
}

/// Quit, asking first only when a song is being generated. The window asks;
/// a window that never showed the question (frozen, still loading) does not
/// keep the studio open when the person closes it again.
fn confirm_quit(app: &tauri::AppHandle) {
    use std::sync::atomic::Ordering;
    use tauri::{Emitter, Manager};
    let running = songs_in_progress(Duration::from_secs(2));
    if running.as_ref().is_some_and(Vec::is_empty) {
        quit_now(app, Vec::new());
        return;
    }
    let prompts = app.state::<Prompts>();
    if prompts.quit_asked.load(Ordering::SeqCst) && !prompts.quit_seen.load(Ordering::SeqCst) {
        quit_now(app, running.unwrap_or_default());
        return;
    }
    prompts.quit_asked.store(true, Ordering::SeqCst);
    prompts.quit_seen.store(false, Ordering::SeqCst);
    if app.emit_to("main", "studio://quit-asked", ()).is_err() {
        quit_now(app, running.unwrap_or_default());
    }
}

/// A song the person agreed to stop is stopped before the studio goes, so the
/// next start does not make it again as one the studio was cut off on; the
/// day's statistics leave now rather than at the next start.
fn quit_now(app: &tauri::AppHandle, running: Vec<String>) {
    use tauri::Manager;
    // the person is done with the window; what follows can take seconds on a slow network
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    let running = if running.is_empty() { songs_in_progress(Duration::from_secs(15)).unwrap_or_default() } else { running };
    for id in running {
        let _ = service_call("POST", &format!("/v1/music/jobs/{id}"), Duration::from_secs(5));
    }
    let _ = service_call("POST", "/v1/hub/flush", Duration::from_secs(10));
    app.exit(0);
}

#[tauri::command]
fn quit_prompt_shown(prompts: tauri::State<'_, Prompts>) {
    prompts.quit_seen.store(true, std::sync::atomic::Ordering::SeqCst);
}

#[tauri::command]
fn quit_prompt_cancelled(prompts: tauri::State<'_, Prompts>) {
    prompts.quit_asked.store(false, std::sync::atomic::Ordering::SeqCst);
}

#[tauri::command]
fn quit_studio(app: tauri::AppHandle) {
    std::thread::spawn(move || quit_now(&app, Vec::new()));
}

const TRAY_ID: &str = "studio";
const TRAY_OPEN: &str = "tray-open";
const TRAY_QUIT: &str = "tray-quit";
/// The choices only the shell needs, beside window-state.json: the service never sees them.
const SHELL_SETTINGS_FILE: &str = "shell-settings.json";

/// Starts the studio for an agent with no window on screen. The window is still made - parts
/// of the studio run in its page - but hidden, and the studio stops by itself once nothing has
/// used it for a while.
const HEADLESS_FLAG: &str = "--headless";
/// Asks a running studio to quit when nobody is using its window. With nothing running it
/// does nothing and starts nothing.
const STOP_FLAG: &str = "--stop";
/// Ten minutes: long enough for an agent that thinks between calls, short
/// enough that a forgotten studio gives the graphics card back the same evening.
const DEFAULT_IDLE_STOP: Duration = Duration::from_secs(600);
/// Seconds instead of the ten minutes.
const IDLE_STOP_VARIABLE: &str = "YUE_STUDIO_IDLE_SHUTDOWN_S";
/// How often the idle watch looks.
const IDLE_LOOK: Duration = Duration::from_secs(15);

/// What a launch of the executable asks for.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Launch {
    headless: bool,
    stop: bool,
}

/// The flags anywhere on the command line. A second launch's arguments come over from the
/// single-instance plugin the same way, the executable first.
fn parse_launch<S: AsRef<str>>(arguments: &[S]) -> Launch {
    let has = |flag: &str| arguments.iter().any(|argument| argument.as_ref() == flag);
    Launch { headless: has(HEADLESS_FLAG), stop: has(STOP_FLAG) }
}

/// The idle period: the variable's seconds when it holds a positive number, ten minutes otherwise.
fn idle_period(raw: Option<&str>) -> Duration {
    raw.and_then(|text| text.trim().parse::<f64>().ok())
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
        .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
        .unwrap_or(DEFAULT_IDLE_STOP)
}

/// Who the running studio belongs to.
struct Session {
    /// Still the agent's: started with --headless and not opened since by a plain launch.
    /// Its tray icon stays and its close button hides, whatever the close-to-tray setting says.
    headless_owned: std::sync::atomic::AtomicBool,
    /// The last use the shell saw itself: the window on screen, a second --headless launch,
    /// work under way. The service keeps the agents' calls.
    last_use: std::sync::Mutex<Instant>,
}

impl Session {
    fn new(headless: bool) -> Self {
        Self { headless_owned: std::sync::atomic::AtomicBool::new(headless), last_use: std::sync::Mutex::new(Instant::now()) }
    }

    fn headless_owned(&self) -> bool {
        self.headless_owned.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn touch(&self) {
        *self.last_use.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Instant::now();
    }

    fn unused_for(&self) -> Duration {
        self.last_use.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).elapsed()
    }
}

/// What the main window's close button does.
#[derive(Debug, PartialEq, Eq)]
enum WindowClose {
    /// Ask about quitting, and quit.
    Quit,
    /// Hide the window; the service, the engine and running songs carry on.
    HideToTray,
}

/// The window goes to the tray when the person chose that, or while the studio is still the
/// agent's that started it headless - the agent is using it, and the window only came up from
/// the tray for a look. Either way only with the icon there to bring it back: a window with no
/// way back would be a studio nobody can reach. Explicit quits (tray Quit, Cmd+Q, the update
/// install, `--stop`, the idle stop) never come through the close button, so they never reach this.
fn decide_window_close(close_to_tray: bool, tray_present: bool, headless_owned: bool) -> WindowClose {
    if tray_present && (close_to_tray || headless_owned) { WindowClose::HideToTray } else { WindowClose::Quit }
}

/// The tray icon is there while the setting keeps it, and in a studio started headless, where
/// it is the only way to the window.
fn tray_wanted(close_to_tray: bool, headless_owned: bool) -> bool {
    close_to_tray || headless_owned
}

/// What `--stop` does with the running studio.
#[derive(Debug, PartialEq, Eq)]
enum TerminalStop {
    /// The orderly quit, with nothing to ask.
    Quit,
    /// Nothing, and nothing shown.
    Stay,
}

/// `--stop` never takes the studio from a person using its window, and never stops work under
/// way - a song, stems, training, a download: it would have to ask, and a question popping up
/// on screen is what a terminal command must not do. `working` is none when the service could
/// not say, which counts as working.
fn decide_terminal_stop(window_visible: bool, working: Option<bool>) -> TerminalStop {
    if !window_visible && working == Some(false) { TerminalStop::Quit } else { TerminalStop::Stay }
}

/// Whether the service in this process has anything at work; none when it could not say in time.
fn service_working() -> Option<bool> {
    music_server::work_in_progress(Duration::from_secs(5)).map(|work| !work.is_empty())
}

/// One look of the idle watch.
#[derive(Debug, PartialEq, Eq)]
enum IdleLook {
    /// The window is on screen or something is at work: the studio counts as used now.
    InUse,
    /// Unused, not for long enough yet.
    Quiet,
    /// Unused for the whole idle period: stop.
    Stop,
}

/// One look of the idle watch of a studio still headless-owned: a window brought up from the
/// tray and hidden again leaves it to the clock once more. `working` is what the service says
/// is at work; none when it could not say, which is never idle - a long step can keep it from
/// answering. `unused_for` is the shorter of the shell's own clock and the time since an
/// agent's last call.
fn look_for_idleness(window_visible: bool, working: Option<bool>, unused_for: Duration, limit: Duration) -> IdleLook {
    if window_visible || working != Some(false) {
        IdleLook::InUse
    } else if unused_for >= limit {
        IdleLook::Stop
    } else {
        IdleLook::Quiet
    }
}

/// Off unless the person turned it on; a missing or unreadable file is off.
fn read_close_to_tray(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok())
        .is_some_and(|saved| saved.get("closeToTray").and_then(serde_json::Value::as_bool) == Some(true))
}

/// Fails when the choice cannot be written, so the window never shows a value that was not kept.
fn write_close_to_tray(path: &Path, enabled: bool) -> Result<(), String> {
    let mut saved = std::fs::read_to_string(path)
        .ok()
        .and_then(|body| serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&body).ok())
        .unwrap_or_default();
    saved.insert("closeToTray".into(), enabled.into());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temporary = path.with_extension("json.part");
    std::fs::write(&temporary, serde_json::to_vec_pretty(&saved).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    std::fs::rename(temporary, path).map_err(|error| error.to_string())
}

fn close_to_tray_enabled() -> bool {
    read_close_to_tray(&studio_data_directory().join(SHELL_SETTINGS_FILE))
}

/// The main window in front of the person, from the tray, a second launch, or a quit question that is drawn in it.
fn show_main_window(app: &tauri::AppHandle) {
    use tauri::Manager;
    // a studio started headless on a Mac left the Dock; a window on screen belongs there
    #[cfg(target_os = "macos")]
    let _ = app.set_dock_visibility(true);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Whether the main window is on screen; minimised counts, as a window the person still uses.
/// A window whose state cannot be read counts as on screen.
fn main_window_visible(app: &tauri::AppHandle) -> bool {
    use tauri::Manager;
    app.get_webview_window("main").is_some_and(|window| window.is_visible().unwrap_or(true))
}

/// A later launch of the executable, handed over by the single-instance plugin on the main thread.
fn second_launch(app: &tauri::AppHandle, launch: Launch) {
    use tauri::Manager;
    if launch.stop {
        stop_from_terminal(app.clone());
    } else if launch.headless {
        // already running for an agent: another start only counts as use
        app.state::<Session>().touch();
    } else {
        open_by_person(app);
    }
}

/// A plain launch while the studio runs: the person wants the window. A studio started for an
/// agent becomes an ordinary one from now on - its close button and its tray icon follow the
/// setting, and it no longer stops by itself.
fn open_by_person(app: &tauri::AppHandle) {
    use tauri::Manager;
    app.state::<Session>().headless_owned.store(false, std::sync::atomic::Ordering::SeqCst);
    show_main_window(app);
    if let Err(error) = sync_tray(app) {
        eprintln!("[ERROR] the tray icon did not follow the close-to-tray setting: {error}");
    }
}

/// `--stop` from a terminal. Off the main thread: the service can take seconds to answer. The
/// service is asked inside the process, not over HTTP, where the question would count as an
/// agent's call.
fn stop_from_terminal(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let visible = main_window_visible(&app);
        let working = if visible { None } else { service_working() };
        match decide_terminal_stop(visible, working) {
            TerminalStop::Quit => quit_now(&app, Vec::new()),
            TerminalStop::Stay if visible => eprintln!("[studio] --stop left the studio running: its window is in use"),
            TerminalStop::Stay => eprintln!("[studio] --stop left the studio running: something is at work, or the service could not say"),
        }
    });
}

/// Stops a studio started headless once its window is hidden and nothing has used it for
/// `limit`: no agent's call, no work under way. The window's own requests are not use. Ends
/// without stopping anything once a plain launch made the session an ordinary one.
fn spawn_idle_watch(app: tauri::AppHandle, limit: Duration) {
    let watch = std::thread::Builder::new().name("idle-stop".into()).spawn(move || {
        use tauri::Manager;
        loop {
            std::thread::sleep(IDLE_LOOK.min(limit));
            let session = app.state::<Session>();
            if !session.headless_owned() {
                return;
            }
            let visible = main_window_visible(&app);
            // a service that does not answer at all - failed, or never started - leaves it to the clock
            let working = if visible || !service_is_ready() { Some(false) } else { service_working() };
            let own = session.unused_for();
            let unused_for = music_server::agent_quiet_for().map_or(own, |quiet| quiet.min(own));
            match look_for_idleness(visible, working, unused_for, limit) {
                IdleLook::InUse => session.touch(),
                IdleLook::Quiet => {}
                IdleLook::Stop => {
                    // work started a moment ago is not stopped: the next look sees it
                    if service_working() == Some(false) && !main_window_visible(&app) {
                        eprintln!("[studio] unused for {} s with its window hidden; stopping", unused_for.as_secs());
                        quit_now(&app, Vec::new());
                        return;
                    }
                    session.touch();
                }
            }
        }
    });
    if let Err(error) = watch {
        eprintln!("[ERROR] the idle stop could not start, so this studio runs until it is stopped: {error}");
    }
}

/// Quit from the tray. The question about a song being made is drawn inside the
/// window, so the window comes forward first - but only when there is a question.
fn quit_from_tray(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        if songs_in_progress(Duration::from_secs(2)).is_none_or(|songs| !songs.is_empty()) {
            show_main_window(&app);
        }
        confirm_quit(&app);
    });
}

/// The tray menu's two items.
fn tray_menu_event(app: &tauri::AppHandle, event: tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        TRAY_OPEN => show_main_window(app),
        TRAY_QUIT => quit_from_tray(app.clone()),
        _ => {}
    }
}

fn build_tray(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::{
        menu::{Menu, MenuItem},
        tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    };
    let icon = app.default_window_icon().cloned().ok_or("the studio has no icon for the tray")?;
    let failed = |error: tauri::Error| error.to_string();
    let open = MenuItem::with_id(app, TRAY_OPEN, "Open YuE2 Studio", true, None::<&str>).map_err(failed)?;
    let quit = MenuItem::with_id(app, TRAY_QUIT, "Quit", true, None::<&str>).map_err(failed)?;
    let menu = Menu::with_items(app, &[&open, &quit]).map_err(failed)?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("YuE2 Studio")
        .menu(&menu)
        // a click brings the window back; the menu is for the right button
        .show_menu_on_left_click(false)
        // The menu's items are answered by `tray_menu_event`, registered once at setup: Tauri keeps a tray's menu
        // handler among the app's global ones and never drops it, so one given here would be answered once more
        // each time the setting brings the icon back.
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
            | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)
        .map(|_| ())
        .map_err(failed)
}

/// The tray icon there or gone. Called on the main thread only, where the icon lives.
fn apply_tray(app: &tauri::AppHandle, wanted: bool) -> Result<(), String> {
    let present = app.tray_by_id(TRAY_ID).is_some();
    if wanted && !present {
        build_tray(app)?;
    } else if !wanted && present {
        // a window still hidden would have no way back once the icon is gone
        if !main_window_visible(app) {
            show_main_window(app);
        }
        let _ = app.remove_tray_by_id(TRAY_ID);
    }
    Ok(())
}

/// The tray icon as the setting and the session say.
fn sync_tray(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let headless_owned = app.state::<Session>().headless_owned();
    apply_tray(app, tray_wanted(close_to_tray_enabled(), headless_owned))
}

#[tauri::command]
fn close_to_tray() -> bool {
    close_to_tray_enabled()
}

/// Turns the setting on or off and the tray icon with it - a studio started headless keeps its
/// icon either way. Not async: the icon is made on the main thread. An error means nothing
/// changed, and the window says so.
#[tauri::command]
fn set_close_to_tray(app: tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    let path = studio_data_directory().join(SHELL_SETTINGS_FILE);
    if enabled {
        apply_tray(&app, true)?;
        if let Err(error) = write_close_to_tray(&path, true) {
            // the file still holds the old choice, so this puts the icon back as it was
            let _ = sync_tray(&app);
            return Err(error);
        }
    } else {
        write_close_to_tray(&path, false)?;
        sync_tray(&app)?;
    }
    Ok(enabled)
}

/// The update found at start, for a window that loaded after the event went out.
#[tauri::command]
fn pending_update(prompts: tauri::State<'_, Prompts>) -> Option<serde_json::Value> {
    prompts.update.lock().ok()?.as_ref().map(|update| update_offer(&update.version))
}

/// Downloads and runs the installer; the studio restarts into the new version.
/// An error comes back to the window, which shows it.
#[tauri::command]
async fn install_update(app: tauri::AppHandle, prompts: tauri::State<'_, Prompts>) -> Result<(), String> {
    let update = prompts.update.lock().map_err(|_| "the update state is poisoned".to_string())?.take().ok_or("no update is waiting")?;
    if let Err(error) = update.download_and_install(|_, _| {}, || {}).await {
        let message = error.to_string();
        if let Ok(mut slot) = prompts.update.lock() {
            *slot = Some(update);
        }
        return Err(message);
    }
    app.restart();
}

fn spawn_update_check(app: tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    use tauri_plugin_updater::UpdaterExt;

    tauri::async_runtime::spawn(async move {
        // The installer is a child of this process, which sits in its own
        // kill-on-close job: without releasing it, the installer died with the
        // studio a moment after starting and the update never happened.
        let cleanup = app.clone();
        // Started from the studio, the installer did not find the previous
        // folder and installed a second copy into its default one. NSIS takes
        // the folder as `/D=`, which must be the last argument and unquoted.
        let install_directory = format!("/D={}", executable_directory().display());
        // The proxy chosen in Settings: GitHub, where updates come from, is
        // among the sites a proxy is set up for.
        let builder = match music_server::net::fixed() {
            music_server::net::Fixed::System => app.updater_builder(),
            music_server::net::Fixed::Direct => app.updater_builder().no_proxy(),
            music_server::net::Fixed::Through(proxy) => app.updater_builder().proxy(proxy),
        };
        let updater = match builder
            .installer_arg(install_directory)
            .on_before_exit(move || {
                cleanup.cleanup_before_exit();
                music_engine::process_group::terminate_group_members();
                if !music_engine::process_group::release_children() {
                    eprintln!("[ERROR] could not release the update installer from the studio's job");
                }
            })
            .build()
        {
            Ok(updater) => updater,
            Err(_) => return,
        };
        let update = match updater.check().await {
            Ok(Some(update)) => update,
            _ => return,
        };
        let offer = update_offer(&update.version);
        if let Ok(mut slot) = app.state::<Prompts>().update.lock() {
            *slot = Some(update);
        }
        let _ = app.emit_to("main", "studio://update-available", offer);
    });
}

#[cfg(windows)]
fn hide_own_console_window() {
    unsafe extern "system" {
        fn GetConsoleWindow() -> isize;
        fn GetConsoleProcessList(processes: *mut u32, count: u32) -> u32;
        fn ShowWindow(window: isize, command: i32) -> i32;
    }

    unsafe {
        let mut processes = [0_u32; 4];
        if GetConsoleProcessList(processes.as_mut_ptr(), processes.len() as u32) == 1 {
            let window = GetConsoleWindow();
            if window != 0 {
                ShowWindow(window, 0);
            }
        }
    }
}

/// The studio's console without a window. On Windows 11 24H2 and later the manifest's consoleAllocationPolicy keeps
/// a start from Explorer without a console, and `AllocConsoleWithOptions` makes one with no window and no Terminal
/// tab, which every child program inherits instead of opening its own; started from a terminal, the studio keeps
/// that terminal's console and writes its log there. Before 24H2 the function does not exist and the console Windows
/// gave is hidden as before.
#[cfg(windows)]
fn console_without_window() {
    #[repr(C)]
    struct AllocConsoleOptions {
        mode: i32,
        use_show_window: i32,
        show_window: u16,
    }
    const ALLOC_CONSOLE_MODE_NO_WINDOW: i32 = 2;
    type AllocConsoleWithOptions = unsafe extern "system" fn(options: *const AllocConsoleOptions, result: *mut i32) -> i32;
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> isize;
        fn GetProcAddress(module: isize, name: *const u8) -> *const std::ffi::c_void;
    }

    let kernel32: Vec<u16> = "kernel32.dll".encode_utf16().chain(std::iter::once(0)).collect();
    let allocate = unsafe {
        let module = GetModuleHandleW(kernel32.as_ptr());
        if module == 0 { std::ptr::null() } else { GetProcAddress(module, c"AllocConsoleWithOptions".as_ptr().cast()) }
    };
    if allocate.is_null() {
        hide_own_console_window();
        return;
    }
    let allocate: AllocConsoleWithOptions = unsafe { std::mem::transmute(allocate) };
    let options = AllocConsoleOptions { mode: ALLOC_CONSOLE_MODE_NO_WINDOW, use_show_window: 0, show_window: 0 };
    let mut result = 0;
    let failed = unsafe { allocate(&options, &mut result) } < 0;
    if failed {
        console_then_hidden();
    }
}

/// A start from Explorer on 24H2 has no console at all (the manifest's `detached` policy): when the windowless one
/// could not be made, one is made and hidden, so the child programs share it instead of opening one each.
#[cfg(windows)]
fn console_then_hidden() {
    unsafe extern "system" {
        fn GetConsoleWindow() -> isize;
        fn AllocConsole() -> i32;
    }
    unsafe {
        if GetConsoleWindow() == 0 {
            AllocConsole();
        }
    }
    hide_own_console_window();
}

/// Tauri's own WebView2 switches, followed by whatever the user set in the
/// variable WebView2 documents for exactly this.
#[cfg(windows)]
fn webview_browser_arguments() -> String {
    // The visualiser's own window hears the studio through an audio context
    // of its own, which nothing in that window has clicked on to start.
    let mut own = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required".to_owned();
    // "No proxy" for what the window loads itself; a proxy of the user's own
    // is given through Tauri's proxy_url where it takes the scheme.
    let proxy = music_server::saved_proxy();
    if proxy.mode == music_server::net::ProxyMode::Off {
        own.push_str(" --no-proxy-server");
    } else if let Some(url) = proxy.window_proxy().filter(|url| !matches!(url.scheme(), "http" | "socks5")) {
        own.push_str(&format!(" --proxy-server={}://{}:{}", url.scheme(), url.host_str().unwrap_or_default(), url.port_or_known_default().unwrap_or_default()));
    }
    match std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS") {
        Ok(extra) if !extra.trim().is_empty() => format!("{own} {}", extra.trim()),
        _ => own,
    }
}

/// The proxy a window is given through Tauri's proxy_url, which takes HTTP and
/// SOCKS5; the other schemes go in the browser arguments. Every window takes
/// the same, or WebView2 refuses the second one.
fn window_proxy_url() -> Option<reqwest::Url> {
    music_server::saved_proxy().window_proxy().filter(|url| matches!(url.scheme(), "http" | "socks5"))
}

/// The visualiser in a window of its own, or the one already open brought
/// forward. Built here rather than from the page: every window of the app
/// shares one WebView2 environment, which must be opened with the same
/// switches the main window was, or the new window fails to open. Async:
/// on Windows a window made from a synchronous command deadlocks WebView2.
#[tauri::command]
async fn open_visualizer_window(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager as _;
    if let Some(window) = app.get_webview_window("visualizer") {
        window.show().map_err(|error| error.to_string())?;
        return window.set_focus().map_err(|error| error.to_string());
    }
    let title = format!("{} - Visualizer", app.package_info().name);
    let window = tauri::WebviewWindowBuilder::new(&app, "visualizer", tauri::WebviewUrl::App("visualizer.html".into()))
        .title(title)
        .inner_size(960.0, 540.0)
        .min_inner_size(320.0, 200.0);
    #[cfg(windows)]
    let window = window.additional_browser_args(&webview_browser_arguments());
    let window = match window_proxy_url() {
        Some(url) => window.proxy_url(url),
        None => window,
    };
    window.build().map(|_| ()).map_err(|error| error.to_string())
}

/// The audio editor (AudioMass) in a window of its own, or the one already
/// open sent to the file asked for and brought forward. The editor is static
/// files the local service serves, so the page is an external URL rather than
/// one in the app bundle. Built here, like the visualiser, because every
/// window of the app shares one WebView2 environment and must be opened with
/// the same switches the main window was.
#[tauri::command]
async fn open_audio_editor_window(app: tauri::AppHandle, url: String) -> Result<(), String> {
    use tauri::Manager as _;
    let target: tauri::Url = url.parse().map_err(|error| format!("Not an editor address: {error}"))?;
    // the editor is a page of the studio's own service: no other address gets a window of the app
    if !matches!(target.host_str(), Some("127.0.0.1" | "localhost")) {
        return Err(format!("The audio editor opens pages of the studio's own service only, not {}", target.host_str().unwrap_or("an address without a host")));
    }
    if let Some(window) = app.get_webview_window("audio-editor") {
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        return window.navigate(target).map_err(|error| error.to_string());
    }
    let title = format!("{} - Audio Editor", app.package_info().name);
    let window = tauri::WebviewWindowBuilder::new(&app, "audio-editor", tauri::WebviewUrl::External(target))
        .title(title)
        .inner_size(1280.0, 800.0)
        .min_inner_size(640.0, 400.0);
    #[cfg(windows)]
    let window = window.additional_browser_args(&webview_browser_arguments());
    let window = match window_proxy_url() {
        Some(url) => window.proxy_url(url),
        None => window,
    };
    window.build().map(|_| ()).map_err(|error| error.to_string())
}

/// The window's shape: only these rectangles, [left, top, right, bottom] in
/// physical pixels of the page, are the window, and a click anywhere else
/// goes to whatever lies below. No rectangles: the whole window again. The
/// Winamp mode spreads the window over the screen and cuts it to Winamp's
/// own windows, so they can be moved apart, docked and resized like Winamp's.
#[tauri::command]
fn set_window_region(window: tauri::WebviewWindow, rects: Vec<[i32; 4]>) -> Result<(), String> {
    #[cfg(windows)]
    {
        #[link(name = "gdi32")]
        unsafe extern "system" {
            fn CreateRectRgn(left: i32, top: i32, right: i32, bottom: i32) -> isize;
            fn CombineRgn(destination: isize, first: isize, second: isize, mode: i32) -> i32;
            fn DeleteObject(object: isize) -> i32;
        }
        #[link(name = "user32")]
        unsafe extern "system" {
            fn SetWindowRgn(window: isize, region: isize, redraw: i32) -> i32;
            fn GetWindowRect(window: isize, rect: *mut [i32; 4]) -> i32;
            fn ClientToScreen(window: isize, point: *mut [i32; 2]) -> i32;
        }
        const RGN_OR: i32 = 2;
        let hwnd = window.hwnd().map_err(|error| error.to_string())?.0 as isize;
        unsafe {
            let region = if rects.is_empty() {
                0
            } else {
                // a region counts from the window's outer corner, which a frameless window
                // still keeps a few invisible border pixels away from its page
                let mut outer = [0i32; 4];
                let mut page = [0i32; 2];
                if GetWindowRect(hwnd, &mut outer) == 0 || ClientToScreen(hwnd, &mut page) == 0 {
                    return Err("Windows did not tell where the window's page is".into());
                }
                let (dx, dy) = (page[0] - outer[0], page[1] - outer[1]);
                let region = CreateRectRgn(0, 0, 0, 0);
                if region == 0 {
                    return Err("Windows made no region for the window".into());
                }
                // Chromium apps take a window whose region is one plain rectangle for an opaque
                // one of its whole size (IsWindowVisibleAndFullyOpaque) and stop drawing beneath
                // it: two lone pixels in the invisible border keep the shape never that plain
                let stubs = [[-dx, -dy, 1 - dx, 1 - dy], [2 - dx, -dy, 3 - dx, 1 - dy]];
                for [left, top, right, bottom] in stubs.into_iter().chain(rects) {
                    let part = CreateRectRgn(left + dx, top + dy, right + dx, bottom + dy);
                    if part == 0 || CombineRgn(region, region, part, RGN_OR) == 0 {
                        DeleteObject(region);
                        if part != 0 {
                            DeleteObject(part);
                        }
                        return Err("Windows could not build the window's shape".into());
                    }
                    DeleteObject(part);
                }
                region
            };
            // the window owns the region from here on
            if SetWindowRgn(hwnd, region, 1) == 0 {
                if region != 0 {
                    DeleteObject(region);
                }
                return Err("Windows did not take the window's shape".into());
            }
        }
    }
    #[cfg(not(windows))]
    let _ = (window, rects);
    Ok(())
}

/// The GStreamer paths of an AppImage, pointed inside it. AppRun sources linuxdeploy's GStreamer hook before the GTK
/// hook that sets APPDIR for an extracted AppImage, so there the hook's paths name the host's plugins; WebKit's media
/// process then loads the host's libgstsoup, whose libsoup 2 kills it next to WebKitGTK's libsoup 3. Each wrong
/// variable comes back with the path it gets inside the AppImage, and the registry is the studio's own so a cache
/// written with the host's plugins is not read.
#[cfg(any(target_os = "linux", test))]
fn appimage_gstreamer_paths(appdir: &std::path::Path, current: impl Fn(&str) -> Option<String>, cache: &std::path::Path) -> Vec<(&'static str, String)> {
    let inside = |relative: &str| appdir.join(relative).to_string_lossy().into_owned();
    let expected = [
        ("GST_PLUGIN_SYSTEM_PATH_1_0", inside("usr/lib/gstreamer-1.0")),
        ("GST_PLUGIN_PATH_1_0", inside("usr/lib/gstreamer-1.0")),
        ("GST_PLUGIN_SCANNER_1_0", inside("usr/lib/gstreamer1.0/gstreamer-1.0/gst-plugin-scanner")),
        ("GST_PTP_HELPER_1_0", inside("usr/lib/gstreamer1.0/gstreamer-1.0/gst-ptp-helper")),
    ];
    if !appdir.join("usr/lib/gstreamer-1.0").is_dir() {
        return Vec::new();
    }
    let mut fixed: Vec<(&'static str, String)> = expected
        .into_iter()
        .filter(|(name, path)| current(name).is_some_and(|value| value != *path))
        .collect();
    if !fixed.is_empty() && current("GST_REGISTRY_1_0").is_none() {
        fixed.push(("GST_REGISTRY_1_0", cache.join("yue2-studio").join("gstreamer-registry.bin").to_string_lossy().into_owned()));
    }
    fixed
}

#[cfg(target_os = "linux")]
fn point_gstreamer_inside_the_appimage() {
    let Some(appdir) = std::env::var_os("APPDIR").map(PathBuf::from) else { return };
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    for (name, value) in appimage_gstreamer_paths(&appdir, |name| std::env::var(name).ok(), &cache) {
        eprintln!("[studio] {name} pointed outside the AppImage; set to {value}");
        // single-threaded here, before Tauri starts any thread
        unsafe { std::env::set_var(name, value) };
    }
}

pub fn run() {
    let arguments: Vec<String> = std::env::args_os().map(|argument| argument.to_string_lossy().into_owned()).collect();
    let launch = parse_launch(&arguments);

    #[cfg(windows)]
    console_without_window();
    #[cfg(target_os = "linux")]
    point_gstreamer_inside_the_appimage();

    if keeps_everything_beside_itself() && std::env::var_os("WEBVIEW2_USER_DATA_FOLDER").is_none() {
        // The process is still single-threaded here, before Tauri or its
        // worker threads are created, so updating its child WebView environment
        // cannot race with an environment read.
        unsafe {
            std::env::set_var(
                "WEBVIEW2_USER_DATA_FOLDER",
                executable_directory().join("webview-data"),
            );
        }
    }

    // The native engine is started only by the setup gate after a complete
    // verified profile is present. This keeps first launch download-free and
    // avoids starting a runtime against partial weights.
    // Whatever ends this process - the window, Task Manager, a crash - takes
    // the engine with it. Without this the engine outlived a force-killed
    // studio, holding the graphics card and the port it listens on.
    if !music_engine::process_group::bind_children_to_this_process() {
        eprintln!("this process could not create its own job object; the engine is stopped by the supervisor only");
    }

    configure_studio_runtime_paths();
    // the card chosen in the engine settings, for every CUDA process after this
    music_server::apply_saved_gpu();

    // the service names the studio's version to agents, which only the shell knows
    let context = tauri::generate_context!();
    music_server::set_studio_version(context.package_info().version.to_string());

    // The updater is configured only in release builds, where the signing
    // public key and the release endpoint are injected into the config. The
    // plugin refuses to initialise without that section and would take the
    // whole window down with it, so a development build simply runs without an
    // updater instead of crashing at launch.
    let updater_configured = context
        .config()
        .plugins
        .0
        .get("updater")
        .is_some_and(|value| !value.is_null());

    // A second launch has nothing to do next to a running studio: it shares the service, the engine and the data.
    // It hands its flags to the first one and ends - a plain launch brings the window forward, the one hidden to the
    // tray or never shown included; `--headless` and `--stop` show nothing. Registered first: the plugin ends a second
    // launch while `build` initialises the plugins, before the service below is started or any window is made.
    // There is no exception for a development run: the shell always talks to the service on 8791, so a second shell
    // beside a running studio would only have been a second window on the same service.
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, arguments, _directory| second_launch(app, parse_launch(&arguments))))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(tauri_plugin_window_state::StateFlags::SIZE | tauri_plugin_window_state::StateFlags::POSITION | tauri_plugin_window_state::StateFlags::MAXIMIZED)
                // an absolute name: the plugin joins it to AppData, which a portable copy never writes
                .with_filename(studio_data_directory().join("window-state.json").to_string_lossy())
                .with_filter(|label| label == "main")
                .build(),
        )
        .manage(Prompts::default())
        .manage(Session::new(launch.headless))
        .invoke_handler(tauri::generate_handler![
            open_visualizer_window,
            open_audio_editor_window,
            set_window_region,
            quit_prompt_shown,
            quit_prompt_cancelled,
            quit_studio,
            close_to_tray,
            set_close_to_tray,
            pending_update,
            install_update
        ]);
    if updater_configured {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    }

    let app = builder
        .setup(move |app| {
            // the window's own files, for a browser on another computer when
            // access from the network is on
            let resolver = app.asset_resolver();
            music_server::set_asset_source(Box::new(move |path: &str| resolver.get(path.to_string()).map(|asset| (asset.bytes.to_vec(), asset.mime_type))));
            // "Save as" for what the window saves: Windows' own dialog, over
            // the studio's window, asked for by the service off the UI thread.
            let dialogs = app.handle().clone();
            music_server::set_save_dialog(Box::new(move |name: &str, start: Option<std::path::PathBuf>| {
                use tauri::Manager;
                use tauri_plugin_dialog::DialogExt;
                let mut dialog = dialogs.dialog().file().set_file_name(name);
                if let Some(extension) = std::path::Path::new(name).extension().and_then(|extension| extension.to_str()) {
                    dialog = dialog.add_filter(extension.to_uppercase(), &[extension]);
                }
                if let Some(folder) = start {
                    dialog = dialog.set_directory(folder);
                }
                if let Some(window) = dialogs.get_webview_window("main") {
                    dialog = dialog.set_parent(&window);
                }
                dialog.blocking_save_file().and_then(|path| path.into_path().ok())
            }));
            // The window is built here rather than from the configuration so
            // the WebView2 arguments can be extended: Tauri passes its own, and
            // WebView2 then ignores WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
            // entirely, so a remote-debugging port or any other documented
            // switch set by the user would silently never arrive.
            let window_config = app
                .config()
                .app
                .windows
                .iter()
                .find(|window| window.label == "main")
                .cloned()
                .expect("the main window is declared in tauri.conf.json");
            let title = format!("{} {}{}", window_config.title, app.package_info().version, if is_portable() { " Portable" } else { "" });
            let window = tauri::WebviewWindowBuilder::from_config(app.handle(), &window_config)?.title(title);
            // Started for an agent: the page is made, as parts of the studio run in it, but the window never comes up
            // or takes the focus - no taskbar button, nothing taken from what the person is doing. The size, place and
            // maximised state window-state restores into it wait, hidden, for the first show: tao moves and sizes a
            // hidden window without showing it, and on Windows keeps a maximise for the show (it calls
            // ShowWindow(SW_MAXIMIZE) only on a visible window), so the restore needs no deferring.
            // `--stop` with nothing running ends right after this: its window never comes up either
            let window = if launch.headless || launch.stop { window.visible(false).focused(false) } else { window };
            // WebView2 leaves a microphone request with no answer and no prompt: the window's own bundled page
            // (tauri.localhost on Windows, tauri://localhost elsewhere) - recording in the audio editor - waited for ever
            let window = window.on_permission_request(|webview, kind| match kind {
                tauri::webview::PermissionKind::Microphone
                    if webview.url().is_ok_and(|url| url.scheme() == "tauri" || url.host_str() == Some("tauri.localhost")) =>
                {
                    tauri::webview::PermissionResponse::Allow
                }
                _ => tauri::webview::PermissionResponse::Default,
            });
            #[cfg(windows)]
            let window = window.additional_browser_args(&webview_browser_arguments());
            // The proxy chosen in Settings for what the window loads itself -
            // the image and video searches of the video maker; the service
            // routes its own requests.
            let window = match window_proxy_url() {
                Some(url) => window.proxy_url(url),
                None => window,
            };

            window.build()?;
            app.on_menu_event(tray_menu_event);
            // The icon is there from the start when the person chose to keep the studio in the tray, and in a studio
            // started headless, where it is the only way to the window. Without it the close button quits, as it does
            // with the setting off; a headless studio is then still reached by starting it again.
            if tray_wanted(close_to_tray_enabled(), launch.headless) {
                if let Err(error) = apply_tray(app.handle(), true) {
                    eprintln!("[ERROR] the tray icon could not be made, so closing the window quits: {error}");
                }
            }
            if launch.headless {
                #[cfg(target_os = "macos")]
                let _ = app.handle().set_dock_visibility(false);
                spawn_idle_watch(app.handle().clone(), idle_period(std::env::var(IDLE_STOP_VARIABLE).ok().as_deref()));
            }
            // The stock Quit item terminates the app, closing the window
            // before anything can ask. Cmd+Q is a menu item of our own that
            // asks first; the window stays up until the answer.
            #[cfg(target_os = "macos")]
            {
                use tauri::menu::{Menu, MenuItem, MenuItemKind};
                let menu = Menu::default(app.handle())?;
                for item in menu.items()? {
                    if let MenuItemKind::Submenu(submenu) = item {
                        for entry in submenu.items()? {
                            if let MenuItemKind::Predefined(predefined) = &entry {
                                if predefined.text()?.starts_with("Quit") {
                                    submenu.remove(predefined)?;
                                    submenu.append(&MenuItem::with_id(app.handle(), "quit-confirm", "Quit YuE2 Studio", true, Some("CmdOrCtrl+Q"))?)?;
                                }
                            }
                        }
                    }
                }
                app.set_menu(menu)?;
                app.on_menu_event(|app, event| {
                    if event.id().as_ref() != "quit-confirm" {
                        return;
                    }
                    let app = app.clone();
                    std::thread::spawn(move || confirm_quit(&app));
                });
            }
            if updater_configured {
                spawn_update_check(app.handle().clone());
            }
            // The webview starts loading the moment the window exists, and the
            // service can finish binding a fraction of a second later. That is
            // enough for the browser to keep its own connection error on screen
            // for good - checking readiness once here is not enough, because the
            // race is already lost by then. So the page is reloaded whenever it
            // is still empty, for the first half minute of the session.
            {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    use tauri::Manager as _;
                    if !wait_until_ready(Duration::from_secs(90)) {
                        return;
                    }
                    let started = Instant::now();
                    while started.elapsed() < Duration::from_secs(30) {
                        std::thread::sleep(Duration::from_millis(700));
                        for (_, window) in handle.webview_windows() {
                            // Reload only while nothing has rendered: reloading
                            // a working studio would throw away what the user
                            // already has on screen.
                            let _ = window.eval(
                                "if (!document.getElementById('root') || !document.getElementById('root').firstElementChild) { window.location.reload(); }",
                            );
                        }
                    }
                });
            }
            Ok(())
        })
        .build(context)
        .expect("error while building YuE2 Studio");

    // A second launch ended inside `build`, its flags handed to the running studio; this process is the only studio
    // from here on. `--stop` with nothing running has nothing to stop and starts nothing.
    if launch.stop {
        return;
    }
    if let Err(error) = start_service() {
        eprintln!("failed to start the studio service: {error}");
    }

    app.run(|app, event| {
        // Quitting stops a song that is being generated, so ask first, but
        // only then. Closing the main window quits the studio on every
        // platform - unless the person chose to keep it in the tray, or the
        // studio is still the agent's that started it headless, and the icon
        // is there, when it only hides; Cmd+Q and the Dock reach it as an exit
        // request with no code. An exit the app requests itself carries a code
        // and goes through.
        match &event {
            // the service is asked off the event loop: it can take seconds to answer
            tauri::RunEvent::WindowEvent { label, event: tauri::WindowEvent::CloseRequested { api, .. }, .. } if label == "main" => {
                use tauri::Manager as _;
                api.prevent_close();
                let headless_owned = app.state::<Session>().headless_owned();
                match decide_window_close(close_to_tray_enabled(), app.tray_by_id(TRAY_ID).is_some(), headless_owned) {
                    // hidden, not closed: the page and what is typed in it survive
                    WindowClose::HideToTray => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.hide();
                        }
                    }
                    WindowClose::Quit => {
                        let app = app.clone();
                        std::thread::spawn(move || confirm_quit(&app));
                    }
                }
            }
            // Opening the app again on a Mac - the Dock, Finder, `open` - reaches the running studio as this, not as
            // a second launch: the person wants the window, the one hidden to the tray or never shown included.
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => open_by_person(app),
            #[cfg(target_os = "macos")]
            tauri::RunEvent::ExitRequested { code: None, api, .. } => {
                api.prevent_exit();
                let app = app.clone();
                std::thread::spawn(move || confirm_quit(&app));
            }
            tauri::RunEvent::Exit => {
                music_server::stop_engine();
                // the servers and engines the studio started end with it, also where no job object ends them (macOS)
                music_engine::process_group::terminate_group_members();
            }
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_close_button_hides_with_the_setting_on_or_for_the_agent_and_only_with_the_tray_there() {
        // close_to_tray, tray_present, headless_owned
        assert_eq!(decide_window_close(true, true, false), WindowClose::HideToTray);
        assert_eq!(decide_window_close(true, false, false), WindowClose::Quit, "no icon, no way back");
        assert_eq!(decide_window_close(false, true, false), WindowClose::Quit);
        assert_eq!(decide_window_close(false, false, false), WindowClose::Quit);
        assert_eq!(decide_window_close(false, true, true), WindowClose::HideToTray, "the agent's studio stays, setting or not");
        assert_eq!(decide_window_close(true, true, true), WindowClose::HideToTray);
        assert_eq!(decide_window_close(false, false, true), WindowClose::Quit, "no icon, no way back, headless or not");
        assert_eq!(decide_window_close(true, false, true), WindowClose::Quit);
    }

    #[test]
    fn the_tray_is_there_for_the_setting_or_for_a_headless_studio() {
        assert!(tray_wanted(true, false));
        assert!(tray_wanted(false, true));
        assert!(tray_wanted(true, true));
        assert!(!tray_wanted(false, false));
    }

    #[test]
    fn the_flags_are_read_anywhere_on_the_command_line() {
        assert_eq!(parse_launch(&["C:\\YuE2 Studio\\YuE2-Studio.exe"]), Launch::default());
        assert_eq!(parse_launch(&["YuE2-Studio.exe", "--headless"]), Launch { headless: true, stop: false });
        assert_eq!(parse_launch(&["YuE2-Studio.exe", "--stop"]), Launch { headless: false, stop: true });
        assert_eq!(parse_launch(&["YuE2-Studio.exe", "--stop", "--headless"]), Launch { headless: true, stop: true });
        assert_eq!(parse_launch(&["YuE2-Studio.exe", "--headlessly", "stop"]), Launch::default(), "whole flags only");
        let handed_over: Vec<String> = vec!["YuE2-Studio.exe".into(), "--headless".into()];
        assert!(parse_launch(&handed_over).headless);
    }

    #[test]
    fn the_idle_period_is_ten_minutes_unless_the_variable_says_otherwise() {
        assert_eq!(idle_period(None), Duration::from_secs(600));
        assert_eq!(idle_period(Some("90")), Duration::from_secs(90));
        assert_eq!(idle_period(Some(" 1.5 ")), Duration::from_millis(1500));
        for wrong in ["", "0", "-5", "soon", "NaN", "inf", "1e300"] {
            assert_eq!(idle_period(Some(wrong)), Duration::from_secs(600), "{wrong}");
        }
    }

    #[test]
    fn stop_from_a_terminal_quits_only_a_studio_nobody_uses_and_that_has_nothing_at_work() {
        assert_eq!(decide_terminal_stop(false, Some(false)), TerminalStop::Quit, "hidden or headless, nothing at work");
        assert_eq!(decide_terminal_stop(true, Some(false)), TerminalStop::Stay, "the person's window");
        assert_eq!(decide_terminal_stop(true, None), TerminalStop::Stay);
        assert_eq!(decide_terminal_stop(false, Some(true)), TerminalStop::Stay, "a song is being made: no question, no quit");
        assert_eq!(decide_terminal_stop(false, None), TerminalStop::Stay, "the service could not say");
    }

    #[test]
    fn a_hidden_studio_stops_after_the_idle_period_with_nothing_at_work() {
        let limit = Duration::from_secs(600);
        let long = Duration::from_secs(601);
        let short = Duration::from_secs(30);
        assert_eq!(look_for_idleness(false, Some(false), long, limit), IdleLook::Stop);
        assert_eq!(look_for_idleness(false, Some(false), limit, limit), IdleLook::Stop);
        assert_eq!(look_for_idleness(false, Some(false), short, limit), IdleLook::Quiet);
        assert_eq!(look_for_idleness(true, Some(false), long, limit), IdleLook::InUse, "a window on screen is use");
        assert_eq!(look_for_idleness(false, Some(true), long, limit), IdleLook::InUse, "work under way is use");
        assert_eq!(look_for_idleness(false, None, long, limit), IdleLook::InUse, "a service that cannot say is never idle");
    }

    #[test]
    fn close_to_tray_is_off_until_it_was_saved_on() {
        let directory = std::env::temp_dir().join(format!("yue2-shell-settings-{}", std::process::id()));
        let path = directory.join(SHELL_SETTINGS_FILE);
        assert!(!read_close_to_tray(&path), "no file");
        write_close_to_tray(&path, true).unwrap();
        assert!(read_close_to_tray(&path));
        write_close_to_tray(&path, false).unwrap();
        assert!(!read_close_to_tray(&path));
        std::fs::write(&path, "not json").unwrap();
        assert!(!read_close_to_tray(&path), "an unreadable file is off");
        std::fs::write(&path, r#"{"closeToTray":"yes"}"#).unwrap();
        assert!(!read_close_to_tray(&path), "only a true counts");
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn gstreamer_paths_left_outside_an_extracted_appimage_point_inside_it() {
        let appdir = std::env::temp_dir().join(format!("yue2-appdir-{}", std::process::id()));
        std::fs::create_dir_all(appdir.join("usr/lib/gstreamer-1.0")).unwrap();
        let inside = |relative: &str| appdir.join(relative).to_string_lossy().into_owned();
        let cache = PathBuf::from("/home/someone/.cache");
        let hooked = |name: &str| -> Option<String> {
            match name {
                "GST_PLUGIN_SYSTEM_PATH_1_0" => Some(inside("usr/lib/gstreamer-1.0")),
                "GST_PLUGIN_PATH_1_0" => Some("/usr/lib/gstreamer-1.0".into()),
                "GST_PLUGIN_SCANNER_1_0" => Some("/usr/lib/gstreamer1.0/gstreamer-1.0/gst-plugin-scanner".into()),
                "GST_PTP_HELPER_1_0" => Some("/usr/lib/gstreamer1.0/gstreamer-1.0/gst-ptp-helper".into()),
                _ => None,
            }
        };
        let fixed = appimage_gstreamer_paths(&appdir, hooked, &cache);
        let names: Vec<&str> = fixed.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, ["GST_PLUGIN_PATH_1_0", "GST_PLUGIN_SCANNER_1_0", "GST_PTP_HELPER_1_0", "GST_REGISTRY_1_0"]);
        assert_eq!(fixed[0].1, inside("usr/lib/gstreamer-1.0"));
        assert_eq!(fixed[1].1, inside("usr/lib/gstreamer1.0/gstreamer-1.0/gst-plugin-scanner"));
        assert!(fixed[3].1.starts_with("/home/someone/.cache"));

        let right = |name: &str| fixed.iter().find(|(fixed, _)| *fixed == name).map(|(_, value)| value.clone()).or_else(|| hooked(name));
        assert!(appimage_gstreamer_paths(&appdir, right, &cache).is_empty(), "paths inside the AppImage are kept");
        assert!(appimage_gstreamer_paths(&appdir, |_| None, &cache).is_empty(), "nothing set, nothing to correct");
        std::fs::remove_dir_all(&appdir).ok();
    }
}

