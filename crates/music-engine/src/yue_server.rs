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
    /// The device name `yue-server` reads from `GGML_BACKEND`, none for `Auto`.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct YueServerOptions {
    /// The device to compute on, passed as `GGML_BACKEND`.
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
    /// `--no-fa`: disable flash attention.
    pub disable_flash_attention: bool,
    /// `--clamp-fp16`: clamp hidden states to the FP16 range.
    pub clamp_fp16: bool,
    /// The folder beside the executable whose `ggml-cuda.dll` the engine
    /// loads, passed as `YUE_CUDA_BACKEND`; none off CUDA. A release keeps
    /// one CUDA backend per toolkit in folders of their own.
    pub cuda_folder: Option<&'static str>,
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
        arguments
    }
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
        command.args(self.config.arguments());
        match self.config.cuda_backend()? {
            Some(backend) => {
                command.env("YUE_CUDA_BACKEND", backend);
            }
            None => {
                command.env_remove("YUE_CUDA_BACKEND");
            }
        }
        match self.config.options.backend.ggml_device() {
            Some(device) => {
                command.env("GGML_BACKEND", device);
            }
            None => {
                command.env_remove("GGML_BACKEND");
            }
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
            note_in_log(&format!("the engine had already exited: {status}"));
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
                    bail!("yue-server exited during startup with status {status}");
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
fn strip_verbatim(path: &Path) -> PathBuf {
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
    let Ok(address) = host.parse::<IpAddr>() else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&SocketAddr::new(address, port), timeout) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    if stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut response = String::new();
    stream.read_to_string(&mut response).is_ok() && response.starts_with("HTTP/1.1 200")
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
            disable_flash_attention: true,
            clamp_fp16: true,
            cuda_folder: None,
        }
        .arguments();
        assert_eq!(
            arguments,
            vec!["--keep-loaded", "--max-batch", "2", "--max-seq", "8192", "--vae-core", "256", "--vae-halo", "16", "--no-fa", "--clamp-fp16"]
        );
        assert!(YueServerOptions::default().arguments().is_empty());
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
    fn health_check_is_false_without_a_server() {
        assert!(!health_check("127.0.0.1", 65534, Duration::from_millis(10)));
    }
}
