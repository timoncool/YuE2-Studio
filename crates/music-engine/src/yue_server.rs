//! Supervisor for the `yue-server` process of yue2.cpp.
//!
//! Upstream `server.cmd` launches `yue-server --model <backbone> --vae <vae>
//! [--transcriber <gguf>] --host --port`; the models are fixed for the life of
//! the process, so choosing another quantisation restarts the engine. The
//! server stops its listener, cancels the active job and frees the models on
//! SIGINT/SIGTERM (CTRL_BREAK on Windows).

use std::{
    fs,
    io::{Read, Write},
    net::{IpAddr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};

const DEFAULT_HOST: &str = "127.0.0.1";
/// Not upstream's 8087: a `yue-server` the user runs by hand with other
/// weights must not be mistaken for the studio's own engine.
pub const DEFAULT_PORT: u16 = 18087;

/// The loopback port the engine listens on and the studio asks it on: `YUE_ENGINE_PORT` when set, else the
/// engine's own (`DEFAULT_PORT`) when this computer lets the studio listen there, else one the system gives. Decided
/// once per run; `engine_port_note` says why it is not the engine's own.
pub fn engine_port() -> u16 {
    resolved_port().0
}

/// Why `engine_port` is not the engine's own port: what holds it, or that Windows reserves it.
pub fn engine_port_note() -> Option<&'static str> {
    resolved_port().1.as_deref()
}

fn resolved_port() -> &'static (u16, Option<String>) {
    static PORT: std::sync::OnceLock<(u16, Option<String>)> = std::sync::OnceLock::new();
    PORT.get_or_init(|| match (std::env::var("YUE_ENGINE_PORT"), port_from_environment()) {
        (_, Some(port)) => (port, None),
        (Ok(text), None) => {
            note_in_log(&format!("YUE_ENGINE_PORT={text} is not a port, so the studio chooses one"));
            let (port, note) = choose_port(DEFAULT_PORT);
            let note = format!("YUE_ENGINE_PORT={text} is not a port, so the studio chose one{}", note.map(|note| format!(": {note}")).unwrap_or_default());
            (port, Some(note))
        }
        (Err(_), None) => choose_port(DEFAULT_PORT),
    })
}

/// The port YUE_ENGINE_PORT sets, when it holds one.
fn port_from_environment() -> Option<u16> {
    std::env::var("YUE_ENGINE_PORT").ok()?.trim().parse::<u16>().ok().filter(|port| *port > 0)
}

/// Whether the engine's port is the one YUE_ENGINE_PORT sets rather than one the studio chose.
pub fn engine_port_is_set() -> bool {
    port_from_environment().is_some()
}

/// `wanted` when the studio can listen on it on loopback or this engine already answers there, else the first free
/// port of the hundred above it, which the system does not hand out to outgoing connections as it does its dynamic
/// range, else one the system gives; with the reason.
fn choose_port(wanted: u16) -> (u16, Option<String>) {
    let bindable = |port: u16| std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).map(drop);
    let refused = match bindable(wanted) {
        Ok(()) => return (wanted, None),
        Err(error) => error,
    };
    if answers_as_engine(DEFAULT_HOST, wanted, Duration::from_millis(500)) {
        return (wanted, None);
    }
    let why = port_unavailable(wanted, &refused);
    let other = (wanted.saturating_add(1)..=wanted.saturating_add(100))
        .find(|port| bindable(*port).is_ok())
        .map(Ok)
        .unwrap_or_else(|| std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).and_then(|listener| listener.local_addr()).map(|address| address.port()));
    let (port, note) = match other {
        Ok(port) => (port, format!("port {wanted} is not available to the engine ({why}); it listens on {port} instead")),
        Err(error) => (wanted, format!("port {wanted} is not available to the engine ({why}) and the system gave no other: {error}")),
    };
    note_in_log(&note);
    (port, Some(note))
}

/// What keeps the studio from listening on `port`: the range Windows reserves it in, or the program that holds it.
fn port_unavailable(port: u16, error: &std::io::Error) -> String {
    #[cfg(windows)]
    {
        // 10013 is WSAEACCES, which a reserved range gives
        if error.raw_os_error() == Some(10013) {
            if let Some((first, last)) = excluded_range(port) {
                return format!("Windows reserves ports {first}-{last}");
            }
        }
        if let Some((pid, name)) = listener_of(port) {
            return format!("{name} (PID {pid}) listens on it");
        }
    }
    #[cfg(not(windows))]
    let _ = port;
    if error.kind() == std::io::ErrorKind::AddrInUse { format!("another program listens on it: {error}") } else { error.to_string() }
}

