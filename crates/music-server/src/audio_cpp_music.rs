//! Offline music tasks through the shared audio.cpp server.
use super::*;
use acpp_server::asset::Build;
use acpp_server::{Mode, ModelEntry};
use base64::Engine as _;
use std::io::Read;
use std::path::{Path as FsPath, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) const ENGINE: &str = "audio.cpp";
const HEARTMULA_INSTALL: &str = "audio-cpp-heartmula";
const FAMILY: &str = "minimax_music3";
const PROFILE: &str = "quality-q8";
const QUANTIZATION: &str = "Q8_0";
const WEIGHTS: [(&str, &str); 3] = [
    (
        "minimax_music3.language_model_gguf",
        "language_model_q8_0.gguf",
    ),
    (
        "minimax_music3.rvq_depth_decoder_gguf",
        "rvq_depth_decoder_q8_0.gguf",
    ),
    (
        "minimax_music3.flow_transformer_gguf",
        "transformer_q8_0.gguf",
    ),
];
const SHARED_WEIGHTS: [&str; 2] = ["condition_encoder.gguf", "vocoder.gguf"];
const CONFIGS: [&str; 8] = [
    "config.json",
    "config/language_model.json",
    "config/rvq_depth_decoder.json",
    "config/condition_encoder.json",
    "config/transformer.json",
    "config/vocoder.json",
    "tokenizer/tokenizer.json",
    "tokenizer/tokenizer_config.json",
];

#[derive(Clone, Copy)]
enum Family {
    MiniMax,
    MiniMaxH3,
    HeartMuLa,
    StableAudio,
    MiDasheng,
    MuLaCover,
    Magenta,
    Vevo2,
    SoulX(audio_cpp_soulx::Variant),
}

impl Family {
    fn parse(id: &str) -> Option<Self> {
        match id {
            FAMILY => Some(Self::MiniMax),
            audio_cpp_minimax_h3::FAMILY => Some(Self::MiniMaxH3),
            audio_cpp_heartmula::FAMILY => Some(Self::HeartMuLa),
            audio_cpp_magenta::FAMILY => Some(Self::Magenta),
            audio_cpp_stable_audio::FAMILY => Some(Self::StableAudio),
            audio_cpp_midasheng::FAMILY => Some(Self::MiDasheng),
            audio_cpp_mulacover::FAMILY => Some(Self::MuLaCover),
            audio_cpp_vevo2::FAMILY => Some(Self::Vevo2),
            audio_cpp_soulx::SYNTHESIS => Some(Self::SoulX(audio_cpp_soulx::Variant::Synthesis)),
            audio_cpp_soulx::CONVERSION => Some(Self::SoulX(audio_cpp_soulx::Variant::Conversion)),
            _ => None,
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::MiniMax => FAMILY,
            Self::MiniMaxH3 => audio_cpp_minimax_h3::FAMILY,
            Self::HeartMuLa => audio_cpp_heartmula::FAMILY,
            Self::StableAudio => audio_cpp_stable_audio::FAMILY,
            Self::MiDasheng => audio_cpp_midasheng::FAMILY,
            Self::MuLaCover => audio_cpp_mulacover::FAMILY,
            Self::Magenta => audio_cpp_magenta::FAMILY,
            Self::Vevo2 => audio_cpp_vevo2::FAMILY,
            Self::SoulX(variant) => variant.id(),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::MiniMax => "MiniMax Music3",
            Self::MiniMaxH3 => audio_cpp_minimax_h3::NAME,
            Self::HeartMuLa => "HeartMuLa",
            Self::StableAudio => "Stable Audio 3",
            Self::MiDasheng => "MiDashengLM-Gen",
            Self::MuLaCover => audio_cpp_mulacover::NAME,
            Self::Magenta => "Magenta RealTime 2",
            Self::Vevo2 => audio_cpp_vevo2::NAME,
            Self::SoulX(variant) => variant.name(),
        }
    }

