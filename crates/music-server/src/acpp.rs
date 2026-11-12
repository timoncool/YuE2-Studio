//! audio.cpp's `audiocpp_server` for the studio's tools: HT-Demucs stems, MuScriptor audio to MIDI and SheetSage2
//! scores, through Dub Studio's crate `acpp-server` as Speech Studio and Dub Studio run it: one process per build,
//! the models read where they lie on disk, a model left out of the config until its files are there.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use acpp_server::asset::Build;
use acpp_server::{AcppError, ModelEntry, Server, ServerConfig};
use anyhow::{anyhow, bail, Result};
use serde_json::Value;

use crate::downloads::{Asset, AssetKind, Downloader};

pub const HTDEMUCS: &str = "htdemucs";
/// The id of a MuScriptor size in the server's config.
pub fn muscriptor_id(size: &str) -> String {
    format!("muscriptor-{size}")
}

/// How long a start may take before the server answers `/health` (it loads no model at start).
const READY_WITHIN: Duration = Duration::from_secs(60);

/// The build of the server a device choice runs: `cpu`; `gpu`, the card (its CUDA build: 13, else 12.9; Vulkan on a
/// card of another make); or `auto`: the card when there is one, else the processor. An NVIDIA card runs Vulkan only
/// when it runs neither CUDA build and Auto falls to the processor then. A Mac on Apple silicon has one program, Metal
/// and the processor in it. An error when the card is asked for and there is none it runs on.
pub fn build_for(choice: &str) -> Result<Build> {
    if let Some(mac) = Build::for_this_mac() {
        return Ok(mac);
    }
    let hardware = crate::hardware::hardware();
    let card = match hardware.card_cuda {
        Some(crate::hardware::CudaBuild::Cuda13) => Some(Build::Cuda13),
        Some(crate::hardware::CudaBuild::Cuda12) => Some(Build::Cuda12),
        None if hardware.gpu_name.is_some() && !hardware.nvidia => Some(Build::Vulkan),
        None => None,
    };
    match choice {
        "cpu" => Ok(Build::Cpu),
        "gpu" | "cuda" => card.ok_or_else(|| anyhow!("there is no graphics card here audio.cpp computes on (an NVIDIA card needs a driver that runs CUDA 12.9 or 13); choose Auto or the processor")),
        _ => Ok(card.unwrap_or(Build::Cpu)),
    }
}

/// Whether a build computes on the graphics card.
pub fn on_card(build: Build) -> bool {
    !matches!(build, Build::Cpu)
}

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

fn exe_name() -> &'static str {
    if cfg!(windows) { "audiocpp_server.exe" } else { "audiocpp_server" }
}

/// Every build of the server, published for this platform or not.
pub const BUILDS: [Build; 5] = [Build::Cuda13, Build::Cuda12, Build::Vulkan, Build::Cpu, Build::Metal];

/// The server archive of a build, as the studio's downloader fetches it into `runtime/<build folder>`.
fn server_asset(build: Build) -> Option<&'static Asset> {
    static ASSETS: OnceLock<Vec<(Build, Asset)>> = OnceLock::new();
    let all = ASSETS.get_or_init(|| {
        BUILDS
            .into_iter()
            .filter_map(|build| {
                let pinned = build.server()?;
                Some((
                    build,
                    Asset {
                        id: leak(format!("audiocpp-{}", build.folder())),
                        label: leak(format!("audio.cpp server ({build:?})")),
                        kind: AssetKind::Runtime,
                        url: pinned.url,
                        relative_path: leak(format!("runtime/{}", pinned.file)),
                        bytes: pinned.size,
                        unzip_into: Some(build.folder()),
                        marker: exe_name(),
                        pick: &[],
                        vram_gb: None,
                        note: "Runs HT-Demucs, MuScriptor and SheetSage2: audio.cpp's server, as Speech Studio and Dub Studio run it.",
                    },
                ))
            })
            .collect()
    });
    all.iter().find(|(of, _)| *of == build).map(|(_, asset)| asset)
}

/// cuFFT 12 (CUDA 13.4 Update 2, NVIDIA's redistributable, as Speech Studio pins it), which the CUDA 13 build imports
/// beside cuBLAS 13; picked out of the archive into the build's folder.
#[cfg(windows)]
const CUFFT13: Asset = Asset {
    id: "audiocpp-cufft12",
    label: "NVIDIA cuFFT 12.4",
    kind: AssetKind::Runtime,
    url: "https://developer.download.nvidia.com/compute/cuda/redist/libcufft/windows-x86_64/libcufft-windows-x86_64-12.4.0.43-archive.zip",
    relative_path: "runtime/libcufft-windows-x86_64-12.4.0.43-archive.zip",
    bytes: 159_602_693,
    unzip_into: Some("audiocpp"),
    marker: "cufft64_12.dll",
    pick: &["cufft64_12.dll"],
    vram_gb: None,
    note: "The transforms audio.cpp's CUDA 13 build computes with.",
};

