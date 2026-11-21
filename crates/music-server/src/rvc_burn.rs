use super::*;
use crate::rvc_voices::ApiFailure;
use anyhow::{Context, ensure};
use music_engine::yue_train::TrainingStage;
use std::{ffi::OsString, path::Path as FsPath, process::Stdio, time::Duration};

const CONTENT: &str = "vec-768-layer-12.onnx";
const PITCH: &str = "rmvpe.onnx";
const BASE_G: &str = "f0G48k.pth";
const BASE_D: &str = "f0D48k.pth";
pub(super) const STAGE: &str = "rvc-burn";
pub(super) const CHECKPOINT: &str = "rvc-output/voice.safetensors";

#[derive(Clone)]
pub(super) struct Runtime {
    folder: PathBuf,
    pub(super) exe: PathBuf,
    ort: PathBuf,
    managed: Option<(rvc_burn_install::Profile, Arc<downloads::Downloader>)>,
}

impl Runtime {
    pub(super) fn discover(state: &AppState) -> Self {
        let override_path = std::env::var_os("YUE_RVC_BURN_ROOT").filter(|value| !value.is_empty());
        if let Some(folder) = override_path {
            return Self::from_folder(folder.into());
        }
        let mut runtime = Self::from_folder(update_data_root(state).join("rvc/burn"));
        if !runtime.exe.is_file() && let Ok(profile) = rvc_burn_install::selected() {
            let folder = runtime.folder.join("runtime").join(profile.folder());
            runtime.exe = folder.join("rvc.exe");
            runtime.ort = folder.join("onnxruntime.dll");
            runtime.managed = Some((profile, state.rvc_burn_downloads.clone()));
        }
        runtime
    }

    pub(super) fn from_folder(folder: PathBuf) -> Self {
        let exe = folder.join(if cfg!(windows) { "rvc.exe" } else { "rvc" });
        let ort = folder.join(if cfg!(windows) {
            "onnxruntime.dll"
        } else {
            "libonnxruntime.so"
        });
        Self { folder, exe, ort, managed: None }
    }

    fn missing(&self, training: bool) -> Vec<String> {
        if let Some((profile, downloads)) = &self.managed {
            return profile.assets(training)
                .filter(|asset| !rvc_burn_install::present(downloads, asset))
                .map(|asset| asset.label.to_owned()).collect();
        }
        let mut paths = vec![(&self.exe, None), (&self.ort, None)];
        let candidates = [
            (CONTENT, 377_655_729),
            (PITCH, 361_688_443),
            (BASE_G, 75_465_569),
            (BASE_D, 142_875_703),
        ];
        let models: Vec<_> = candidates[..if training { 4 } else { 2 }]
            .iter()
            .map(|(name, size)| (self.folder.join(name), Some(*size)))
            .collect();
        paths.extend(models.iter().map(|(path, size)| (path, *size)));
        paths
            .into_iter()
            .filter(|(path, size)| {
                !std::fs::metadata(path).is_ok_and(|metadata| {
                    metadata.is_file() && size.is_none_or(|n| n == metadata.len())
                })
            })
            .map(|(path, _)| {
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    pub(super) fn ready(&self, training: bool) -> anyhow::Result<()> {
        ensure!(
            hardware::hardware().card_cuda.is_some(),
            "RVC Burn needs an NVIDIA CUDA card."
        );
        let missing = self.missing(training);
        ensure!(
            missing.is_empty(),
            "RVC Burn components missing: {}",
            missing.join(", ")
        );
        Ok(())
    }

    pub(super) fn environment(&self) -> Vec<(OsString, OsString)> {
        let path = std::env::join_paths(
            self.exe.parent().map(FsPath::to_path_buf).into_iter().chain(
                std::env::var_os("PATH")
                    .iter()
                    .flat_map(std::env::split_paths),
            ),
        )
        .unwrap_or_default();
        vec![
            ("PATH".into(), path),
            ("ORT_DYLIB_PATH".into(), self.ort.clone().into_os_string()),
            ("RUST_LOG".into(), "info".into()),
        ]
    }

    fn common_args(&self, local: &FsPath) -> Vec<OsString> {
        vec![
            "--backend".into(),
            "cuda".into(),
            "--device".into(),
            "gpu:0".into(),
            "--content".into(),
            self.folder.join(CONTENT).into_os_string(),
            "--rmvpe".into(),
            self.folder.join(PITCH).into_os_string(),
            "--content-vec-backend".into(),
            "onnx".into(),
            "--rmvpe-backend".into(),
            "onnx".into(),
            "--cache-dir".into(),
            local.join("cache").into_os_string(),
        ]
    }

    pub(super) fn training_stage(
        &self,
        clips: Vec<PathBuf>,
        run: &FsPath,
        settings: &Settings,
    ) -> TrainingStage {
        let mut args = vec!["train".into()];
        args.extend(clips.into_iter().map(PathBuf::into_os_string));
        args.extend([
            "--out".into(),
            run.join("rvc-output/voice").into_os_string(),
            "--epochs".into(),
            settings.epochs.to_string().into(),
            "--batch-size".into(),
            settings.batch_size.to_string().into(),
            "--lr".into(),
            settings.learning_rate.to_string().into(),
            "--pretrained-g".into(),
            self.folder.join(BASE_G).into_os_string(),
            "--pretrained-d".into(),
            self.folder.join(BASE_D).into_os_string(),
            "--no-tui".into(),
        ]);
        args.extend(self.common_args(run));
        TrainingStage { id: STAGE, args }
    }

    pub(super) fn convert(
        &self,
        vocal: &FsPath,
        voice: &FsPath,
        stage: &FsPath,
        semitone_shift: i32,
        cancelled: impl Fn() -> bool,
    ) -> anyhow::Result<audio_post::Stereo> {
        let output = stage.join("converted");
        std::fs::create_dir(&output)?;
        let stderr = stage.join("rvc-convert.log");
        let mut command = std::process::Command::new(&self.exe);
        command
            .arg("convert")
            .arg(vocal)
            .arg("--model")
            .arg(voice)
            .arg("--output-dir")
            .arg(&output)
            .args([
                "--model-sr",
                "48000",
                "--transpose",
                &semitone_shift.to_string(),
            ])
            .args(self.common_args(stage))
            .envs(self.environment())
            .current_dir(&self.folder)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&stderr)?);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn().context("Start RVC Burn conversion")?;
        let started = std::time::Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if cancelled() || started.elapsed() > Duration::from_secs(900) {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("RVC Burn conversion stopped.");
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        if !status.success() {
            let summary = std::fs::read_to_string(stderr).unwrap_or_default();
            anyhow::bail!(
                "RVC Burn stopped ({status}): {}",
                summary.lines().last().unwrap_or("")
            );
        }
        let path = std::fs::read_dir(output)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.extension().is_some_and(|ext| ext == "wav"))
            .context("RVC Burn returned no converted audio.")?;
        audio_pcm::decode_stereo(&path)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub(super) epochs: u32,
    pub(super) batch_size: u32,
    pub(super) learning_rate: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            epochs: 5,
            batch_size: 4,
            learning_rate: 0.0001,
        }
    }
}