    fn profile(self) -> &'static str {
        match self {
            Self::MiniMax => PROFILE,
            Self::MiniMaxH3 => audio_cpp_minimax_h3::PROFILE,
            Self::HeartMuLa => audio_cpp_heartmula::PROFILE,
            Self::StableAudio => audio_cpp_stable_audio::PROFILE,
            Self::MiDasheng => audio_cpp_midasheng::PROFILE,
            Self::MuLaCover => audio_cpp_mulacover::PROFILE,
            Self::Magenta => audio_cpp_magenta::PROFILE,
            Self::Vevo2 => audio_cpp_vevo2::PROFILE,
            Self::SoulX(_) => audio_cpp_soulx::PROFILE,
        }
    }

    fn quantization(self) -> &'static str {
        match self {
            Self::Magenta => audio_cpp_magenta::QUANTIZATION,
            Self::MiniMaxH3 => audio_cpp_minimax_h3::QUANTIZATION,
            _ => QUANTIZATION,
        }
    }

    fn directory(self, root: &FsPath) -> PathBuf {
        match self {
            Self::MiniMax => model_dir(root),
            Self::MiniMaxH3 => audio_cpp_minimax_h3::model_dir(root),
            Self::HeartMuLa => audio_cpp_heartmula::model_dir(root),
            Self::StableAudio => audio_cpp_stable_audio::model_dir(root),
            Self::MiDasheng => audio_cpp_midasheng::model_dir(root),
            Self::MuLaCover => audio_cpp_mulacover::model_dir(root),
            Self::Magenta => audio_cpp_magenta::model_dir(root),
            Self::Vevo2 => audio_cpp_vevo2::model_dir(root),
            Self::SoulX(_) => audio_cpp_soulx::model_dir(root),
        }
    }

    fn missing(self, path: &FsPath) -> Vec<&'static str> {
        match self {
            Self::MiniMax => missing_files(path),
            Self::MiniMaxH3 => audio_cpp_minimax_h3::missing_files(path),
            Self::HeartMuLa => audio_cpp_heartmula::missing_files(path),
            Self::StableAudio => audio_cpp_stable_audio::missing_files(path),
            Self::MiDasheng => audio_cpp_midasheng::missing_files(path),
            Self::MuLaCover => audio_cpp_mulacover::missing_files(path),
            Self::Magenta => audio_cpp_magenta::missing_files(path),
            Self::Vevo2 => audio_cpp_vevo2::missing_files(path),
            Self::SoulX(variant) => variant.missing(path),
        }
    }

    fn entry(self, path: PathBuf) -> ModelEntry {
        match self {
            Self::MiniMax => entry(path),
            Self::MiniMaxH3 => audio_cpp_minimax_h3::entry(path),
            Self::HeartMuLa => audio_cpp_heartmula::entry(path),
            Self::StableAudio => audio_cpp_stable_audio::entry(path),
            Self::MiDasheng => audio_cpp_midasheng::entry(path),
            Self::MuLaCover => audio_cpp_mulacover::entry(path),
            Self::Magenta => audio_cpp_magenta::entry(path),
            Self::Vevo2 => audio_cpp_vevo2::entry(path),
            Self::SoulX(variant) => variant.entry(path),
        }
    }

    fn task(
        self,
        request: &CreateMusicJobRequest,
        library: &library::Library,
        resumed: bool,
    ) -> anyhow::Result<PreparedTask> {
        match self {
            Self::MiniMax => task(request).map(PreparedTask::Json),
            Self::MiniMaxH3 => audio_cpp_minimax_h3::task(request).map(PreparedTask::Json),
            Self::HeartMuLa => audio_cpp_heartmula::task(request).map(PreparedTask::Json),
            Self::StableAudio => audio_cpp_stable_audio::task(request).map(PreparedTask::Json),
            Self::MiDasheng => audio_cpp_midasheng::task(request).map(PreparedTask::Json),
            Self::MuLaCover => {
                audio_cpp_mulacover::task(request, library, resumed).map(PreparedTask::Cover)
            }
            Self::Magenta => audio_cpp_magenta::task(request, library).map(PreparedTask::Magenta),
            Self::Vevo2 => audio_cpp_vevo2::task(request, library).map(PreparedTask::Singing),
            Self::SoulX(variant) => audio_cpp_soulx::task(variant, request, library).map(PreparedTask::SoulX),
        }
    }

    fn catalog(self, root: Option<&FsPath>, runtime_ready: bool, soxr_ready: bool) -> Value {
        match self {
            Self::MiniMax => minimax_catalog(root, runtime_ready),
            Self::MiniMaxH3 => audio_cpp_minimax_h3::catalog(root, runtime_ready),
            Self::HeartMuLa => audio_cpp_heartmula::catalog(root, runtime_ready),
            Self::StableAudio => audio_cpp_stable_audio::catalog(root, runtime_ready),
            Self::MiDasheng => audio_cpp_midasheng::catalog(root, runtime_ready),
            Self::Magenta => audio_cpp_magenta::catalog(root, runtime_ready),
            Self::MuLaCover => audio_cpp_mulacover::catalog(root, runtime_ready, soxr_ready),
            Self::Vevo2 => audio_cpp_vevo2::catalog(root, runtime_ready),
            Self::SoulX(variant) => variant.catalog(root, runtime_ready, soxr_ready),
        }
    }
}

struct ManagedModel {
    family: Family,
    scope: &'static str,
    asset: &'static downloads::Asset,
    sha256: &'static str,
    lease: Arc<tokio::sync::Mutex<()>>,
}

fn managed_model(family: Family, state: &AppState) -> Option<ManagedModel> {
    let (scope, asset, sha256, lease) = match family {
        Family::HeartMuLa => (
            HEARTMULA_INSTALL,
            &audio_cpp_heartmula::ASSET,
            audio_cpp_heartmula::SHA256,
            &state.heartmula_install,
        ),
        Family::StableAudio => (
            "audio-cpp-stable-audio",
            &audio_cpp_stable_audio::ASSET,
            audio_cpp_stable_audio::SHA256,
            &state.stable_audio_install,
        ),
        Family::SoulX(variant) => {
            let (asset, sha256) = variant.asset();
            (variant.id(), asset, sha256, &state.soulx_install)
        }
        _ => return None,
    };
    Some(ManagedModel {
        family,
        scope,
        asset,
        sha256,
        lease: lease.clone(),
    })
}

