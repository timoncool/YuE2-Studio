//! Video-conditioned Foley through the shared audio.cpp process.
use super::*;
use acpp_server::{Mode, ModelEntry};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub(super) const FAMILY: &str = "controlfoley";
pub(super) const NAME: &str = "ControlFoley";
pub(super) const PROFILE: &str = "controlfoley_large_44k_q8_0";
const FILE: &str = "controlfoley-large-44k-q8_0.gguf";
const BYTES: u64 = 12_556_682_144;
const DIRECTORY: &str = "audiocpp/models/controlfoley";
const PACKAGE: &str = "https://huggingface.co/audio-cpp/audio.cpp-gguf/blob/33120a4cfdf0d56786db934a03a093d4675de938/ControlFoley-GGUF/controlfoley-large-44k-q8_0.gguf";
const LICENSE: &str = "https://huggingface.co/YJX-Xiaomi/ControlFoley";
const MAX_SECONDS: f64 = 8.0;
const MIN_SECONDS: f64 = 0.2;
const FPS: usize = 25;

fn ffmpeg() -> Option<PathBuf> {
    let executable = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .filter(|path| path.is_absolute())
        .map(|path| path.join(executable))
        .find(|path| path.is_file())
}

pub(super) fn video_name(name: &str) -> anyhow::Result<&str> {
    let mut parts = Path::new(name).components();
    anyhow::ensure!(
        matches!(parts.next(), Some(Component::Normal(part)) if part == name)
            && parts.next().is_none()
            && !name.contains(['/', '\\', ':'])
            && demuxer(name).is_some(),
        "Choose an MP4, MOV, WebM or MKV video."
    );
    Ok(name)
}

fn demuxer(name: &str) -> Option<&'static str> {
    let extension = Path::new(name).extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "mp4" | "mov" => Some("mov"),
        "webm" | "mkv" => Some("matroska"),
        _ => None,
    }
}

pub(super) fn model_dir(root: &Path) -> PathBuf {
    root.join(DIRECTORY)
}

pub(super) fn missing_files(directory: &Path) -> Vec<&'static str> {
    let valid = audio_cpp_music::matches_gguf_package(&directory.join(FILE), BYTES, 4737, 16);
    if valid { vec![] } else { vec![FILE] }
}

pub(super) fn catalog(root: Option<&Path>, runtime_ready: bool) -> Value {
    let mut missing = root.map_or_else(
        || vec!["Studio data folder"],
        |root| missing_files(&model_dir(root)),
    );
    let missing_bytes = if missing.is_empty() { 0 } else { BYTES };
    if !runtime_ready {
        missing.push("audio.cpp runtime");
    }
    if ffmpeg().is_none() {
        missing.push("FFmpeg");
    }
    serde_json::json!({
        "id": FAMILY, "name": NAME, "profile_id": PROFILE, "ready": missing.is_empty(), "missing": missing,
        "bytes": BYTES, "missing_bytes": missing_bytes, "model_directory": DIRECTORY,
        "package_url": PACKAGE, "license_url": LICENSE,
        "duration_seconds": { "min": MIN_SECONDS, "max": MAX_SECONDS, "default": MAX_SECONDS },
    })
}

pub(super) fn entry(directory: PathBuf) -> ModelEntry {
    ModelEntry {
        id: FAMILY.into(),
        family: FAMILY.into(),
        task: "gen".into(),
        mode: Mode::Offline,
        path: directory.join(FILE),
        load_options: Default::default(),
        session_options: Default::default(),
        default_request_options: Default::default(),
        busy_timeout_ms: None,
        output_rate: None,
    }
}

pub(super) struct VideoTask {
    source: PathBuf,
    demuxer: &'static str,
    decoder: PathBuf,
    pub seconds: f64,
    pub steps: u32,
    pub seed: u32,
    description: String,
}

pub(super) fn task(request: &CreateMusicJobRequest) -> anyhow::Result<VideoTask> {
    anyhow::ensure!(
        request.lyrics.trim().is_empty() && request.cover_of.is_none() && request.abc.is_none(),
        "ControlFoley takes a video and an optional sound description."
    );
    let seconds = request.duration_seconds.unwrap_or(MAX_SECONDS);
    anyhow::ensure!(
        (MIN_SECONDS..=MAX_SECONDS).contains(&seconds),
        "Choose a duration from 0.2 to 8 seconds."
    );
    let steps = request.steps.unwrap_or(25);
    anyhow::ensure!(steps > 0, "Use at least one generation step.");
    let seed =
        u32::try_from(request.seed.unwrap_or(42)).context("Use a seed from 0 to 4294967295.")?;
    let name = video_name(
        request
            .controlfoley_video
            .as_deref()
            .context("Choose a video first.")?,
    )?;
    let directory = studio_data_root()
        .context("Studio data folder is unavailable.")?
        .join("videos");
    let source = directory.join(name);
    let base = directory
        .canonicalize()
        .context("Upload the video first.")?;
    let resolved = source
        .canonicalize()
        .context("The uploaded video is missing.")?;
    anyhow::ensure!(
        resolved.starts_with(base) && source.is_file(),
        "The video must be in the Studio video folder."
    );
    let decoder =
        ffmpeg().context("Install FFmpeg and make its executable available on Studio's PATH.")?;
    Ok(VideoTask {
        source,
        demuxer: demuxer(name).expect("the video extension was checked"),
        decoder,
        seconds,
        steps,
        seed,
        description: request.style.trim().to_owned(),
    })
}

