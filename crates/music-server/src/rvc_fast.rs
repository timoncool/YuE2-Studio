//! Optional native RVC runtime. Each process owns its backend and keeps
//! the last voice's compiled graphs between requests.

use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Stdio},
    sync::{Mutex, mpsc},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::json;

use crate::rvc_fast_install::RuntimeBackend;

static WORKER: Mutex<Option<Worker>> = Mutex::new(None);
const METAL_DEVICE_UNAVAILABLE: &str = "No Metal/MPS device with a command queue is available.";
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(12);
const CONVERSION_TIMEOUT: Duration = Duration::from_secs(15 * 60);

pub struct Request<'a> {
    pub vocal: &'a Path,
    pub voice: &'a Path,
    pub index: Option<&'a Path>,
    pub semitone_shift: i32,
    pub retrieval_blend: f64,
}

pub fn availability(data_root: &Path, metal: bool) -> Result<(), String> {
    let (runtime, cache, backend) =
        runtime_paths(data_root, metal).map_err(|error| error.to_string())?;
    let mut worker = WORKER
        .lock()
        .map_err(|_| "The native RVC worker is unavailable.".to_string())?;
    ensure_worker(&mut worker, runtime, &cache, backend)
        .map(|_| ())
        .map_err(|error| format!("{error:#}"))
}

/// Reports installation and an already-running worker without starting one.
pub fn status(data_root: &Path, metal: bool) -> Result<bool, String> {
    let (runtime, _, backend) =
        runtime_paths(data_root, metal).map_err(|error| error.to_string())?;
    let Ok(mut slot) = WORKER.try_lock() else {
        return Ok(false);
    };
    let Some(worker) = slot
        .as_mut()
        .filter(|worker| worker.runtime == runtime && worker.device == device(backend))
    else {
        return Ok(false);
    };
    if !worker.device_available {
        return Err(METAL_DEVICE_UNAVAILABLE.into());
    }
    worker
        .child
        .try_wait()
        .map(|exit| exit.is_none())
        .map_err(|error| error.to_string())
}

#[derive(Debug)]
pub(super) struct BackendFailure(String);

impl std::fmt::Display for BackendFailure {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(&self.0)
    }
}
impl std::error::Error for BackendFailure {}

fn from_worker_failure(message: &str) -> anyhow::Error {
    let reason = message.to_ascii_lowercase();
    if !reason.contains("cancel")
        && !reason.contains("stopped")
        && [
            "tensorrt-rtx api failed",
            "engine backend is unavailable",
            "cuda out of memory",
            "cuda runtime error",
        ]
        .iter()
        .any(|diagnostic| reason.contains(diagnostic))
    {
        BackendFailure(message.to_owned()).into()
    } else {
        anyhow!("Native RVC conversion failed: {message}")
    }
}

pub fn convert(
    data_root: &Path,
    metal: bool,
    request: &Request<'_>,
    cancelled: &impl Fn() -> bool,
) -> Result<audio_post::Stereo> {
    anyhow::ensure!(!cancelled(), "Native RVC conversion cancelled.");
    let (runtime, cache, backend) =
        runtime_paths(data_root, metal).map_err(|error| BackendFailure(format!("{error:#}")))?;
    std::fs::create_dir_all(&cache).context("Create native RVC cache")?;
    let handoff = tempfile::tempdir_in(&cache)?;
    let output = handoff.path().join("output.wav");
    let ipc = json!({
        "vocal": request.vocal,
        "voice": request.voice,
        "index": request.index,
        "voice_sha256": crate::install_record::sha256_of(request.voice)?,
        "index_sha256": request.index.map(crate::install_record::sha256_of).transpose()?,
        "semitone_shift": request.semitone_shift,
        "retrieval_blend": request.retrieval_blend,
        "output": output,
    });
    let mut worker = WORKER
        .lock()
        .map_err(|_| anyhow!("The native RVC worker is unavailable."))?;
    let transport = ensure_worker(&mut worker, runtime, &cache, backend)
        .map_err(|error| BackendFailure(format!("{error:#}")))?
        .roundtrip(&ipc, cancelled);
    let reply = match transport {
        Ok(reply) => reply,
        Err(error) => {
            *worker = None;
            if cancelled() {
                return Err(error);
            }
            return Err(BackendFailure(format!("{error:#}")).into());
        }
    };
    drop(worker);
    if !reply.ok {
        return Err(from_worker_failure(
            reply.error.as_deref().unwrap_or("unknown error"),
        ));
    }
    let audio = crate::audio_pcm::decode_stereo(&output).context("Read native RVC output WAV")?;
    if audio.frames() == 0 {
        bail!("Native RVC returned an empty WAV.");
    }
    Ok(audio)
}