enum PreparedTask {
    Json(Value),
    Magenta(audio_cpp_magenta::MagentaTask),
    Cover(audio_cpp_mulacover::CoverTask),
    Singing(audio_cpp_vevo2::SingingTask),
    SoulX(audio_cpp_soulx::SingingTask),
}

impl PreparedTask {
    fn into_request(self) -> anyhow::Result<(Value, Option<tempfile::TempDir>)> {
        match self {
            Self::Json(body) => Ok((body, None)),
            Self::Magenta(task) => Ok(task.into_request()),
            Self::SoulX(task) => task.into_request().map(|(body, directory)| (body, Some(directory))),
            Self::Cover(task) => task
                .into_request()
                .map(|(body, directory)| (body, Some(directory))),
            Self::Singing(task) => task
                .into_request()
                .map(|(body, directory)| (body, Some(directory))),
        }
    }
}

pub(super) fn matches_gguf_package(path: &FsPath, bytes: u64, tensors: u64, metadata_keys: u64) -> bool {
    std::fs::File::open(path).is_ok_and(|mut file| {
        use std::io::Read as _;
        let mut header = [0; 24];
        file.metadata().is_ok_and(|info| info.is_file() && info.len() == bytes)
            && file.read_exact(&mut header).is_ok()
            && header[..4] == *b"GGUF"
            && header[4..8] == 3_u32.to_le_bytes()
            && header[8..16] == tensors.to_le_bytes()
            && header[16..24] == metadata_keys.to_le_bytes()
    })
}

pub(super) fn registered_model_ready(model: &ModelEntry) -> Option<bool> {
    let family = Family::parse(&model.id)?;
    let expected_family = if matches!(family, Family::SoulX(_)) {
        audio_cpp_soulx::FAMILY
    } else {
        family.id()
    };
    if model.family != expected_family || !matches!(model.mode, Mode::Offline) {
        return None;
    }
    let directory = match family {
        Family::MiniMax if model.task == "gen" => Some(model.path.as_path()),
        Family::HeartMuLa
        | Family::StableAudio
        | Family::Magenta
        | Family::MiDasheng
        | Family::MiniMaxH3
        | Family::MuLaCover
            if model.task == "gen" =>
        {
            model.path.parent()
        }
        Family::Vevo2 if model.task == "tts" => model.path.parent(),
        Family::SoulX(variant) if model.task == variant.engine_task() => model.path.parent(),
        _ => return None,
    };
    Some(directory.is_some_and(|path| family.missing(path).is_empty()))
}

fn model_dir(root: &FsPath) -> PathBuf {
    root.join("audiocpp/models").join(FAMILY)
}

pub(super) fn missing_files(path: &FsPath) -> Vec<&'static str> {
    let configs = CONFIGS.into_iter().filter(|file| {
        !std::fs::read(path.join(file)).ok().is_some_and(|body| {
            serde_json::from_slice::<Value>(&body).is_ok_and(|value| value.is_object())
        })
    });
    let weights = WEIGHTS
        .into_iter()
        .map(|(_, file)| file)
        .chain(SHARED_WEIGHTS)
        .filter(|file| {
            let mut magic = [0; 4];
            !std::fs::File::open(path.join(file)).is_ok_and(|mut file| {
                file.metadata().is_ok_and(|info| info.len() > 24)
                    && file.read_exact(&mut magic).is_ok()
                    && &magic == b"GGUF"
            })
        });
    configs.chain(weights).collect()
}

fn weights() -> serde_json::Map<String, Value> {
    WEIGHTS
        .into_iter()
        .map(|(key, file)| (key.into(), Value::from(file)))
        .collect()
}

fn entry(path: PathBuf) -> ModelEntry {
    ModelEntry {
        id: FAMILY.into(),
        family: FAMILY.into(),
        task: "gen".into(),
        mode: Mode::Offline,
        path,
        load_options: Default::default(),
        session_options: weights(),
        default_request_options: Default::default(),
        busy_timeout_ms: None,
        output_rate: None,
    }
}

async fn build(state: &AppState) -> anyhow::Result<Build> {
    use music_engine::yue_server::ComputeBackend;
    match state.engine_options.read().await.backend {
        ComputeBackend::Auto => acpp::build_for("auto"),
        ComputeBackend::Cpu => Ok(Build::Cpu),
        ComputeBackend::Vulkan => Ok(Build::Vulkan),
        ComputeBackend::Cuda => match acpp::build_for("cuda")? {
            build @ (Build::Cuda12 | Build::Cuda13) => Ok(build),
            _ => anyhow::bail!("The CUDA audio.cpp runtime is unavailable."),
        },
    }
}