impl VideoTask {
    pub(super) fn into_request(
        self,
        stop: &AtomicBool,
    ) -> anyhow::Result<(Value, tempfile::TempDir)> {
        let frames = tempfile::Builder::new()
            .prefix("yue-controlfoley-")
            .tempdir()?;
        self.decode(frames.path(), stop)?;
        Ok((
            serde_json::json!({
                "text": self.description,
                "options": {
                    "video_file": frames.path(),
                    "duration_sec": self.seconds.to_string(),
                    "num_inference_steps": self.steps.to_string(),
                    "seed": self.seed.to_string(),
                },
            }),
            frames,
        ))
    }

    fn decode(&self, directory: &Path, stop: &AtomicBool) -> anyhow::Result<()> {
        anyhow::ensure!(!stop.load(Ordering::SeqCst), "stopped");
        let limit = (self.seconds * FPS as f64).ceil() as usize;
        let mut command = Command::new(&self.decoder);
        command
            .args([
                "-nostdin",
                "-hide_banner",
                "-v",
                "error",
                "-protocol_whitelist",
                "file,pipe",
                "-probesize",
                "10000000",
                "-analyzeduration",
                "5000000",
                "-max_alloc",
                "134217728",
                "-threads",
                "2",
                "-f",
                self.demuxer,
                "-i",
            ])
            .arg(&self.source)
            .args([
                "-map",
                "0:v:0",
                "-an",
                "-sn",
                "-dn",
                "-filter_threads",
                "1",
                "-vf",
                "fps=25,scale=512:512:force_original_aspect_ratio=decrease",
                "-threads",
                "1",
                "-t",
            ])
            .arg(self.seconds.to_string())
            .arg("-frames:v")
            .arg(limit.to_string())
            .arg(directory.join("frame-%05d.png"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(directory.join("decode.log"))?);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().context("Start FFmpeg video decoding.")?;
        let until = Instant::now() + Duration::from_secs(45);
        let success = loop {
            let status = match child.try_wait() {
                Ok(status) => status,
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error).context("Wait for FFmpeg video decoding.");
                }
            };
            if let Some(status) = status {
                break status.success();
            }
            if stop.load(Ordering::SeqCst) || Instant::now() >= until {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::ensure!(!stop.load(Ordering::SeqCst), "stopped");
                anyhow::bail!("FFmpeg timed out while decoding the video.");
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        anyhow::ensure!(!stop.load(Ordering::SeqCst), "stopped");
        if !success {
            let error = std::fs::read_to_string(directory.join("decode.log")).unwrap_or_default();
            anyhow::bail!(
                "FFmpeg could not decode the video: {}",
                error.lines().take(3).collect::<Vec<_>>().join(" ")
            );
        }
        let count = std::fs::read_dir(directory)?
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "png"))
            .count();
        anyhow::ensure!(
            count > 0 && count <= limit,
            "The video did not provide usable frames."
        );
        let timestamps: String = (0..count)
            .map(|index| format!("{:.2}\n", index as f64 / FPS as f64))
            .collect();
        std::fs::write(directory.join("timestamps.txt"), timestamps)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_saved_video_basenames_are_accepted() {
        assert_eq!(video_name("clip.MP4").unwrap(), "clip.MP4");
        for invalid in [
            "../clip.mp4",
            "C:/clip.mp4",
            "a\\b.mkv",
            "clip.wav",
            "clip.mp4/.",
        ] {
            assert!(video_name(invalid).is_err(), "{invalid}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_removes_only_the_temporary_frames() {
        use std::os::unix::fs::PermissionsExt as _;
        let fixture = tempfile::tempdir().unwrap();
        let decoder = fixture.path().join("fake-ffmpeg");
        std::fs::write(
            &decoder,
            r#"#!/bin/sh
for arg do last="$arg"; done
printf '%s\n' "$last" > "$(dirname "$0")/started"
exec sleep 15
"#,
        )
        .unwrap();
        std::fs::set_permissions(&decoder, std::fs::Permissions::from_mode(0o700)).unwrap();
        let source = fixture.path().join("source.mp4");
        std::fs::write(&source, b"keep source bytes").unwrap();
        let marker = fixture.path().join("started");
        let stop = AtomicBool::new(false);
        let task = VideoTask {
            source: source.clone(),
            decoder,
            demuxer: "mov",
            seconds: 2.0,
            steps: 8,
            seed: 42,
            description: String::new(),
        };
        let result = std::thread::scope(|scope| {
            let runner = scope.spawn(|| task.into_request(&stop));
            let until = Instant::now() + Duration::from_secs(5);
            while !marker.is_file() && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(10));
            }
            stop.store(true, Ordering::SeqCst);
            let result = runner.join().unwrap();
            result.unwrap_err().to_string()
        });
        assert_eq!(result, "stopped");
        let pattern = std::fs::read_to_string(marker).unwrap();
        let frames = Path::new(pattern.trim()).parent().unwrap();
        assert!(!frames.exists());
        assert_eq!(std::fs::read(source).unwrap(), b"keep source bytes");
    }
}