#[derive(Deserialize)]
pub(super) struct Start {
    dataset_id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    settings: Settings,
}

pub(super) async fn status(State(state): State<AppState>) -> Json<Value> {
    let runtime = Runtime::discover(&state);
    let mut result = rvc_burn_install::status(&state).await;
    let missing = runtime.missing(true);
    result["ready"] = serde_json::json!(hardware::hardware().card_cuda.is_some() && missing.is_empty());
    result["conversion_ready"] = serde_json::json!(runtime.ready(false).is_ok());
    result["missing"] = serde_json::json!(missing);
    result["folder"] = serde_json::json!(runtime.folder);
    result["cuda"] = serde_json::json!(hardware::hardware().card_cuda.is_some());
    Json(result)
}

pub(super) async fn start(
    State(state): State<AppState>,
    Json(input): Json<Start>,
) -> Result<Json<training::Run>, ApiFailure> {
    let settings = input.settings;
    if !(1..=1000).contains(&settings.epochs)
        || !(1..=64).contains(&settings.batch_size)
        || !(0.000000001..=0.1).contains(&settings.learning_rate)
    {
        return Err(rvc_voices::invalid(
            "Invalid RVC epochs, batch size or learning rate.",
        ));
    }
    let outcome = async {
        let runtime = Runtime::discover(&state);
        runtime.ready(true)?;
        let _lease = state
            .engine_use
            .try_write()
            .context("The local music engine is busy.")?;
        no_song_rendering(&state)
            .await
            .map_err(anyhow::Error::msg)?;
        ensure!(
            state.training.active_run().await.is_none(),
            "A training run is already going."
        );
        ensure!(
            !state
                .prepare
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .as_ref()
                .is_some_and(|job| !job.finished),
            "Wait until dataset preparation ends."
        );
        state.ace_runtime.stop().await?;
        state.minimax.stop().await;
        evict_kept_models(&state, "before RVC Burn training").await;
        state
            .training
            .start_rvc_burn(
                runtime,
                &input.dataset_id,
                &input.name,
                settings,
                card_hooks(&state).await,
            )
            .await
    }
    .await;
    outcome
        .map(Json)
        .map_err(|error| api_error(StatusCode::CONFLICT, format!("{error:#}")))
}

pub(super) async fn install(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<rvc_voices::Voice>), ApiFailure> {
    let run = state.training.run(&id).map_err(rvc_voices::invalid)?;
    if run.engine != STAGE || run.status == training::RunStatus::Running {
        return Err(rvc_voices::invalid(
            "Choose an ended RVC Burn run with a saved checkpoint.",
        ));
    }
    let checkpoint = state
        .training
        .rvc_burn_checkpoint(&id)
        .map_err(rvc_voices::invalid)?;
    let result = rvc_voices::install_trained_burn(
        update_data_root(&state).join("rvc/voices"),
        checkpoint,
        run.name,
    )
    .await?;
    state
        .training
        .record_rvc_burn_voice(&id, &result.1.id)
        .map_err(rvc_voices::internal)?;
    Ok(result)
}

pub(super) fn progress(line: &str) -> Option<(u32, training::TrainingStepRecord)> {
    let fields: Vec<_> = line.split_whitespace().collect();
    let at = fields.iter().position(|field| *field == "mel")?;
    let step = fields[..at].iter().find_map(|field| {
        let (done, total) = field.split_once('/')?;
        Some((done.parse::<u32>().ok()?, total.parse::<u32>().ok()?))
    })?;
    let loss: f64 = fields.get(at + 1)?.parse().ok()?;
    if !loss.is_finite() {
        return None;
    }
    Some((
        step.1,
        training::TrainingStepRecord {
            step: step.0,
            loss,
            ar_kl: None,
            step_ms: None,
        },
    ))
}