pub(super) async fn models(State(state): State<AppState>) -> Json<Value> {
    let root = studio_data_root();
    let build = build(&state).await;
    let runtime_ready = build
        .as_ref()
        .is_ok_and(|build| state.acpp.exe(*build).is_file() && state.acpp.installed(*build));
    let soxr_ready = state.acpp.soxr_ready();
    let mut models = [
        Family::MiniMax,
        Family::MiniMaxH3,
        Family::HeartMuLa,
        Family::StableAudio,
        Family::MiDasheng,
        Family::Magenta,
        Family::MuLaCover,
        Family::Vevo2,
        Family::SoulX(audio_cpp_soulx::Variant::Synthesis),
        Family::SoulX(audio_cpp_soulx::Variant::Conversion),
    ]
    .map(|family| family.catalog(root.as_deref(), runtime_ready, soxr_ready));
    for model in &mut models {
        let Some(managed) = model["id"]
            .as_str()
            .and_then(Family::parse)
            .and_then(|family| managed_model(family, &state))
        else {
            continue;
        };
        let installing = managed.lease.try_lock().is_err();
        model["installing"] = Value::Bool(installing);
        if installing {
            model["ready"] = Value::Bool(false);
        }
        model["download"] = serde_json::json!(acpp_download(&state, managed.scope).await);
    }
    Json(serde_json::json!({
        "engine": ENGINE,
        "engine_installed": runtime_ready,
        "models": models
    }))
}

#[derive(Deserialize)]
pub(super) struct InstallModel {
    model: String,
}

pub(super) async fn install(
    State(state): State<AppState>,
    Json(request): Json<InstallModel>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let Some(managed) =
        Family::parse(&request.model).and_then(|family| managed_model(family, &state))
    else {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "This audio.cpp music model has no managed installation.".into(),
        ));
    };
    let build = build(&state)
        .await
        .map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, error.to_string()))?;
    let root = studio_data_root().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Studio data folder is unavailable.".into(),
        )
    })?;
    let Ok(lease) = managed.lease.clone().try_lock_owned() else {
        return Ok(Json(
            serde_json::json!({"started": false, "installing": true}),
        ));
    };
    tokio::spawn(async move {
        let _lease = lease;
        if let Err(error) = install_managed_model(&state, build, root, &managed).await {
            state
                .acpp
                .downloader()
                .fail(managed.scope, format!("{error:#}"))
                .await;
        }
    });
    Ok(Json(
        serde_json::json!({"started": true, "installing": true}),
    ))
}

async fn install_managed_model(
    state: &AppState,
    build: Build,
    root: PathBuf,
    managed: &ManagedModel,
) -> anyhow::Result<()> {
    let asset = managed.asset;
    let sha256 = managed.sha256;
    let name = managed.family.name();
    let downloader = state.acpp.downloader();
    let target = downloader.path_of(asset);
    tokio::task::spawn_blocking(move || repair_managed_file(&target, asset, sha256)).await??;
    if !state.acpp.installed(build) {
        install_acpp(state, build, managed.scope)
            .await
            .context("Installing audio.cpp runtime")?;
    }
    if matches!(managed.family, Family::SoulX(audio_cpp_soulx::Variant::Conversion)) {
        #[cfg(windows)]
        if !state.acpp.soxr_ready() {
            state.acpp.install_soxr().await.context("Installing SoulX-Singer SVC resampler")?;
        }
        anyhow::ensure!(state.acpp.soxr_ready(), "Install the libsoxr runtime for SoulX-Singer SVC.");
    }
    downloader
        .install_all(managed.scope, &[asset])
        .await
        .with_context(|| format!("Installing {name} weights"))?;
    anyhow::ensure!(
        managed
            .family
            .missing(&managed.family.directory(&root))
            .is_empty()
            && state.acpp.exe(build).is_file()
            && state.acpp.installed(build),
        "{name} installation is incomplete."
    );
    Ok(())
}

pub(super) fn repair_managed_file(
    target: &FsPath,
    asset: &downloads::Asset,
    sha256: &str,
) -> anyhow::Result<()> {
    match std::fs::symlink_metadata(target) {
        Ok(metadata) if metadata.is_file() => {
            if metadata.len() != asset.bytes || !crate::pin_sha::holds(target, sha256) {
                // Keep the downloader's .part and its resume manifest.
                std::fs::remove_file(target).with_context(|| {
                    format!("Replacing damaged model weights at {}", target.display())
                })?;
            }
        }
        Ok(_) => anyhow::bail!(
            "Model installation expected a regular file at {}",
            target.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Checking model weights at {}", target.display()));
        }
    }
    Ok(())
}