/// The range of `netsh interface ipv4 show excludedportrange protocol=tcp` that holds `port`; none when netsh does
/// not answer within three seconds.
#[cfg(windows)]
fn excluded_range(port: u16) -> Option<(u16, u16)> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // Windows' own netsh by its full path: a bare name is looked for beside the studio first
    let system = std::path::PathBuf::from(std::env::var_os("SystemRoot")?);
    let mut child = Command::new(system.join("System32").join("netsh.exe"))
        .args(["interface", "ipv4", "show", "excludedportrange", "protocol=tcp"])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // read while it runs, so a listing longer than the pipe holds does not stop it
    let mut stdout = child.stdout.take()?;
    // the listing comes in the console's code page, not UTF-8, on a Windows in another language
    let reader = thread::spawn(move || {
        let mut listing = Vec::new();
        stdout.read_to_end(&mut listing).map(|_| String::from_utf8_lossy(&listing).into_owned())
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    while child.try_wait().ok()?.is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let listing = reader.join().ok()?.ok()?;
    excluded_range_in(&listing, port)
}

/// The range of a netsh port-range listing (lines of a first and a last port) that holds `port`.
#[cfg(any(windows, test))]
fn excluded_range_in(listing: &str, port: u16) -> Option<(u16, u16)> {
    listing.lines().find_map(|line| {
        let mut numbers = line.split_whitespace().map(str::parse::<u16>);
        let (Some(Ok(first)), Some(Ok(last))) = (numbers.next(), numbers.next()) else { return None };
        (first <= port && port <= last).then_some((first, last))
    })
}

/// The process listening on `port` on loopback or every address, IPv4 or IPv6 (a dual-stack listener on [::] holds
/// the IPv4 port too), by its PID and program name.
#[cfg(windows)]
fn listener_of(port: u16) -> Option<(u32, String)> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::NetworkManagement::IpHelper::{GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER};
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
    const AF_INET: u32 = 2;
    const AF_INET6: u32 = 23;
    // the table: a u32 count, then the rows
    let table = |family: u32| -> Option<Vec<u32>> {
        let mut size = 0u32;
        unsafe { GetExtendedTcpTable(std::ptr::null_mut(), &mut size, 0, family, TCP_TABLE_OWNER_PID_LISTENER, 0) };
        if size == 0 {
            return None;
        }
        let mut buffer = vec![0u32; (size as usize).div_ceil(4)];
        (unsafe { GetExtendedTcpTable(buffer.as_mut_ptr().cast(), &mut size, 0, family, TCP_TABLE_OWNER_PID_LISTENER, 0) } == 0).then_some(buffer)
    };
    // addresses and ports in network byte order, a port in its field's low 16 bits
    let v4 = table(AF_INET).and_then(|buffer| {
        let rows = unsafe { std::slice::from_raw_parts(buffer.as_ptr().add(1).cast::<MIB_TCPROW_OWNER_PID>(), buffer[0] as usize) };
        let loopback = u32::from_ne_bytes([127, 0, 0, 1]);
        rows.iter().find(|row| u16::from_be(row.dwLocalPort as u16) == port && (row.dwLocalAddr == loopback || row.dwLocalAddr == 0)).map(|row| row.dwOwningPid)
    });
    let pid = v4.or_else(|| {
        table(AF_INET6).and_then(|buffer| {
            let rows = unsafe { std::slice::from_raw_parts(buffer.as_ptr().add(1).cast::<MIB_TCP6ROW_OWNER_PID>(), buffer[0] as usize) };
            let any = [0u8; 16];
            let loopback = std::net::Ipv6Addr::LOCALHOST.octets();
            rows.iter().find(|row| u16::from_be(row.dwLocalPort as u16) == port && (row.ucLocalAddr == any || row.ucLocalAddr == loopback)).map(|row| row.dwOwningPid)
        })
    })?;
    let mut name = format!("process {pid}");
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if !process.is_null() {
            let mut path = vec![0u16; 1024];
            let mut length = path.len() as u32;
            if QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut length) != 0 {
                let full = String::from_utf16_lossy(&path[..length as usize]);
                name = full.rsplit(['\\', '/']).next().unwrap_or(&full).to_string();
            }
            CloseHandle(process);
        }
    }
    Some((pid, name))
}

#[cfg(windows)]
const EXECUTABLE: &str = "yue-server.exe";
#[cfg(not(windows))]
const EXECUTABLE: &str = "yue-server";

/// The weights one engine process serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YueModelFiles {
    pub backbone: PathBuf,
    pub vae: PathBuf,
    /// SheetSage2; without it the engine has no `/transcribe` route.
    pub transcriber: Option<PathBuf>,
    /// The folder of adapters requests may name; without it they name none.
    pub adapters: Option<PathBuf>,
    /// The decoder adapter merged under every render, beneath the request's.
    pub companion: PathBuf,
}

/// A part of the model an adapter can change, with its own strength.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct AdapterSlot {
    /// The engine's name for the part, its flag in `/props` and the prefix of
    /// its `<id>_scale` request field.
    pub id: &'static str,
    /// What changing that part does to a song, for the interface to name.
    pub role: &'static str,
}