fn runtime_paths(data_root: &Path, metal: bool) -> Result<(PathBuf, PathBuf, RuntimeBackend)> {
    let root = data_root.join("rvc/native");
    let runtime = crate::rvc_fast_install::installed_worker(data_root, metal)
        .map_err(|error| anyhow!(error))?
        .context("Install the native RVC runtime and acoustic models first.")?;
    let variant = runtime
        .directory
        .file_name()
        .context("Worker has no runtime directory")?;
    Ok((
        runtime.directory.canonicalize()?,
        root.canonicalize()?.join("cache-v1").join(variant),
        runtime.backend,
    ))
}

fn device(backend: RuntimeBackend) -> Option<std::ffi::OsString> {
    if backend == RuntimeBackend::TensorRtRtx {
        return std::env::var_os("CUDA_VISIBLE_DEVICES");
    }
    None
}

fn ensure_worker<'a>(
    slot: &'a mut Option<Worker>,
    runtime: PathBuf,
    cache: &Path,
    backend: RuntimeBackend,
) -> Result<&'a mut Worker> {
    if let Some(worker) = slot
        && (worker.runtime != runtime
            || !worker.device_available
            || worker.device != device(backend)
            || worker.child.try_wait()?.is_some())
    {
        *slot = None;
    }
    if slot.is_none() {
        *slot = Some(Worker::start(runtime, cache, backend)?);
    }
    let worker = slot.as_mut().context("Native RVC worker failed to start")?;
    anyhow::ensure!(worker.device_available, METAL_DEVICE_UNAVAILABLE);
    Ok(worker)
}

#[derive(Deserialize)]
struct Hello {
    protocol: u32,
    backend: String,
    version: String,
    device_available: Option<bool>,
}

#[derive(Deserialize)]
struct Reply {
    ok: bool,
    error: Option<String>,
}

struct Worker {
    runtime: PathBuf,
    device: Option<std::ffi::OsString>,
    device_available: bool,
    child: Child,
    input: ChildStdin,
    output: mpsc::Receiver<std::io::Result<String>>,
}

