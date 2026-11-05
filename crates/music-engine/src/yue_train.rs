//! How YuE2 adapters are trained: the weights the trainer needs, the stages it
//! runs and what they print.
//!
//! The trainer is HOT-Step's `ace-train`, built from a pinned commit and
//! shipped as `music-train`. This module knows only YuE2's side of it - which
//! files, which subcommands with which arguments, how a checkpoint looks - so
//! the studio's training page stays the same for every engine and another
//! engine brings a recipe of its own.
//!
//! A dataset reaches the trainer as a folder of 48 kHz WAV files, each with a
//! `<stem>.txt` sidecar (`caption:`, `is_instrumental:`, `lyrics:` last) and a
//! `<stem>.yue2.txt` holding the one-line style the planner is prompted with.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Video memory a run of the default recipe needs, in GB: the INT8 ConvRot
/// base, the LoKr factors with Prodigy's state and the activations of a
/// ten-second window.
pub const MIN_VRAM_GB: u32 = 11;

/// The Hugging Face repository and commit the training weights come from.
pub const WEIGHTS_REPOSITORY: &str = "scragnog/YuE2-GGUF";
pub const WEIGHTS_REVISION: &str = "eb7de0903bf4dfbd14a2384ae0c0d14150cfa2c7";

/// One file of the training pack, stored flat in the training models folder.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct TrainingFile {
    pub id: &'static str,
    pub label: &'static str,
    /// The path inside the repository.
    pub source: &'static str,
    /// The name it is stored under, the one the trainer looks for.
    pub file: &'static str,
    pub bytes: u64,
}

pub const TRAINING_FILES: &[TrainingFile] = &[
    TrainingFile {
        id: "yue2-convrot-base",
        label: "YuE2 base for training (INT8 ConvRot)",
        source: "checkpoints/yue2_3b_int8_convrot.safetensors",
        file: "yue2_3b_int8_convrot.safetensors",
        bytes: 3_960_938_800,
    },
    TrainingFile {
        id: "yue2-vae-encoder",
        label: "YuE2 VAE with its encoder",
        source: "yue2-vae-standard-f32.gguf",
        file: "yue2-vae-standard-f32.gguf",
        bytes: 530_348_768,
    },
    TrainingFile {
        id: "yue2-semantic-tokenizer",
        label: "Semantic tokenizer",
        source: "yue2-tok-f16.gguf",
        file: "yue2-tok-f16.gguf",
        bytes: 1_211_632_224,
    },
    TrainingFile {
        id: "yue2-sheetsage",
        label: "SheetSage2 score transcriber",
        source: "sheetsage2-f16.gguf",
        file: "sheetsage2-f16.gguf",
        bytes: 1_360_020_736,
    },
    TrainingFile {
        id: "yue2-lyric-aligner",
        label: "MMS forced aligner (lyric timing)",
        source: "mms-fa/mms-fa-f32.gguf",
        file: "mms-fa-f32.gguf",
        bytes: 1_261_892_736,
    },
];

pub fn training_file_url(file: &TrainingFile) -> String {
    format!("https://huggingface.co/{WEIGHTS_REPOSITORY}/resolve/{WEIGHTS_REVISION}/{}", file.source)
}

/// What a run is asked to do.
#[derive(Debug, Clone)]
pub struct TrainingInputs {
    /// The folder of WAV files and sidecars.
    pub audio: PathBuf,
    /// The training models folder, holding [`TRAINING_FILES`].
    pub models: PathBuf,
    /// A YuE2 GGUF the studio already has, read for its text tokenizer.
    pub tokenizer: PathBuf,
    /// The decoder adapter every render merges, frozen under the LoRA so it
    /// trains on the decoder it will be heard through.
    pub companion: PathBuf,
    /// The run's own folder; every stage writes below it.
    pub run: PathBuf,
    /// Holds `<song>/vocals.wav` for every song with lyrics, the audio the
    /// lyric timing is aligned against.
    pub vocals: PathBuf,
    pub trigger: String,
    pub recipe: Recipe,
}