fn minimax_catalog(root: Option<&FsPath>, runtime_ready: bool) -> Value {
    let mut missing = root.map_or_else(
        || vec!["Studio data folder"],
        |root| missing_files(&model_dir(root)),
    );
    if !runtime_ready {
        missing.push("audio.cpp runtime");
    }
    serde_json::json!({
        "id": FAMILY, "name": "MiniMax Music3", "profile_id": PROFILE,
        "quantization": QUANTIZATION, "status": "experimental", "ready": missing.is_empty(), "missing": missing,
        "requires": ["style", "lyrics"], "output_formats": ["flac", "mp3", "wav32"]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_magenta_and_midasheng_catalogues_reject_truncated_packages() {
        let root = tempfile::tempdir().unwrap();
        for (family, file, bytes) in [
            (Family::Magenta, "magenta-rt2-small-f16.gguf", 1_529_647_616_u64),
            (Family::MiDasheng, "midashenglm-gen-q8_0.gguf", 3_381_370_752_u64),
        ] {
            let directory = family.directory(root.path());
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join(file), [b"GGUF".as_slice(), &[0; 32]].concat()).unwrap();
            let row = family.catalog(Some(root.path()), false, true);
            assert_eq!(row["ready"], false);
            assert_eq!(row["missing"], serde_json::json!([file, "audio.cpp runtime"]));
            assert_eq!(row["missing_bytes"], bytes);
            assert_eq!(row["bytes"], bytes);
            assert!(row["package_url"].as_str().unwrap().ends_with(file));
            assert!(row["license_url"].is_string());
        }
    }

    #[test]
    fn repairing_heartmula_preserves_download_resume_files() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("heartmula-q8_0.gguf");
        let partial = target.with_extension("part");
        let manifest = crate::chunked::manifest_path(&partial);
        std::fs::write(&target, b"broken GGUF").unwrap();
        std::fs::write(&partial, b"partial bytes").unwrap();
        std::fs::write(&manifest, b"resume manifest").unwrap();
        repair_managed_file(
            &target,
            &audio_cpp_heartmula::ASSET,
            audio_cpp_heartmula::SHA256,
        )
        .unwrap();
        assert!(!target.exists());
        assert_eq!(std::fs::read(partial).unwrap(), b"partial bytes");
        assert_eq!(std::fs::read(manifest).unwrap(), b"resume manifest");
    }

    #[test]
    fn music_registry_keeps_native_task_and_model_profile() {
        let root = tempfile::tempdir().unwrap();
        for (family, task) in [
            (Family::MiniMax, "gen"),
            (Family::MiniMaxH3, "gen"),
            (Family::HeartMuLa, "gen"),
            (Family::StableAudio, "gen"),
            (Family::MiDasheng, "gen"),
            (Family::Magenta, "gen"),
            (Family::MuLaCover, "gen"),
            (Family::Vevo2, "tts"),
        ] {
            let row = family.catalog(Some(root.path()), true, true);
            assert_eq!(row["id"], family.id());
            assert_eq!(row["profile_id"], family.profile());
            assert_eq!(row["ready"], false);
            let entry = family.entry(family.directory(root.path()));
            assert_eq!(entry.id, family.id());
            assert_eq!(entry.task, task);
            assert_eq!(registered_model_ready(&entry), Some(false));
        }
        assert!(Family::parse("unknown_model").is_none());
    }
}

fn task(request: &CreateMusicJobRequest) -> anyhow::Result<Value> {
    anyhow::ensure!(
        !request.style.trim().is_empty() && !request.lyrics.trim().is_empty(),
        "MiniMax Music3 needs a style and lyrics."
    );
    anyhow::ensure!(
        request.cover_of.is_none()
            && request.semantic_tokens.is_none()
            && request
                .abc
                .as_deref()
                .is_none_or(|abc| abc.trim().is_empty())
            && !request.abc_continue
            && request.adapters.is_empty()
            && request.ace.is_none()
            && !request.vocals_only
            && request.solver.is_none()
            && request.lm_seed.is_none()
            && request.synth_batch_size.is_none_or(|n| n == 1)
            && request.lm_batch_size.is_none_or(|n| n == 1),
        "This audio.cpp model accepts one text-and-lyrics generation with a single seed."
    );
    let duration = request.duration_seconds.unwrap_or(20.0);
    let steps = request.steps.unwrap_or(30);
    let seed = request.seed.unwrap_or(0);
    anyhow::ensure!(
        duration.is_finite() && duration >= 0.1,
        "Duration must be at least 0.1 seconds."
    );
    anyhow::ensure!(steps > 0, "Inference needs at least one step.");
    anyhow::ensure!(seed >= 0, "The seed must be nonnegative.");
    let mut body = serde_json::json!({
        "text": request.style, "lyrics": request.lyrics, "seed": seed,
        "duration_seconds": duration, "num_inference_steps": steps, "options": {}
    });
    if let Some(cfg) = request.cfg_scale {
        anyhow::ensure!(
            cfg.is_finite() && cfg > 0.0,
            "Guidance scale must be positive."
        );
        body["guidance_scale"] = Value::from(cfg);
    }
    Ok(body)
}