impl Worker {
    fn start(runtime: PathBuf, cache: &Path, backend: RuntimeBackend) -> Result<Self> {
        #[cfg(windows)]
        let launch_dir = windows_launch_dir(&runtime)?;
        #[cfg(not(windows))]
        let launch_dir = runtime.clone();
        let exe = launch_dir.join(format!("vocal-fast{}", std::env::consts::EXE_SUFFIX));
        let models = runtime
            .parent()
            .context("Native RVC model directory missing")?;
        let mut command = dub_core::proc::host_command(&exe);
        command
            .args([
                std::ffi::OsStr::new("yue2-worker"),
                models.as_os_str(),
                cache.as_os_str(),
            ])
            .current_dir(&launch_dir)
            .env("RUST_LOG", "warn")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if backend == RuntimeBackend::TensorRtRtx {
            command
                .env("NVIDIA_TF32_OVERRIDE", "0")
                .env("VOCAL_TRT_PLAN_CACHE_DIR", cache.join("plans"));
        }
        #[cfg(target_os = "macos")]
        command
            .env_remove("DYLD_LIBRARY_PATH")
            .env_remove("DYLD_FALLBACK_LIBRARY_PATH");
        #[cfg(target_os = "linux")]
        {
            // TensorRT opens its companion libraries at runtime. Prepend their
            // private directory to the host environment, outside AppImage.
            let host_paths = command
                .get_envs()
                .find(|(key, _)| *key == "LD_LIBRARY_PATH")
                .map(|(_, value)| value.map(std::ffi::OsStr::to_os_string))
                .unwrap_or_else(|| std::env::var_os("LD_LIBRARY_PATH"))
                .unwrap_or_default();
            let combined = std::env::join_paths(
                std::iter::once(runtime.clone()).chain(std::env::split_paths(&host_paths)),
            )?;
            command.env("LD_LIBRARY_PATH", combined);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = music_core::process::spawn_dying_with_parent(command)
            .with_context(|| format!("Start native RVC process for {}", backend.protocol_name()))?;
        music_core::process::adopt(&child);
        let stdout = child.stdout.take().expect("worker stdout is piped");
        let input = child.stdin.take().expect("worker stdin is piped");
        let (sender, output) = mpsc::sync_channel(1);
        let mut worker = Self {
            child,
            input,
            output,
            runtime,
            device: device(backend),
            device_available: true,
        };
        std::thread::Builder::new()
            .name("rvc-worker-output".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    if sender.send(line).is_err() {
                        break;
                    }
                }
            })?;
        let hello = worker
            .receive::<Hello>(HANDSHAKE_TIMEOUT)
            .with_context(|| {
                format!(
                    "Native RVC {} runtime self-test failed",
                    backend.protocol_name()
                )
            })?;
        if hello.protocol != 1
            || hello.backend != backend.protocol_name()
            || hello.version != "0.1.7"
        {
            bail!("The native RVC worker version or backend is incompatible.");
        }
        if backend == RuntimeBackend::MetalProjections {
            worker.device_available = hello.device_available == Some(true);
        }
        Ok(worker)
    }

    fn roundtrip(
        &mut self,
        request: &serde_json::Value,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Reply> {
        anyhow::ensure!(!cancelled(), "Native RVC conversion cancelled.");
        serde_json::to_writer(&mut self.input, request)?;
        self.input.write_all(b"\n")?;
        self.input.flush()?;
        let deadline = Instant::now() + CONVERSION_TIMEOUT;
        loop {
            anyhow::ensure!(!cancelled(), "Native RVC conversion cancelled.");
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self
                .output
                .recv_timeout(remaining.min(Duration::from_millis(100)))
            {
                Ok(line) => return Ok(serde_json::from_str(&line?)?),
                Err(mpsc::RecvTimeoutError::Timeout) if !remaining.is_zero() => {}
                Err(error) => {
                    return Err(error).context("Native RVC worker ended or did not respond");
                }
            }
        }
    }

    fn receive<T: for<'de> Deserialize<'de>>(&self, timeout: Duration) -> Result<T> {
        let line = self.output.recv_timeout(timeout)??;
        Ok(serde_json::from_str(&line)?)
    }
}

#[cfg(windows)]
fn windows_launch_dir(runtime: &Path) -> Result<PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    if runtime.to_string_lossy().is_ascii() {
        return Ok(runtime.to_owned());
    }

    // TensorRT's Myelin runtime translates the directory of its loaded DLL
    // through an ANSI codepage. The executable and CWD use the existing short
    // alias while Rust opens acoustic models and audio by their Unicode paths.
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetShortPathNameW(path: *const u16, short: *mut u16, capacity: u32) -> u32;
    }
    let path: Vec<u16> = runtime.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: path is NUL-terminated and this call only requests the length.
    let capacity = unsafe { GetShortPathNameW(path.as_ptr(), std::ptr::null_mut(), 0) };
    if capacity != 0 {
        let mut short = vec![0u16; capacity as usize];
        // SAFETY: short holds capacity code units and path remains valid.
        let length = unsafe { GetShortPathNameW(path.as_ptr(), short.as_mut_ptr(), capacity) };
        if length != 0
            && length < capacity
            && short[..length as usize].iter().all(|unit| *unit < 128)
        {
            return Ok(PathBuf::from(std::ffi::OsString::from_wide(
                &short[..length as usize],
            )));
        }
    }
    bail!(
        "This TensorRT runtime needs an ASCII installation path or a Windows short path. Use audio.cpp or place Studio data in an ASCII path."
    )
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod failure_tests {
    use super::*;

    #[test]
    fn retries_backend_errors_and_keeps_input_and_cancellation_errors() {
        assert!(
            from_worker_failure(
                "offline inference: TensorRT-RTX API failed: createExecutionContext"
            )
            .is::<BackendFailure>()
        );
        assert!(!from_worker_failure("failed to decode vocals WAV").is::<BackendFailure>());
        assert!(!from_worker_failure("cancelled after CUDA runtime error").is::<BackendFailure>());
    }
}