/// What a run is asked for.
///
/// The default method is HOT-Step's base-matched recipe (its training page
/// since 2026-09-27): the adapter is trained the way the YuE2 report says the
/// base was - AdamW at 1e-4 with the report's betas, several songs per
/// update, cosine to a floor, the planner's loss on the tokens it generates
/// at a quarter weight, prompt dropout, the decoder on 60 s crops, a dim-128
/// LoKr - with no KL stop and no lyric timing. `fast`, `balanced` and
/// `thorough` are its three sizes, each a fixed recipe. `tuned` is the recipe before it (KL stop, Prodigy, lyric
/// timing), kept as it was so a run can go back to it; the fields from `stop`
/// to `cursor_weight` are that recipe's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Recipe {
    /// `fast`, `balanced`, `thorough` or `tuned`. A run saved before there
    /// was a choice was trained the tuned way.
    #[serde(default = "tuned_preset")]
    pub preset: String,
    /// `kl`: stop once the planner has moved `target_kl` from the base model,
    /// `steps` a cap; `epochs`: train `epochs` passes over the songs.
    pub stop: String,
    /// One epoch is one pass over the dataset: a step trains one song.
    pub epochs: u32,
    /// A cap: with `target_kl` the run usually stops well before it.
    pub steps: u32,
    pub save_every: u32,
    pub seed: u32,
    /// Stop once the planner's KL to the base model, averaged over 20 steps,
    /// reaches this; 0 trains every step.
    pub target_kl: f64,
    /// `lokr` or `lora`.
    pub adapter: String,
    pub rank: u32,
    pub alpha: f64,
    pub lokr_dim: u32,
    pub lokr_factor: u32,
    /// `prodigy` finds its own step size; `adamw` uses `learning_rate`.
    pub optimizer: String,
    pub learning_rate: f64,
    /// The planner's share of the rate: likeness lives in the renderer, and a
    /// planner at the full rate memorises the songs.
    pub planner_lr_scale: f64,
    /// Supervises where each lyric line is sung, aligned on the vocals.
    pub lyric_timing: bool,
    pub cursor_weight: f64,
    /// Songs per optimizer update: the method's size, 1 under `tuned`.
    #[serde(default = "one")]
    pub grad_accum: u32,
    /// Warmup updates, fixed when the run starts so a continuation keeps them.
    pub warmup: u32,
}

fn tuned_preset() -> String {
    "tuned".into()
}

fn one() -> u32 {
    1
}

/// The base-matched sizes, as HOT-Step's presets since 2026-09-29: updates,
/// songs per update, checkpoint every. Ten checkpoints each.
pub const PRESETS: [(&str, u32, u32, u32); 3] = [("fast", 100, 4, 10), ("balanced", 200, 4, 20), ("thorough", 300, 8, 30)];

/// The decoder window of a base-matched run, 60 s: HOT-Step's presets crop
/// to it since a blind test scored it level with whole songs.
const BASE_MATCHED_DECODER_CROP: &str = "1500";

/// The adapter every base-matched preset trains: LoKr dim 128, factor 4,
/// alpha 256.
const BASE_MATCHED_LOKR: (u32, u32, f64) = (128, 4, 256.0);

impl Default for Recipe {
    fn default() -> Self {
        Self {
            preset: "balanced".into(),
            stop: "kl".into(),
            epochs: 20,
            steps: 750,
            save_every: 50,
            seed: 42,
            target_kl: 1.4,
            adapter: "lokr".into(),
            rank: 64,
            alpha: 256.0,
            lokr_dim: 64,
            lokr_factor: 4,
            optimizer: "prodigy".into(),
            learning_rate: 2e-4,
            planner_lr_scale: 0.3,
            lyric_timing: true,
            cursor_weight: 0.08,
            grad_accum: 4,
            warmup: 0,
        }
    }
}

/// How the training page shows one setting of a recipe. The page renders
/// whatever an engine lists, so each engine brings its own settings; labels
/// and hints are looked up as `trainingField_<key>` and `trainingHint_<key>`.
#[derive(Debug, Clone, Serialize)]
pub struct RecipeField {
    pub key: &'static str,
    /// Fields sharing a group are shown together, under `trainingGroup_<group>`.
    pub group: &'static str,
    pub kind: FieldKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    pub choices: &'static [&'static str],
    /// Shown only while every present condition holds.
    #[serde(serialize_with = "present_conditions", skip_serializing_if = "no_conditions")]
    pub shown_when: [Option<FieldCondition>; 2],
    /// Greyed out, with `trainingHint_<key>_off`, while another field holds one of these values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub off_when: Option<FieldCondition>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    Number,
    Integer,
    Choice,
    Toggle,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct FieldCondition {
    pub field: &'static str,
    pub values: &'static [&'static str],
}

fn no_conditions(conditions: &[Option<FieldCondition>; 2]) -> bool {
    conditions.iter().all(Option::is_none)
}

fn present_conditions<S: serde::Serializer>(conditions: &[Option<FieldCondition>; 2], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(conditions.iter().flatten())
}

const fn number(key: &'static str, group: &'static str, min: f64, max: f64, step: f64) -> RecipeField {
    RecipeField { key, group, kind: FieldKind::Number, min: Some(min), max: Some(max), step: Some(step), choices: &[], shown_when: [None, None], off_when: None }
}

const fn integer(key: &'static str, group: &'static str, min: f64, max: f64, step: f64) -> RecipeField {
    RecipeField { key, group, kind: FieldKind::Integer, min: Some(min), max: Some(max), step: Some(step), choices: &[], shown_when: [None, None], off_when: None }
}

