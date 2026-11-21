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

const SAVED_BEST: &str = "rvc-output/studio-checkpoints";

pub(super) fn best_report(line: &str) -> Option<(u32, f64)> {
    let (_, report) = line.split_once("best mel ")?;
    let (mel, step) = report.strip_suffix(") saved")?.split_once(" (step ")?;
    let mel: f64 = mel.parse().ok()?;
    mel.is_finite().then_some((step.parse().ok()?, mel))
}

pub(super) fn best_snapshot(run: &FsPath) -> Option<PathBuf> {
    let root = run.join(SAVED_BEST);
    std::fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let step = entry.file_name().to_str()?.parse::<u32>().ok()?;
            let path = entry.path();
            (path.join("voice.safetensors").is_file() && path.join("best.json").is_file())
                .then_some((step, path))
        })
        .max_by_key(|(step, _)| *step)
        .map(|(_, path)| path)
}

// The pinned CLI overwrites its best file in place. Keep an independent
// complete generation so stopping during the next write cannot destroy it.
pub(super) fn snapshot_best(run: &FsPath, step: u32, mel: f64) -> anyhow::Result<()> {
    let source = run.join("rvc-output/checkpoint/voice.best.safetensors");
    let output = run.join(SAVED_BEST);
    std::fs::create_dir_all(&output)?;
    let destination = output.join(step.to_string());
    if destination.is_dir() {
        return Ok(());
    }
    let previous = best_snapshot(run);
    let stage = tempfile::tempdir_in(&output)?;
    let voice = stage.path().join("voice.safetensors");
    let before = std::fs::metadata(&source)?;
    std::fs::copy(&source, &voice)?;
    let after = std::fs::metadata(&source)?;
    ensure!(
        before.len() == after.len() && before.modified()? == after.modified()?,
        "RVC replaced the source checkpoint while it was being saved."
    );
    rvc_voices::validate_burn_voice(&voice)?;
    std::fs::write(
        stage.path().join("best.json"),
        serde_json::to_vec(&serde_json::json!({ "step": step, "mel": mel }))?,
    )?;
    std::fs::rename(stage.path(), destination)?;
    if let Some(previous) = previous
        && let Err(error) = std::fs::remove_dir_all(&previous)
    {
        studio_log!(
            "Could not remove older RVC checkpoint {}: {error}",
            previous.display()
        );
    }
    Ok(())
}

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
        if !runtime.exe.is_file()
            && let Ok(profile) = rvc_burn_install::selected()
        {
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
        Self {
            folder,
            exe,
            ort,
            managed: None,
        }
    }

    fn missing(&self, training: bool) -> Vec<String> {
        if let Some((profile, downloads)) = &self.managed {
            return profile
                .assets(training)
                .filter(|asset| !rvc_burn_install::present(downloads, asset))
                .map(|asset| asset.label.to_owned())
                .collect();
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

    fn prepare_sdk(&self) -> anyhow::Result<()> {
        music_core::process::runtime_beside(&self.exe);
        if let Some((profile, downloader)) = &self.managed {
            rvc_burn_install::prepare_sdk(*profile, downloader)?;
        }
        Ok(())
    }

    pub(super) fn environment(&self, working_dir: &FsPath) -> Vec<(OsString, OsString)> {
        let path = std::env::join_paths(
            self.exe
                .parent()
                .map(FsPath::to_path_buf)
                .into_iter()
                .chain(
                    std::env::var_os("PATH")
                        .iter()
                        .flat_map(std::env::split_paths),
                ),
        )
        .unwrap_or_default();
        let mut env = vec![
            ("PATH".into(), path),
            ("ORT_DYLIB_PATH".into(), self.ort.clone().into_os_string()),
            ("RUST_LOG".into(), "info".into()),
        ];
        if let Some(sdk) = self
            .exe
            .parent()
            .map(|folder| {
                rvc_burn_install::Profile::from_folder(folder)
                    .map(|profile| profile.sdk_dir(folder))
                    .filter(|sdk| sdk.is_dir())
                    .unwrap_or_else(|| folder.join("cuda-sdk"))
            })
            .filter(|sdk| sdk.is_dir())
        {
            env.push((
                "CUDA_PATH".into(),
                sdk_environment_path(&sdk, working_dir).into_os_string(),
            ));
        }
        env
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
        self.prepare_sdk()?;
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
            .envs(self.environment(&self.folder))
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

// NVRTC interprets the byte strings of its include options using the Windows
// ANSI API. An ASCII relative path keeps a Unicode profile folder in the
// child process's wide-character working directory instead.
fn sdk_environment_path(sdk: &FsPath, working_dir: &FsPath) -> PathBuf {
    if cfg!(windows) && !sdk.as_os_str().as_encoded_bytes().is_ascii() {
        for (parents, ancestor) in working_dir.ancestors().enumerate() {
            if let Ok(rest) = sdk.strip_prefix(ancestor) {
                let mut relative = PathBuf::from(".");
                for _ in 0..parents {
                    relative.push("..");
                }
                relative.push(rest);
                if relative.as_os_str().as_encoded_bytes().is_ascii() {
                    return relative;
                }
                break;
            }
        }
    }
    sdk.to_owned()
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
    result["ready"] =
        serde_json::json!(hardware::hardware().card_cuda.is_some() && missing.is_empty());
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
        let sdk_runtime = runtime.clone();
        tokio::task::spawn_blocking(move || sdk_runtime.prepare_sdk()).await??;
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

#[cfg(all(test, windows))]
#[test]
fn nvrtc_uses_an_ascii_relative_include_root_for_a_unicode_profile() {
    let studio = FsPath::new("studio/данные");
    let sdk = studio.join("rvc/burn/runtime/cuda13/cuda-sdk-pinned");
    let training = studio.join("training/runs/saved");
    let convert = studio.join("rvc/burn");
    assert_eq!(
        sdk_environment_path(&sdk, &training),
        PathBuf::from("./../../../rvc/burn/runtime/cuda13/cuda-sdk-pinned")
    );
    assert_eq!(
        sdk_environment_path(&sdk, &convert),
        PathBuf::from("./runtime/cuda13/cuda-sdk-pinned")
    );
}

#[cfg(test)]
#[test]
fn interrupted_best_write_keeps_the_preceding_completed_checkpoint() {
    let run = tempfile::tempdir().unwrap();
    let raw = run
        .path()
        .join("rvc-output/checkpoint/voice.best.safetensors");
    std::fs::create_dir_all(raw.parent().unwrap()).unwrap();
    let header = serde_json::to_vec(&serde_json::json!({
        "__metadata__": {"producer": "burn"},
        "enc_p.emb_phone.weight": {"shape": [768], "data_offsets": [0, 1]},
        "emb_g.weight": {"data_offsets": [1, 2]},
        "dec.conv_post.weight": {"data_offsets": [2, 3]},
    }))
    .unwrap();
    let mut checkpoint = (header.len() as u64).to_le_bytes().to_vec();
    checkpoint.extend(header);
    checkpoint.extend([0x0a, 0x0b, 0x0c]);
    std::fs::write(&raw, &checkpoint).unwrap();
    assert_eq!(
        best_report("00:42 INFO best mel 45.500 (step 2) saved"),
        Some((2, 45.5))
    );
    snapshot_best(run.path(), 2, 45.5).unwrap();
    let previous = best_snapshot(run.path()).unwrap();

    std::fs::write(&raw, &checkpoint[..checkpoint.len() - 2]).unwrap();
    assert!(snapshot_best(run.path(), 3, 42.0).is_err());
    assert_eq!(best_snapshot(run.path()).unwrap(), previous);
    assert_eq!(
        std::fs::read(previous.join("voice.safetensors")).unwrap(),
        checkpoint
    );
}
