//! Export: a song written out as WAV, FLAC or MP3 at a chosen
//! sample rate and bit depth, optionally brought to a loudness target, into a
//! new file. The song's own file is only read; its SHA-256 is taken before and
//! after and the export fails if it changed.
//!
//! What is done to the sound is decided here and reported, never implied:
//! - sample rate conversion only when another rate is asked for;
//! - TPDF dither only when bits are taken away (samples already on the target
//!   format's grid are copied exactly);
//! - loudness normalisation (ITU-R BS.1770 / EBU R 128) by a single gain, held
//!   back so the true peak stays under the ceiling. There is no limiter: when the
//!   ceiling stops the gain short, the report says how far short;
//! - without a target, a float source too loud for an integer format is turned
//!   down to fit, and the report says by how much.
//!
//! The file is written as `<name>.part` and renamed when complete; a cancelled
//! or failed export leaves nothing behind. It waits in the studio's `exports`
//! folder until the window has saved it where the person said, and is removed
//! when the window lets the export go; what an earlier run left is removed at start.
//!
//! The stage of a running export and the notes of its report are codes with
//! their numbers: the window says them in its own language.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{bail, Context, Result};
use audio_post::{dither, encode, loudness, Stereo};
use axum::extract::{Path as UrlPath, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{api_error, ApiError, AppState};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Wav,
    Flac,
    Mp3,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Depth {
    #[serde(rename = "16")]
    Int16,
    #[serde(rename = "24")]
    Int24,
    #[serde(rename = "32f")]
    Float32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Loudness {
    /// Integrated loudness to aim for, LUFS (-14 for most streaming services, -23 for EBU broadcast).
    pub target_lufs: f64,
    /// The true peak the gain may not push past, dBTP.
    pub ceiling_dbtp: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub format: Format,
    /// None keeps the source's rate.
    #[serde(default)]
    pub sample_rate: Option<u32>,
    /// WAV: 16, 24 or 32f; FLAC: 16 or 24; MP3: none.
    #[serde(default)]
    pub bit_depth: Option<Depth>,
    #[serde(default)]
    pub mp3_kbps: Option<u32>,
    #[serde(default)]
    pub loudness: Option<Loudness>,
}

pub const RATES: [u32; 4] = [44_100, 48_000, 88_200, 96_000];

/// The same dither for the same song and settings: an export made twice is the same file.
const DITHER_SEED: u64 = 0x5955_4532_4558_50;

impl Settings {
    pub fn check(&self) -> Result<(), String> {
        if let Some(rate) = self.sample_rate {
            if !RATES.contains(&rate) {
                return Err(format!("sample rate {rate} Hz is not offered (44100, 48000, 88200 or 96000)"));
            }
        }
        match (self.format, self.bit_depth) {
            (Format::Wav, Some(_)) => {}
            (Format::Flac, Some(Depth::Int16 | Depth::Int24)) => {}
            (Format::Flac, Some(Depth::Float32)) => return Err("FLAC holds 16 or 24-bit integers, not 32-bit float".into()),
            (Format::Mp3, Some(_)) => return Err("MP3 has no bit depth".into()),
            (Format::Wav | Format::Flac, None) => return Err("choose a bit depth".into()),
            (Format::Mp3, None) => {}
        }
        if self.format == Format::Mp3 {
            if self.sample_rate.is_some_and(|rate| rate > 48_000) {
                return Err("MP3 carries at most 48 kHz".into());
            }
            if self.mp3_kbps.is_some_and(|kbps| encode::mp3_bitrate(kbps) != kbps) {
                return Err("MP3 bitrates are 320, 256, 224, 192, 160 or 128 kbps".into());
            }
        }
        if let Some(target) = self.loudness {
            if !(-40.0..=-5.0).contains(&target.target_lufs) {
                return Err("the loudness target must be between -40 and -5 LUFS".into());
            }
            if !(-9.0..=0.0).contains(&target.ceiling_dbtp) {
                return Err("the true-peak ceiling must be between -9 and 0 dBTP".into());
            }
        }
        Ok(())
    }

    fn extension(&self) -> &'static str {
        match self.format {
            Format::Wav => "wav",
            Format::Flac => "flac",
            Format::Mp3 => "mp3",
        }
    }
}

/// How the source is stored: integer bits when it is integer PCM, none for float or lossy.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct SourceFormat {
    pub bits: Option<u32>,
    pub float: bool,
    pub lossy: bool,
}