const fn choice(key: &'static str, group: &'static str, choices: &'static [&'static str]) -> RecipeField {
    RecipeField { key, group, kind: FieldKind::Choice, min: None, max: None, step: None, choices, shown_when: [None, None], off_when: None }
}

const fn toggle(key: &'static str, group: &'static str) -> RecipeField {
    RecipeField { key, group, kind: FieldKind::Toggle, min: None, max: None, step: None, choices: &[], shown_when: [None, None], off_when: None }
}

/// The settings of a YuE2 run, in the order the page shows them.
pub fn recipe_fields() -> Vec<RecipeField> {
    const TUNED: FieldCondition = FieldCondition { field: "preset", values: &["tuned"] };
    let lokr = FieldCondition { field: "adapter", values: &["lokr"] };
    let tuned = |field: RecipeField| RecipeField { shown_when: [Some(TUNED), field.shown_when[0]], ..field };
    vec![
        choice("preset", "stop", &["fast", "balanced", "thorough", "tuned"]),
        tuned(choice("stop", "stop", &["kl", "epochs"])),
        tuned(RecipeField { shown_when: [Some(FieldCondition { field: "stop", values: &["kl"] }), None], ..number("target_kl", "stop", 0.1, 5.0, 0.1) }),
        tuned(RecipeField { shown_when: [Some(FieldCondition { field: "stop", values: &["kl"] }), None], ..integer("steps", "stop", 1.0, 5000.0, 50.0) }),
        tuned(RecipeField { shown_when: [Some(FieldCondition { field: "stop", values: &["epochs"] }), None], ..integer("epochs", "stop", 1.0, 500.0, 1.0) }),
        tuned(integer("save_every", "stop", 1.0, 1000.0, 10.0)),
        integer("seed", "stop", 0.0, 4_294_967_295.0, 1.0),
        tuned(choice("adapter", "adapter", &["lokr", "lora"])),
        tuned(integer("rank", "adapter", 1.0, 512.0, 8.0)),
        tuned(number("alpha", "adapter", 1.0, 1024.0, 8.0)),
        RecipeField { shown_when: [Some(TUNED), Some(lokr)], ..integer("lokr_dim", "adapter", 1.0, 512.0, 8.0) },
        RecipeField { shown_when: [Some(TUNED), Some(lokr)], ..integer("lokr_factor", "adapter", 1.0, 64.0, 1.0) },
        tuned(choice("optimizer", "optimizer", &["prodigy", "adamw"])),
        tuned(RecipeField { off_when: Some(FieldCondition { field: "optimizer", values: &["prodigy"] }), ..number("learning_rate", "optimizer", 0.0, 0.01, 0.00005) }),
        tuned(number("planner_lr_scale", "optimizer", 0.05, 1.0, 0.05)),
        tuned(toggle("lyric_timing", "lyrics")),
        tuned(RecipeField { off_when: Some(FieldCondition { field: "lyric_timing", values: &["false"] }), ..number("cursor_weight", "lyrics", 0.0, 1.0, 0.01) }),
    ]
}

impl Recipe {
    /// The recipe the trainer runs for a dataset of `songs`: by epochs, the
    /// steps are the epochs times the songs and nothing stops it earlier.
    pub fn for_songs(&self, songs: usize) -> Recipe {
        let mut recipe = self.clone();
        if let Some((_, updates, songs_per_update, save_every)) = PRESETS.iter().find(|(name, ..)| *name == recipe.preset) {
            recipe.steps = *updates;
            recipe.grad_accum = *songs_per_update;
            recipe.save_every = *save_every;
            recipe.adapter = "lokr".into();
            (recipe.lokr_dim, recipe.lokr_factor, recipe.alpha) = BASE_MATCHED_LOKR;
            recipe.target_kl = 0.0;
            recipe.lyric_timing = false;
            recipe.cursor_weight = 0.0;
            // the base's joint phase warmed up over about 3% of its updates
            recipe.warmup = ((f64::from(*updates) * 0.03).round() as u32).max(1);
            return recipe;
        }
        recipe.grad_accum = 1;
        if recipe.stop == "epochs" {
            recipe.steps = recipe.epochs.saturating_mul(songs.max(1) as u32);
            recipe.target_kl = 0.0;
        }
        recipe
    }

    /// Trained the base-matched way: any method but `tuned`.
    pub fn base_matched(&self) -> bool {
        self.preset != "tuned"
    }