pub(super) async fn submit(
    state: AppState,
    request: CreateMusicJobRequest,
    model: String,
    attempt: u32,
) -> (StatusCode, Json<MusicJob>) {
    let submitted = std::time::Instant::now();
    let reject = |status, reason| {
        (
            status,
            Json(failed_request_job(request.clone(), ENGINE.into(), reason)),
        )
    };
    let Some(family) = Family::parse(&model) else {
        return reject(
            StatusCode::BAD_REQUEST,
            format!("Unknown audio.cpp music model {model}."),
        );
    };
    if managed_model(family, &state).is_some_and(|managed| managed.lease.try_lock().is_err()) {
        return reject(
            StatusCode::CONFLICT,
            format!("{} installation is in progress.", family.name()),
        );
    }
    let prepared = if matches!(family, Family::Magenta) {
        let request = request.clone();
        let library = state.library.clone();
        tokio::task::spawn_blocking(move || family.task(&request, &library, attempt > 0))
            .await
            .unwrap_or_else(|error| Err(error.into()))
    } else {
        family.task(&request, &state.library, attempt > 0)
    };
    let task = match prepared {
        Ok(task) => task,
        Err(error) => return reject(StatusCode::BAD_REQUEST, error.to_string()),
    };
    let format = request.output_format.as_deref().unwrap_or("flac");
    if let Err(error) = validate_output_format(format) {
        return reject(StatusCode::BAD_REQUEST, error);
    }
    if let Some(bitrate) = request.mp3_bitrate
        && let Err(error) = validate_mp3_bitrate(bitrate)
    {
        return reject(StatusCode::BAD_REQUEST, error);
    }
    let bitrate = (format == "mp3")
        .then(|| audio_post::encode::mp3_bitrate(request.mp3_bitrate.unwrap_or(DEFAULT_MP3_KBPS)));
    let Some(root) = studio_data_root() else {
        return reject(
            StatusCode::SERVICE_UNAVAILABLE,
            "Studio data folder is unavailable.".into(),
        );
    };
    let path = family.directory(&root);
    let missing = family.missing(&path);
    if !missing.is_empty() {
        return reject(
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "{} weights in {} are incomplete: {}.",
                family.name(),
                path.display(),
                missing.join(", ")
            ),
        );
    }
    let build = match build(&state).await {
        Ok(build) if state.acpp.exe(build).is_file() && state.acpp.installed(build) => build,
        Ok(_) => {
            return reject(
                StatusCode::SERVICE_UNAVAILABLE,
                "Install the selected audio.cpp runtime in Settings - Models.".into(),
            );
        }
        Err(error) => return reject(StatusCode::SERVICE_UNAVAILABLE, error.to_string()),
    };
    if matches!(family, Family::MuLaCover) && !state.acpp.soxr_ready() {
        return reject(
            StatusCode::SERVICE_UNAVAILABLE,
            "Install the libsoxr runtime for MuLaCover.".into(),
        );
    }
    if matches!(family, Family::SoulX(audio_cpp_soulx::Variant::Conversion)) && !state.acpp.soxr_ready() {
        return reject(StatusCode::SERVICE_UNAVAILABLE, "Install the libsoxr runtime for SoulX-Singer SVC.".into());
    }
    let Ok(lease) = state.engine_use.clone().try_write_owned() else {
        return reject(
            StatusCode::CONFLICT,
            "The local music engine is busy.".into(),
        );
    };
    if state.jobs.read().await.values().any(song_in_engine)
        || state.training.active_run().await.is_some()
    {
        return reject(
            StatusCode::CONFLICT,
            "Wait for the active generation or training run to finish.".into(),
        );
    }
    if let Err(error) = state.ace_runtime.stop().await {
        return reject(StatusCode::CONFLICT, format!("ACE-Step is busy: {error:#}"));
    }
    state.minimax.stop().await;
    evict_kept_models(&state, "before audio.cpp music generation").await;
    let mut settings = serde_json::json!({
        "music_source": request.music_source, "family": family.id(), "profile_id": family.profile(),
        "quantization": family.quantization(), "backend": build.backend(),
        "duration_seconds": request.duration_seconds, "steps": request.steps,
        "seed": request.seed, "guidance_scale": request.cfg_scale,
        "output_format": format, "mp3_bitrate": bitrate,
        "fade_in": request.fade_in.unwrap_or_default().max(0.0),
        "fade_out": request.fade_out.unwrap_or_default().max(0.0)
    });
    match (&family, &task) {
        (Family::MiniMax, PreparedTask::Json(body)) => {
            settings["weight_files"] = serde_json::json!(weights());
            settings["shared_weight_files"] = serde_json::json!(SHARED_WEIGHTS);
            settings["duration_seconds"] = body["duration_seconds"].clone();
            settings["steps"] = body["num_inference_steps"].clone();
            settings["seed"] = body["seed"].clone();
        }
        (Family::MiniMaxH3, PreparedTask::Json(body)) => {
            settings["duration_seconds"] = body["duration_seconds"].clone();
            settings["steps"] = body["num_inference_steps"].clone();
            settings["seed"] = body["seed"].clone();
            settings["guidance_scale"] = body["guidance_scale"].clone();
        }
        (Family::HeartMuLa, _) => settings["heartmula"] = serde_json::json!(request.heartmula),
        (Family::StableAudio, PreparedTask::Json(body)) => {
            settings["stable_audio"] = body["options"].clone();
            settings["duration_seconds"] = body["duration_seconds"].clone();
            settings["steps"] = body["num_inference_steps"].clone();
            settings["seed"] = body["seed"].clone();
            settings["guidance_scale"] = body["guidance_scale"].clone();
        }
        (Family::MiDasheng, _) => settings["midasheng"] = serde_json::json!(request.midasheng),
        (Family::Magenta, _) => {
            settings["magenta"] = serde_json::json!(request.magenta);
        }
        (Family::MuLaCover, PreparedTask::Cover(cover)) => {
            settings["cover_of"] = serde_json::json!(request.cover_of);
            settings["duration_seconds"] = cover.body["options"]["duration_sec"].clone();
            settings["steps"] = cover.body["num_inference_steps"].clone();
            settings["seed"] = cover.body["seed"].clone();
            settings["guidance_scale"] = cover.body["guidance_scale"].clone();
        }
        (Family::SoulX(variant), _) => {
            settings["route"] = Value::from(variant.engine_task());
            settings["soulx"] = serde_json::json!(request.soulx);
            settings["seed"] = Value::from(request.seed.unwrap_or(2026));
        }
        (Family::Vevo2, _) => {
            settings["route"] = Value::from(audio_cpp_vevo2::ROUTE);
            settings["vevo2"] = serde_json::json!(request.vevo2);
        }
        _ => unreachable!(),
    }
    let job = MusicJob {
        derived: match &task {
            PreparedTask::Cover(cover) => Some(cover.derived.clone()),
            _ => None,
        },
        id: format!("audio-cpp-{}", uuid_suffix()),
        client_ref: request.client_ref.clone(),
        submitted_at: unix_millis(),
        engine_id: ENGINE.into(),
        cover_prompt: request.cover_prompt.clone(),
        title: Some(titled(&request, &state.adapters)),
        status: MusicJobStatus::Running,
        dispatch: MusicJobDispatch::Local,
        phase: MusicJobPhase::Running,
        style: request.style.clone(),
        lyrics: request.lyrics.clone(),
        duration_seconds: settings["duration_seconds"].as_f64().unwrap_or_default(),
        generation_settings: settings,
        song: None,
        songs: vec![],
        message: format!("{} is generating through audio.cpp.", family.name()),
        code: None,
        params: None,
        playlist_id: request.playlist_id.clone(),
        laid: None,
        card_memory_short: false,
        tracks_asked: Some(1),
    };
    if let Err(error) =
        state
            .library
            .save_music_job(&stored_job(&job, serde_json::json!(request), attempt))
    {
        return reject(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
    }
    let stop = Arc::new(AtomicBool::new(false));
    state
        .audio_cpp_music_stops
        .write()
        .await
        .insert(job.id.clone(), stop.clone());
    state.jobs.write().await.insert(job.id.clone(), job.clone());
    let accepted = job.clone();
    tokio::spawn(async move {
        let _lease = lease;
        let acpp = state.acpp.clone();
        let settings = job.generation_settings.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            anyhow::ensure!(!stop.load(Ordering::SeqCst), "stopped");
            let (body, _references) = task.into_request()?;
            acpp.with_server(
                build,
                family.entry(path),
                &|| stop.load(Ordering::SeqCst),
                |server| {
                    anyhow::ensure!(!stop.load(Ordering::SeqCst), "stopped");
                    // A disconnected JSON client still leaves the shared server running.
                    // Hold its lease until the answer so tools cannot replace it mid-task.
                    let reply = server.run_task(family.id(), body, &AtomicBool::new(false));
                    // Release only this model before another engine can claim the card.
                    if let Err(error) = server.unload(&[family.id()]) {
                        studio_log!("[ERROR] unloading audio.cpp {}: {error:#}", family.name());
                    }
                    anyhow::ensure!(!stop.load(Ordering::SeqCst), "stopped");
                    shape(&reply?, &settings)
                },
            )
        })
        .await
        .context("the audio.cpp worker stopped")
        .and_then(|result| result);
        complete(
            &state,
            &job.id,
            family.name(),
            family.profile(),
            LocalRun::AudioCpp(family, build, submitted),
            outcome,
        )
        .await;
        state.audio_cpp_music_stops.write().await.remove(&job.id);
    });
    (StatusCode::ACCEPTED, Json(accepted))
}