pub fn source_format(path: &Path) -> SourceFormat {
    let extension = path.extension().and_then(|value| value.to_str()).unwrap_or("").to_ascii_lowercase();
    match extension.as_str() {
        "wav" => match hound::WavReader::open(path) {
            Ok(reader) => {
                let spec = reader.spec();
                let float = spec.sample_format == hound::SampleFormat::Float;
                SourceFormat { bits: (!float).then_some(spec.bits_per_sample as u32), float, lossy: false }
            }
            Err(_) => SourceFormat { bits: None, float: false, lossy: false },
        },
        "flac" => SourceFormat { bits: flac_bits(path), float: false, lossy: false },
        _ => SourceFormat { bits: None, float: false, lossy: true },
    }
}

/// Bits per sample from a FLAC file's STREAMINFO block.
fn flac_bits(path: &Path) -> Option<u32> {
    let mut head = [0u8; 26];
    std::fs::File::open(path).ok()?.read_exact(&mut head).ok()?;
    if &head[..4] != b"fLaC" || head[4] & 0x7f != 0 {
        return None;
    }
    let packed = u64::from_be_bytes(head[18..26].try_into().ok()?);
    Some(((packed >> 36) & 0x1f) as u32 + 1)
}

pub fn sha256(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Measure {
    pub lufs: Option<f64>,
    pub true_peak_dbtp: f64,
    pub sample_peak_dbfs: f64,
}

fn measure(audio: &Stereo) -> Measure {
    let round = |value: f64| if value.is_finite() { (value * 100.0).round() / 100.0 } else { value };
    Measure {
        lufs: loudness::integrated(audio).map(round),
        true_peak_dbtp: round(loudness::true_peak_db(audio)),
        sample_peak_dbfs: round(loudness::sample_peak_db(audio)),
    }
}

/// One thing done to the sound: `mp3_rate`, `resampled`, `too_quiet`,
/// `ceiling_short`, `gain`, `turned_down`, `mp3`, `dithered` or `exact`, with
/// the numbers its sentence carries.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Note {
    pub code: &'static str,
    pub params: Value,
}

fn note(code: &'static str, params: Value) -> Note {
    Note { code, params }
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub file_name: String,
    pub bytes: u64,
    pub format: Format,
    pub sample_rate: u32,
    pub bit_depth: Option<Depth>,
    pub mp3_kbps: Option<u32>,
    pub source: SourceFormat,
    pub source_rate: u32,
    pub resampled: bool,
    pub dithered: bool,
    pub gain_db: f64,
    pub before: Measure,
    pub after: Measure,
    /// Everything done to the sound.
    pub notes: Vec<Note>,
    pub original_sha256: String,
    pub original_unchanged: bool,
}

/// Where an export stands, for the window to poll.
#[derive(Debug, Clone, Serialize, Default)]
pub struct Progress {
    pub stage: String,
    pub fraction: f64,
}