/// YuE2's two halves share no weight: the autoregressive half writes the score
/// and the semantic codes, the flow-matching half renders the sound.
pub const ADAPTER_SLOTS: &[AdapterSlot] =
    &[AdapterSlot { id: "ar", role: "composition" }, AdapterSlot { id: "nar", role: "sound" }];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YueServerLaunchConfig {
    pub executable: PathBuf,
    pub models: YueModelFiles,
    pub host: String,
    pub port: u16,
    pub options: YueServerOptions,
}

/// The ggml backend the engine computes on. The bundled engine loads its
/// backends at run time, so one build serves NVIDIA (CUDA), AMD and Intel
/// (Vulkan) and the processor; `Auto` lets ggml take the best device it finds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputeBackend {
    #[default]
    Auto,
    Cuda,
    Vulkan,
    Cpu,
}

impl ComputeBackend {
    /// The device name `yue-server` takes as `--device`, none for `Auto`.
    pub fn ggml_device(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Cuda => Some("CUDA0"),
            Self::Vulkan => Some("Vulkan0"),
            Self::Cpu => Some("CPU"),
        }
    }
}

/// Launch flags of `yue-server`, as its usage text documents them. They are
/// read once at startup, so changing one restarts the engine.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct YueServerOptions {
    /// The device to compute on, passed as `--device`.
    pub backend: ComputeBackend,
    /// `--keep-loaded`: keep every module resident instead of evicting
    /// between the AR, NAR and VAE stages.
    pub keep_loaded: bool,
    /// `--max-batch`: songs per request, one KV set each.
    pub max_batch: Option<u32>,
    /// `--max-seq`: KV cache size; defaults to the model context (24576).
    pub max_seq: Option<u32>,
    /// `--vae-core`: VAE tile core frames (upstream default 512).
    pub vae_core: Option<u32>,
    /// `--vae-halo`: VAE tile halo frames (upstream default 16).
    pub vae_halo: Option<u32>,
    /// `--nar-chunk`: the longest acoustic chunk in frames; none is what the
    /// context holds.
    pub nar_chunk: Option<u32>,
    /// `--no-fa`: disable flash attention.
    pub disable_flash_attention: bool,
    /// `--clamp-fp16`: clamp hidden states to the FP16 range.
    pub clamp_fp16: bool,
    /// The folder beside the executable whose `ggml-cuda.dll` the engine
    /// loads, passed as `YUE_CUDA_BACKEND`; none off CUDA. A release keeps
    /// one CUDA backend per toolkit in folders of their own.
    pub cuda_folder: Option<&'static str>,
    /// Anything else the engine accepts as a launch flag, as the line the user
    /// typed it - split like a shell would when the process starts. It is
    /// appended after every flag above, so it can also override one of them.
    pub extra_args: String,
}

impl YueServerOptions {
    pub fn arguments(&self) -> Vec<String> {
        let mut arguments = Vec::new();
        if self.keep_loaded {
            arguments.push("--keep-loaded".into());
        }
        for (flag, value) in [
            ("--max-batch", self.max_batch),
            ("--max-seq", self.max_seq),
            ("--vae-core", self.vae_core),
            ("--vae-halo", self.vae_halo),
            ("--nar-chunk", self.nar_chunk),
        ] {
            if let Some(value) = value {
                arguments.push(flag.into());
                arguments.push(value.to_string());
            }
        }
        if self.disable_flash_attention {
            arguments.push("--no-fa".into());
        }
        if self.clamp_fp16 {
            arguments.push("--clamp-fp16".into());
        }
        // Last, so a line of extra arguments can also override one of the
        // flags the studio set itself - the engine keeps the value it read
        // last.
        match split_extra_args(&self.extra_args) {
            Ok(extra) => arguments.extend(extra),
            // Settings refuses a line that cannot be split, so this is a
            // settings file written by hand: better no extra flags at all
            // than half of them, said where the next start is explained.
            Err(reason) => note_in_log(&format!("the extra engine arguments were not used: {reason}")),
        }
        arguments
    }
}