fn shape(reply: &Value, settings: &Value) -> anyhow::Result<native_music_jobs::ShapedOutput> {
    let encoded = reply["audio"]
        .as_str()
        .context("audio.cpp returned no song audio")?;
    let payload = encoded
        .strip_prefix("data:")
        .and_then(|url| url.split_once(','))
        .map_or(encoded, |(_, data)| data);
    let bytes = base64::engine::general_purpose::STANDARD.decode(payload)?;
    let format = output_format(settings);
    let bitrate = settings["mp3_bitrate"].as_u64().map(|value| value as u32);
    let fade = |key| settings[key].as_f64().unwrap_or_default() as f32;
    native_music_jobs::shape_output(bytes, format, bitrate, (fade("fade_in"), fade("fade_out")))
}

fn save(
    library: &library::Library,
    job: &MusicJob,
    profile: &str,
    output: native_music_jobs::ShapedOutput,
) -> anyhow::Result<library::ImportedSong> {
    let mut settings = job.generation_settings.clone();
    settings["fade_in"] = Value::from(output.fades.0);
    settings["fade_out"] = Value::from(output.fades.1);
    library.import_generated_song(library::GeneratedSongInput {
        title: job.title.clone(),
        caption: job.style.clone(),
        lyrics: job.lyrics.clone(),
        metadata: serde_json::json!({"job_id": job.id, "duration_seconds": output.duration,
            "output_format": settings["output_format"], "cover_prompt": job.cover_prompt, "derived": job.derived}),
        generation_settings: settings,
        replay_request: None,
        audio_codes: None,
        engine_id: job.engine_id.clone(),
        profile_id: Some(profile.into()),
        source: "local_generation".into(),
        audio_extension: output.extension,
        audio: output.audio,
    })
}