/// Writes `audio_path` out to `folder/<name>.<ext>` per `settings`.
pub fn run(source: &Path, folder: &Path, name: &str, settings: &Settings, cancel: &AtomicBool, mut tell: impl FnMut(&str, f64)) -> Result<(PathBuf, Report)> {
    settings.check().map_err(anyhow::Error::msg)?;
    let stop = || -> Result<()> {
        if cancel.load(Ordering::Relaxed) {
            bail!("cancelled");
        }
        Ok(())
    };
    let mut notes = Vec::new();
    tell("checking", 0.02);
    let original_sha256 = sha256(source)?;
    let source_format = source_format(source);
    stop()?;
    tell("decoding", 0.1);
    let mut audio = crate::audio_pcm::decode_stereo(source)?;
    let source_rate = audio.rate;
    stop()?;

    let mut rate = settings.sample_rate.unwrap_or(source_rate);
    if settings.format == Format::Mp3 && !matches!(rate, 32_000 | 44_100 | 48_000) {
        rate = 48_000;
        notes.push(note("mp3_rate", json!({ "from": source_rate })));
    }
    let resampled = rate != source_rate;
    if resampled {
        tell("resampling", 0.2);
        audio = audio.resampled(rate)?;
        notes.push(note("resampled", json!({ "from": source_rate, "to": rate })));
        stop()?;
    }

    tell("measuring", 0.4);
    let before = measure(&audio);
    stop()?;

    let tenth = |value: f64| (value * 10.0).round() / 10.0;
    let hundredth = |value: f64| (value * 100.0).round() / 100.0;
    let mut gain_db = 0.0;
    if let Some(target) = settings.loudness {
        match before.lufs {
            None => notes.push(note("too_quiet", json!({}))),
            Some(measured) => {
                let wanted = target.target_lufs - measured;
                let room = target.ceiling_dbtp - before.true_peak_dbtp;
                gain_db = wanted.min(room);
                if gain_db < wanted - 0.05 {
                    notes.push(note("ceiling_short", json!({ "reached": tenth(measured + gain_db), "target": tenth(target.target_lufs), "ceiling": tenth(target.ceiling_dbtp) })));
                } else {
                    notes.push(note("gain", json!({ "gain": hundredth(gain_db), "from": tenth(measured), "to": tenth(target.target_lufs) })));
                }
            }
        }
    } else if settings.bit_depth != Some(Depth::Float32) {
        // an integer file or MP3 cannot hold samples over full scale
        let fit = encode::fitting_gain(&audio);
        if fit < 1.0 {
            gain_db = 20.0 * (fit as f64).log10();
            notes.push(note("turned_down", json!({ "db": hundredth(-gain_db) })));
        }
    }
    if gain_db != 0.0 {
        let gain = 10f64.powf(gain_db / 20.0) as f32;
        audio.left.iter_mut().chain(audio.right.iter_mut()).for_each(|sample| *sample *= gain);
    }
    stop()?;

    tell("encoding", 0.6);
    let bits = match settings.bit_depth {
        Some(Depth::Int16) => Some(16),
        Some(Depth::Int24) => Some(24),
        _ => None,
    };
    let mut dithered = false;
    let bytes = match (settings.format, bits) {
        (Format::Mp3, _) => {
            let kbps = settings.mp3_kbps.unwrap_or(320);
            notes.push(note("mp3", json!({ "kbps": kbps })));
            encode::mp3(&audio, kbps)?
        }
        (format, Some(bits)) => {
            dithered = !dither::on_grid(&audio, bits);
            notes.push(note(if dithered { "dithered" } else { "exact" }, json!({ "bits": bits })));
            let samples = dither::quantize(&audio, bits, dithered, DITHER_SEED);
            match format {
                Format::Flac => encode::flac_pcm(&samples, bits, rate)?,
                _ => wav_int(&samples, bits, rate)?,
            }
        }
        (_, None) => wav_float(&audio)?,
    };
    stop()?;

    tell("verifying", 0.85);
    let after = measure(&audio);
    let file_name = format!("{}.{}", clean_name(name), settings.extension());
    std::fs::create_dir_all(folder).with_context(|| format!("create {}", folder.display()))?;
    let target = unused(folder, &file_name);
    let partial = target.with_file_name(format!("{}.part", target.file_name().and_then(|value| value.to_str()).unwrap_or("export")));
    let written = (|| -> Result<()> {
        std::fs::write(&partial, &bytes)?;
        stop()?;
        if sha256(source)? != original_sha256 {
            bail!("the original file changed while it was exported; nothing was written");
        }
        std::fs::rename(&partial, &target)?;
        Ok(())
    })();
    if let Err(error) = written {
        let _ = std::fs::remove_file(&partial);
        return Err(error);
    }
    tell("done", 1.0);
    let report = Report {
        file_name: target.file_name().and_then(|value| value.to_str()).unwrap_or_default().to_owned(),
        bytes: bytes.len() as u64,
        format: settings.format,
        sample_rate: rate,
        bit_depth: settings.bit_depth,
        mp3_kbps: (settings.format == Format::Mp3).then(|| settings.mp3_kbps.unwrap_or(320)),
        source: source_format,
        source_rate,
        resampled,
        dithered,
        gain_db: (gain_db * 100.0).round() / 100.0,
        before,
        after,
        notes,
        original_sha256,
        original_unchanged: true,
    };
    Ok((target, report))
}