    /// Refuses what the trainer would refuse, before any stage starts.
    pub fn check(&self) -> Result<(), String> {
        if self.steps == 0 || self.save_every == 0 || self.epochs == 0 {
            return Err("steps, epochs and the checkpoint interval must be at least 1".into());
        }
        if self.preset != "tuned" && !PRESETS.iter().any(|(name, ..)| *name == self.preset) {
            return Err(format!("unknown training preset {}", self.preset));
        }
        if self.grad_accum == 0 {
            return Err("songs per update must be at least 1".into());
        }
        if !matches!(self.stop.as_str(), "kl" | "epochs") {
            return Err(format!("unknown stopping rule {}", self.stop));
        }
        if !matches!(self.adapter.as_str(), "lokr" | "lora") {
            return Err(format!("unknown adapter type {}", self.adapter));
        }
        if !matches!(self.optimizer.as_str(), "prodigy" | "adamw") {
            return Err(format!("unknown optimizer {}", self.optimizer));
        }
        let finite = [self.target_kl, self.alpha, self.learning_rate, self.planner_lr_scale, self.cursor_weight].iter().all(|value| value.is_finite());
        if !finite || self.target_kl < 0.0 || self.alpha <= 0.0 || self.learning_rate <= 0.0 || self.planner_lr_scale <= 0.0 || !(0.0..=10.0).contains(&self.cursor_weight) {
            return Err("a recipe number is out of range".into());
        }
        if self.rank == 0 || self.lokr_dim == 0 || self.lokr_factor == 0 {
            return Err("rank, LoKr dimension and factor must be at least 1".into());
        }
        Ok(())
    }
}

/// One trainer invocation.
#[derive(Debug, Clone)]
pub struct TrainingStage {
    /// The stage's name for the interface to translate.
    pub id: &'static str,
    pub args: Vec<OsString>,
}

fn path(value: &Path) -> OsString {
    value.as_os_str().to_owned()
}

/// The training stage's arguments: the prepared songs of `run`, trained by
/// `recipe` into `output`, which the trainer wants new.
fn train_args(models: &Path, companion: &Path, run: &Path, recipe: &Recipe, output: &Path) -> Vec<OsString> {
    let arg = |text: &str| OsString::from(text);
    let text = |value: &dyn ToString| OsString::from(value.to_string());
    let mut train = vec![
        arg("yue2-joint-train"),
        arg("--checkpoint"),
        path(&models.join(TRAINING_FILES[0].file)),
        arg("--companion"),
        path(companion),
        arg("--dataset"),
        path(&run.join("prepared").join("dataset.json")),
        arg("--output"),
        path(output),
        arg("--steps"),
        text(&recipe.steps),
        arg("--save-every"),
        text(&recipe.save_every.max(1)),
        arg("--seed"),
        text(&recipe.seed),
        arg("--device"),
        arg("CUDA0"),
        arg("--rank"),
        text(&recipe.rank),
        arg("--alpha"),
        text(&recipe.alpha),
        arg("--adapter-type"),
        arg(&recipe.adapter),
    ];
    if recipe.adapter == "lokr" {
        train.extend([arg("--lokr-dim"), text(&recipe.lokr_dim), arg("--lokr-factor"), text(&recipe.lokr_factor)]);
    }
    if recipe.base_matched() {
        // HOT-Step's applyBaseMatchedRecipe: its forced parts and its defaults
        train.extend(
            [
                "--optimizer", "adamw-lm", "--lr", "1e-4", "--weight-decay", "0.1", "--beta1", "0.9", "--beta2", "0.95",
                "--kl-weight", "0", "--caption-dropout", "0", "--abc-dropout", "0.5", "--planner-lr-scale", "1",
                "--lr-schedule", "cosine-floor", "--lr-floor", "0.1", "--ar-loss-weight", "0.25", "--ar-targets", "base",
                "--nar-crop-frames", BASE_MATCHED_DECODER_CROP, "--text-dropout", "0.1", "--lyric-dropout", "0.1", "--both-dropout", "0.1",
                "--cursor-weight", "0",
            ]
            .map(arg),
        );
        train.extend([arg("--warmup"), text(&recipe.warmup.max(1)), arg("--grad-accum"), text(&recipe.grad_accum.max(1))]);
        return train;
    }
    train.extend([
        arg("--optimizer"),
        arg(&recipe.optimizer),
        arg("--planner-lr-scale"),
        text(&recipe.planner_lr_scale),
        arg("--cursor-weight"),
        if recipe.lyric_timing { text(&recipe.cursor_weight) } else { arg("0") },
    ]);
    if recipe.optimizer == "prodigy" {
        train.extend([arg("--prodigy-d0"), arg("1e-6")]);
    } else {
        train.extend([arg("--lr"), text(&recipe.learning_rate)]);
    }
    if recipe.target_kl > 0.0 {
        train.extend([arg("--target-kl"), text(&recipe.target_kl)]);
    }
    train
}

/// A finished or stopped run trained further, up to `recipe.steps`: the
/// trainer picks up the optimizer, the song order and the step count from
/// `resume` and writes the new checkpoints into `output`. The recipe must be
/// the one the run started with; the trainer refuses any other.
pub fn continuation_stage(models: &Path, companion: &Path, run: &Path, recipe: &Recipe, output: &Path, resume: &Path) -> TrainingStage {
    let mut args = train_args(models, companion, run, recipe, output);
    args.extend([OsString::from("--resume"), path(resume)]);
    TrainingStage { id: "train", args }
}