enum LocalRun {
    AudioCpp(Family, Build, std::time::Instant),
    HotStep(&'static str, std::time::Instant),
}

pub(super) async fn complete_local(
    state: &AppState,
    id: &str,
    name: &str,
    profile: &str,
    set: &'static str,
    submitted: std::time::Instant,
    outcome: anyhow::Result<native_music_jobs::ShapedOutput>,
) {
    complete(state, id, name, profile, LocalRun::HotStep(set, submitted), outcome).await;
}

async fn complete(
    state: &AppState,
    id: &str,
    name: &str,
    profile: &str,
    telemetry: LocalRun,
    outcome: anyhow::Result<native_music_jobs::ShapedOutput>,
) {
    let mut jobs = state.jobs.write().await;
    let Some(job) = jobs.get_mut(id) else { return };
    if !matches!(job.status, MusicJobStatus::Running) {
        return;
    }
    let song_id = match outcome.and_then(|output| save(&state.library, job, profile, output)) {
        Ok(imported) => {
            let id = imported.song.id.clone();
            if let Some(playlist) = &job.playlist_id
                && let Err(error) =
                    add_to_playlist(&state.library, playlist, std::iter::once(id.clone()))
            {
                studio_log!("[ERROR] adding audio.cpp song to playlist: {error:#}");
            }
            job.duration_seconds = imported.song.metadata["duration_seconds"]
                .as_f64()
                .unwrap_or(job.duration_seconds);
            job.generation_settings = imported.song.generation_settings.clone();
            let song = CompletedSong {
                audio_url: format!("/v1/library/media/{id}"),
                id: id.clone(),
                song: imported.song,
            };
            job.song = Some(song.clone());
            job.songs = vec![song];
            job.status = MusicJobStatus::Completed;
            job.phase = MusicJobPhase::Completed;
            job.message = format!("{name} song saved to the library.");
            Some(id)
        }
        Err(error) => {
            job.status = MusicJobStatus::Failed;
            job.phase = MusicJobPhase::Failed;
            job.message = format!("{name} music generation failed: {error:#}");
            None
        }
    };
    persist(state, job);
    let seconds = songs_seconds(&job.songs);
    let completed = song_id.is_some();
    let report = native_music_jobs::SongReport::from_job(job);
    drop(jobs);
    if let LocalRun::HotStep(set, submitted) = telemetry {
        report.record(state, hotstep_cover::ENGINE, set, submitted.elapsed()).await;
    }
    if let LocalRun::AudioCpp(family, build, submitted) = telemetry
        && let Some(hub) = state.hub.clone().filter(|hub| hub.telemetry_on())
    {
        let elapsed = submitted.elapsed();
        // Only enumerated runtime and model names enter the existing daily
        // report. A cancelled job returns above before recording an outcome.
        let hub = hub.for_scheduled_report();
        let write = tokio::task::spawn_blocking(move || {
            if completed {
                let backend = match build {
                    Build::Cuda12 => "cuda12",
                    Build::Cuda13 => "cuda13",
                    Build::Vulkan => "vulkan",
                    Build::Metal => "metal",
                    Build::Cpu => "cpu",
                };
                let set = format!("audio.cpp.{}.{}", family.id(), backend);
                let parts = [
                    ENGINE.to_string(),
                    family.profile().to_string(),
                    backend.to_string(),
                ];
                hub.count("songs", 1);
                hub.count("song_done", 1);
                hub.used_models(Some(&set), &parts, 1);
                hub.worked_on("song", Some(&set), seconds, elapsed);
            } else {
                hub.count("song_failed", 1);
            }
        });
        if let Err(error) = write.await {
            studio_log!("[ERROR] recording audio.cpp music outcome: {error}");
        }
    }
    if let Some(id) = song_id {
        tag_stored_song(state, &id).await;
        after_import(state, &id);
    }
}

fn persist(state: &AppState, job: &MusicJob) {
    if let Err(error) =
        state
            .library
            .set_music_job_status(&job.id, job_status_name(&job.status), &job.message)
    {
        studio_log!("[ERROR] saving audio.cpp music job status: {error:#}");
    }
}

pub(super) async fn cancel(
    state: &AppState,
    id: &str,
) -> Result<Json<MusicJob>, (StatusCode, Json<ApiError>)> {
    let mut jobs = state.jobs.write().await;
    let job = jobs
        .get_mut(id)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Music job was not found.".into()))?;
    if matches!(job.status, MusicJobStatus::Running) {
        if let Some(stop) = state.audio_cpp_music_stops.read().await.get(id) {
            stop.store(true, Ordering::SeqCst);
        }
        stop_on_request(job);
        persist(state, job);
    }
    Ok(Json(job.clone()))
}