fn wav_int(samples: &[i32], bits: u32, rate: u32) -> Result<Vec<u8>> {
    let spec = hound::WavSpec { channels: 2, sample_rate: rate, bits_per_sample: bits as u16, sample_format: hound::SampleFormat::Int };
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)?;
        for sample in samples {
            writer.write_sample(*sample)?;
        }
        writer.finalize()?;
    }
    Ok(cursor.into_inner())
}

fn wav_float(audio: &Stereo) -> Result<Vec<u8>> {
    let spec = hound::WavSpec { channels: 2, sample_rate: audio.rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)?;
        for (left, right) in audio.left.iter().zip(&audio.right) {
            writer.write_sample(*left)?;
            writer.write_sample(*right)?;
        }
        writer.finalize()?;
    }
    Ok(cursor.into_inner())
}

/// A file name every system accepts.
fn clean_name(name: &str) -> String {
    let cleaned: String = name.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { '_' } else { c }).collect();
    let cleaned = cleaned.trim().trim_matches('.').to_owned();
    if cleaned.is_empty() { "Export".into() } else { cleaned.chars().take(120).collect() }
}

/// `name.ext`, or `name (2).ext` and so on when that is taken: an export never replaces another file.
fn unused(folder: &Path, file_name: &str) -> PathBuf {
    let first = folder.join(file_name);
    if !first.exists() {
        return first;
    }
    let (stem, extension) = file_name.rsplit_once('.').unwrap_or((file_name, ""));
    (2..).map(|n| folder.join(format!("{stem} ({n}).{extension}"))).find(|path| !path.exists()).expect("some number is free")
}

// ---- jobs ----

struct Job {
    progress: Progress,
    done: bool,
    error: Option<String>,
    report: Option<Report>,
    file: Option<PathBuf>,
    /// Written into a folder the caller named: the file is theirs and stays.
    kept: bool,
    cancel: Arc<AtomicBool>,
}

fn jobs() -> &'static Mutex<HashMap<String, Job>> {
    static JOBS: OnceLock<Mutex<HashMap<String, Job>>> = OnceLock::new();
    JOBS.get_or_init(Default::default)
}

fn folder() -> Result<PathBuf, (StatusCode, Json<ApiError>)> {
    crate::studio_data_root().map(|root| root.join("exports")).ok_or_else(|| api_error(StatusCode::INTERNAL_SERVER_ERROR, "the studio has no data folder".into()))
}

#[derive(Debug, Deserialize)]
pub struct Start {
    pub song_id: String,
    /// A folder of the person's to write the file into, for a caller without the window's Save dialog (an agent).
    /// The file there is theirs: letting the export go leaves it.
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(flatten)]
    pub settings: Settings,
}

pub async fn start(State(state): State<AppState>, Json(start): Json<Start>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    start.settings.check().map_err(|error| api_error(StatusCode::BAD_REQUEST, error))?;
    let song = state
        .library
        .get_song(&start.song_id)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))?;
    let source = state.library.media_path_for_song(&song).ok_or_else(|| api_error(StatusCode::NOT_FOUND, "the song's audio is not in the library".into()))?;
    let kept = start.folder.as_deref().map(str::trim).filter(|folder| !folder.is_empty()).map(PathBuf::from);
    if let Some(folder) = &kept {
        if !folder.is_absolute() {
            return Err(api_error(StatusCode::BAD_REQUEST, "folder must be a full path".into()));
        }
    }
    let folder = match &kept {
        Some(folder) => folder.clone(),
        None => folder()?,
    };
    let id = uuid::Uuid::now_v7().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    jobs().lock().expect("jobs").insert(id.clone(), Job { progress: Progress { stage: "queued".into(), fraction: 0.0 }, done: false, error: None, report: None, file: None, kept: kept.is_some(), cancel: cancel.clone() });
    let title = if song.title.trim().is_empty() { "Untitled".to_owned() } else { song.title.clone() };
    let job = id.clone();
    let in_named_folder = kept.is_some();
    tokio::task::spawn_blocking(move || {
        let tell = |stage: &str, fraction: f64| {
            if let Some(entry) = jobs().lock().expect("jobs").get_mut(&job) {
                entry.progress = Progress { stage: stage.into(), fraction };
            }
        };
        let outcome = run(&source, &folder, &title, &start.settings, &cancel, tell);
        match jobs().lock().expect("jobs").get_mut(&job) {
            Some(entry) => {
                entry.done = true;
                match outcome {
                    Ok((file, report)) => {
                        entry.file = Some(file);
                        entry.report = Some(report);
                    }
                    Err(error) => entry.error = Some(error.to_string()),
                }
            }
            // let go while it ran: nobody is left to take the file from the studio's folder
            None => {
                if let (Ok((file, _)), false) = (outcome, in_named_folder) {
                    let _ = std::fs::remove_file(file);
                }
            }
        }
    });
    Ok(Json(json!({ "id": id })))
}