/// The checkpoint a run can be continued from: the latest one that kept its
/// optimizer state, with that state's file.
pub fn resume_point(run: &Path) -> Option<(u32, PathBuf)> {
    checkpoint_dirs(run)
        .into_iter()
        .filter_map(|(step, dir)| {
            let state = dir.join("optimizer.resume");
            state.is_file().then_some((step, state))
        })
        .max_by_key(|(step, _)| *step)
}

/// Every `checkpoint-step<N>` folder of a run: the first training writes into
/// `output`, each continuation into an `output-<id>` of its own.
fn checkpoint_dirs(run: &Path) -> Vec<(u32, PathBuf)> {
    let Ok(outputs) = std::fs::read_dir(run) else { return Vec::new() };
    outputs
        .flatten()
        .filter(|entry| entry.file_name().to_str().is_some_and(|name| name == "output" || name.starts_with("output-")))
        .filter_map(|entry| std::fs::read_dir(entry.path()).ok())
        .flat_map(|entries| entries.flatten())
        .filter_map(|entry| Some((entry.file_name().to_str()?.strip_prefix("checkpoint-step")?.parse().ok()?, entry.path())))
        .collect()
}

/// The trainer's stages of a run, in order: latents, semantic codes, lyric
/// timing, scores, the joint dataset, then training. The vocal stems the
/// timing stage reads are the studio's to separate before these start.
pub fn training_stages(inputs: &TrainingInputs) -> Vec<TrainingStage> {
    let models = &inputs.models;
    let cache = inputs.run.join("cache");
    let manifest = cache.join("yue2_preprocess.json");
    let prepared = inputs.run.join("prepared");
    let arg = |text: &str| OsString::from(text);
    let model = |name: &str, file: &str| OsString::from(format!("{name}={}", models.join(file).display()));
    let recipe = &inputs.recipe;
    let train = train_args(models, &inputs.companion, &inputs.run, recipe, &inputs.run.join("output"));
    let mut prepare = vec![
        arg("yue2-prepare-aitk"),
        arg("--legacy-manifest"),
        path(&manifest),
        arg("--checkpoint"),
        path(&models.join(TRAINING_FILES[0].file)),
        arg("--tokenizer"),
        path(&inputs.tokenizer),
        arg("--output"),
        path(&prepared),
        arg("--model"),
        model("vae", TRAINING_FILES[1].file),
        arg("--model"),
        model("semantic", TRAINING_FILES[2].file),
        arg("--model"),
        model("sheetsage", TRAINING_FILES[3].file),
        arg("--lyric-timing"),
        arg(if recipe.lyric_timing { "1" } else { "0" }),
    ];
    if !inputs.trigger.trim().is_empty() {
        prepare.extend([arg("--trigger"), arg(inputs.trigger.trim())]);
    }
    let mut stages = vec![
        TrainingStage {
            id: "latents",
            args: vec![
                arg("yue2-preprocess"),
                arg("--audio"),
                path(&inputs.audio),
                arg("--out"),
                path(&cache),
                arg("--models"),
                path(models),
                arg("--vae"),
                arg("standard"),
                arg("--caption-mode"),
                arg("yue2"),
                // the recordings as mastered: the base was trained on them, and
                // the tuned recipe ran on them before the trainer's -14 LUFS default
                arg("--loudness-lufs"),
                arg("0"),
            ],
        },
        TrainingStage { id: "codes", args: vec![arg("yue2-tokenize"), arg("--manifest"), path(&manifest), arg("--models"), path(models)] },
        TrainingStage {
            id: "align",
            args: vec![
                arg("yue2-align"),
                arg("--manifest"),
                path(&manifest),
                arg("--mmsfa"),
                path(&models.join(TRAINING_FILES[4].file)),
                arg("--stems"),
                path(&inputs.vocals),
            ],
        },
        TrainingStage { id: "scores", args: vec![arg("yue2-sheet"), arg("--manifest"), path(&manifest), arg("--models"), path(models)] },
        TrainingStage { id: "prepare", args: prepare },
        TrainingStage { id: "train", args: train },
    ];
    if !recipe.lyric_timing {
        stages.retain(|stage| stage.id != "align");
    }
    stages
}

/// The two sidecars a song needs beside its audio.
pub fn sidecars(style: &str, lyrics: &str, instrumental: bool) -> [(&'static str, String); 2] {
    let style = style.split_whitespace().collect::<Vec<_>>().join(" ");
    [
        (".txt", format!("caption: {style}\nis_instrumental: {instrumental}\nlyrics:\n{}\n", lyrics.trim())),
        (".yue2.txt", format!("{style}\n")),
    ]
}

/// A step of the training stage, as the trainer reports it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TrainingStep {
    pub step: u32,
    /// The combined loss HOT-Step charts: AR cross-entropy, 0.2 x AR KL and
    /// the NAR flow error.
    pub loss: f64,
    /// How far the composition half has moved from the base model; it keeps
    /// climbing once the adapter starts memorising the dataset.
    pub ar_kl: f64,
    pub step_ms: Option<f64>,
}

