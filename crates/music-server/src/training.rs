//! Training adapters on the user's own songs, in a sidecar process.
//!
//! The trainer (`music-train`) and its weights are optional: nothing is
//! downloaded until the user asks on the training page. Datasets are engine
//! neutral - a folder of 48 kHz WAV files and a `dataset.json` with each song's
//! style and lyrics - so another studio of the family opens the same folder.
//! Runs are the engine's: the recipe, the stages and the checkpoints come from
//! the engine crate, and a finished checkpoint becomes an ordinary adapter.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use anyhow::{bail, Context, Result};
use music_engine::yue_train::{self, TrainingStep};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::RwLock;

use crate::downloads::{Asset, AssetKind, Downloader};

/// The downloader scope the training page reads its progress under.
pub const SCOPE: &str = "training";
/// The optional listening pack downloads under a scope of its own.
pub const LISTEN_SCOPE: &str = "listen";

/// One song of a dataset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetItem {
    pub id: String,
    pub title: String,
    /// Who sings it, for finding its lyrics; from the file's tags or name.
    #[serde(default)]
    pub artist: String,
    /// Where the lyrics came from: a lyrics database's name, "recognised",
    /// or empty when they were written or came with the file.
    #[serde(default)]
    pub lyrics_source: String,
    /// The style sentence the model is prompted with.
    #[serde(default)]
    pub style: String,
    #[serde(default)]
    pub lyrics: String,
    #[serde(default)]
    pub instrumental: bool,
    /// The WAV file inside the dataset's `audio` folder.
    pub file: String,
    pub seconds: f64,
    /// Where it came from: a library song id or an imported file name.
    #[serde(default)]
    pub source: String,
    /// Where the lyrics stand in preparation; a stopped job picks up here.
    pub lyrics_state: LyricsState,
    /// Where the style stands in preparation.
    pub style_state: StyleState,
    /// What the listening model heard, kept until the style is written from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heard: Option<HeardNote>,
}

/// Where a song's lyrics stand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LyricsState {
    /// To be looked up or recognised.
    Wanted,
    /// Found or recognised; still to be laid out in sections.
    Found,
    /// Laid out, written by hand, or an instrumental.
    Done,
}

/// Where a song's style stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StyleState {
    /// To be listened to.
    Wanted,
    /// Heard; the style sentence is still to be written from it.
    Heard,
    /// Written, by the assistant or by hand.
    Done,
}

/// What the listening model heard in a song.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeardNote {
    pub genre: String,
    pub caption: String,
    pub bpm: u32,
}

/// A trigger word made from a dataset's name: the name in Latin letters,
/// each word's vowels after its first letter left out - "Нейромонах Феофан"
/// becomes "nrmnkhffn", a word no prompt has by chance.
pub fn trigger_from(name: &str) -> String {
    const CYRILLIC: [(char, &str); 33] = [
        ('а', "a"), ('б', "b"), ('в', "v"), ('г', "g"), ('д', "d"), ('е', "e"), ('ё', "e"), ('ж', "zh"), ('з', "z"), ('и', "i"), ('й', "y"),
        ('к', "k"), ('л', "l"), ('м', "m"), ('н', "n"), ('о', "o"), ('п', "p"), ('р', "r"), ('с', "s"), ('т', "t"), ('у', "u"), ('ф', "f"),
        ('х', "kh"), ('ц', "ts"), ('ч', "ch"), ('ш', "sh"), ('щ', "sch"), ('ъ', ""), ('ы', "y"), ('ь', ""), ('э', "e"), ('ю', "yu"), ('я', "ya"),
    ];
    let latin: String = name
        .to_lowercase()
        .chars()
        .map(|c| match CYRILLIC.iter().find(|(letter, _)| *letter == c) {
            Some((_, spelled)) => spelled.to_string(),
            None if c.is_ascii_alphanumeric() => c.to_string(),
            None => " ".to_string(),
        })
        .collect();
    let trigger: String = latin
        .split_whitespace()
        .map(|word| {
            let mut letters = word.chars();
            let first = letters.next().map(String::from).unwrap_or_default();
            first + &letters.filter(|c| !"aeiouy".contains(*c)).collect::<String>()
        })
        .collect::<String>()
        .chars()
        .take(12)
        .collect();
    if trigger.len() >= 3 {
        return trigger;
    }
    // a name in another script, or too short: a stable hash of it keeps the
    // word rare and different for every name
    let hash = name.bytes().fold(0x811c_9dc5u32, |hash, byte| (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193));
    format!("lora{trigger}{:06x}", hash & 0x00ff_ffff)
}

/// Lyrics laid out in sections carry a heading line such as "[Verse 1]".
pub fn has_sections(lyrics: &str) -> bool {
    lyrics.lines().map(str::trim).any(|line| line.len() > 2 && line.starts_with('[') && line.ends_with(']'))
}

/// Where lyrics stand judged by the text alone, for a song that comes with
/// them or a dataset written before the states were kept.
pub fn lyrics_state_of(lyrics: &str, instrumental: bool) -> LyricsState {
    if instrumental || has_sections(lyrics) {
        LyricsState::Done
    } else if lyrics.trim().is_empty() {
        LyricsState::Wanted
    } else {
        LyricsState::Found
    }
}

pub fn style_state_of(style: &str) -> StyleState {
    if style.trim().is_empty() { StyleState::Wanted } else { StyleState::Done }
}

/// Fills in what a dataset written by an older studio lacks: its songs get
/// the states their text shows, and the dataset a trigger word from its name.
fn settle_states(dataset: &mut serde_json::Value) {
    let named = dataset.get("name").and_then(serde_json::Value::as_str).map(trigger_from);
    if let (Some(trigger), Some(fields)) = (named, dataset.as_object_mut()) {
        let chosen = fields.get("trigger_chosen").and_then(serde_json::Value::as_bool).unwrap_or(false);
        if !chosen && fields.get("trigger").and_then(serde_json::Value::as_str).is_none_or(|word| word.trim().is_empty()) {
            fields.insert("trigger".into(), trigger.into());
        }
    }
    let Some(items) = dataset.get_mut("items").and_then(serde_json::Value::as_array_mut) else { return };
    for item in items {
        let Some(item) = item.as_object_mut() else { continue };
        let text = |field: &str| item.get(field).and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
        let (lyrics, style) = (text("lyrics"), text("style"));
        let instrumental = item.get("instrumental").and_then(serde_json::Value::as_bool).unwrap_or(false);
        if !item.contains_key("lyrics_state") {
            item.insert("lyrics_state".into(), serde_json::to_value(lyrics_state_of(&lyrics, instrumental)).expect("a state"));
        }
        if !item.contains_key("style_state") {
            item.insert("style_state".into(), serde_json::to_value(style_state_of(&style)).expect("a state"));
        }
    }
}

/// A dataset file, songs written before the states were kept given theirs.
fn read_dataset(path: &Path) -> Result<Dataset> {
    let mut dataset: serde_json::Value = serde_json::from_slice(&std::fs::read(path).with_context(|| format!("read {}", path.display()))?)
        .with_context(|| format!("{} is not JSON", path.display()))?;
    settle_states(&mut dataset);
    serde_json::from_value(dataset).with_context(|| format!("{} is not a dataset", path.display()))
}

/// A set of songs to train on, independent of any engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dataset {
    #[serde(default = "dataset_format")]
    pub format: String,
    pub id: String,
    pub name: String,
    /// A rare word the adapter learns to answer to.
    #[serde(default)]
    pub trigger: String,
    /// The user set the trigger, an empty one included; a word made from the
    /// name fills it only while this is false.
    #[serde(default)]
    pub trigger_chosen: bool,
    pub created_at: String,
    #[serde(default)]
    pub items: Vec<DatasetItem>,
}

fn dataset_format() -> String {
    "music-dataset-v1".into()
}

pub use yue_train::Recipe;

/// Separates the vocals of a song for lyric timing; the studio's separator.
pub trait VocalSeparator: Send + Sync {
    /// Writes the vocals of `mix` to `out` as WAV.
    fn separate(&self, mix: &Path, out: &Path) -> Result<()>;
}

/// The studio-side stage that runs before the trainer's.
const VOCALS_STAGE: &str = "vocals";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Done,
    Failed,
    Cancelled,
    /// The studio closed while it ran.
    Interrupted,
}

/// The device of the last "[Load] <part> backend: <device>" line a trainer
/// wrote.
fn trainer_device(log: &str) -> Option<String> {
    log.lines().rev().find_map(|line| {
        let rest = line.trim().strip_prefix("[Load] ")?;
        let device = rest.split_once(" backend: ")?.1;
        Some(device.split_whitespace().next()?.to_string())
    })
}

/// A training run as it is kept on disk and shown on the page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub engine: String,
    pub dataset_id: String,
    pub dataset_name: String,
    pub name: String,
    pub trigger: String,
    pub recipe: Recipe,
    pub status: RunStatus,
    /// The stage working now, or the one that failed.
    pub stage: Option<String>,
    pub stages: Vec<String>,
    #[serde(default)]
    pub steps: Vec<TrainingStepRecord>,
    #[serde(default)]
    pub error: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub finished_at: Option<String>,
    /// Checkpoints already added to the adapter library, by step.
    #[serde(default)]
    pub installed: Vec<u32>,
    /// Each time the run was trained further, oldest first.
    #[serde(default)]
    pub continuations: Vec<Continuation>,
}

/// The run trained on from the checkpoint at `from` up to `to` steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Continuation {
    pub from: u32,
    pub to: u32,
    pub at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TrainingStepRecord {
    pub step: u32,
    pub loss: f64,
    #[serde(default)]
    pub ar_kl: Option<f64>,
    #[serde(default)]
    pub step_ms: Option<f64>,
}