pub async fn status(UrlPath(id): UrlPath<String>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let jobs = jobs().lock().expect("jobs");
    let job = jobs.get(&id).ok_or_else(|| api_error(StatusCode::NOT_FOUND, "no such export".into()))?;
    Ok(Json(json!({
        "id": id,
        "stage": job.progress.stage,
        "fraction": job.progress.fraction,
        "done": job.done,
        "error": job.error,
        "report": job.report,
        "path": job.file.as_ref().filter(|_| job.kept).map(|file| file.display().to_string()),
    })))
}

pub async fn cancel(UrlPath(id): UrlPath<String>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let jobs = jobs().lock().expect("jobs");
    let job = jobs.get(&id).ok_or_else(|| api_error(StatusCode::NOT_FOUND, "no such export".into()))?;
    job.cancel.store(true, Ordering::Relaxed);
    Ok(Json(json!({ "id": id, "cancelling": !job.done })))
}

/// Lets an export go: its file leaves the `exports` folder, and a running one is stopped.
pub async fn forget(UrlPath(id): UrlPath<String>) -> StatusCode {
    if let Some(job) = jobs().lock().expect("jobs").remove(&id) {
        job.cancel.store(true, Ordering::Relaxed);
        if let Some(file) = job.file.filter(|_| !job.kept) {
            let _ = std::fs::remove_file(file);
        }
    }
    StatusCode::NO_CONTENT
}

/// Removes what earlier runs left in the `exports` folder: nothing there outlives the window that asked for it.
pub fn sweep() {
    let Some(folder) = crate::studio_data_root().map(|root| root.join("exports")) else { return };
    let Ok(entries) = std::fs::read_dir(&folder) else { return };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|kind| kind.is_file()) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