/// Reads one line of the training stage's output; progress lines are JSON.
pub fn parse_training_step(line: &str) -> Option<TrainingStep> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if value.get("stage")?.as_str()? != "joint" {
        return None;
    }
    let number = |key: &str| value.get(key).and_then(serde_json::Value::as_f64).filter(|value| value.is_finite());
    Some(TrainingStep {
        step: value.get("step")?.as_u64()? as u32,
        loss: number("ar_ce")? + 0.2 * number("ar_kl")? + number("nar_mse")?,
        ar_kl: number("ar_kl")?,
        step_ms: number("step_ms"),
    })
}

/// A finished checkpoint: its step and the adapter files generation uses.
#[derive(Debug, Clone, Serialize)]
pub struct TrainingCheckpoint {
    pub step: u32,
    pub files: Vec<PathBuf>,
}

/// The native AR or NAR export of a checkpoint folder (`half` is "ar" or
/// "nar"), by whichever name the trainer wrote it under. Older runs wrote
/// `native-ar.safetensors` / `native-nar.safetensors`; newer ones write
/// `<trigger>_yue2_joint_{ar,nar}_step<N>.safetensors` (#177). The engine
/// reads a checkpoint's tensor keys to place each half, so the file name
/// only has to be found here.
fn native_export(dir: &Path, half: &str) -> Option<PathBuf> {
    let native = dir.join(format!("native-{half}.safetensors"));
    if native.is_file() {
        return Some(native);
    }
    let needle = format!("_joint_{half}_step");
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|entry| {
            entry.file_name().to_str().is_some_and(|name| name.ends_with(".safetensors") && name.contains(&needle))
        })
        .map(|entry| entry.path())
        .max_by_key(|path| newest(path))
}

/// The checkpoints a run has written so far, the latest first. A folder
/// counts once both native exports are in it, under either the older
/// `native-{ar,nar}` names or the newer `*_yue2_joint_{ar,nar}_step<N>` ones.
pub fn checkpoints(run: &Path) -> Vec<TrainingCheckpoint> {
    let mut found: Vec<TrainingCheckpoint> = checkpoint_dirs(run)
        .into_iter()
        .filter_map(|(step, dir)| {
            let ar = native_export(&dir, "ar")?;
            let nar = native_export(&dir, "nar")?;
            Some(TrainingCheckpoint { step, files: vec![ar, nar] })
        })
        .collect();
    // a continuation that retrains steps a stopped run had passed writes them again: the newer one counts
    found.sort_by(|a, b| b.step.cmp(&a.step).then_with(|| newest(&b.files[0]).cmp(&newest(&a.files[0]))));
    found.dedup_by_key(|checkpoint| checkpoint.step);
    found
}