/// The CUDA 12.9 libraries of the CUDA 12 build (`acpp_server::asset::CUDA12_RUNTIME`, PyPI wheels), picked into the
/// build's folder.
#[cfg(windows)]
fn cuda12_assets() -> &'static [Asset] {
    static ASSETS: OnceLock<Vec<Asset>> = OnceLock::new();
    ASSETS.get_or_init(|| {
        let picks: [&'static [&'static str]; 3] = [&["cudart64_12.dll"], &["cublas64_12.dll", "cublasLt64_12.dll"], &["cufft64_11.dll"]];
        acpp_server::asset::CUDA12_RUNTIME
            .iter()
            .zip(picks)
            .map(|(wheel, pick)| Asset {
                id: leak(format!("audiocpp-{}", wheel.file.split('-').next().unwrap_or(wheel.file))),
                label: leak(format!("NVIDIA {}", wheel.file.split('-').next().unwrap_or(wheel.file))),
                kind: AssetKind::Runtime,
                url: wheel.url,
                relative_path: leak(format!("runtime/{}", wheel.file)),
                bytes: wheel.size,
                unzip_into: Some("audiocpp-cuda12"),
                marker: pick[0],
                pick,
                vram_gb: None,
                note: "The CUDA 12.9 libraries audio.cpp's CUDA 12 build computes with.",
            })
            .collect()
    })
}

/// Every download a build of the server needs, the program first.
pub fn assets(build: Build) -> Vec<&'static Asset> {
    let mut assets: Vec<&'static Asset> = server_asset(build).into_iter().collect();
    #[cfg(windows)]
    match build {
        Build::Cuda13 => assets.push(&CUFFT13),
        Build::Cuda12 => assets.extend(cuda12_assets().iter()),
        Build::Vulkan | Build::Cpu | Build::Metal => {}
    }
    assets
}

/// The audio.cpp server of the studio: its downloads in `<data>/audiocpp`, its logs in `<data>/logs/audiocpp`.
pub struct Acpp {
    downloader: Downloader,
    logs: PathBuf,
    /// cuBLAS 13 of the music engine, which the CUDA 13 build imports.
    cuda13_libraries: OnceLock<PathBuf>,
    /// Every model the server may carry, by id. One server per build serves them all: a config that names only the
    /// model of the run at hand would restart the server whenever another tool asks for its own.
    known: std::sync::Mutex<Vec<ModelEntry>>,
    /// Runs at work, by build.
    active: std::sync::Mutex<Vec<(Build, usize)>>,
    /// Signalled whenever a run ends: one waiting for its backend to be free looks again.
    ended: std::sync::Condvar,
}

impl Acpp {
    pub fn new(data_root: &Path) -> Self {
        Self {
            downloader: Downloader::new(data_root.join("audiocpp")),
            logs: data_root.join("logs").join("audiocpp"),
            cuda13_libraries: OnceLock::new(),
            known: std::sync::Mutex::new(Vec::new()),
            active: std::sync::Mutex::new(Vec::new()),
            ended: std::sync::Condvar::new(),
        }
    }

    /// A model the server may carry, in place of an earlier one of the same id.
    pub fn register(&self, entry: ModelEntry) {
        let mut known = self.known.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        known.retain(|model| model.id != entry.id);
        known.push(entry);
    }

    /// The registered models whose files are on disk now: the server reads each configured model's contract when it
    /// starts, and one missing file would stop all of them. A model folder (MuScriptor's) counts once both of its
    /// files are there, since the folder appears with the first of them.
    fn models_on_disk(&self) -> Vec<ModelEntry> {
        let complete = |path: &Path| path.is_file() || ["config.json", "model.safetensors"].iter().all(|file| path.join(file).is_file());
        let known = self.known.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut models: Vec<ModelEntry> = known.iter().filter(|model| complete(&model.path)).cloned().collect();
        models.sort_by(|a, b| a.id.cmp(&b.id));
        models
    }

    pub fn downloader(&self) -> &Downloader {
        &self.downloader
    }