pub async fn file(UrlPath(id): UrlPath<String>, request: Request) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let path = jobs().lock().expect("jobs").get(&id).and_then(|job| job.file.clone()).ok_or_else(|| api_error(StatusCode::NOT_FOUND, "this export has no file".into()))?;
    let mut response = crate::serve_audio_file(&path, request).await;
    if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
        let ascii: String = name.chars().map(|c| if c.is_ascii() && c != '"' { c } else { '_' }).collect();
        if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{}", crate::mcp::segment(name))) {
            response.headers_mut().insert(header::CONTENT_DISPOSITION, value);
        }
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, seconds: f64, amplitude: f64) -> Stereo {
        let n = (rate as f64 * seconds) as usize;
        let left: Vec<f32> = (0..n).map(|i| (amplitude * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / rate as f64).sin()) as f32).collect();
        let right: Vec<f32> = (0..n).map(|i| (amplitude * 0.5 * (2.0 * std::f64::consts::PI * 660.0 * i as f64 / rate as f64).sin()) as f32).collect();
        Stereo::new(left, right, rate)
    }

    fn write_source(folder: &Path, name: &str, audio: &Stereo, bits: u16) -> PathBuf {
        let path = folder.join(name);
        let float = bits == 32;
        let spec = hound::WavSpec { channels: 2, sample_rate: audio.rate, bits_per_sample: bits, sample_format: if float { hound::SampleFormat::Float } else { hound::SampleFormat::Int } };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        let full = (1i64 << (bits - 1)) as f64;
        for (l, r) in audio.left.iter().zip(&audio.right) {
            for s in [*l, *r] {
                if float {
                    writer.write_sample(s).unwrap();
                } else {
                    writer.write_sample(((s as f64 * full).round().clamp(-full, full - 1.0)) as i32).unwrap();
                }
            }
        }
        writer.finalize().unwrap();
        path
    }

    fn settings(format: Format, rate: Option<u32>, depth: Option<Depth>) -> Settings {
        Settings { format, sample_rate: rate, bit_depth: depth, mp3_kbps: None, loudness: None }
    }

    fn export(source: &Path, out: &Path, settings: &Settings) -> (PathBuf, Report) {
        run(source, out, "Song", settings, &AtomicBool::new(false), |_, _| {}).unwrap()
    }

    #[test]
    fn every_file_is_written_at_the_rate_and_depth_asked_for() {
        let folder = tempfile::tempdir().unwrap();
        let source = write_source(folder.path(), "source.wav", &tone(48_000, 2.0, 0.5), 32);
        let out = folder.path().join("out");
        for (depth, bits, float) in [(Depth::Int16, 16, false), (Depth::Int24, 24, false), (Depth::Float32, 32, true)] {
            for rate in [44_100, 48_000, 96_000] {
                let (file, report) = export(&source, &out, &settings(Format::Wav, Some(rate), Some(depth)));
                let spec = hound::WavReader::open(&file).unwrap().spec();
                assert_eq!((spec.sample_rate, spec.bits_per_sample as u32, spec.sample_format == hound::SampleFormat::Float, spec.channels), (rate, bits, float, 2));
                assert_eq!(report.resampled, rate != 48_000);
            }
        }
        for (depth, bits) in [(Depth::Int16, 16), (Depth::Int24, 24)] {
            let (file, _) = export(&source, &out, &settings(Format::Flac, Some(44_100), Some(depth)));
            assert_eq!(flac_bits(&file), Some(bits));
            let decoded = crate::audio_pcm::decode_stereo(&file).unwrap();
            assert_eq!(decoded.rate, 44_100);
        }
    }

    #[test]
    fn dither_only_when_bits_are_taken_away() {
        let folder = tempfile::tempdir().unwrap();
        let out = folder.path().join("out");
        // a 16-bit source written again at 16 and 24 bits: copied sample for sample, no dither
        let source16 = write_source(folder.path(), "s16.wav", &tone(48_000, 1.0, 0.5), 16);
        let original: Vec<i32> = hound::WavReader::open(&source16).unwrap().samples::<i32>().map(Result::unwrap).collect();
        let (file, report) = export(&source16, &out, &settings(Format::Wav, None, Some(Depth::Int16)));
        assert!(!report.dithered);
        let copied: Vec<i32> = hound::WavReader::open(&file).unwrap().samples::<i32>().map(Result::unwrap).collect();
        assert_eq!(copied, original);
        let (file, report) = export(&source16, &out, &settings(Format::Flac, None, Some(Depth::Int24)));
        assert!(!report.dithered);
        let widened = crate::audio_pcm::decode_stereo(&file).unwrap();
        let reference = crate::audio_pcm::decode_stereo(&source16).unwrap();
        assert_eq!(widened.left, reference.left);
        // float to 16 or 24 bits, and any gain or rate change: dithered
        let float = write_source(folder.path(), "f32.wav", &tone(48_000, 1.0, 0.5), 32);
        assert!(export(&float, &out, &settings(Format::Wav, None, Some(Depth::Int16))).1.dithered);
        assert!(export(&float, &out, &settings(Format::Flac, None, Some(Depth::Int24))).1.dithered);
        assert!(export(&source16, &out, &settings(Format::Wav, Some(44_100), Some(Depth::Int16))).1.dithered);
        // float kept as float: nothing to dither
        assert!(!export(&float, &out, &settings(Format::Wav, None, Some(Depth::Float32))).1.dithered);
    }

    #[test]
    fn the_original_is_never_touched_and_exports_never_replace_each_other() {
        let folder = tempfile::tempdir().unwrap();
        let source = write_source(folder.path(), "source.wav", &tone(48_000, 1.0, 0.5), 24);
        let before = sha256(&source).unwrap();
        let out = folder.path().join("out");
        let mut loud = settings(Format::Wav, Some(44_100), Some(Depth::Int16));
        loud.loudness = Some(Loudness { target_lufs: -14.0, ceiling_dbtp: -1.0 });
        let (first, report) = export(&source, &out, &loud);
        let (second, _) = export(&source, &out, &loud);
        assert_ne!(first, second);
        assert!(second.to_string_lossy().ends_with("Song (2).wav"));
        assert_eq!(report.original_sha256, before);
        assert!(report.original_unchanged);
        assert_eq!(sha256(&source).unwrap(), before);
    }

    #[test]
    fn mp3_frames_carry_the_bitrate_asked_for() {
        let folder = tempfile::tempdir().unwrap();
        let source = write_source(folder.path(), "source.wav", &tone(48_000, 2.0, 0.5), 24);
        let out = folder.path().join("out");
        for kbps in [320, 192, 128] {
            let mut mp3 = settings(Format::Mp3, Some(44_100), None);
            mp3.mp3_kbps = Some(kbps);
            let (file, report) = export(&source, &out, &mp3);
            assert_eq!(report.sample_rate, 44_100);
            let bytes = std::fs::read(&file).unwrap();
            // every MPEG-1 Layer III frame header in the file
            let table = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];
            let mut position = 0;
            let mut frames = 0;
            while position + 4 <= bytes.len() {
                let header = &bytes[position..position + 4];
                if header[0] == 0xff && header[1] & 0xfe == 0xfa {
                    let rate_index = (header[2] >> 4) as usize;
                    let sample_index = (header[2] >> 2) & 3;
                    let padding = ((header[2] >> 1) & 1) as usize;
                    assert_eq!(table[rate_index], kbps, "frame {frames}");
                    assert_eq!(sample_index, 0, "44.1 kHz");
                    position += 144 * kbps as usize * 1000 / 44_100 + padding;
                    frames += 1;
                } else {
                    position += 1;
                }
            }
            assert!(frames > 70, "{frames} frames");
        }
    }

    #[test]
    fn loudness_targets_are_met_and_the_ceiling_is_kept() {
        let folder = tempfile::tempdir().unwrap();
        let source = write_source(folder.path(), "quiet.wav", &tone(48_000, 10.0, 0.05), 32);
        let out = folder.path().join("out");
        for target in [-23.0, -16.0, -14.0] {
            let mut job = settings(Format::Wav, None, Some(Depth::Int24));
            job.loudness = Some(Loudness { target_lufs: target, ceiling_dbtp: -1.0 });
            let (file, report) = export(&source, &out, &job);
            let written = loudness::integrated(&crate::audio_pcm::decode_stereo(&file).unwrap()).unwrap();
            assert!((written - target).abs() <= 0.1, "target {target}, file measures {written:.2}");
            assert!(report.after.true_peak_dbtp <= -1.0 + 0.01);
        }
        // a target the ceiling forbids: held back and said so
        let mut job = settings(Format::Wav, None, Some(Depth::Int24));
        job.loudness = Some(Loudness { target_lufs: -5.0, ceiling_dbtp: -6.0 });
        let (file, report) = export(&source, &out, &job);
        assert!(loudness::true_peak_db(&crate::audio_pcm::decode_stereo(&file).unwrap()) <= -5.99);
        assert!(report.notes.iter().any(|note| note.code == "ceiling_short" && note.params["target"] == -5.0), "{:?}", report.notes);
    }

    #[test]
    fn a_cancelled_export_leaves_no_file() {
        let folder = tempfile::tempdir().unwrap();
        let source = write_source(folder.path(), "source.wav", &tone(48_000, 2.0, 0.5), 24);
        let out = folder.path().join("out");
        for stop_at in ["decoding", "measuring", "encoding", "verifying"] {
            let cancel = AtomicBool::new(false);
            let result = run(&source, &out, "Song", &settings(Format::Flac, Some(44_100), Some(Depth::Int16)), &cancel, |stage, _| {
                if stage == stop_at {
                    cancel.store(true, Ordering::Relaxed);
                }
            });
            assert!(result.is_err(), "{stop_at}");
            let left: Vec<_> = std::fs::read_dir(&out).map(|entries| entries.flatten().map(|entry| entry.file_name()).collect()).unwrap_or_default();
            assert!(left.is_empty(), "after cancelling at {stop_at}: {left:?}");
        }
    }

    #[test]
    fn impossible_combinations_are_refused() {
        assert!(settings(Format::Flac, None, Some(Depth::Float32)).check().is_err());
        assert!(settings(Format::Mp3, None, Some(Depth::Int16)).check().is_err());
        assert!(settings(Format::Wav, None, None).check().is_err());
        assert!(settings(Format::Wav, Some(22_050), Some(Depth::Int16)).check().is_err());
        assert!(settings(Format::Mp3, Some(96_000), None).check().is_err());
        let mut odd = settings(Format::Mp3, None, None);
        odd.mp3_kbps = Some(300);
        assert!(odd.check().is_err());
    }
}