impl From<TrainingStep> for TrainingStepRecord {
    fn from(step: TrainingStep) -> Self {
        Self { step: step.step, loss: step.loss, ar_kl: Some(step.ar_kl), step_ms: step.step_ms }
    }
}

/// What the studio does with the graphics card around a run: `take` frees it
/// once the run is accepted, before the first stage, and `give_back` runs when
/// the run has ended however it ended.
pub struct CardHooks {
    pub take: Box<dyn FnOnce() -> futures_util::future::BoxFuture<'static, ()> + Send>,
    pub give_back: Box<dyn FnOnce() -> futures_util::future::BoxFuture<'static, ()> + Send>,
}

pub struct Training {
    root: PathBuf,
    engine: String,
    downloader: Downloader,
    /// The run in progress, with its trainer process to stop.
    active: RwLock<Option<Active>>,
    /// Held over each read-change-write of a `run.json` or `dataset.json`, so
    /// two edits never write over each other.
    edits: std::sync::Mutex<()>,
}

struct Active {
    run_id: String,
    cancel: Arc<tokio::sync::Notify>,
}

/// The trainer build this studio uses, shared by every engine of the family.
#[derive(Deserialize)]
struct TrainerSource {
    commit: String,
    shipped_as: String,
    release_tag: String,
    asset: String,
    /// Size of the released archive.
    bytes: u64,
    /// Size of the captioner's archive in the same release.
    caption_bytes: u64,
}

fn trainer_source() -> &'static TrainerSource {
    static SOURCE: OnceLock<TrainerSource> = OnceLock::new();
    SOURCE.get_or_init(|| serde_json::from_str(include_str!("../../../engines/music-train-source.json")).expect("engines/music-train-source.json is valid"))
}

/// Where HOT-Step's tools unpack: the trainer, the captioner and the MIDI
/// transcriber are one build of one commit and share its ggml, so the training
/// pack and Audio to MIDI download it once into the same folder.
pub fn tools_root(data_root: &Path) -> PathBuf {
    data_root.join("hotstep")
}

/// The prefix of a tools release's folder.
pub const TOOLS_FOLDER_PREFIX: &str = "hotstep-tools";

/// Removes the tools folders of earlier releases from the shared root, and
/// the folders of before the tools were one build (`legacy_prefix` under the
/// capability's own `runtime`): nothing runs them any more. The folders of
/// today's release (`current_folder`, and the transcriber's other builds named
/// `<current_folder>-...`) stay.
pub fn remove_older_tool_folders(downloader: &Downloader, current_folder: &str, legacy_prefix: &str) {
    let current = downloader.runtime_dir(current_folder);
    let today = |name: &str| name == current_folder || name.starts_with(&format!("{current_folder}-"));
    let sweep = |directory: &Path, prefix: &str| {
        let Ok(entries) = std::fs::read_dir(directory) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(prefix) && !today(&name) && entry.path().is_dir() {
                if let Err(error) = std::fs::remove_dir_all(entry.path()) {
                    eprintln!("[ERROR] could not remove the older {} {}: {error}", prefix, entry.path().display());
                }
            }
        }
    };
    if let Some(shared) = current.parent() {
        sweep(shared, TOOLS_FOLDER_PREFIX);
    }
    sweep(&downloader.root().join("runtime"), legacy_prefix);
}

/// The folder the trainer archive unpacks into, named by its release: a
/// studio updated to a newer trainer must not find the older one's folder and
/// take it for installed, then call it with options it does not know.
fn trainer_folder() -> &'static str {
    static FOLDER: OnceLock<String> = OnceLock::new();
    FOLDER.get_or_init(|| trainer_source().release_tag.clone())
}

/// Everything training needs: the trainer, released beside the studio, and
/// the engine's weights for it.
pub(crate) fn pack() -> &'static [Asset] {
    static PACK: OnceLock<Vec<Asset>> = OnceLock::new();
    PACK.get_or_init(|| {
        let leak = |text: String| -> &'static str { Box::leak(text.into_boxed_str()) };
        let source = trainer_source();
        let mut assets = vec![Asset {
            id: "music-train",
            label: leak(format!("Trainer (HOT-Step {})", &source.commit[..8])),
            kind: AssetKind::Runtime,
            url: leak(format!("https://github.com/timoncool/YuE2-Studio/releases/download/{}/{}", source.release_tag, source.asset)),
            relative_path: leak(source.asset.clone()),
            bytes: source.bytes,
            unzip_into: Some(trainer_folder()),
            marker: leak(source.shipped_as.clone()),
            pick: &[],
            vram_gb: None,
            note: "",
        }];
        assets.extend(yue_train::TRAINING_FILES.iter().map(|file| Asset {
            id: file.id,
            label: file.label,
            kind: AssetKind::Model,
            url: leak(yue_train::training_file_url(file)),
            relative_path: leak(format!("models/{}", file.file)),
            bytes: file.bytes,
            unzip_into: None,
            marker: "",
            pick: &[],
            vram_gb: None,
            note: "",
        }));
        assets
    })
}

/// The captioner, beside the trainer whose ggml it shares. Picked out of its
/// archive by name, so removing it never takes the trainer's folder along.
const CAPTIONER: &str = "ace-caption.exe";
/// The captioner with the ggml it was built with, processor builds included:
/// it runs where the training pack, which carries the CUDA build, is not
/// installed - a machine without an NVIDIA card is not offered that pack.
const CAPTIONER_PICK: [&str; 12] = [
    CAPTIONER,
    "ggml.dll",
    "ggml-base.dll",
    "ggml-cpu-alderlake.dll",
    "ggml-cpu-cannonlake.dll",
    "ggml-cpu-cascadelake.dll",
    "ggml-cpu-haswell.dll",
    "ggml-cpu-icelake.dll",
    "ggml-cpu-sandybridge.dll",
    "ggml-cpu-skylakex.dll",
    "ggml-cpu-sse42.dll",
    "ggml-cpu-x64.dll",
];

/// The captioner the macOS app bundle and the Linux packages carry in
/// `resources/music-midi`, built with the MIDI transcriber from the same
/// HOT-Step; the downloadable archive is a Windows build.
fn bundled_captioner() -> Option<PathBuf> {
    if cfg!(windows) {
        return None;
    }
    let tool = crate::engine_bundle_root().parent()?.join("music-midi").join("music-caption");
    tool.is_file().then_some(tool)
}

/// Describing dataset songs by ear, optional: HOT-Step's `ace-caption` beside
/// the trainer, MOSS-Music-8B-Instruct (OpenMOSS, Apache-2.0; GGUF by
/// scragnog) for what it hears and Beat This! (CPJKU, MIT; ONNX by
/// mosynthkey) for the beat.
pub(crate) fn listen_pack() -> &'static [Asset] {
    static PACK: OnceLock<Vec<Asset>> = OnceLock::new();
    PACK.get_or_init(|| {
        let leak = |text: String| -> &'static str { Box::leak(text.into_boxed_str()) };
        let source = trainer_source();
        let model = |id: &'static str, label: &'static str, url: &'static str, path: &'static str, bytes: u64| Asset {
            id,
            label,
            kind: AssetKind::Model,
            url,
            relative_path: path,
            bytes,
            unzip_into: None,
            marker: "",
            pick: &[],
            vram_gb: None,
            note: "",
        };
        vec![
            Asset {
                id: "ace-caption",
                label: leak(format!("Captioner (HOT-Step {})", &source.commit[..8])),
                kind: AssetKind::Runtime,
                url: leak(format!("https://github.com/timoncool/YuE2-Studio/releases/download/{}/ace-caption-windows-x64.zip", source.release_tag)),
                relative_path: "ace-caption-windows-x64.zip",
                bytes: source.caption_bytes,
                unzip_into: Some(trainer_folder()),
                marker: CAPTIONER,
                pick: &CAPTIONER_PICK,
                vram_gb: None,
                note: "",
            },
            model(
                "moss-audio",
                "MOSS-Music audio tower (F16)",
                "https://huggingface.co/scragnog/MOSS-Music-8B-Instruct-GGUF/resolve/9fe86435bd542bf29cbf4984e9e0ab0ea8ca7e00/moss-aud-f16.gguf",
                "models/moss/moss-aud-f16.gguf",
                1_724_480_640,
            ),
            model(
                "moss-lm",
                "MOSS-Music 8B (Q8_0)",
                "https://huggingface.co/scragnog/MOSS-Music-8B-Instruct-GGUF/resolve/9fe86435bd542bf29cbf4984e9e0ab0ea8ca7e00/moss-lm-q8_0.gguf",
                "models/moss/moss-lm-q8_0.gguf",
                8_709_513_792,
            ),
            model(
                "moss-lm-q4",
                "MOSS-Music 8B (Q4_K_M)",
                "https://huggingface.co/nerualdreming/MOSS-Music-8B-Instruct-GGUF/resolve/0b4eac01086c26baf93238aa8b142ec07b3ae56d/moss-lm-q4_k_m.gguf",
                "models/moss/moss-lm-q4_k_m.gguf",
                5_188_223_648,
            ),
            model(
                "beat-this",
                "Beat This! (tempo)",
                "https://github.com/mosynthkey/beat_this_cpp/raw/07ab790a9ec2eda8093d52d249e3ec4f0510ee72/onnx/beat_this.onnx",
                "models/audio-facts/beat_this.onnx",
                83_077_778,
            ),
        ]
    })
}

fn now() -> String {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs().to_string()).unwrap_or_default()
}

fn new_id() -> String {
    uuid::Uuid::now_v7().simple().to_string()
}

/// A bare `name.wav`: no folder, no drive (`C:x.wav` is a path on Windows).
fn is_plain_wav_name(name: &str) -> bool {
    name.ends_with(".wav") && !name.contains([':', '/', '\\']) && Path::new(name).file_name().and_then(|n| n.to_str()) == Some(name) && name != ".wav"
}