fn newest(file: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(file).and_then(|meta| meta.modified()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yue-train-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_run_goes_on_from_its_latest_checkpoint_with_a_state_across_outputs() {
        let run = scratch("resume");
        for (output, step, state) in [("output", 500, true), ("output", 750, false), ("output-2", 900, true)] {
            let dir = run.join(output).join(format!("checkpoint-step{step}"));
            std::fs::create_dir_all(&dir).unwrap();
            if state {
                std::fs::write(dir.join("optimizer.resume"), b"state").unwrap();
            }
            for file in ["native-ar.safetensors", "native-nar.safetensors"] {
                std::fs::write(dir.join(file), b"w").unwrap();
            }
        }
        let (step, state) = resume_point(&run).unwrap();
        assert_eq!(step, 900);
        assert!(state.ends_with("output-2/checkpoint-step900/optimizer.resume"));
        assert_eq!(checkpoints(&run).iter().map(|checkpoint| checkpoint.step).collect::<Vec<_>>(), [900, 750, 500]);
        let _ = std::fs::remove_dir_all(&run);
    }

    #[test]
    fn a_checkpoint_with_the_newer_joint_export_names_is_found() {
        // the trainer now writes <trigger>_yue2_joint_{ar,nar}_step<N>.safetensors (#177)
        let run = scratch("joint-names");
        let dir = run.join("output").join("checkpoint-step200");
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["adapter.safetensors", "anm_yue2_joint_ar_step200.safetensors", "anm_yue2_joint_nar_step200.safetensors", "optimizer.resume"] {
            std::fs::write(dir.join(name), name.as_bytes()).unwrap();
        }
        let found = checkpoints(&run);
        assert_eq!(found.iter().map(|checkpoint| checkpoint.step).collect::<Vec<_>>(), [200]);
        assert_eq!(found[0].files.len(), 2);
        assert!(found[0].files[0].file_name().unwrap().to_str().unwrap().contains("_joint_ar_step"));
        assert!(found[0].files[1].file_name().unwrap().to_str().unwrap().contains("_joint_nar_step"));
        assert_eq!(resume_point(&run).map(|(step, _)| step), Some(200));
        let _ = std::fs::remove_dir_all(&run);
    }

    #[test]
    fn a_continuation_trains_the_prepared_songs_into_a_new_output_from_the_state() {
        let recipe = Recipe { steps: 1400, target_kl: 0.0, ..Recipe::default() };
        let stage = continuation_stage(Path::new("models"), Path::new("models/c.safetensors"), Path::new("run"), &recipe, Path::new("run/output-x"), Path::new("run/output/checkpoint-step1000/optimizer.resume"));
        let args: Vec<String> = stage.args.iter().map(|arg| arg.to_string_lossy().replace('\\', "/")).collect();
        let after = |flag: &str| args[args.iter().position(|arg| arg == flag).unwrap() + 1].clone();
        assert_eq!(stage.id, "train");
        assert_eq!(after("--resume"), "run/output/checkpoint-step1000/optimizer.resume");
        assert_eq!(after("--output"), "run/output-x");
        assert_eq!(after("--dataset"), "run/prepared/dataset.json");
        assert_eq!(after("--steps"), "1400");
        assert_eq!(after("--companion"), "models/c.safetensors");
        assert!(!args.iter().any(|arg| arg == "--target-kl"), "a continuation stops by steps");
    }

    #[test]
    fn epochs_become_steps_and_turn_the_likeness_stop_off() {
        let tuned = Recipe { preset: "tuned".into(), ..Recipe::default() };
        let by_epochs = Recipe { stop: "epochs".into(), epochs: 20, ..tuned.clone() }.for_songs(61);
        assert_eq!(by_epochs.steps, 1220);
        assert_eq!(by_epochs.target_kl, 0.0);
        let by_likeness = tuned.for_songs(61);
        assert_eq!((by_likeness.steps, by_likeness.target_kl, by_likeness.grad_accum), (750, 1.4, 1));
    }

    #[test]
    fn a_method_size_sets_the_updates_the_songs_per_update_and_the_warmup() {
        let balanced = Recipe::default().for_songs(61);
        assert_eq!((balanced.steps, balanced.grad_accum, balanced.warmup, balanced.save_every), (200, 4, 6, 20));
        assert_eq!((balanced.adapter.as_str(), balanced.lokr_dim, balanced.lokr_factor, balanced.alpha), ("lokr", 128, 4, 256.0));
        assert!(!balanced.lyric_timing && balanced.target_kl == 0.0);
        let thorough = Recipe { preset: "thorough".into(), ..Recipe::default() }.for_songs(61);
        assert_eq!((thorough.steps, thorough.grad_accum, thorough.warmup, thorough.save_every), (300, 8, 9, 30));
        let fast = Recipe { preset: "fast".into(), ..Recipe::default() }.for_songs(3);
        assert_eq!((fast.steps, fast.grad_accum, fast.warmup, fast.save_every), (100, 4, 3, 10));
        // a preset is a fixed recipe: an adapter set by hand for the previous one does not carry over
        let hand = Recipe { adapter: "lora".into(), lokr_dim: 32, ..Recipe::default() }.for_songs(61);
        assert_eq!((hand.adapter.as_str(), hand.lokr_dim), ("lokr", 128));
        let tuned = Recipe { preset: "tuned".into(), lokr_dim: 32, ..Recipe::default() }.for_songs(61);
        assert_eq!(tuned.lokr_dim, 32);
        assert!(Recipe { preset: "slow".into(), ..Recipe::default() }.check().is_err());
    }

    #[test]
    fn a_run_saved_before_the_methods_was_trained_the_tuned_way() {
        let old: Recipe = serde_json::from_str(r#"{"stop":"kl","steps":750,"target_kl":1.4}"#).unwrap();
        assert_eq!((old.preset.as_str(), old.grad_accum), ("tuned", 1));
    }

    #[test]
    fn a_joint_line_becomes_a_step_and_other_lines_do_not() {
        let step = parse_training_step(r#"{"stage":"joint","step":12,"ar_ce":2.0,"ar_kl":0.5,"nar_mse":0.3,"step_ms":800}"#).unwrap();
        assert_eq!(step.step, 12);
        assert!((step.loss - 2.4).abs() < 1e-9);
        assert_eq!(step.ar_kl, 0.5);
        assert_eq!(step.step_ms, Some(800.0));
        assert!(parse_training_step(r#"{"stage":"AR transformer backward","step":3}"#).is_none());
        assert!(parse_training_step("[YuE2] Unloaded").is_none());
    }

    #[test]
    fn stages_run_in_order_and_train_last() {
        let inputs = TrainingInputs {
            audio: "a".into(),
            models: "m".into(),
            tokenizer: "t.gguf".into(),
            companion: "c.safetensors".into(),
            run: "r".into(),
            vocals: "v".into(),
            trigger: "sks".into(),
            recipe: Recipe::default().for_songs(10),
        };
        // base-matched: no lyric timing, the recordings as mastered
        let stages = training_stages(&inputs);
        assert_eq!(stages.iter().map(|stage| stage.id).collect::<Vec<_>>(), ["latents", "codes", "scores", "prepare", "train"]);
        let args = |stage: &TrainingStage| -> Vec<String> { stage.args.iter().map(|arg| arg.to_string_lossy().into_owned()).collect() };
        assert!(args(&stages[0]).windows(2).any(|pair| pair == ["--loudness-lufs", "0"]));
        let train = args(&stages[4]);
        for pair in [["--optimizer", "adamw-lm"], ["--lr", "1e-4"], ["--grad-accum", "4"], ["--warmup", "6"], ["--ar-targets", "base"], ["--cursor-weight", "0"], ["--lokr-dim", "128"], ["--nar-crop-frames", "1500"], ["--steps", "200"], ["--save-every", "20"], ["--companion", "c.safetensors"]] {
            assert!(train.windows(2).any(|window| window == pair), "{pair:?}");
        }
        assert!(!train.iter().any(|arg| arg == "--target-kl" || arg == "--prodigy-d0"));
        assert!(args(&stages[3]).windows(2).any(|pair| pair == ["--lyric-timing", "0"]));

        let inputs = TrainingInputs { recipe: Recipe { preset: "tuned".into(), ..Recipe::default() }.for_songs(10), ..inputs };
        let stages = training_stages(&inputs);
        assert_eq!(stages.iter().map(|stage| stage.id).collect::<Vec<_>>(), ["latents", "codes", "align", "scores", "prepare", "train"]);
        let strings = |index: usize| -> Vec<String> { stages[index].args.iter().map(|arg| arg.to_string_lossy().into_owned()).collect() };
        let train = strings(5);
        assert_eq!(train[0], "yue2-joint-train");
        for pair in [["--optimizer", "prodigy"], ["--adapter-type", "lokr"], ["--planner-lr-scale", "0.3"], ["--target-kl", "1.4"], ["--cursor-weight", "0.08"]] {
            assert!(train.windows(2).any(|window| window == pair), "{pair:?}");
        }
        assert!(!train.iter().any(|arg| arg == "--lr"), "Prodigy sets its own step size");
        let prepare = strings(4);
        assert!(prepare.windows(2).any(|pair| pair == ["--trigger", "sks"]));
        assert!(prepare.windows(2).any(|pair| pair == ["--lyric-timing", "1"]));
        assert!(strings(2).windows(2).any(|pair| pair == ["--stems", "v"]));

        let plain = TrainingInputs { recipe: Recipe { preset: "tuned".into(), lyric_timing: false, optimizer: "adamw".into(), adapter: "lora".into(), ..Recipe::default() }, ..inputs };
        let stages = training_stages(&plain);
        assert!(!stages.iter().any(|stage| stage.id == "align"));
        let train: Vec<String> = stages.last().unwrap().args.iter().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert!(train.windows(2).any(|window| window == ["--cursor-weight", "0"]));
        assert!(train.windows(2).any(|window| window == ["--lr", "0.0002"]));
        assert!(train.windows(2).any(|window| window == ["--companion", "c.safetensors"]));
        assert!(!train.iter().any(|arg| arg == "--lokr-dim"));
    }

    #[test]
    fn every_recipe_setting_has_a_field() {
        let recipe = serde_json::to_value(Recipe::default()).unwrap();
        let keys: Vec<&str> = recipe.as_object().unwrap().keys().map(String::as_str).collect();
        let fields: Vec<&str> = recipe_fields().iter().map(|field| field.key).collect();
        // set by the method, not by hand
        let keys: Vec<&str> = keys.into_iter().filter(|key| !["grad_accum", "warmup"].contains(key)).collect();
        for key in &keys {
            assert!(fields.contains(key), "{key} has no field");
        }
        for field in &fields {
            assert!(keys.contains(field), "{field} is not a recipe setting");
        }
    }

    #[test]
    fn sidecars_put_lyrics_last() {
        let [(ext, text), (style_ext, style)] = sidecars("indie  pop,\nfemale", "[Verse]\nla", false);
        assert_eq!(ext, ".txt");
        assert!(text.starts_with("caption: indie pop, female\n"));
        assert!(text.ends_with("lyrics:\n[Verse]\nla\n"));
        assert_eq!((style_ext, style.as_str()), (".yue2.txt", "indie pop, female\n"));
    }
}