    /// The folder of the cuBLAS 13 the music engine runs on.
    pub fn set_cuda13_libraries(&self, folder: PathBuf) {
        self.cuda13_libraries.set(folder).ok();
    }

    fn folder(&self, build: Build) -> PathBuf {
        self.downloader.runtime_dir(build.folder())
    }

    fn exe(&self, build: Build) -> PathBuf {
        std::env::var_os("YUE_AUDIOCPP_SERVER").map(PathBuf::from).unwrap_or_else(|| self.folder(build).join(exe_name()))
    }

    /// The downloads of a build not installed yet.
    pub fn missing(&self, build: Build) -> Vec<&'static Asset> {
        if std::env::var_os("YUE_AUDIOCPP_SERVER").is_some() {
            return Vec::new();
        }
        assets(build).into_iter().filter(|asset| !self.downloader.is_installed(asset)).collect()
    }

    /// Its downloads are here, and the CUDA libraries it imports are where it finds them: the CUDA 13 build takes the
    /// music engine's cuBLAS 13, which the engine runtime downloads.
    pub fn installed(&self, build: Build) -> bool {
        self.missing(build).is_empty() && (std::env::var_os("YUE_AUDIOCPP_SERVER").is_some() || self.missing_libraries(build).is_empty())
    }

    /// The CUDA libraries a CUDA build imports that are in none of the places Windows loads them from for the server:
    /// beside the program, the music engine's cuBLAS 13, and PATH, which the server inherits (an installed CUDA
    /// Toolkit's own, for which the engine downloads none).
    fn missing_libraries(&self, build: Build) -> Vec<&'static str> {
        let beside = self.folder(build);
        let path: Vec<PathBuf> = std::env::var_os("PATH").map(|path| std::env::split_paths(&path).collect()).unwrap_or_default();
        let mut dirs: Vec<&Path> = vec![&beside];
        if let Some(cuda13) = self.cuda13_libraries.get() {
            dirs.push(cuda13);
        }
        dirs.extend(path.iter().map(PathBuf::as_path));
        acpp_server::asset::missing_libraries(build, &dirs)
    }

    fn config(&self, build: Build, models: Vec<ModelEntry>) -> ServerConfig {
        ServerConfig {
            exe: self.exe(build),
            backend: build.backend(),
            device: 0,
            threads: std::thread::available_parallelism().map(|count| count.get() as u32).unwrap_or(8).min(16),
            models,
            data_dir: self.logs.join(build.folder()),
            log: false,
        }
    }

    /// The server of this build carrying every registered model on disk, `id` among them: the running one when its
    /// models are these, else a new one in its place. An error, before the program starts, when the program or its
    /// CUDA libraries are missing or `id` is not on disk.
    fn server(&self, build: Build, id: &str) -> Result<Arc<Server>> {
        let config = self.config(build, self.models_on_disk());
        if !config.models.iter().any(|entry| entry.id == id) {
            bail!("{id} is not on disk; download it in Settings - Models");
        }
        if !config.exe.is_file() {
            bail!("audio.cpp's server is not installed: {} is missing; download it in Settings - Models", config.exe.display());
        }
        if std::env::var_os("YUE_AUDIOCPP_SERVER").is_none() {
            let missing = self.missing_libraries(build);
            if !missing.is_empty() {
                bail!("audio.cpp's server is missing {} beside it; download it again in Settings - Models", missing.join(", "));
            }
        }
        let cuda13 = self.cuda13_libraries.get().cloned();
        let server = acpp_server::SERVERS
            .get(&config, READY_WITHIN, &|command| {
                // cuBLAS 13 lies beside the music engine, which the studio's PATH already names first
                if let Some(folder) = &cuda13 {
                    let existing = std::env::var_os("PATH").unwrap_or_default();
                    if let Ok(path) = std::env::join_paths(std::iter::once(folder.clone()).chain(std::env::split_paths(&existing))) {
                        command.env("PATH", path);
                    }
                }
            })
            .map_err(|error| anyhow!("{}", describe(&error)))?;
        Ok(server)
    }

    /// The server `choose` gives for a run of `build`, counted as at work on it. `choose` is asked, with whether a run is
    /// at work on the backend, under the count's lock: `None` waits for a run to end and asks again. `stop`, asked while
    /// the run waits, ends it with "stopped" before it counts or touches a server.
    fn claim<S>(&self, build: Build, stop: &(dyn Fn() -> bool + Sync), mut choose: impl FnMut(bool) -> Result<Option<S>>) -> Result<S> {
        let mut active = self.active.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let server = loop {
            if let Some(server) = choose(busy(&active, build.backend()))? {
                break server;
            }
            if stop() {
                bail!("stopped");
            }
            active = self.ended.wait_timeout(active, Duration::from_millis(100)).unwrap_or_else(|poisoned| poisoned.into_inner()).0;
        };
        begin(&mut active, build);
        Ok(server)
    }
}