fn safe_id(id: &str) -> Result<&str> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        bail!("not an id: {id}");
    }
    Ok(id)
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let temporary = path.with_extension(format!("{}.part", new_id()));
    std::fs::write(&temporary, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(&temporary, path)?;
    Ok(())
}

fn read_json<T: for<'a> Deserialize<'a>>(path: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// A run's trained steps, one JSON line each, beside its run.json.
const STEPS_FILE: &str = "steps.jsonl";

/// A run as its folder holds it: run.json, with the steps from their own file
/// once it exists. A line still being written has no newline yet and is left
/// for the next read, and one a stop cut off mid-write goes with the next step's
/// append.
fn read_run(dir: &Path) -> Option<Run> {
    let mut run: Run = read_json(&dir.join("run.json"))?;
    if let Ok(text) = std::fs::read_to_string(dir.join(STEPS_FILE)) {
        let complete = &text[..text.rfind('\n').map_or(0, |end| end + 1)];
        let mut steps = Vec::new();
        for line in complete.lines() {
            match serde_json::from_str(line) {
                Ok(record) => steps.push(record),
                Err(error) => eprintln!("[ERROR] {}: a step line that is not JSON is left out ({error}): {line}", dir.join(STEPS_FILE).display()),
            }
        }
        run.steps = steps;
    }
    Some(run)
}

fn write_steps(dir: &Path, steps: &[TrainingStepRecord]) -> Result<()> {
    let mut lines = String::new();
    for record in steps {
        lines.push_str(&serde_json::to_string(record)?);
        lines.push('\n');
    }
    let path = dir.join(STEPS_FILE);
    let temporary = path.with_extension(format!("{}.part", new_id()));
    std::fs::write(&temporary, lines)?;
    std::fs::rename(&temporary, &path)?;
    Ok(())
}

impl Training {
    pub fn new(data_root: &Path, engine: &str) -> Self {
        let root = data_root.join("training");
        Self { downloader: Downloader::with_runtimes(root.clone(), tools_root(data_root)), root, engine: engine.to_string(), active: RwLock::new(None), edits: std::sync::Mutex::new(()) }
    }

    pub fn downloader(&self) -> &Downloader {
        &self.downloader
    }

    /// The trainer: where `YUE_TRAIN_BIN` points in a developer build, else
    /// the one the pack unpacked.
    pub fn trainer(&self) -> PathBuf {
        std::env::var_os("YUE_TRAIN_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| self.downloader.runtime_dir(trainer_folder()).join(&trainer_source().shipped_as))
    }

    pub fn models_dir(&self) -> PathBuf {
        self.root.join("models")
    }

    /// The captioner: `YUE_CAPTION_BIN` in a developer build, the one bundled
    /// beside the MIDI transcriber on macOS and Linux, else the one the
    /// listening pack unpacked beside the trainer.
    pub fn captioner(&self) -> PathBuf {
        std::env::var_os("YUE_CAPTION_BIN")
            .map(PathBuf::from)
            .or_else(bundled_captioner)
            .unwrap_or_else(|| self.downloader.runtime_dir(trainer_folder()).join(CAPTIONER))
    }

    pub fn moss_dir(&self) -> PathBuf {
        self.models_dir().join("moss")
    }

    pub fn audio_facts_dir(&self) -> PathBuf {
        self.models_dir().join("audio-facts")
    }

    fn listen_installed(&self, asset: &Asset) -> bool {
        if asset.marker == CAPTIONER {
            return self.captioner().is_file();
        }
        self.downloader.is_installed(asset)
    }

    /// The listening pack this machine downloads and runs: every file but the MOSS-Music build its card does not take
    /// (`listen::hearing`).
    fn listen_files(&self) -> impl Iterator<Item = &'static Asset> {
        let hearing = crate::listen::hearing(&crate::hardware::hardware(), false);
        listen_pack().iter().filter(move |asset| !asset.id.starts_with("moss-lm") || asset.relative_path.ends_with(hearing.lm))
    }

    /// The listening pack's files with whether each is on disk.
    pub fn listen_status(&self) -> Vec<serde_json::Value> {
        self.listen_files()
            .map(|asset| serde_json::json!({ "id": asset.id, "label": asset.label, "bytes": asset.bytes, "installed": self.listen_installed(asset) }))
            .collect()
    }

    pub fn listen_ready(&self) -> bool {
        self.listen_files().all(|asset| self.listen_installed(asset))
    }

    pub async fn install_listen(&self) -> Result<()> {
        let missing: Vec<&'static Asset> = self.listen_files().filter(|asset| !self.listen_installed(asset)).collect();
        if missing.is_empty() {
            return Ok(());
        }
        self.downloader.install_all(LISTEN_SCOPE, &missing).await
    }

    fn datasets_dir(&self) -> PathBuf {
        self.root.join("datasets")
    }

    fn runs_dir(&self) -> PathBuf {
        self.root.join("runs")
    }

    fn dataset_dir(&self, id: &str) -> Result<PathBuf> {
        Ok(self.datasets_dir().join(safe_id(id)?))
    }

    /// Where a dataset keeps its separated vocals: `<song>/vocals.wav`, the
    /// layout the trainer's aligner reads.
    fn vocals_dir(&self, id: &str) -> Result<PathBuf> {
        Ok(self.dataset_dir(id)?.join("vocals"))
    }

    /// The separated vocals of one dataset song, whether or not made yet.
    pub fn item_vocals(&self, id: &str, item_id: &str) -> Result<PathBuf> {
        let dataset = self.dataset(id)?;
        let item = dataset.items.iter().find(|item| item.id == item_id).with_context(|| format!("no song {item_id} in the dataset"))?;
        Ok(self.vocals_dir(id)?.join(item.file.trim_end_matches(".wav")).join("vocals.wav"))
    }

    /// Songs with lyrics whose vocals are not separated yet.
    fn missing_vocals(&self, dataset: &Dataset) -> Result<Vec<(PathBuf, PathBuf)>> {
        let audio = self.dataset_dir(&dataset.id)?.join("audio");
        let vocals = self.vocals_dir(&dataset.id)?;
        Ok(dataset
            .items
            .iter()
            .filter(|item| !item.instrumental && !item.lyrics.trim().is_empty())
            .map(|item| (audio.join(&item.file), vocals.join(item.file.trim_end_matches(".wav")).join("vocals.wav")))
            .filter(|(_, out)| !out.is_file())
            .collect())
    }

    fn run_dir(&self, id: &str) -> Result<PathBuf> {
        Ok(self.runs_dir().join(safe_id(id)?))
    }

    /// Whether a part of the pack is usable; the trainer counts as present
    /// wherever `trainer` finds it.
    fn installed(&self, asset: &Asset) -> bool {
        if asset.id == "music-train" {
            return self.trainer().is_file();
        }
        self.downloader.is_installed(asset)
    }

    /// The pack's files with whether each is on disk.
    pub fn pack_status(&self) -> Vec<serde_json::Value> {
        pack()
            .iter()
            .map(|asset| serde_json::json!({ "id": asset.id, "label": asset.label, "bytes": asset.bytes, "installed": self.installed(asset) }))
            .collect()
    }

    pub fn pack_ready(&self) -> bool {
        pack().iter().all(|asset| self.installed(asset))
    }

    pub async fn install_pack(&self) -> Result<()> {
        let missing: Vec<&'static Asset> = pack().iter().filter(|asset| !self.installed(asset)).collect();
        if missing.is_empty() {
            return Ok(());
        }
        self.downloader.install_all(SCOPE, &missing).await?;
        self.remove_older_trainers();
        Ok(())
    }

    /// The trainers of earlier releases; nothing runs them any more.
    fn remove_older_trainers(&self) {
        remove_older_tool_folders(&self.downloader, trainer_folder(), "music-train");
    }

    // ── datasets ────────────────────────────────────────────────────────────

    pub fn datasets(&self) -> Vec<Dataset> {
        let Ok(entries) = std::fs::read_dir(self.datasets_dir()) else { return Vec::new() };
        let mut list: Vec<Dataset> = entries
            .flatten()
            .filter(|entry| entry.path().join("dataset.json").is_file())
            .filter_map(|entry| match read_dataset(&entry.path().join("dataset.json")) {
                Ok(dataset) => Some(dataset),
                Err(error) => {
                    eprintln!("[ERROR] {error:#}");
                    None
                }
            })
            .collect();
        list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        list
    }

    pub fn dataset(&self, id: &str) -> Result<Dataset> {
        let path = self.dataset_dir(id)?.join("dataset.json");
        if !path.is_file() {
            bail!("no dataset {id}");
        }
        read_dataset(&path)
    }

    fn save_dataset(&self, dataset: &Dataset) -> Result<()> {
        let dir = self.dataset_dir(&dataset.id)?;
        std::fs::create_dir_all(dir.join("audio"))?;
        write_json(&dir.join("dataset.json"), dataset)
    }

    pub fn create_dataset(&self, name: &str, trigger: &str) -> Result<Dataset> {
        let name = name.trim();
        if name.is_empty() {
            bail!("a dataset needs a name");
        }
        // every dataset has a trigger word from the start; the user can change it
        let trigger = if trigger.trim().is_empty() { trigger_from(name) } else { trigger.trim().to_string() };
        let dataset = Dataset { format: dataset_format(), id: new_id(), name: name.into(), trigger, trigger_chosen: false, created_at: now(), items: Vec::new() };
        self.save_dataset(&dataset)?;
        Ok(dataset)
    }

    pub fn update_dataset(&self, id: &str, name: Option<String>, trigger: Option<String>) -> Result<Dataset> {
        let _edit = self.edits.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut dataset = self.dataset(id)?;
        if let Some(name) = name.map(|name| name.trim().to_string()).filter(|name| !name.is_empty()) {
            dataset.name = name;
        }
        if let Some(trigger) = trigger {
            dataset.trigger = trigger.trim().into();
            dataset.trigger_chosen = true;
        }
        self.save_dataset(&dataset)?;
        Ok(dataset)
    }

    /// Adds a song, stored as 48 kHz 24-bit WAV whatever it came as: the
    /// model's own rate, and more depth than any source has.
    /// `instrumental` is what is known of the song: a library song made without
    /// lyrics is one, an audio file is sung until recognition hears no words.
    #[allow(clippy::too_many_arguments)]
    pub fn add_item(&self, id: &str, source_audio: &Path, title: &str, artist: &str, style: &str, lyrics: &str, instrumental: bool, source: &str) -> Result<Dataset> {
        self.dataset(id)?;
        let audio = crate::audio_pcm::decode_stereo(source_audio)?;
        self.add_audio(id, audio, title, artist, style, lyrics, instrumental, source)
    }

    /// A whole-album file cut into its songs by the cue sheet beside it; the
    /// album is decoded once. Pieces under ten seconds, such as a hidden
    /// intro, are left out.
    pub fn add_album(&self, id: &str, source_audio: &Path, tracks: &[CueTrack], artist: &str, source: &str) -> Result<Dataset> {
        let album = crate::audio_pcm::decode_stereo(source_audio)?;
        let rate = album.rate as f64;
        let mut dataset = self.dataset(id)?;
        for (index, track) in tracks.iter().enumerate() {
            let start = ((track.start * rate) as usize).min(album.frames());
            let end = tracks.get(index + 1).map_or(album.frames(), |next| ((next.start * rate) as usize).min(album.frames()));
            if (end.saturating_sub(start) as f64) < 10.0 * rate {
                continue;
            }
            let piece = audio_post::Stereo::new(album.left[start..end].to_vec(), album.right[start..end].to_vec(), album.rate);
            let performer = if track.performer.is_empty() { artist } else { track.performer.as_str() };
            dataset = self.add_audio(id, piece, &track.title, performer, "", "", false, &format!("{source}#{}", index + 1))?;
        }
        Ok(dataset)
    }

    #[allow(clippy::too_many_arguments)]
    fn add_audio(&self, id: &str, audio: audio_post::Stereo, title: &str, artist: &str, style: &str, lyrics: &str, instrumental: bool, source: &str) -> Result<Dataset> {
        let audio = if audio.rate == 48_000 { audio } else { audio.resampled(48_000)? };
        let seconds = audio.frames() as f64 / 48_000.0;
        if seconds < 10.0 {
            bail!("{title} is shorter than ten seconds; the trainer cuts songs into ten-second pieces");
        }
        let item_id = new_id();
        let file = format!("{item_id}.wav");
        crate::audio_pcm::write_wav24(&self.dataset_dir(id)?.join("audio").join(&file), &audio)?;
        let _edit = self.edits.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut dataset = self.dataset(id)?;
        dataset.items.push(DatasetItem {
            id: item_id,
            title: title.trim().into(),
            artist: artist.trim().into(),
            lyrics_source: String::new(),
            style: style.trim().into(),
            lyrics: lyrics.trim().into(),
            instrumental,
            file,
            seconds,
            source: source.into(),
            lyrics_state: lyrics_state_of(lyrics, instrumental),
            style_state: style_state_of(style),
            heard: None,
        });
        self.save_dataset(&dataset)?;
        Ok(dataset)
    }

    pub fn update_item(&self, id: &str, item_id: &str, patch: ItemPatch) -> Result<Dataset> {
        let _edit = self.edits.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut dataset = self.dataset(id)?;
        let item = dataset.items.iter_mut().find(|item| item.id == item_id).with_context(|| format!("no song {item_id} in the dataset"))?;
        if let Some(title) = patch.title {
            item.title = title.trim().into();
        }
        if let Some(artist) = patch.artist {
            item.artist = artist.trim().into();
        }
        if let Some(source) = patch.lyrics_source {
            item.lyrics_source = source;
        }
        if let Some(style) = patch.style {
            item.style = style.trim().into();
        }
        if let Some(lyrics) = patch.lyrics {
            item.lyrics = lyrics.trim().into();
        }
        if let Some(instrumental) = patch.instrumental {
            item.instrumental = instrumental;
        }
        if let Some(state) = patch.lyrics_state {
            item.lyrics_state = state;
        }
        if let Some(state) = patch.style_state {
            item.style_state = state;
        }
        if let Some(heard) = patch.heard {
            item.heard = heard;
        }
        self.save_dataset(&dataset)?;
        Ok(dataset)
    }

    /// An edit by the user: what they write is final, and a preparation
    /// leaves it alone.
    pub fn edit_item(&self, id: &str, item_id: &str, mut patch: ItemPatch) -> Result<Dataset> {
        let dataset = self.dataset(id)?;
        let item = dataset.items.iter().find(|item| item.id == item_id).with_context(|| format!("no song {item_id} in the dataset"))?;
        if patch.lyrics.is_some() || patch.instrumental.is_some() {
            let instrumental = patch.instrumental.unwrap_or(item.instrumental);
            let lyrics = patch.lyrics.as_deref().unwrap_or(&item.lyrics);
            patch.lyrics_state = Some(if instrumental || !lyrics.trim().is_empty() { LyricsState::Done } else { LyricsState::Wanted });
        }
        if let Some(style) = &patch.style {
            patch.style_state = Some(style_state_of(style));
            patch.heard = Some(None);
        }
        self.update_item(id, item_id, patch)
    }

    /// Sends the chosen songs back to the start of a step, their text kept
    /// until the new one is there: "find again", "describe again".
    pub fn reset_states(&self, id: &str, items: &[String], lyrics: bool, style: bool) -> Result<()> {
        let _edit = self.edits.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut dataset = self.dataset(id)?;
        for item in dataset.items.iter_mut().filter(|item| items.contains(&item.id)) {
            if lyrics {
                item.lyrics_state = LyricsState::Wanted;
            }
            if style {
                item.style_state = StyleState::Wanted;
                item.heard = None;
            }
        }
        self.save_dataset(&dataset)
    }

    /// The preparation at work, kept on disk so a restart picks it up.
    pub fn prepare_job_path(&self) -> PathBuf {
        self.root.join("prepare.json")
    }

    /// The stored audio of one song of a dataset.
    pub fn item_audio(&self, id: &str, item_id: &str) -> Result<PathBuf> {
        let dataset = self.dataset(id)?;
        let item = dataset.items.iter().find(|item| item.id == item_id).with_context(|| format!("no song {item_id} in the dataset"))?;
        Ok(self.dataset_dir(id)?.join("audio").join(&item.file))
    }

    pub fn remove_item(&self, id: &str, item_id: &str) -> Result<Dataset> {
        let _edit = self.edits.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut dataset = self.dataset(id)?;
        let position = dataset.items.iter().position(|item| item.id == item_id).with_context(|| format!("no song {item_id} in the dataset"))?;
        let item = dataset.items.remove(position);
        let _ = std::fs::remove_file(self.dataset_dir(id)?.join("audio").join(&item.file));
        let _ = std::fs::remove_dir_all(self.vocals_dir(id)?.join(item.file.trim_end_matches(".wav")));
        self.save_dataset(&dataset)?;
        Ok(dataset)
    }

    /// Takes a dataset another studio of the family wrote: its `dataset.json`
    /// and the WAV files it names. The copy gets an id of its own, so the same
    /// folder can come in twice without the two meeting.
    /// Takes a dataset another studio wrote: its `dataset.json` and the audio
    /// files uploaded beside it, by their file names. Every song is checked
    /// before anything is written, and a failed import leaves nothing behind.
    pub fn import_dataset(&self, manifest: &[u8], files: &[(String, PathBuf)]) -> Result<Dataset> {
        let mut dataset: serde_json::Value = serde_json::from_slice(manifest).context("dataset.json is not a dataset")?;
        settle_states(&mut dataset);
        let mut dataset: Dataset = serde_json::from_value(dataset).context("dataset.json is not a dataset")?;
        if dataset.format != dataset_format() {
            bail!("dataset.json is {}, this studio reads {}", dataset.format, dataset_format());
        }
        if dataset.items.is_empty() {
            bail!("the dataset has no songs");
        }
        for item in &dataset.items {
            if !is_plain_wav_name(&item.file) {
                bail!("{} is not a dataset audio file", item.file);
            }
        }
        let by_name = |name: &str| files.iter().find(|(file, _)| file == name).map(|(_, path)| path);
        let missing: Vec<&str> = dataset.items.iter().map(|item| item.file.as_str()).filter(|file| by_name(file).is_none()).collect();
        if !missing.is_empty() {
            bail!("the folder lacks {} of the dataset's songs: {}", missing.len(), missing.join(", "));
        }
        dataset.id = new_id();
        dataset.created_at = now();
        let dir = self.dataset_dir(&dataset.id)?;
        let written = (|| -> Result<()> {
            let audio = dir.join("audio");
            std::fs::create_dir_all(&audio)?;
            // every song takes a fresh id and file name here, whatever the other
            // studio called it
            for item in &mut dataset.items {
                let source = by_name(&item.file).context("song vanished")?.clone();
                item.id = new_id();
                item.file = format!("{}.wav", item.id);
                std::fs::copy(&source, audio.join(&item.file)).with_context(|| format!("copy {}", item.title))?;
            }
            self.save_dataset(&dataset)
        })();
        if let Err(error) = written {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(error);
        }
        Ok(dataset)
    }

    /// The folder a dataset lives in, to share it with another studio.
    pub fn dataset_folder(&self, id: &str) -> Result<PathBuf> {
        let dir = self.dataset_dir(id)?;
        if !dir.join("dataset.json").is_file() {
            bail!("no dataset {id}");
        }
        Ok(dir)
    }

    pub fn remove_dataset(&self, id: &str) -> Result<()> {
        let dir = self.dataset_dir(id)?;
        if !dir.join("dataset.json").is_file() {
            bail!("no dataset {id}");
        }
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }

    /// Writes the sidecars the recipe reads beside every song, from what the
    /// dataset says now, so an edit made after import is what trains.
    fn write_sidecars(&self, dataset: &Dataset) -> Result<PathBuf> {
        let audio = self.dataset_dir(&dataset.id)?.join("audio");
        for item in &dataset.items {
            let stem = item.file.trim_end_matches(".wav");
            let lyrics = if item.instrumental { "" } else { item.lyrics.as_str() };
            for (extension, text) in yue_train::sidecars(&item.style, lyrics, item.instrumental) {
                std::fs::write(audio.join(format!("{stem}{extension}")), text)?;
            }
        }
        Ok(audio)
    }

    // ── runs ────────────────────────────────────────────────────────────────

    pub fn runs(&self) -> Vec<Run> {
        let Ok(entries) = std::fs::read_dir(self.runs_dir()) else { return Vec::new() };
        let mut list: Vec<Run> = entries.flatten().filter_map(|entry| read_run(&entry.path())).filter(|run: &Run| run.engine == self.engine).collect();
        list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        list
    }

    pub fn run(&self, id: &str) -> Result<Run> {
        read_run(&self.run_dir(id)?).with_context(|| format!("no training run {id}"))
    }

    /// Writes the run; once its steps have a file of their own, they go there
    /// and run.json keeps none.
    fn save_run(&self, run: &Run) -> Result<()> {
        let dir = self.run_dir(&run.id)?;
        std::fs::create_dir_all(&dir)?;
        if !dir.join(STEPS_FILE).is_file() {
            return write_json(&dir.join("run.json"), run);
        }
        write_steps(&dir, &run.steps)?;
        write_json(&dir.join("run.json"), &Run { steps: Vec::new(), ..run.clone() })
    }

    /// Records one trained step by adding a line, not by writing the whole run again.
    fn append_step(&self, run_id: &str, record: TrainingStepRecord) -> Result<()> {
        use std::io::{Read, Seek, SeekFrom, Write};
        let dir = self.run_dir(run_id)?;
        if !dir.join(STEPS_FILE).is_file() {
            let mut run = self.run(run_id)?;
            run.steps.push(record);
            write_steps(&dir, &run.steps)?;
            return write_json(&dir.join("run.json"), &Run { steps: Vec::new(), ..run });
        }
        let mut file = std::fs::OpenOptions::new().read(true).write(true).open(dir.join(STEPS_FILE))?;
        // a line cut off by a stop mid-write ends without its newline: it goes, and the next record takes its place
        let length = file.metadata()?.len();
        if length > 0 {
            let mut last = [0u8; 1];
            file.seek(SeekFrom::End(-1))?;
            file.read_exact(&mut last)?;
            if last[0] != b'\n' {
                let mut text = Vec::new();
                file.seek(SeekFrom::Start(0))?;
                file.read_to_end(&mut text)?;
                let kept = text.iter().rposition(|byte| *byte == b'\n').map_or(0, |end| end + 1);
                file.set_len(kept as u64)?;
            }
        }
        file.seek(SeekFrom::End(0))?;
        file.write_all(format!("{}\n", serde_json::to_string(&record)?).as_bytes())?;
        Ok(())
    }

    /// A run left `running` by a studio that closed is marked interrupted.
    pub fn recover(&self) {
        for mut run in self.runs().into_iter().filter(|run| run.status == RunStatus::Running) {
            run.status = RunStatus::Interrupted;
            run.finished_at = Some(now());
            let _ = self.save_run(&run);
        }
    }

    /// Whether the run going now trains on this dataset.
    pub async fn dataset_in_use(&self, dataset_id: &str) -> bool {
        match self.active_run().await {
            Some(run_id) => self.run(&run_id).is_ok_and(|run| run.dataset_id == dataset_id),
            None => false,
        }
    }

    pub async fn active_run(&self) -> Option<String> {
        self.active.read().await.as_ref().map(|active| active.run_id.clone())
    }

    pub fn checkpoints(&self, run_id: &str) -> Vec<yue_train::TrainingCheckpoint> {
        self.run_dir(run_id).map(|dir| yue_train::checkpoints(&dir)).unwrap_or_default()
    }

    /// The last `lines` of a run's log that are not progress JSON, and what
    /// the trainer computes on as the log names it ("CUDA0", "Vulkan0", "CPU";
    /// none before it has said), from one read of the log.
    pub fn log_view(&self, run_id: &str, lines: usize) -> (Vec<String>, Option<String>) {
        let Ok(text) = self.run_dir(run_id).and_then(|dir| Ok(std::fs::read_to_string(dir.join("run.log"))?)) else { return (Vec::new(), None) };
        let all: Vec<&str> = text.lines().filter(|line| !line.trim_start().starts_with('{')).collect();
        let tail = all[all.len().saturating_sub(lines)..].iter().map(|line| line.to_string()).collect();
        (tail, trainer_device(&text))
    }

    /// Starts training a dataset; one run at a time, since each wants the card.
    /// `libraries` is where the CUDA runtime the trainer imports lives: the
    /// engine's, fetched on its first start, so it is not downloaded twice.
    pub async fn start(
        self: &Arc<Self>,
        libraries: Option<PathBuf>,
        tokenizer: PathBuf,
        companion: PathBuf,
        separator: Option<Arc<dyn VocalSeparator>>,
        dataset_id: &str,
        name: &str,
        recipe: Recipe,
        card: CardHooks,
    ) -> Result<Run> {
        let trainer = self.trainer();
        if !self.pack_ready() {
            bail!("the training files are not downloaded yet");
        }
        if !trainer.is_file() {
            bail!("the trainer is not installed: {} is missing", trainer.display());
        }
        let dataset = self.dataset(dataset_id)?;
        if dataset.items.is_empty() {
            bail!("the dataset has no songs yet");
        }
        let unsung: Vec<&str> = dataset.items.iter().filter(|item| !item.instrumental && item.lyrics.trim().is_empty()).map(|item| item.title.as_str()).collect();
        if !unsung.is_empty() {
            bail!("these songs have no lyrics: {}; prepare the dataset, write them, or mark the songs instrumental", unsung.join(", "));
        }
        // by epochs, the run's steps follow from the songs it has
        let recipe = recipe.for_songs(dataset.items.len());
        if let Err(problem) = recipe.check() {
            bail!("{problem}");
        }
        let missing = if recipe.lyric_timing { self.missing_vocals(&dataset)? } else { Vec::new() };
        if !missing.is_empty() && separator.is_none() {
            bail!("the vocal separator is not installed; lyric timing needs the vocals of every song with lyrics");
        }
        let mut active = self.active.write().await;
        if active.is_some() {
            bail!("a training run is already going");
        }
        let audio = self.write_sidecars(&dataset)?;
        let run_id = new_id();
        let run_dir = self.run_dir(&run_id)?;
        let inputs = yue_train::TrainingInputs {
            audio,
            models: self.models_dir(),
            tokenizer,
            companion,
            run: run_dir.clone(),
            vocals: self.vocals_dir(&dataset.id)?,
            trigger: dataset.trigger.clone(),
            recipe: recipe.clone(),
        };
        let stages = yue_train::training_stages(&inputs);
        let run = Run {
            id: run_id.clone(),
            engine: self.engine.clone(),
            dataset_id: dataset.id.clone(),
            dataset_name: dataset.name.clone(),
            name: if name.trim().is_empty() { dataset.name.clone() } else { name.trim().into() },
            trigger: dataset.trigger.clone(),
            recipe,
            status: RunStatus::Running,
            stage: None,
            stages: inputs.recipe.lyric_timing.then_some(VOCALS_STAGE).into_iter().chain(stages.iter().map(|stage| stage.id)).map(str::to_string).collect(),
            steps: Vec::new(),
            error: None,
            created_at: now(),
            finished_at: None,
            installed: Vec::new(),
            continuations: Vec::new(),
        };
        self.save_run(&run)?;
        let cancel = Arc::new(tokio::sync::Notify::new());
        *active = Some(Active { run_id: run_id.clone(), cancel: cancel.clone() });
        drop(active);

        let training = self.clone();
        tokio::spawn(async move {
            (card.take)().await;
            let outcome = match training.separate_vocals(&run_id, separator, missing, cancel.clone()).await {
                Ok(true) => training.work(&trainer, libraries.as_deref(), &run_dir, &run_id, stages, cancel).await,
                other => other,
            };
            training.settle(&run_id, outcome).await;
            (card.give_back)().await;
        });
        Ok(run)
    }

    /// Where a run can be trained further from, or which [`RESUME_REFUSALS`]
    /// keeps it from going on.
    pub fn resume_point(&self, run_id: &str) -> Result<(u32, PathBuf), &'static str> {
        let dir = self.run_dir(run_id).map_err(|_| "no_run")?;
        if !dir.join("prepared").join("dataset.json").is_file() {
            return Err("prepared_gone");
        }
        yue_train::resume_point(&dir).ok_or("no_checkpoint")
    }

    /// Trains a run further, up to `steps` in all, from its latest checkpoint:
    /// the same recipe, the same prepared songs, the steps and the chart go on
    /// where they stopped. Stops by steps only - a likeness target would be
    /// judged on a window the resumed trainer starts empty.
    pub async fn continue_run(self: &Arc<Self>, libraries: Option<PathBuf>, companion: PathBuf, run_id: &str, steps: u32, card: CardHooks) -> Result<Run> {
        let trainer = self.trainer();
        if !self.pack_ready() {
            bail!("the training files are not downloaded yet");
        }
        if !trainer.is_file() {
            bail!("the trainer is not installed: {} is missing", trainer.display());
        }
        let (from, state) = self.resume_point(run_id).map_err(|code| anyhow::anyhow!(refusal(code)))?;
        if steps <= from {
            bail!("set the steps above {from}, the checkpoint the run goes on from");
        }
        let mut active = self.active.write().await;
        if active.is_some() {
            bail!("a training run is already going");
        }
        let run_dir = self.run_dir(run_id)?;
        let edit = self.edits.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut run = self.run(run_id)?;
        let mut recipe = run.recipe.clone();
        recipe.steps = steps;
        recipe.target_kl = 0.0;
        recipe.stop = "kl".into();
        let output = run_dir.join(format!("output-{}", new_id()));
        let stage = yue_train::continuation_stage(&self.models_dir(), &companion, &run_dir, &recipe, &output, &state);
        run.recipe = recipe;
        run.status = RunStatus::Running;
        run.stage = None;
        run.stages = vec![stage.id.to_string()];
        run.error = None;
        run.finished_at = None;
        // steps past the checkpoint are trained again; the chart keeps the new ones
        run.steps.retain(|record| record.step <= from);
        run.continuations.push(Continuation { from, to: steps, at: now() });
        self.save_run(&run)?;
        drop(edit);
        let cancel = Arc::new(tokio::sync::Notify::new());
        *active = Some(Active { run_id: run_id.to_string(), cancel: cancel.clone() });
        drop(active);

        let training = self.clone();
        let run_id = run_id.to_string();
        tokio::spawn(async move {
            (card.take)().await;
            let outcome = training.work(&trainer, libraries.as_deref(), &run_dir, &run_id, vec![stage], cancel).await;
            training.settle(&run_id, outcome).await;
            (card.give_back)().await;
        });
        Ok(run)
    }

    /// Records how a run ended and lets the next one start.
    async fn settle(&self, run_id: &str, outcome: Result<bool>) {
        {
            let _edit = self.edits.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Ok(mut run) = self.run(run_id) {
                run.finished_at = Some(now());
                match outcome {
                    Ok(true) => {
                        run.status = RunStatus::Done;
                        run.stage = None;
                    }
                    Ok(false) => run.status = RunStatus::Cancelled,
                    Err(error) => {
                        run.status = RunStatus::Failed;
                        run.error = Some(format!("{error:#}"));
                    }
                }
                let _ = self.save_run(&run);
            }
        }
        *self.active.write().await = None;
    }

    /// Separates the vocals still missing, one song at a time; the result is
    /// kept with the dataset, so the next run and lyric recognition reuse it.
    async fn separate_vocals(&self, run_id: &str, separator: Option<Arc<dyn VocalSeparator>>, missing: Vec<(PathBuf, PathBuf)>, cancel: Arc<tokio::sync::Notify>) -> Result<bool> {
        if missing.is_empty() {
            return Ok(true);
        }
        {
            let _edit = self.edits.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut run = self.run(run_id)?;
            run.stage = Some(VOCALS_STAGE.into());
            self.save_run(&run)?;
        }
        let Some(separator) = separator else { return Ok(true) };
        for (mix, out) in missing {
            let separator = separator.clone();
            let mut job = tokio::task::spawn_blocking(move || -> Result<()> {
                let folder = out.parent().context("vocals folder")?;
                std::fs::create_dir_all(folder)?;
                let partial = folder.join(format!("vocals.{}.part.wav", new_id()));
                let separated = separator.separate(&mix, &partial).and_then(|()| Ok(std::fs::rename(&partial, &out)?));
                if separated.is_err() {
                    let _ = std::fs::remove_file(&partial);
                }
                separated
            });
            tokio::select! {
                done = &mut job => done.context("vocal separation")?.context("separating the vocals")?,
                _ = cancel.notified() => {
                    // the separator cannot be stopped mid-song; the run ends
                    // once it has let go of the card
                    let _ = job.await;
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    /// Runs the stages in order; `Ok(false)` when cancelled.
    async fn work(&self, trainer: &Path, libraries: Option<&Path>, run_dir: &Path, run_id: &str, stages: Vec<yue_train::TrainingStage>, cancel: Arc<tokio::sync::Notify>) -> Result<bool> {
        use tokio::io::AsyncWriteExt;
        let mut log = tokio::fs::OpenOptions::new().create(true).append(true).open(run_dir.join("run.log")).await?;
        for stage in stages {
            {
                {
                    let _edit = self.edits.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    let mut run = self.run(run_id)?;
                    run.stage = Some(stage.id.to_string());
                    self.save_run(&run)?;
                }
            }
            log.write_all(format!("==== {}\n", stage.id).as_bytes()).await?;
            let mut command = tokio::process::Command::new(trainer);
            if let Some(path) = music_core::process::own_libraries_first(trainer) {
                command.env("LD_LIBRARY_PATH", path);
            }
            dub_core::proc::outside_appimage(command.as_std_mut());
            command
                .args(&stage.args)
                .current_dir(run_dir)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true);
            if let Some(libraries) = libraries {
                let mut paths = vec![std::path::PathBuf::from(libraries.as_os_str())];
                paths.extend(std::env::var_os("PATH").iter().flat_map(std::env::split_paths));
                if let Ok(path) = std::env::join_paths(paths) {
                    command.env("PATH", path);
                }
            }
            // The pack carries ggml's processor builds beside CUDA; without this
            // a card whose CUDA does not load would train on the processor.
            command.env("GGML_BACKEND", "CUDA0");
            #[cfg(windows)]
            command.creation_flags(0x0800_0000);
            let mut child = command.spawn().with_context(|| format!("start {}", trainer.display()))?;
            let stdout = child.stdout.take().context("trainer output")?;
            let stderr = child.stderr.take().context("trainer errors")?;
            let (lines_out, mut lines_in) = tokio::sync::mpsc::unbounded_channel::<String>();
            for stream in [Box::new(stdout) as Box<dyn tokio::io::AsyncRead + Unpin + Send>, Box::new(stderr)] {
                let lines_out = lines_out.clone();
                tokio::spawn(async move {
                    let mut lines = BufReader::new(stream).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        if lines_out.send(line).is_err() {
                            break;
                        }
                    }
                });
            }
            drop(lines_out);
            let pid = child.id();
            let mut last_error = String::new();
            let status = loop {
                tokio::select! {
                    line = lines_in.recv() => match line {
                        Some(line) => {
                            log.write_all(line.as_bytes()).await?;
                            log.write_all(b"\n").await?;
                            if let Some(step) = yue_train::parse_training_step(&line) {
                                let recorded = {
                                    let _edit = self.edits.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                    self.append_step(run_id, step.into())
                                };
                                if let Err(error) = recorded {
                                    log.write_all(format!("[studio] the step was not recorded: {error:#}\n").as_bytes()).await?;
                                }
                            } else if line.contains("FATAL") || line.to_ascii_lowercase().contains("error") {
                                last_error = line;
                            }
                        }
                        None => break child.wait().await?,
                    },
                    _ = cancel.notified() => {
                        let _ = child.kill().await;
                        return Ok(false);
                    }
                }
            };
            if !status.success() {
                if last_error.contains("GGML_BACKEND=CUDA0 not found") {
                    bail!("{} stopped: the trainer's CUDA did not load on the graphics card, and training does not run on the processor; it needs NVIDIA driver {} or newer", stage.id, crate::hardware::CUDA13_DRIVER);
                }
                // a crash that wrote nothing says where it happened only in Windows' record of it
                if last_error.is_empty() {
                    if let Some(pid) = pid {
                        let fault = tokio::task::spawn_blocking(move || music_core::process::fault_of(pid, std::time::Duration::from_secs(5))).await.ok().flatten();
                        if let Some(fault) = fault {
                            last_error = fault.to_string();
                        }
                    }
                }
                bail!("{} stopped ({status}){}", stage.id, if last_error.is_empty() { String::new() } else { format!(": {last_error}") });
            }
        }
        Ok(true)
    }

    pub async fn cancel(&self, run_id: &str) -> Result<()> {
        let active = self.active.read().await;
        match active.as_ref() {
            Some(active) if active.run_id == run_id => {
                active.cancel.notify_one();
                Ok(())
            }
            _ => bail!("run {run_id} is not going"),
        }
    }

    pub fn mark_installed(&self, run_id: &str, step: u32) -> Result<()> {
        let _edit = self.edits.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut run = self.run(run_id)?;
        if !run.installed.contains(&step) {
            run.installed.push(step);
        }
        self.save_run(&run)
    }

    pub async fn remove_run(&self, run_id: &str) -> Result<()> {
        if self.active_run().await.as_deref() == Some(run_id) {
            bail!("stop the run before removing it");
        }
        let dir = self.run_dir(run_id)?;
        if !dir.join("run.json").is_file() {
            bail!("no training run {run_id}");
        }
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
}

/// Why a run cannot be trained further, by the code the page translates.
fn refusal(code: &str) -> &'static str {
    match code {
        "prepared_gone" => "the prepared songs of this run are gone; train it again from the dataset",
        "no_checkpoint" => "this run saved no checkpoint to continue from",
        _ => "no such training run",
    }
}

/// What an edit of a dataset song may change.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ItemPatch {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub lyrics_source: Option<String>,
    pub style: Option<String>,
    pub lyrics: Option<String>,
    pub instrumental: Option<bool>,
    /// Set by the preparation, never by the page.
    #[serde(skip)]
    pub lyrics_state: Option<LyricsState>,
    #[serde(skip)]
    pub style_state: Option<StyleState>,
    #[serde(skip)]
    pub heard: Option<Option<HeardNote>>,
}

/// One song of a cue sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct CueTrack {
    pub title: String,
    /// The track's own performer, or the sheet's.
    pub performer: String,
    /// Where the song starts in the album file, in seconds.
    pub start: f64,
}

