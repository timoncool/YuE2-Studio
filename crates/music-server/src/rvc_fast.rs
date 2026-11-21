//! Optional native RVC runtime. Each process owns its CUDA context and keeps
//! the last voice's compiled graphs between requests.

use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Stdio},
    sync::{Mutex, mpsc},
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::json;

static WORKER: Mutex<Option<Worker>> = Mutex::new(None);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(12);
const CONVERSION_TIMEOUT: Duration = Duration::from_secs(15 * 60);

pub struct Request<'a> {
    pub vocal: &'a Path,
    pub voice: &'a Path,
    pub index: Option<&'a Path>,
    pub semitone_shift: i32,
    pub retrieval_blend: f64,
}

pub fn availability(data_root: &Path) -> Result<(), String> {
    let (runtime, cache) = runtime_paths(data_root).map_err(|error| error.to_string())?;
    let mut worker = WORKER
        .lock()
        .map_err(|_| "The native RVC worker is unavailable.".to_string())?;
    ensure_worker(&mut worker, runtime, &cache)
        .map(|_| ())
        .map_err(|error| format!("{error:#}"))
}

/// Reports installation and an already-running worker without starting one.
pub fn status(data_root: &Path) -> Result<bool, String> {
    let (runtime, _) = runtime_paths(data_root).map_err(|error| error.to_string())?;
    let Ok(mut slot) = WORKER.try_lock() else {
        return Ok(false);
    };
    let Some(worker) = slot.as_mut().filter(|worker| worker.runtime == runtime) else {
        return Ok(false);
    };
    worker
        .child
        .try_wait()
        .map(|exit| exit.is_none())
        .map_err(|error| error.to_string())
}

pub fn convert(data_root: &Path, request: &Request<'_>) -> Result<audio_post::Stereo> {
    let (runtime, cache) = runtime_paths(data_root)?;
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
    let transport = ensure_worker(&mut worker, runtime, &cache)?.roundtrip(&ipc);
    let reply = match transport {
        Ok(reply) => reply,
        Err(error) => {
            *worker = None;
            return Err(error);
        }
    };
    drop(worker);
    if !reply.ok {
        bail!(
            "Native RVC conversion failed: {}",
            reply.error.as_deref().unwrap_or("unknown error")
        );
    }
    let audio = crate::audio_pcm::decode_stereo(&output).context("Read native RVC output WAV")?;
    if audio.frames() == 0 {
        bail!("Native RVC returned an empty WAV.");
    }
    Ok(audio)
}

fn runtime_paths(data_root: &Path) -> Result<(PathBuf, PathBuf)> {
    if !cfg!(any(target_os = "windows", target_os = "linux")) || !cfg!(target_arch = "x86_64") {
        bail!("Native TensorRT-RTX is available on x86-64 Windows and Linux.");
    }
    let root = data_root.join("rvc/native");
    let runtime = root.join("v1");
    let binary = format!("vocal-fast{}", std::env::consts::EXE_SUFFIX);
    for required in [&binary, "hubert.vfrm", "rmvpe.vfrm"] {
        if !runtime.join(required).is_file() {
            bail!("The native RVC component is missing {required}.");
        }
    }
    Ok((
        runtime.canonicalize()?,
        root.canonicalize()?.join("cache-v1"),
    ))
}

fn ensure_worker<'a>(
    slot: &'a mut Option<Worker>,
    runtime: PathBuf,
    cache: &Path,
) -> Result<&'a mut Worker> {
    if let Some(worker) = slot
        && (worker.runtime != runtime || worker.child.try_wait()?.is_some())
    {
        *slot = None;
    }
    if slot.is_none() {
        *slot = Some(Worker::start(runtime, cache)?);
    }
    slot.as_mut().context("Native RVC worker failed to start")
}

#[derive(Deserialize)]
struct Hello {
    protocol: u32,
    backend: String,
    version: String,
}

#[derive(Deserialize)]
struct Reply {
    ok: bool,
    error: Option<String>,
}

struct Worker {
    runtime: PathBuf,
    child: Child,
    input: ChildStdin,
    output: mpsc::Receiver<std::io::Result<String>>,
}

impl Worker {
    fn start(runtime: PathBuf, cache: &Path) -> Result<Self> {
        let exe = runtime.join(format!("vocal-fast{}", std::env::consts::EXE_SUFFIX));
        let mut command = dub_core::proc::host_command(&exe);
        command
            .args([
                std::ffi::OsStr::new("yue2-worker"),
                runtime.as_os_str(),
                cache.as_os_str(),
            ])
            .current_dir(&runtime)
            .env("NVIDIA_TF32_OVERRIDE", "0")
            .env("RUST_LOG", "warn")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
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
        let mut child = music_core::process::spawn_dying_with_parent(command).context(
            "Start native RVC process; check the TensorRT-RTX runtime and NVIDIA driver",
        )?;
        music_core::process::adopt(&child);
        let stdout = child.stdout.take().expect("worker stdout is piped");
        let input = child.stdin.take().expect("worker stdin is piped");
        let (sender, output) = mpsc::sync_channel(1);
        let worker = Self {
            child,
            input,
            output,
            runtime,
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
        let hello = worker.receive::<Hello>(HANDSHAKE_TIMEOUT).context(
            "Native RVC device self-test failed; check the NVIDIA driver and TensorRT runtime",
        )?;
        if hello.protocol != 1 || hello.backend != "tensorrt-rtx" || hello.version != "0.1.7" {
            bail!("The native RVC worker version or backend is incompatible.");
        }
        Ok(worker)
    }

    fn roundtrip(&mut self, request: &serde_json::Value) -> Result<Reply> {
        serde_json::to_writer(&mut self.input, request)?;
        self.input.write_all(b"\n")?;
        self.input.flush()?;
        self.receive(CONVERSION_TIMEOUT)
            .context("Native RVC worker ended or did not respond")
    }

    fn receive<T: for<'de> Deserialize<'de>>(&self, timeout: Duration) -> Result<T> {
        let line = self.output.recv_timeout(timeout)??;
        Ok(serde_json::from_str(&line)?)
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