/// An error of the server said the way the studio shows it.
pub fn describe(error: &AcppError) -> String {
    match error {
        AcppError::Cancelled => "stopped".into(),
        other => other.to_string(),
    }
}

/// Runs `request` on `model` in the server of `build` (`take`): the one at work when it carries the model, else a new
/// one once the runs on it have ended; a server under work is never replaced. `stop` is asked every tenth of a second,
/// while the run waits for the server too. The run counts from the moment its server is chosen.
pub fn run(acpp: &Acpp, build: Build, model: ModelEntry, request: Value, stop: &(dyn Fn() -> bool + Sync)) -> Result<Value> {
    run_with(acpp, build, model, stop, |server, id, cancel| server.run_task(id, request, cancel))
}

/// [`run`] through the server's stream: the artifacts of each event go to `on_event` as the model produces them; the
/// answer is the same result.
pub fn run_stream(
    acpp: &Acpp,
    build: Build,
    model: ModelEntry,
    request: Value,
    stop: &(dyn Fn() -> bool + Sync),
    on_event: &mut dyn FnMut(&[acpp_server::TaskArtifact]),
) -> Result<Value> {
    run_with(acpp, build, model, stop, |server, id, cancel| server.run_task_stream(id, request, cancel, on_event))
}

/// A task on `model` in the server of `build` as [`run`] chooses it, with `stop` turned into the task's cancel.
fn run_with(
    acpp: &Acpp,
    build: Build,
    model: ModelEntry,
    stop: &(dyn Fn() -> bool + Sync),
    task: impl FnOnce(&Server, &str, &AtomicBool) -> std::result::Result<Value, AcppError>,
) -> Result<Value> {
    let id = model.id.clone();
    acpp.register(model);
    let backend = build.backend();
    let server = acpp.claim(build, stop, |busy| {
        let running = acpp_server::SERVERS.running(backend);
        let seen = running.as_ref().map(|server| Seen { alive: server.alive(), carries: server.config().models.iter().any(|entry| entry.id == id) });
        match take(seen, busy) {
            Take::Running => Ok(Some(running.expect("a running server was seen"))),
            Take::Wait => Ok(None),
            Take::Start => acpp.server(build, &id).map(Some),
        }
    })?;
    let _work = Work { acpp, build };
    let id = id.as_str();
    let cancel = AtomicBool::new(false);
    let finished = AtomicBool::new(false);
    let answer = std::thread::scope(|scope| {
        scope.spawn(|| {
            while !finished.load(Ordering::SeqCst) {
                if stop() {
                    cancel.store(true, Ordering::SeqCst);
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        let answer = task(&server, id, &cancel);
        finished.store(true, Ordering::SeqCst);
        answer
    });
    match answer {
        Ok(result) => Ok(result),
        Err(AcppError::Cancelled) => bail!("stopped"),
        Err(error) => Err(anyhow!("audio.cpp's {id}: {}", describe(&error))),
    }
}

/// The server running on a backend, as a run about to start sees it.
#[derive(Debug, Clone, Copy)]
struct Seen {
    alive: bool,
    /// It carries the model the run needs.
    carries: bool,
}

/// What a run does with the server of its backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Take {
    /// The one running, as it is: another run is at work on it.
    Running,
    /// Wait for the runs on it to end: it lacks the model, and replacing it would break them.
    Wait,
    /// `SERVERS.get`: the running one when its models are today's, else a new one in its place.
    Start,
}

/// A server under work is never replaced: the run takes it as it is when it carries the model, else waits.
fn take(running: Option<Seen>, busy: bool) -> Take {
    match running {
        Some(seen) if seen.alive && busy => {
            if seen.carries {
                Take::Running
            } else {
                Take::Wait
            }
        }
        _ => Take::Start,
    }
}

/// Some run is at work on a server of `backend`: the CUDA builds share one.
fn busy(active: &[(Build, usize)], backend: acpp_server::Backend) -> bool {
    active.iter().any(|(build, runs)| build.backend() == backend && *runs > 0)
}

fn begin(active: &mut Vec<(Build, usize)>, build: Build) {
    match active.iter_mut().find(|(at, _)| *at == build) {
        Some((_, runs)) => *runs += 1,
        None => active.push((build, 1)),
    }
}

/// Counts a run of `build` off; whether its backend is free now.
fn end(active: &mut [(Build, usize)], build: Build) -> bool {
    if let Some((_, runs)) = active.iter_mut().find(|(at, _)| *at == build) {
        *runs = runs.saturating_sub(1);
    }
    !busy(active, build.backend())
}

/// A run at work. When it ends - done, failed, stopped or panicking - it is counted off, and the last one on its
/// backend ends the server: an idle server still holds a gigabyte of the card, which belongs to the music engine
/// between runs, and a stopped run would otherwise go on to its end.
struct Work<'a> {
    acpp: &'a Acpp,
    build: Build,
}

impl Drop for Work<'_> {
    fn drop(&mut self) {
        let mut active = self.acpp.active.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if end(&mut active, self.build) {
            if let Some(server) = acpp_server::SERVERS.running(self.build.backend()) {
                server.kill();
            }
        }
        self.acpp.ended.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_build_this_platform_publishes_has_its_downloads() {
        for build in BUILDS {
            let Some(pinned) = build.server() else { continue };
            let downloads = assets(build);
            assert_eq!(downloads[0].url, pinned.url);
            assert_eq!(downloads[0].unzip_into, Some(build.folder()));
            #[cfg(windows)]
            for library in build.libraries() {
                let picked = downloads.iter().any(|asset| asset.pick.contains(library));
                let from_engine = build == Build::Cuda13 && crate::engine_runtime::CUDA13_LIBRARIES.contains(library);
                assert!(picked || from_engine, "{build:?} imports {library}, which nothing downloads");
            }
        }
    }

    #[test]
    fn a_processor_choice_runs_the_processor_build() {
        if Build::for_this_mac().is_none() {
            assert_eq!(build_for("cpu").unwrap(), Build::Cpu);
            assert!(!on_card(Build::Cpu));
        }
    }

    #[test]
    fn a_server_at_work_is_taken_as_it_is_or_waited_for_never_replaced() {
        let carrying = Seen { alive: true, carries: true };
        let lacking = Seen { alive: true, carries: false };
        assert_eq!(take(Some(carrying), true), Take::Running, "another run's server carries the model");
        assert_eq!(take(Some(lacking), true), Take::Wait, "it lacks the model and another run is at work on it");
        assert_eq!(take(Some(lacking), false), Take::Start, "nobody is at work: replaced as before");
        assert_eq!(take(None, false), Take::Start, "no server yet");
        assert_eq!(take(Some(Seen { alive: false, carries: true }), true), Take::Start, "a server that died serves nobody");
    }

    #[test]
    fn runs_are_counted_by_build_and_a_backend_frees_with_its_last_run() {
        let mut active = Vec::new();
        begin(&mut active, Build::Cuda13);
        begin(&mut active, Build::Cpu);
        begin(&mut active, Build::Cuda13);
        assert!(end(&mut active, Build::Cpu), "the processor's server ends with its own run");
        assert!(busy(&active, Build::Cuda13.backend()), "the CUDA runs keep the card's server");
        assert!(!busy(&active, Build::Cpu.backend()));
        assert!(!end(&mut active, Build::Cuda13), "one CUDA run still works");
        assert!(end(&mut active, Build::Cuda13), "the last CUDA run frees its backend");
        assert!(end(&mut active, Build::Cpu), "counting off below zero stays free");
    }

    #[test]
    fn a_run_stopped_while_it_waits_neither_counts_nor_takes_a_server() {
        let root = tempfile::tempdir().unwrap();
        let acpp = Acpp::new(root.path());
        begin(&mut acpp.active.lock().unwrap(), Build::Cuda13);
        let asked = std::sync::atomic::AtomicUsize::new(0);
        let stop = || asked.load(Ordering::SeqCst) >= 2;
        let outcome = acpp.claim(Build::Cuda13, &stop, |busy| {
            assert!(busy, "another run is at work");
            asked.fetch_add(1, Ordering::SeqCst);
            Ok(None::<()>)
        });
        assert_eq!(outcome.unwrap_err().to_string(), "stopped");
        assert_eq!(asked.load(Ordering::SeqCst), 2, "it looked again after the wait, then stopped");
        assert_eq!(*acpp.active.lock().unwrap(), vec![(Build::Cuda13, 1)], "the stopped run is not counted");
        assert!(acpp.claim(Build::Cpu, &|| true, |_| Ok(Some(()))).is_ok(), "a server at hand is taken before stop is asked");
    }
}