/// A cue sheet: the audio file it describes and the songs in it. Rippers
/// write them in the system code page as often as in UTF-8.
pub fn cue_sheet(bytes: &[u8]) -> (Option<String>, Vec<CueTrack>) {
    cue_sheet_text(&crate::legacy_text::decode(bytes))
}

fn cue_sheet_text(text: &str) -> (Option<String>, Vec<CueTrack>) {
    let mut sheet_performer = String::new();
    let mut performer: Option<String> = None;
    let quoted = |rest: &str| rest.trim().trim_matches('"').to_string();
    let mut file = None;
    let mut tracks: Vec<CueTrack> = Vec::new();
    let mut title: Option<String> = None;
    let mut in_track = false;
    for line in text.lines() {
        let line = line.trim();
        let (command, rest) = line.split_once(' ').unwrap_or((line, ""));
        match command.to_ascii_uppercase().as_str() {
            "FILE" if file.is_none() => {
                let rest = rest.trim();
                let name = match rest.strip_prefix('"') {
                    Some(inner) => inner.split('"').next().unwrap_or_default().to_string(),
                    None => rest.rsplit_once(' ').map_or(rest, |(name, _)| name).to_string(),
                };
                file = Some(name);
            }
            "TRACK" => {
                in_track = true;
                title = None;
                performer = None;
            }
            "TITLE" if in_track => title = Some(quoted(rest)),
            "PERFORMER" if in_track => performer = Some(quoted(rest)),
            "PERFORMER" => sheet_performer = quoted(rest),
            "INDEX" if in_track => {
                let mut parts = rest.split_whitespace();
                if parts.next() == Some("01") {
                    let stamp: Vec<f64> = parts.next().unwrap_or_default().split(':').filter_map(|part| part.parse().ok()).collect();
                    if let [minutes, seconds, frames] = stamp[..] {
                        let number = tracks.len() + 1;
                        tracks.push(CueTrack {
                            title: title.clone().filter(|title| !title.is_empty()).unwrap_or_else(|| format!("{number:02}")),
                            performer: performer.clone().filter(|performer| !performer.is_empty()).unwrap_or_else(|| sheet_performer.clone()),
                            start: minutes * 60.0 + seconds + frames / 75.0,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    (file, tracks)
}

/// Artist and title of an audio file: its own tags first, then its name,
/// then the folders it was dropped in. `relative` is the path inside what
/// was dropped ("Artist/2011 - Album/01. Title.flac"), or just the file name.
pub fn identify(path: &Path, relative: &str) -> (String, String) {
    let tags = crate::audio_pcm::tags(path);
    let parts: Vec<&str> = relative.split(['/', '\\']).filter(|part| !part.is_empty()).collect();
    let stem = parts.last().map(|name| Path::new(name).file_stem().and_then(|stem| stem.to_str()).unwrap_or(name)).unwrap_or_default();
    let (named_artist, named_title) = split_file_name(stem);
    let folders = if parts.len() > 1 { &parts[..parts.len() - 1] } else { &[][..] };
    let tag_artist = Some(tags.artist.trim().to_string()).filter(|artist| !artist.is_empty() && !is_various(artist));
    let artist = tag_artist.or(named_artist).or_else(|| artist_from_folders(folders)).unwrap_or_default();
    let title = Some(tags.title.trim().to_string()).filter(|title| !title.is_empty()).unwrap_or(named_title);
    (artist, title)
}

fn is_various(artist: &str) -> bool {
    matches!(artist.trim().to_lowercase().as_str(), "various artists" | "various" | "va" | "v.a." | "сборник" | "разные исполнители")
}

fn dash_split(text: &str) -> Vec<String> {
    let dashes = regex::Regex::new(r"\s+[-–—]\s+").expect("valid regex");
    dashes.split(text).map(|part| part.trim().to_string()).filter(|part| !part.is_empty()).collect()
}

/// "01 Title", "12. Title", "A3 - Title" lose their track number; "99 Luftballons"
/// keeps its own: a number followed by a bare space is a track number only
/// when it is zero-padded or three digits (disc and track).
fn without_track_number(text: &str) -> String {
    static NUMBER: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"^\s*(?:[A-Da-d]?\d{1,3}\s*[.)_\-–]\s*|[A-Da-d]?0\d\s+|\d{3}\s+)").expect("valid regex"));
    let stripped = NUMBER.replace(text, "").trim().to_string();
    if stripped.is_empty() { text.trim().to_string() } else { stripped }
}

/// "01. Artist - Title", "Artist - Album - 06 Title", "03 - Title", "Title".
fn split_file_name(stem: &str) -> (Option<String>, String) {
    let spaced = if stem.contains(' ') { stem.to_string() } else { stem.replace('_', " ") };
    let bare = without_track_number(&spaced);
    let parts = dash_split(&bare);
    match parts.len() {
        0 => (None, bare),
        1 => (None, parts[0].clone()),
        _ => {
            let artist = parts[0].clone();
            let title = without_track_number(parts.last().expect("two parts or more"));
            if artist.chars().all(|c| c.is_ascii_digit()) { (None, title) } else { (Some(artist), title) }
        }
    }
}

/// The artist the folders name, nearest first: "Artist - 2019 - Album",
/// "(2004) Artist - Album", "Artist - Album", "Artist - Discography"; a plain
/// name above an album folder is the artist, unless it names a collection.
fn artist_from_folders(folders: &[&str]) -> Option<String> {
    let brackets = regex::Regex::new(r"\s*[\(\[][^\)\]]*[\)\]]").expect("valid regex");
    let year_first = regex::Regex::new(r"^[\(\[]?\d{4}[\)\]]?(?:\s*[-–—.]\s*|\s+)").expect("valid regex");
    let disc = regex::Regex::new(r"(?i)^(cd|disc|disk|диск)\s*\d+$").expect("valid regex");
    let collection = regex::Regex::new(r"(?i)сборник|трибьют|коллекц|любимые|плейлист|collection|compilation|favorit|playlist|singles|soundtrack|\bost\b|best of|music|lossless|flac|mp3|downloads|spoty|spotify").expect("valid regex");
    let mut under_album = false;
    for folder in folders.iter().rev() {
        let original = folder.trim();
        // "(2004) Sirenia - Album": the year comes first, the artist after it
        if let Some(found) = year_first.find(original) {
            let rest = &original[found.end()..];
            let parts: Vec<String> = dash_split(&brackets.replace_all(rest, "")).into_iter().filter(|part| !disc.is_match(part)).collect();
            if parts.len() >= 2 && !parts[0].chars().all(|c| c.is_ascii_digit()) && !collection.is_match(&parts[0]) {
                return Some(parts[0].clone());
            }
            under_album = true;
            continue;
        }
        let clean = brackets.replace_all(original, "").trim().to_string();
        if clean.is_empty() || disc.is_match(&clean) {
            under_album = true;
            continue;
        }
        let parts = dash_split(&clean);
        if parts.len() >= 2 {
            if !parts[0].chars().all(|c| c.is_ascii_digit()) && !collection.is_match(&parts[0]) {
                return Some(parts[0].clone());
            }
            continue;
        }
        let words = clean.split_whitespace().count();
        if collection.is_match(&clean) || words > 4 {
            continue;
        }
        if under_album || folders.len() == 1 || std::ptr::eq(folder, &folders[0]) {
            return Some(clean);
        }
    }
    None
}


/// Lyrics from a `.txt` or `.lrc` file: time stamps and word timings removed,
/// metadata tags such as `[ar:...]` dropped, section tags kept.
pub fn plain_lyrics(text: &str) -> String {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut rest = line.trim();
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(end) = stripped.find(']') else { break };
            let tag = &stripped[..end];
            let timed = tag.chars().next().is_some_and(|c| c.is_ascii_digit());
            let meta = tag.contains(':') && !timed;
            if !(timed || meta) {
                break;
            }
            rest = stripped[end + 1..].trim_start();
        }
        let mut clean = String::new();
        let mut inside = false;
        for c in rest.chars() {
            match c {
                '<' => inside = true,
                '>' if inside => inside = false,
                _ if !inside => clean.push(c),
                _ => {}
            }
        }
        out.push(clean.trim().to_string());
    }
    out.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_trainer_says_what_it_computes_on() {
        assert_eq!(trainer_device("x\n[Load] YuE2 backend: CUDA0 (shared)\nstep 1"), Some("CUDA0".into()));
        assert_eq!(trainer_device("[Load] DiT backend: CPU (CPU threads: 16)"), Some("CPU".into()));
        assert_eq!(trainer_device("nothing yet"), None);
    }

    /// The trainer's cleanup keeps the transcriber's CUDA 12.9 and processor
    /// builds of today's release, and takes every folder of an earlier one.
    #[test]
    fn todays_tool_folders_stay_and_earlier_releases_go() {
        let root = std::env::temp_dir().join(format!("yue-tool-folders-{}", uuid::Uuid::now_v7()));
        let shared = root.join("hotstep");
        let downloader = Downloader::with_runtimes(root.join("training"), shared.clone());
        for name in ["hotstep-tools-new", "hotstep-tools-new-midi-cuda12", "hotstep-tools-new-midi-cpu", "hotstep-tools-old", "hotstep-tools-old-midi-cpu"] {
            std::fs::create_dir_all(shared.join(name)).unwrap();
        }
        remove_older_tool_folders(&downloader, "hotstep-tools-new", "music-train");
        let mut left: Vec<String> = std::fs::read_dir(&shared).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["hotstep-tools-new", "hotstep-tools-new-midi-cpu", "hotstep-tools-new-midi-cuda12"]);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_step_is_a_line_and_a_run_written_before_keeps_its_steps() {
        let root = std::env::temp_dir().join(format!("yue-train-steps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let training = Training::new(&root, "yue2");
        let record = |step: u32| TrainingStepRecord { step, loss: 1.0, ar_kl: None, step_ms: Some(10.0) };
        let run = Run {
            id: "r1".into(),
            engine: "yue2".into(),
            dataset_id: "d".into(),
            dataset_name: "d".into(),
            name: "n".into(),
            trigger: "t".into(),
            recipe: Recipe::default(),
            status: RunStatus::Running,
            stage: None,
            stages: Vec::new(),
            steps: vec![record(1), record(2)],
            error: None,
            created_at: now(),
            finished_at: None,
            installed: Vec::new(),
            continuations: Vec::new(),
        };
        training.save_run(&run).unwrap();
        training.append_step("r1", record(3)).unwrap();
        training.append_step("r1", record(4)).unwrap();
        let dir = training.run_dir("r1").unwrap();
        let kept: Run = read_json(&dir.join("run.json")).unwrap();
        assert!(kept.steps.is_empty());
        assert_eq!(training.run("r1").unwrap().steps.iter().map(|record| record.step).collect::<Vec<_>>(), [1, 2, 3, 4]);
        let mut torn = std::fs::OpenOptions::new().append(true).open(dir.join(STEPS_FILE)).unwrap();
        std::io::Write::write_all(&mut torn, b"{\"step\":5,\"lo").unwrap();
        drop(torn);
        assert_eq!(training.run("r1").unwrap().steps.len(), 4);
        // a stop cut that line off: the run carries on after it
        training.append_step("r1", record(6)).unwrap();
        assert_eq!(training.run("r1").unwrap().steps.iter().map(|record| record.step).collect::<Vec<_>>(), [1, 2, 3, 4, 6]);
        let mut again = training.run("r1").unwrap();
        again.steps.retain(|record| record.step <= 2);
        again.status = RunStatus::Done;
        training.save_run(&again).unwrap();
        assert_eq!(training.runs()[0].steps.iter().map(|record| record.step).collect::<Vec<_>>(), [1, 2]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_trigger_word_is_made_from_the_name() {
        assert_eq!(trigger_from("Нейромонах Феофан"), "nrmnkhffn");
        assert_eq!(trigger_from("Mono Inc."), "mninc");
        assert!(trigger_from("Ария").starts_with("loraar"));
        assert_ne!(trigger_from("周杰伦"), trigger_from("夜に駆ける"));
        assert!(trigger_from("周杰伦").starts_with("lora") && trigger_from("周杰伦").len() == 10);
        assert_eq!(trigger_from("Before The Dawn, Black Sun Aeon"), "bfrthdwnblck");
    }

    #[test]
    fn a_dataset_written_before_the_states_gets_them_from_its_text() {
        let mut dataset = serde_json::json!({"items": [
            {"lyrics": "[Verse 1]\nline", "style": "pop"},
            {"lyrics": "a line\nanother", "style": ""},
            {"lyrics": "", "instrumental": true},
            {"lyrics": ""},
            {"lyrics": "", "lyrics_state": "found", "style_state": "heard"},
        ]});
        settle_states(&mut dataset);
        let states: Vec<(String, String)> = dataset["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| (item["lyrics_state"].as_str().unwrap().to_string(), item["style_state"].as_str().unwrap().to_string()))
            .collect();
        let expected = [("done", "done"), ("found", "wanted"), ("done", "wanted"), ("wanted", "wanted"), ("found", "heard")];
        assert_eq!(states, expected.map(|(lyrics, style)| (lyrics.to_string(), style.to_string())));
    }

    #[test]
    fn songs_are_named_from_their_paths() {
        let name = |relative: &str| {
            let parts: Vec<&str> = relative.split('/').collect();
            let stem = Path::new(parts.last().unwrap()).file_stem().unwrap().to_str().unwrap().to_string();
            let (artist, title) = split_file_name(&stem);
            (artist.or_else(|| artist_from_folders(&parts[..parts.len() - 1])).unwrap_or_default(), title)
        };
        let cases = [
            ("Нейромонах Феофан/Нейромонах Феофан - Тьма во мне 2018/1. Тьма во мне.flac", "Нейромонах Феофан", "Тьма во мне"),
            ("Нейромонах Феофан - Велики силы добра (FLAC)/01 Нейромонах Феофан - Ураган.flac", "Нейромонах Феофан", "Ураган"),
            ("Adrian von Ziegler/2010 - Requiem/12. A Celtic Tale.mp3", "Adrian von Ziegler", "A Celtic Tale"),
            ("Ария/Сборники, трибьюты, саундтреки, синглы, сольные и концертные альбомы/2007 - Пляска Ада (Live) (CD 1)/01 - Интро.mp3", "Ария", "Интро"),
            ("BrunuhVille/2011 Once Upon A Time/BrunuhVille - Once Upon A Time... - 06 Love Is Pain.flac", "BrunuhVille", "Love Is Pain"),
            ("Adrian von Ziegler/2011 - Mirror Of The Night/01  Into the Shadow Realm.mp3", "Adrian von Ziegler", "Into the Shadow Realm"),
            ("ultra/Garbage - Run Baby Run.mp3", "Garbage", "Run Baby Run"),
            ("Sirenia/(2004) Sirenia - An Elixir For Existence (320 kbps)/07 The Fall Within.mp3", "Sirenia", "The Fall Within"),
            ("April Rain/2014 - One Is Glad To Be Of Service [web]/03 - Last Cry Of A Whale Casted Ashore.flac", "April Rain", "Last Cry Of A Whale Casted Ashore"),
            ("Король и Шут/2001 - Как в старой сказке (2001, Мистерия Звука)/08. Зловещий кузен.mp3", "Король и Шут", "Зловещий кузен"),
            ("Classical/Mozart/2005 - The Very Best Of Mozart - CD2/05. Symphony No.41 'Jupiter'_ Molto allegro.flac", "Mozart", "Symphony No.41 'Jupiter'_ Molto allegro"),
            ("Mono inc/Mono Inc. - 2019 - Symphonic Live (FLAC)/04 - If I Fail.flac", "Mono Inc.", "If I Fail"),
            ("Before The Dawn, Black Sun Aeon, Dawn Of Solace ‎– My Darkness - 1999-2013 - 2015/Before The Dawn/2008 - Soundscape Of Silence/06. Fabrication.flac", "Before The Dawn", "Fabrication"),
            ("Franz Ferdinand -  Take Me Out.mp3", "Franz Ferdinand", "Take Me Out"),
            ("Король и Шут/Мотоцикл.mp3", "Король и Шут", "Мотоцикл"),
            ("Lifelover - Discography (2006-2011)/2006 - Pulver/03 - Nackskott.flac", "Lifelover", "Nackskott"),
            ("Nena/99 Luftballons.mp3", "Nena", "99 Luftballons"),
            ("Youssou N'Dour/7 Seconds.flac", "Youssou N'Dour", "7 Seconds"),
        ];
        for (relative, artist, title) in cases {
            assert_eq!(name(relative), (artist.to_string(), title.to_string()), "{relative}");
        }
    }

    #[test]
    fn a_cue_sheet_reads() {
        let text = "PERFORMER \"Нейромонах Феофан\"\r\nFILE \"Альбом.flac\" WAVE\r\n  TRACK 01 AUDIO\r\n    TITLE \"Ураган\"\r\n    INDEX 01 00:00:00\r\n  TRACK 02 AUDIO\r\n    TITLE \"Пни\"\r\n    INDEX 00 02:29:70\r\n    INDEX 01 02:30:08\r\n";
        let (file, tracks) = cue_sheet_text(text);
        assert_eq!(file.as_deref(), Some("Альбом.flac"));
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[1].title, "Пни");
        assert!((tracks[1].start - (150.0 + 8.0 / 75.0)).abs() < 1e-9);
    }

    #[test]
    fn titles_leave_out_track_numbers_and_the_artist() {
        assert_eq!(split_file_name("01 Нейромонах Феофан - Ураган").1, "Ураган");
        assert_eq!(split_file_name("9. Нейромонах Феофан - Петь. Плясать.").1, "Петь. Плясать.");
        assert_eq!(split_file_name("1. Замиренье (feat. Drummatix)").1, "Замиренье (feat. Drummatix)");
        assert_eq!(split_file_name("2077").1, "2077");
        assert_eq!(split_file_name("Song").1, "Song");
    }

    #[test]
    fn lrc_files_become_plain_lyrics() {
        let lrc = "[ar:Someone]\n[00:01.00]First <00:01.50>line\n\n[Chorus]\n[00:05.20]Second line";
        assert_eq!(plain_lyrics(lrc), "First line\n\n[Chorus]\nSecond line");
        assert_eq!(plain_lyrics("[Verse 1]\nplain"), "[Verse 1]\nplain");
    }
}

#[cfg(test)]
mod import_tests {
    use super::*;

    #[test]
    fn a_dataset_names_its_songs_as_bare_wav_files() {
        assert!(is_plain_wav_name("01-song.wav"));
        for bad in ["C:evil.wav", "../x.wav", r"a\b.wav", "a/b.wav", ".wav", "song.mp3", ""] {
            assert!(!is_plain_wav_name(bad), "{bad}");
        }
    }

    /// The trainer and Audio to MIDI each sweep the shared tools folder of every release but their own: two
    /// releases would delete each other's folder on every start.
    #[test]
    fn the_trainer_and_the_transcriber_are_one_tools_release() {
        let read = |text: &str| serde_json::from_str::<serde_json::Value>(text).unwrap();
        let train = read(include_str!("../../../engines/music-train-source.json"));
        let midi = read(include_str!("../../../engines/music-midi-source.json"));
        for key in ["commit", "release_tag", "asset", "bytes"] {
            assert_eq!(train[key], midi[key], "{key}");
        }
    }
}