/// Splits a line of extra launch arguments the way a shell would: on
/// whitespace, with single and double quotes keeping their contents together
/// - `--prompt "a long value"` reaches the engine as one argument - and a
/// backslash escaping the next character outside quotes. An unmatched quote
/// is refused rather than guessed at: it is nearly always a typo, and the
/// engine would silently start with half of the line.
pub fn split_extra_args(input: &str) -> Result<Vec<String>, String> {
    let mut arguments = Vec::new();
    let mut current = String::new();
    // Whether anything has gone into the current argument, so an empty `""`
    // still counts as one argument of its own.
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut characters = input.chars();
    while let Some(character) = characters.next() {
        match quote {
            Some(delimiter) => {
                if character == delimiter {
                    quote = None;
                } else if character == '\\' && delimiter == '"' {
                    if let Some(escaped) = characters.next() {
                        current.push(escaped);
                    }
                } else {
                    current.push(character);
                }
            }
            None => match character {
                '\'' | '"' => {
                    quote = Some(character);
                    started = true;
                }
                '\\' => {
                    if let Some(escaped) = characters.next() {
                        current.push(escaped);
                        started = true;
                    }
                }
                _ if character.is_whitespace() => {
                    if started {
                        arguments.push(std::mem::take(&mut current));
                        started = false;
                    }
                }
                _ => {
                    current.push(character);
                    started = true;
                }
            },
        }
    }
    if let Some(delimiter) = quote {
        return Err(format!("the extra engine arguments have an unmatched {delimiter} quote"));
    }
    if started {
        arguments.push(current);
    }
    Ok(arguments)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YueServerLocation {
    /// Directory of the bundled runtime, not the studio root.
    pub bundle_root: PathBuf,
    /// Explicit executable; takes priority over the bundled one.
    pub configured_executable: Option<PathBuf>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub options: YueServerOptions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartOutcome {
    ReusedHealthyServer,
    Started,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StopOutcome {
    pub was_running: bool,
    pub graceful: bool,
}

pub struct YueServerSupervisor {
    config: YueServerLaunchConfig,
    child: Option<Child>,
}

impl YueServerLocation {
    pub fn executable(&self) -> Result<PathBuf> {
        match &self.configured_executable {
            Some(path) => canonical_file(path, "configured yue-server executable"),
            None => locate_bundled_executable(&canonical_directory(&self.bundle_root, "yue-server bundle root")?),
        }
    }

    pub fn resolve(self, models: YueModelFiles) -> Result<YueServerLaunchConfig> {
        let executable = self.executable()?;
        let models = YueModelFiles {
            backbone: canonical_file(&models.backbone, "YuE2 backbone")?,
            vae: canonical_file(&models.vae, "YuE2 VAE")?,
            transcriber: models
                .transcriber
                .map(|path| canonical_file(&path, "SheetSage2 transcriber"))
                .transpose()?,
            adapters: models.adapters.map(|path| canonical_directory(&path, "adapter folder")).transpose()?,
            companion: canonical_file(&models.companion, "YuE2 decoder companion")?,
        };
        let host = self.host.unwrap_or_else(|| DEFAULT_HOST.into());
        validate_loopback_host(&host)?;
        Ok(YueServerLaunchConfig { executable, models, host, port: self.port.unwrap_or(DEFAULT_PORT), options: self.options })
    }
}

impl YueServerLaunchConfig {
    /// The CUDA backend the engine loads. A developer build keeps its one
    /// `ggml-cuda.dll` beside the executable, which ggml finds by itself; a
    /// release has only the folders, and one missing is a broken install.
    pub fn cuda_backend(&self) -> Result<Option<PathBuf>> {
        let Some(folder) = self.options.cuda_folder else { return Ok(None) };
        let directory = self.executable.parent().context("the engine executable has no folder")?;
        let backend = directory.join(folder).join("ggml-cuda.dll");
        if backend.is_file() {
            return Ok(Some(backend));
        }
        if directory.join("ggml-cuda.dll").is_file() {
            return Ok(None);
        }
        bail!("the engine's CUDA backend {} is missing; reinstall the studio", backend.display())
    }

    pub fn arguments(&self) -> Vec<std::ffi::OsString> {
        let mut arguments: Vec<std::ffi::OsString> = vec![
            "--model".into(),
            strip_verbatim(&self.models.backbone).into(),
            "--vae".into(),
            strip_verbatim(&self.models.vae).into(),
            "--companion".into(),
            strip_verbatim(&self.models.companion).into(),
        ];
        if let Some(transcriber) = &self.models.transcriber {
            arguments.push("--transcriber".into());
            arguments.push(strip_verbatim(transcriber).into());
        }
        if let Some(adapters) = &self.models.adapters {
            arguments.push("--adapters".into());
            arguments.push(strip_verbatim(adapters).into());
        }
        arguments.extend(["--host".into(), self.host.clone().into(), "--port".into(), self.port.to_string().into()]);
        arguments.extend(self.options.arguments().into_iter().map(Into::into));
        arguments
    }
}

impl YueServerSupervisor {
    pub fn new(config: YueServerLaunchConfig) -> Result<Self> {
        canonical_file(&config.executable, "yue-server executable")?;
        validate_loopback_host(&config.host)?;
        Ok(Self { config, child: None })
    }

    pub fn config(&self) -> &YueServerLaunchConfig {
        &self.config
    }

    pub fn is_healthy(&self, timeout: Duration) -> bool {
        health_check(&self.config.host, self.config.port, timeout)
    }

    /// Starts one owned server unless a healthy one already answers on the
    /// loopback endpoint. A live owned child still initialising is waited for,
    /// never duplicated.
    pub fn ensure_started(&mut self, readiness_timeout: Duration) -> Result<StartOutcome> {
        if self.is_healthy(Duration::from_millis(300)) {
            return Ok(StartOutcome::ReusedHealthyServer);
        }
        self.clear_exited_child()?;
        if self.child.is_some() {
            self.wait_until_healthy(readiness_timeout)?;
            return Ok(StartOutcome::Started);
        }

        let mut command = Command::new(&self.config.executable);
        if let Some(path) = music_core::process::own_libraries_first(&self.config.executable) {
            command.env("LD_LIBRARY_PATH", path);
        }
        command.args(self.config.arguments());
        match self.config.cuda_backend()? {
            Some(backend) => {
                command.env("YUE_CUDA_BACKEND", backend);
            }
            None => {
                command.env_remove("YUE_CUDA_BACKEND");
            }
        }
        if let Some(device) = self.config.options.backend.ggml_device() {
            command.arg("--device").arg(device);
        }
        if let Some(directory) = self.config.executable.parent() {
            command.current_dir(directory);
        }
        command.stdin(Stdio::null());
        // Appended, never truncated: the reason a run failed is in the last
        // lines of the process that failed, so they must outlive it.
        trim_log_if_huge();
        note_in_log(&format!("---- starting {} ----", self.config.executable.display()));
        match fs::OpenOptions::new().create(true).append(true).open(startup_log_path()) {
            Ok(log) => {
                let err = log.try_clone().ok();
                command.stdout(Stdio::from(log));
                match err {
                    Some(handle) => {
                        command.stderr(Stdio::from(handle));
                    }
                    None => {
                        command.stderr(Stdio::null());
                    }
                }
            }
            Err(_) => {
                command.stdout(Stdio::null());
                command.stderr(Stdio::null());
            }
        }
        configure_child_process(&mut command);
        let child = command
            .spawn()
            .with_context(|| format!("start yue-server {}", self.config.executable.display()))?;
        // The engine must not outlive the studio, however the studio ends.
        music_core::process::adopt(&child);
        self.child = Some(child);
        if let Err(error) = self.wait_until_healthy(readiness_timeout) {
            let _ = self.stop(Duration::from_secs(1));
            return Err(error);
        }
        Ok(StartOutcome::Started)
    }

    /// Asks for the upstream graceful shutdown, then forces the process down
    /// after the grace period so no engine is left holding the card.
    pub fn stop(&mut self, grace_period: Duration) -> Result<StopOutcome> {
        let Some(child) = self.child.as_mut() else {
            return Ok(StopOutcome { was_running: false, graceful: true });
        };
        if let Some(status) = child.try_wait().context("inspect yue-server child")? {
            note_in_log(&format!(
                "the engine had already exited: {status}{}",
                status.code().map(|code| format!(" (0x{:x})", code as u32)).unwrap_or_default()
            ));
            self.child = None;
            return Ok(StopOutcome { was_running: false, graceful: true });
        }

        let asked = request_graceful_shutdown(child);
        if let Err(error) = &asked {
            note_in_log(&format!("graceful stop was not delivered ({error}); forcing it"));
        }
        let deadline = if asked.is_ok() { Instant::now() + grace_period } else { Instant::now() };
        while Instant::now() < deadline {
            if child.try_wait().context("wait for yue-server shutdown")?.is_some() {
                self.child = None;
                return Ok(StopOutcome { was_running: true, graceful: true });
            }
            thread::sleep(Duration::from_millis(50));
        }
        child.kill().context("force-stop yue-server after grace period")?;
        child.wait().context("wait for force-stopped yue-server")?;
        self.child = None;
        Ok(StopOutcome { was_running: true, graceful: false })
    }

    /// Whether the engine this supervisor started is still a running process: one inside a long step can miss a
    /// health probe and still be at work.
    pub fn process_alive(&mut self) -> bool {
        self.child.as_mut().is_some_and(|child| matches!(child.try_wait(), Ok(None)))
    }

    fn clear_exited_child(&mut self) -> Result<()> {
        if let Some(child) = self.child.as_mut() {
            if let Some(status) = child.try_wait().context("inspect yue-server child")? {
                note_in_log(&format!(
                    "the previous engine had exited: {status}{}",
                    status.code().map(|code| format!(" (0x{:x})", code as u32)).unwrap_or_default()
                ));
                self.child = None;
            }
        }
        Ok(())
    }

    fn wait_until_healthy(&mut self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.is_healthy(Duration::from_millis(300)) {
                return Ok(());
            }
            if let Some(child) = self.child.as_mut() {
                if let Some(status) = child.try_wait().context("inspect starting yue-server")? {
                    // a crash says nothing of where: Windows' record of it names the module
                    let fault = music_core::process::exited_on_exception(status.code()).then(|| music_core::process::fault_of(child.id(), Duration::from_secs(5))).flatten();
                    match fault {
                        Some(fault) => {
                            note_in_log(&format!("the engine crashed during startup: {fault}"));
                            bail!("yue-server exited during startup with status {status}; {fault}");
                        }
                        None => bail!("yue-server exited during startup with status {status}"),
                    }
                }
            }
            if Instant::now() >= deadline {
                bail!(
                    "yue-server did not pass GET /health on {}:{} before the startup timeout",
                    self.config.host,
                    self.config.port
                );
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for YueServerSupervisor {
    fn drop(&mut self) {
        let _ = self.stop(Duration::from_secs(2));
    }
}

fn locate_bundled_executable(bundle_root: &Path) -> Result<PathBuf> {
    let candidates = [
        bundle_root.join(EXECUTABLE),
        bundle_root.join("bin").join(EXECUTABLE),
        bundle_root.join("build").join("Release").join(EXECUTABLE),
        bundle_root.join("build").join(EXECUTABLE),
    ];
    candidates
        .iter()
        .find(|path| path.is_file())
        .map(|path| canonical_file(path, "bundled yue-server executable"))
        .transpose()?
        .with_context(|| format!("{EXECUTABLE} was not found below {}", bundle_root.display()))
}

fn canonical_file(path: &Path, label: &str) -> Result<PathBuf> {
    if !path.is_file() {
        bail!("{label} is not a file: {}", path.display());
    }
    fs::canonicalize(path).with_context(|| format!("canonicalize {label}: {}", path.display()))
}

fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf> {
    if !path.is_dir() {
        bail!("{label} is not a directory: {}", path.display());
    }
    fs::canonicalize(path).with_context(|| format!("canonicalize {label}: {}", path.display()))
}

/// `canonicalize` on Windows yields `\\?\C:\...`, which the engine's C file
/// APIs accept but its log prints verbatim; the plain form reads better and
/// is equally valid for paths under MAX_PATH.
pub fn strip_verbatim(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

fn validate_loopback_host(host: &str) -> Result<()> {
    let address: IpAddr = host
        .parse()
        .with_context(|| format!("yue-server host must be an IP address, got `{host}`"))?;
    if !address.is_loopback() {
        bail!("yue-server host must remain loopback-only, got `{host}`");
    }
    Ok(())
}

fn health_check(host: &str, port: u16, timeout: Duration) -> bool {
    http_get(host, port, "/health", timeout).is_some_and(|response| response.starts_with("HTTP/1.1 200"))
}

/// Whether the server on `port` is a yue-server: its /props is a JSON object naming its backbone and VAE, which a
/// program that merely answers /health does not give.
fn answers_as_engine(host: &str, port: u16, timeout: Duration) -> bool {
    http_get(host, port, "/props", timeout).is_some_and(|response| response.starts_with("HTTP/1.1 200") && response.contains("\"model\"") && response.contains("\"vae\""))
}

/// The whole answer of a plain HTTP/1.1 GET of `path` on loopback, none when nothing answers within `timeout`.
fn http_get(host: &str, port: u16, path: &str, timeout: Duration) -> Option<String> {
    let address = host.parse::<IpAddr>().ok()?;
    let mut stream = TcpStream::connect_timeout(&SocketAddr::new(address, port), timeout).ok()?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    stream.write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").as_bytes()).ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    Some(response)
}

#[cfg(windows)]
fn configure_child_process(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;
    // The child shares the studio's hidden console, which is what lets
    // CTRL_BREAK reach it for a graceful stop.
    command.creation_flags(CREATE_NEW_PROCESS_GROUP);
    // The job object `adopt` assigns is what kills it with us on Windows.
}

/// Beyond the platform group flags, the engine is bound to the studio's
/// lifetime: Windows does that with a job object in `adopt`, Linux with
/// `PR_SET_PDEATHSIG` set here, before the exec.
#[cfg(not(windows))]
fn configure_child_process(command: &mut Command) {
    music_core::process::ensure_dies_with_parent(command);
}

#[cfg(windows)]
fn request_graceful_shutdown(child: &Child) -> Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::{
        Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT},
        Threading::GetProcessId,
    };
    let process_group = unsafe { GetProcessId(child.as_raw_handle()) };
    if unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, process_group) } == 0 {
        bail!("send CTRL_BREAK to yue-server process group failed")
    }
    Ok(())
}

#[cfg(not(windows))]
fn request_graceful_shutdown(child: &Child) -> Result<()> {
    let result = unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
    if result != 0 {
        bail!("send SIGTERM to yue-server failed: {}", std::io::Error::last_os_error());
    }
    Ok(())
}

/// Writes one line of the studio's own into the engine log, so a reader can
/// see what the studio did between two runs of the engine.
pub fn note_in_log(line: &str) {
    if let Ok(mut log) = fs::OpenOptions::new().create(true).append(true).open(startup_log_path()) {
        let _ = writeln!(log, "[studio] {line}");
    }
}

/// Past eight megabytes the older half of the log goes.
fn trim_log_if_huge() {
    const LIMIT: u64 = 8 * 1024 * 1024;
    let path = startup_log_path();
    let Ok(meta) = fs::metadata(&path) else { return };
    if meta.len() < LIMIT {
        return;
    }
    let Ok(text) = fs::read_to_string(&path) else { return };
    let keep = text.split_at(text.len() / 2).1;
    let start = keep.find('\n').map(|index| index + 1).unwrap_or(0);
    let _ = fs::write(&path, &keep[start..]);
}

/// The engine log lives beside the studio's data, so the first-run screen can
/// read it before the engine serves anything itself.
pub fn startup_log_path() -> PathBuf {
    let root = std::env::var_os("YUE_STUDIO_DATA_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let _ = fs::create_dir_all(&root);
    root.join("engine.log")
}

/// The tail of the engine log, oldest first.
pub fn startup_log_tail(lines: usize) -> Vec<String> {
    let Ok(bytes) = fs::read(startup_log_path()) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    let all: Vec<&str> = text.lines().filter(|line| !line.trim().is_empty()).collect();
    all.iter().rev().take(lines).rev().map(|line| (*line).to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_compute_backend_names_the_device_ggml_reads() {
        assert_eq!(ComputeBackend::Auto.ggml_device(), None);
        assert_eq!(ComputeBackend::Cuda.ggml_device(), Some("CUDA0"));
        assert_eq!(ComputeBackend::Vulkan.ggml_device(), Some("Vulkan0"));
        assert_eq!(ComputeBackend::Cpu.ggml_device(), Some("CPU"));
        assert_eq!(serde_json::to_value(ComputeBackend::Vulkan).unwrap(), "vulkan");
    }

    fn fresh_directory(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("yue-engine-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn resolves_bundled_runtime_and_names_every_model_on_the_command_line() {
        let root = fresh_directory("runtime");
        fs::write(root.join(EXECUTABLE), b"test").unwrap();
        for name in ["b.gguf", "v.gguf", "t.gguf", "c.safetensors"] {
            fs::write(root.join(name), b"gguf").unwrap();
        }
        fs::create_dir_all(root.join("adapters")).unwrap();
        let config = YueServerLocation {
            bundle_root: root.clone(),
            configured_executable: None,
            host: None,
            port: None,
            options: YueServerOptions { keep_loaded: true, max_batch: Some(2), ..Default::default() },
        }
        .resolve(YueModelFiles {
            backbone: root.join("b.gguf"),
            vae: root.join("v.gguf"),
            transcriber: Some(root.join("t.gguf")),
            adapters: Some(root.join("adapters")),
            companion: root.join("c.safetensors"),
        })
        .unwrap();
        assert_eq!(config.port, DEFAULT_PORT);
        let arguments: Vec<String> = config.arguments().iter().map(|value| value.to_string_lossy().into_owned()).collect();
        let flag = |name: &str| arguments.iter().position(|value| value == name).unwrap();
        assert!(arguments[flag("--model") + 1].ends_with("b.gguf"));
        assert!(arguments[flag("--vae") + 1].ends_with("v.gguf"));
        assert!(arguments[flag("--transcriber") + 1].ends_with("t.gguf"));
        assert!(arguments[flag("--adapters") + 1].ends_with("adapters"));
        assert!(arguments[flag("--companion") + 1].ends_with("c.safetensors"));
        assert_eq!(arguments[flag("--port") + 1], DEFAULT_PORT.to_string());
        assert!(arguments.contains(&"--keep-loaded".to_string()));
        assert_eq!(arguments[flag("--max-batch") + 1], "2");
        assert!(!arguments.iter().any(|value| value.starts_with(r"\\?\")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_cuda_backend_comes_from_its_folder_or_a_developer_build() {
        let root = fresh_directory("cuda-backend");
        fs::write(root.join(EXECUTABLE), b"test").unwrap();
        let config = |folder| YueServerLaunchConfig {
            executable: root.join(EXECUTABLE),
            models: YueModelFiles { backbone: root.join("b.gguf"), vae: root.join("v.gguf"), transcriber: None, adapters: None, companion: root.join("c.safetensors") },
            host: DEFAULT_HOST.into(),
            port: DEFAULT_PORT,
            options: YueServerOptions { cuda_folder: folder, ..Default::default() },
        };
        assert_eq!(config(None).cuda_backend().unwrap(), None);
        // A release without the folder is broken, never quietly off CUDA.
        assert!(config(Some("cuda12")).cuda_backend().is_err());
        fs::create_dir_all(root.join("cuda12")).unwrap();
        fs::write(root.join("cuda12").join("ggml-cuda.dll"), b"x").unwrap();
        assert_eq!(config(Some("cuda12")).cuda_backend().unwrap(), Some(root.join("cuda12").join("ggml-cuda.dll")));
        // A developer build: the one backend beside the executable.
        fs::write(root.join("ggml-cuda.dll"), b"x").unwrap();
        assert_eq!(config(Some("cuda13")).cuda_backend().unwrap(), None);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn options_become_the_documented_flags() {
        let arguments = YueServerOptions {
            backend: ComputeBackend::Vulkan,
            keep_loaded: true,
            max_batch: Some(2),
            max_seq: Some(8192),
            vae_core: Some(256),
            vae_halo: Some(16),
            nar_chunk: Some(2560),
            disable_flash_attention: true,
            clamp_fp16: true,
            cuda_folder: None,
            extra_args: String::new(),
        }
        .arguments();
        assert_eq!(
            arguments,
            vec![
                "--keep-loaded", "--max-batch", "2", "--max-seq", "8192", "--vae-core", "256", "--vae-halo", "16", "--nar-chunk", "2560", "--no-fa",
                "--clamp-fp16"
            ]
        );
        assert!(YueServerOptions::default().arguments().is_empty());

        // A line of extra arguments arrives last, split the way a shell would
        // - a quoted value stays one argument - and can therefore also
        // override a flag the studio set above it.
        let extra = YueServerOptions { keep_loaded: true, extra_args: "--clown-mode --prompt \"a long value\"".into(), ..Default::default() }.arguments();
        assert_eq!(extra, vec!["--keep-loaded", "--clown-mode", "--prompt", "a long value"]);
    }

    #[test]
    fn an_extra_argument_line_is_split_like_a_shell_would() {
        assert_eq!(split_extra_args("").unwrap(), Vec::<String>::new());
        assert_eq!(split_extra_args("   \n ").unwrap(), Vec::<String>::new());
        assert_eq!(split_extra_args("--flag value").unwrap(), vec!["--flag", "value"]);
        assert_eq!(split_extra_args("--flag=value --other").unwrap(), vec!["--flag=value", "--other"]);
        assert_eq!(split_extra_args("--prompt 'two words' --n=1").unwrap(), vec!["--prompt", "two words", "--n=1"]);
        assert_eq!(split_extra_args("--prompt \"two words\"").unwrap(), vec!["--prompt", "two words"]);
        assert_eq!(split_extra_args("--prompt two\\ words").unwrap(), vec!["--prompt", "two words"]);
        assert_eq!(split_extra_args("--empty \"\"").unwrap(), vec!["--empty", ""]);
        assert!(split_extra_args("--prompt \"unterminated").is_err());
        assert!(split_extra_args("--prompt 'unterminated").is_err());
    }

    #[test]
    fn a_missing_model_is_refused_before_anything_starts() {
        let root = fresh_directory("missing");
        fs::write(root.join(EXECUTABLE), b"test").unwrap();
        let result = YueServerLocation {
            bundle_root: root.clone(),
            configured_executable: None,
            host: None,
            port: None,
            options: YueServerOptions::default(),
        }
        .resolve(YueModelFiles {
            backbone: root.join("absent.gguf"),
            vae: root.join("absent.gguf"),
            transcriber: None,
            adapters: None,
            companion: root.join("absent.safetensors"),
        });
        assert!(result.unwrap_err().to_string().contains("backbone"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_non_loopback_listener() {
        assert!(validate_loopback_host("0.0.0.0").is_err());
        assert!(validate_loopback_host("127.0.0.1").is_ok());
    }

    #[test]
    fn a_port_another_program_holds_gives_way_to_a_free_one_with_the_holder_named() {
        let held = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let wanted = held.local_addr().unwrap().port();
        let (port, note) = choose_port(wanted);
        assert_ne!(port, wanted);
        let note = note.expect("a reason for the other port");
        assert!(note.contains(&format!("port {wanted} is not available")) && note.contains(&format!("listens on {port} instead")), "{note}");
        #[cfg(windows)]
        assert!(note.contains(&format!("PID {}", std::process::id())), "the holder is named: {note}");
        assert!(std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).is_ok(), "the port given is free");
        drop(held);
        assert_eq!(choose_port(wanted), (wanted, None), "a free port is the one wanted");
    }

    #[test]
    fn a_port_in_a_reserved_range_is_found_in_the_listing() {
        let listing = "\nProtocol tcp Port Exclusion Ranges\n\nStart Port    End Port\n----------    --------\n      5357        5357\n     18040       18139\n     50000       50059     *\n\n* - Administered port exclusions.\n";
        assert_eq!(excluded_range_in(listing, 18087), Some((18040, 18139)));
        assert_eq!(excluded_range_in(listing, 50000), Some((50000, 50059)));
        assert_eq!(excluded_range_in(listing, 9000), None);
    }

    #[test]
    fn health_check_is_false_without_a_server() {
        assert!(!health_check("127.0.0.1", 65534, Duration::from_millis(10)));
    }
}
