//! Karaoke timings for a finished track.
//!
//! The words are already known - YuE2 sang the lyrics it was given - but the
//! *timings* are not, and the video studio's karaoke layer and the player both
//! need them. This module produces an LRC file for a track using whichever
//! recogniser the user picked; like every optional extra here it is off by
//! default and downloads nothing on its own.
//!
//! Three backends, all of them explicit choices:
//!
//!   * **Whisper** - whisper.cpp run as a sidecar. It writes LRC itself, and
//!     the CUDA build is preferred over the CPU one when both are installed.
//!   * **Parakeet** - the NVIDIA TDT model on transcribe.cpp (CUDA, Vulkan or
//!     the processor), the same one Dub Studio and Speech Studio use.
//!   * **OpenRouter** - a cloud model, billed to the user's own key, asked for
//!     verbose output because plain text carries no timings.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

pub use crate::downloads::Asset;
use crate::downloads::{AssetKind, Downloader};

/// Which recogniser produces the timings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AsrProvider {
    #[default]
    None,
    Whisper,
    Parakeet,
    OpenRouter,
}

/// Persisted with the rest of the studio settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LyricsSyncConfig {
    /// The karaoke switch. Off means the buttons do not appear at all.
    pub enabled: bool,
    pub provider: AsrProvider,
    /// Which downloaded Whisper model to run.
    pub whisper_model: Option<String>,
    /// Which OpenRouter speech-to-text model to call.
    pub openrouter_model: Option<String>,
    /// What the local recogniser runs on. It decides which runtime is
    /// downloaded as much as which one is loaded, so it belongs to the setting
    /// rather than to a guess made at load time.
    #[serde(default)]
    pub runtime: OnnxFlavour,
}

impl LyricsSyncConfig {
    pub fn available(&self) -> bool {
        self.enabled && self.provider != AsrProvider::None
    }

    /// A choice the dropdown no longer offers, as the one it became: fp32 Parakeet is the v3 model.
    pub fn migrated(mut self) -> Self {
        if self.whisper_model.as_deref() == Some(PARAKEET_FP32) {
            self.whisper_model = Some(PARAKEET_V3.to_string());
        }
        self
    }
}

/// The reason the interface shows when a recogniser hears nothing sung: a
/// code, not a sentence, so it is put in the reader's language.
pub const NO_WORDS: &str = "karaoke.no-words";

/// Where the recogniser's binaries live once unpacked. CTranslate2 loads its
/// CUDA libraries from beside the executable, so they share one directory - the
/// way Dub Studio arranges it, and the reason its card mode works instead of
/// quietly falling back to the processor.
pub const WHISPER_RUNTIME_DIR: &str = "whisper";

/// The model sizes the recogniser knows, as `--model` names them.
pub const WHISPER_SIZES: &[&str] = &["tiny", "base", "small", "medium", "large-v3", "large-v3-turbo"];

/// Phrases Whisper writes over music and silence instead of saying it heard
/// no words: the verbatim hallucinations of the "Bag of Hallucinations"
/// study (Barański et al., ICASSP 2025, MIT) and the per-language lists of
/// NVIDIA NeMo's Granary pipeline (Apache-2.0), as merged by
/// Scicom-AI/Whisper-Hallucination, kept for the studio's languages and a few
/// common ones, phrases of two words or more, normalised.
const HALLUCINATIONS: &str = include_str!("whisper_hallucinations.txt");

/// Where a batch was recognised: on the device chosen, or on the processor
/// after the card refused it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recognised {
    OnDevice,
    OnProcessor,
}

/// Words that only ever appear in a subtitler's credit, never in a song.
const CREDIT_MARKERS: &[&str] = &["dimatorzok", "субтитр", "amara.org", "untertitel", "sous-titr", "subtítulo", "sottotitol", "legendas por", "subtitles by", "字幕", "자막"];

fn normalised(text: &str) -> String {
    let lowered = text.to_lowercase().replace('ё', "е");
    lowered
        .split(|c: char| c.is_whitespace() || ".,!?…\"'«»“”„-–—:;()[]♪。、！？「」".contains(c))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether a recognised segment is Whisper filling a gap rather than words
/// that were sung: a known hallucination said whole, a subtitler's credit, a
/// sound written as a caption ("ВЕСЕЛАЯ МУЗЫКА", "[Music]"), or no letters at
/// all. A segment is judged whole, so a sung line that merely contains such a
/// phrase stays.
pub fn is_hallucination(text: &str) -> bool {
    static KNOWN: std::sync::OnceLock<std::collections::HashSet<&'static str>> = std::sync::OnceLock::new();
    let trimmed = text.trim();
    if !trimmed.chars().any(char::is_alphabetic) {
        return true;
    }
    let bracketed = (trimmed.starts_with('[') && trimmed.ends_with(']')) || (trimmed.starts_with('(') && trimmed.ends_with(')')) || trimmed.starts_with('♪');
    let letters: Vec<char> = trimmed.chars().filter(|c| c.is_alphabetic()).collect();
    let shouted = letters.len() >= 3 && letters.iter().all(|c| !c.is_lowercase()) && letters.iter().any(|c| c.is_uppercase());
    if bracketed || shouted {
        return true;
    }
    let plain = normalised(trimmed);
    if CREDIT_MARKERS.iter().any(|marker| plain.contains(marker)) {
        return true;
    }
    KNOWN.get_or_init(|| HALLUCINATIONS.lines().filter(|line| !line.is_empty()).collect()).contains(plain.as_str())
}

/// The words and their times out of faster-whisper's JSON.
///
/// A segment that came back without word timestamps becomes one long "word":
/// better a line placed roughly than a line dropped, and the written lyrics are
/// laid back over whatever times these are.
fn whisper_words_from_json(text: &str) -> Vec<(f64, String)> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else { return Vec::new() };
    let Some(segments) = value.get("segments").and_then(|value| value.as_array()) else { return Vec::new() };
    let mut words = Vec::new();
    for segment in segments {
        if is_hallucination(segment.get("text").and_then(|value| value.as_str()).unwrap_or_default()) {
            continue;
        }
        match segment.get("words").and_then(|value| value.as_array()) {
            Some(list) if !list.is_empty() => {
                for entry in list {
                    let word = entry
                        .get("word")
                        .or_else(|| entry.get("text"))
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    if word.is_empty() {
                        continue;
                    }
                    words.push((entry.get("start").and_then(|value| value.as_f64()).unwrap_or(0.0), word));
                }
            }
            _ => {
                let word = segment.get("text").and_then(|value| value.as_str()).unwrap_or_default().trim().to_string();
                if !word.is_empty() {
                    words.push((segment.get("start").and_then(|value| value.as_f64()).unwrap_or(0.0), word));
                }
            }
        }
    }
    words
}

pub const ASSETS: &[Asset] = &[
    #[cfg(windows)]
    Asset {
        id: "whisper-engine",
        label: "Whisper (faster-whisper standalone)",
        kind: AssetKind::Runtime,
        url: "https://github.com/Purfview/whisper-standalone-win/releases/download/faster-whisper/Whisper-Faster_r192.3_windows.zip",
        relative_path: "runtime/whisper-faster.zip",
        bytes: 87_654_143,
        unzip_into: Some(WHISPER_RUNTIME_DIR),
        marker: "whisper-faster",
        pick: &[],
        vram_gb: None,
        note: "Purfview's build of faster-whisper: word timestamps, on the card or the processor.",
    },
    #[cfg(windows)]
    Asset {
        id: "whisper-cublas",
        label: "NVIDIA cuBLAS 11.11 (for Whisper)",
        kind: AssetKind::Runtime,
        url: "https://developer.download.nvidia.com/compute/cuda/redist/libcublas/windows-x86_64/libcublas-windows-x86_64-11.11.3.6-archive.zip",
        relative_path: "runtime/whisper-cublas.zip",
        bytes: 420_850_025,
        unzip_into: Some(WHISPER_RUNTIME_DIR),
        marker: "cublas64_11",
        pick: &["cublas64_11.dll", "cublasLt64_11.dll"],
        vram_gb: None,
        note: "CTranslate2 is built against CUDA 11; without these the card is never used.",
    },
    #[cfg(windows)]
    Asset {
        id: "whisper-cudnn",
        label: "NVIDIA cuDNN 8.9 (for Whisper)",
        kind: AssetKind::Runtime,
        url: "https://developer.download.nvidia.com/compute/cudnn/redist/cudnn/windows-x86_64/cudnn-windows-x86_64-8.9.7.29_cuda11-archive.zip",
        relative_path: "runtime/whisper-cudnn.zip",
        bytes: 704_240_064,
        unzip_into: Some(WHISPER_RUNTIME_DIR),
        marker: "cudnn64_8",
        pick: &["cudnn64_8.dll", "cudnn_ops_infer64_8.dll", "cudnn_cnn_infer64_8.dll"],
        vram_gb: None,
        note: "The convolution kernels the encoder spends its time in.",
    },
    // r189.1 is Purfview's last Linux build of faster-whisper itself (not
    // XXL): CTranslate2 on CUDA 11, the same libraries as on Windows.
    #[cfg(target_os = "linux")]
    Asset {
        id: "whisper-engine",
        label: "Whisper (faster-whisper standalone)",
        kind: AssetKind::Runtime,
        url: "https://github.com/Purfview/whisper-standalone-win/releases/download/faster-whisper/Whisper-Faster_r189.1_linux.zip",
        relative_path: "runtime/whisper-faster.zip",
        bytes: 106_093_648,
        unzip_into: Some(WHISPER_RUNTIME_DIR),
        marker: "whisper-faster",
        pick: &[],
        vram_gb: None,
        note: "Purfview's build of faster-whisper: word timestamps, on the card or the processor.",
    },
    #[cfg(target_os = "linux")]
    Asset {
        id: "whisper-cublas",
        label: "NVIDIA cuBLAS 11.11 (for Whisper)",
        kind: AssetKind::Runtime,
        url: "https://developer.download.nvidia.com/compute/cuda/redist/libcublas/linux-x86_64/libcublas-linux-x86_64-11.11.3.6-archive.tar.xz",
        relative_path: "runtime/whisper-cublas.tar.xz",
        bytes: 500_681_532,
        unzip_into: Some(WHISPER_RUNTIME_DIR),
        marker: "libcublas.so.11",
        pick: &["libcublas.so.11", "libcublasLt.so.11"],
        vram_gb: None,
        note: "CTranslate2 is built against CUDA 11; without these the card is never used.",
    },
    #[cfg(target_os = "linux")]
    Asset {
        id: "whisper-cudnn",
        label: "NVIDIA cuDNN 8.9 (for Whisper)",
        kind: AssetKind::Runtime,
        url: "https://developer.download.nvidia.com/compute/cudnn/redist/cudnn/linux-x86_64/cudnn-linux-x86_64-8.9.7.29_cuda11-archive.tar.xz",
        relative_path: "runtime/whisper-cudnn.tar.xz",
        bytes: 860_967_256,
        unzip_into: Some(WHISPER_RUNTIME_DIR),
        marker: "libcudnn.so.8",
        pick: &["libcudnn.so.8", "libcudnn_ops_infer.so.8", "libcudnn_cnn_infer.so.8"],
        vram_gb: None,
        note: "The convolution kernels the encoder spends its time in.",
    },
    Asset {
        id: "whisper-tiny",
        label: "Whisper tiny",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-tiny/resolve/d90ca5fe260221311c53c58e660288d3deb8d356/model.bin",
        relative_path: "models/whisper/faster-whisper-tiny/model.bin",
        bytes: 75_538_270,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: Some(1),
        note: "The smallest there is. For a quick check, not for lyrics.",
    },
    Asset {
        id: "whisper-tiny-config",
        label: "Whisper tiny (config.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-tiny/resolve/d90ca5fe260221311c53c58e660288d3deb8d356/config.json",
        relative_path: "models/whisper/faster-whisper-tiny/config.json",
        bytes: 2_249,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-tiny-tokenizer",
        label: "Whisper tiny (tokenizer.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-tiny/resolve/d90ca5fe260221311c53c58e660288d3deb8d356/tokenizer.json",
        relative_path: "models/whisper/faster-whisper-tiny/tokenizer.json",
        bytes: 2_203_239,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-tiny-vocabulary",
        label: "Whisper tiny (vocabulary.txt)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-tiny/resolve/d90ca5fe260221311c53c58e660288d3deb8d356/vocabulary.txt",
        relative_path: "models/whisper/faster-whisper-tiny/vocabulary.txt",
        bytes: 459_861,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-base",
        label: "Whisper base",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-base/resolve/ebe41f70d5b6dfa9166e2c581c45c9c0cfc57b66/model.bin",
        relative_path: "models/whisper/faster-whisper-base/model.bin",
        bytes: 145_217_532,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: Some(1),
        note: "Fast and small; misses words in dense mixes.",
    },
    Asset {
        id: "whisper-base-config",
        label: "Whisper base (config.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-base/resolve/ebe41f70d5b6dfa9166e2c581c45c9c0cfc57b66/config.json",
        relative_path: "models/whisper/faster-whisper-base/config.json",
        bytes: 2_309,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-base-tokenizer",
        label: "Whisper base (tokenizer.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-base/resolve/ebe41f70d5b6dfa9166e2c581c45c9c0cfc57b66/tokenizer.json",
        relative_path: "models/whisper/faster-whisper-base/tokenizer.json",
        bytes: 2_203_239,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-base-vocabulary",
        label: "Whisper base (vocabulary.txt)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-base/resolve/ebe41f70d5b6dfa9166e2c581c45c9c0cfc57b66/vocabulary.txt",
        relative_path: "models/whisper/faster-whisper-base/vocabulary.txt",
        bytes: 459_861,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-small",
        label: "Whisper small",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-small/resolve/536b0662742c02347bc0e980a01041f333bce120/model.bin",
        relative_path: "models/whisper/faster-whisper-small/model.bin",
        bytes: 483_546_902,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: Some(2),
        note: "Noticeably better than base without asking much of the card.",
    },
    Asset {
        id: "whisper-small-config",
        label: "Whisper small (config.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-small/resolve/536b0662742c02347bc0e980a01041f333bce120/config.json",
        relative_path: "models/whisper/faster-whisper-small/config.json",
        bytes: 2_370,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-small-tokenizer",
        label: "Whisper small (tokenizer.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-small/resolve/536b0662742c02347bc0e980a01041f333bce120/tokenizer.json",
        relative_path: "models/whisper/faster-whisper-small/tokenizer.json",
        bytes: 2_203_239,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-small-vocabulary",
        label: "Whisper small (vocabulary.txt)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-small/resolve/536b0662742c02347bc0e980a01041f333bce120/vocabulary.txt",
        relative_path: "models/whisper/faster-whisper-small/vocabulary.txt",
        bytes: 459_861,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-medium",
        label: "Whisper medium",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-medium/resolve/08e178d48790749d25932bbc082711ddcfdfbc4f/model.bin",
        relative_path: "models/whisper/faster-whisper-medium/model.bin",
        bytes: 1_527_906_378,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: Some(3),
        note: "Slower than turbo and rarely better on sung words.",
    },
    Asset {
        id: "whisper-medium-config",
        label: "Whisper medium (config.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-medium/resolve/08e178d48790749d25932bbc082711ddcfdfbc4f/config.json",
        relative_path: "models/whisper/faster-whisper-medium/config.json",
        bytes: 2_257,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-medium-tokenizer",
        label: "Whisper medium (tokenizer.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-medium/resolve/08e178d48790749d25932bbc082711ddcfdfbc4f/tokenizer.json",
        relative_path: "models/whisper/faster-whisper-medium/tokenizer.json",
        bytes: 2_203_239,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-medium-vocabulary",
        label: "Whisper medium (vocabulary.txt)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-medium/resolve/08e178d48790749d25932bbc082711ddcfdfbc4f/vocabulary.txt",
        relative_path: "models/whisper/faster-whisper-medium/vocabulary.txt",
        bytes: 459_861,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-large-v3",
        label: "Whisper large-v3",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-large-v3/resolve/edaa852ec7e145841d8ffdb056a99866b5f0a478/model.bin",
        relative_path: "models/whisper/faster-whisper-large-v3/model.bin",
        bytes: 3_087_284_237,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: Some(5),
        note: "The full model. Slower than turbo, and the most accurate on hard mixes.",
    },
    Asset {
        id: "whisper-large-v3-config",
        label: "Whisper large-v3 (config.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-large-v3/resolve/edaa852ec7e145841d8ffdb056a99866b5f0a478/config.json",
        relative_path: "models/whisper/faster-whisper-large-v3/config.json",
        bytes: 2_394,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-large-v3-preprocessor-config",
        label: "Whisper large-v3 (preprocessor_config.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-large-v3/resolve/edaa852ec7e145841d8ffdb056a99866b5f0a478/preprocessor_config.json",
        relative_path: "models/whisper/faster-whisper-large-v3/preprocessor_config.json",
        bytes: 340,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-large-v3-tokenizer",
        label: "Whisper large-v3 (tokenizer.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-large-v3/resolve/edaa852ec7e145841d8ffdb056a99866b5f0a478/tokenizer.json",
        relative_path: "models/whisper/faster-whisper-large-v3/tokenizer.json",
        bytes: 2_480_617,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-large-v3-vocabulary",
        label: "Whisper large-v3 (vocabulary.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-large-v3/resolve/edaa852ec7e145841d8ffdb056a99866b5f0a478/vocabulary.json",
        relative_path: "models/whisper/faster-whisper-large-v3/vocabulary.json",
        bytes: 1_068_114,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-large-v3-turbo",
        label: "Whisper large-v3-turbo",
        kind: AssetKind::Model,
        url: "https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2/resolve/4df90f75321148c3a29a9e2351b7ddf8f5b115a8/model.bin",
        relative_path: "models/whisper/faster-whisper-large-v3-turbo/model.bin",
        bytes: 1_617_884_929,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: Some(3),
        note: "The accurate choice for sung lyrics.",
    },
    Asset {
        id: "whisper-large-v3-turbo-config",
        label: "Whisper large-v3-turbo (config.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2/resolve/4df90f75321148c3a29a9e2351b7ddf8f5b115a8/config.json",
        relative_path: "models/whisper/faster-whisper-large-v3-turbo/config.json",
        bytes: 2_263,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-large-v3-turbo-preprocessor-config",
        label: "Whisper large-v3-turbo (preprocessor_config.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2/resolve/4df90f75321148c3a29a9e2351b7ddf8f5b115a8/preprocessor_config.json",
        relative_path: "models/whisper/faster-whisper-large-v3-turbo/preprocessor_config.json",
        bytes: 340,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-large-v3-turbo-tokenizer",
        label: "Whisper large-v3-turbo (tokenizer.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2/resolve/4df90f75321148c3a29a9e2351b7ddf8f5b115a8/tokenizer.json",
        relative_path: "models/whisper/faster-whisper-large-v3-turbo/tokenizer.json",
        bytes: 2_710_337,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-large-v3-turbo-vocabulary",
        label: "Whisper large-v3-turbo (vocabulary.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2/resolve/4df90f75321148c3a29a9e2351b7ddf8f5b115a8/vocabulary.json",
        relative_path: "models/whisper/faster-whisper-large-v3-turbo/vocabulary.json",
        bytes: 1_068_114,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    // Parakeet on transcribe.cpp, the files Dub Studio and Speech Studio fetch under the same names, so one
    // program's copy serves the others; addresses and sizes are the shared binding's.
    Asset {
        id: PARAKEET_V3,
        label: "Parakeet TDT 0.6B v3 (Q8_0)",
        kind: AssetKind::Model,
        url: dub_tcpp::MODELS[0].download.url,
        relative_path: "models/asr-gguf-parakeet-v3/parakeet-tdt-0.6b-v3-Q8_0.gguf",
        bytes: dub_tcpp::MODELS[0].download.size,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: Some(1),
        note: "NVIDIA's recogniser, the one Dub Studio uses: 25 European languages, the time of each word.",
    },
    Asset {
        id: PARAKEET_ULTRA,
        label: "Parakeet Ultra 0.6B (Q8_0)",
        kind: AssetKind::Model,
        url: dub_tcpp::MODELS[1].download.url,
        relative_path: "models/asr-gguf-parakeet-ultra/parakeet-ultra-Q8_0.gguf",
        bytes: dub_tcpp::MODELS[1].download.size,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: Some(1),
        note: "Moondream's post-trained Parakeet v3: fewer wrong words in Russian and in noise.",
    },
    #[cfg(any(windows, target_os = "linux"))]
    Asset {
        id: "transcribe-engine",
        label: "transcribe.cpp (CUDA)",
        kind: AssetKind::Runtime,
        url: dub_tcpp::Build::Cuda.download().url,
        relative_path: TCPP_CUDA,
        bytes: dub_tcpp::Build::Cuda.download().size,
        unzip_into: Some("transcribe-engine"),
        marker: dub_tcpp::LIBRARY,
        pick: &[],
        vram_gb: None,
        note: "Runs Parakeet on an NVIDIA card.",
    },
    #[cfg(any(windows, target_os = "linux"))]
    Asset {
        id: "transcribe-engine-vulkan",
        label: "transcribe.cpp (Vulkan)",
        kind: AssetKind::Runtime,
        url: dub_tcpp::Build::Vulkan.download().url,
        relative_path: TCPP_VULKAN,
        bytes: dub_tcpp::Build::Vulkan.download().size,
        unzip_into: Some("transcribe-engine-vulkan"),
        marker: dub_tcpp::LIBRARY,
        pick: &[],
        vram_gb: None,
        note: "Runs Parakeet on an AMD or Intel card, or an NVIDIA card CUDA does not run on.",
    },
    #[cfg(any(windows, target_os = "linux"))]
    Asset {
        id: "transcribe-engine-cpu",
        label: "transcribe.cpp (processor)",
        kind: AssetKind::Runtime,
        url: dub_tcpp::Build::Cpu.download().url,
        relative_path: TCPP_CPU,
        bytes: dub_tcpp::Build::Cpu.download().size,
        unzip_into: Some("transcribe-engine-cpu"),
        marker: dub_tcpp::LIBRARY,
        pick: &[],
        vram_gb: None,
        note: "Runs Parakeet on the processor: slower, the same words.",
    },
    #[cfg(target_os = "macos")]
    Asset {
        id: "transcribe-engine-cpu",
        label: "transcribe.cpp (Metal)",
        kind: AssetKind::Runtime,
        url: dub_tcpp::Build::Cpu.download().url,
        relative_path: TCPP_CPU,
        bytes: dub_tcpp::Build::Cpu.download().size,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Runs Parakeet on the Mac: Metal on Apple silicon, the processor on Intel.",
    },
    #[cfg(windows)]
    Asset {
        id: "onnxruntime",
        label: "ONNX Runtime 1.31.1",
        kind: AssetKind::Runtime,
        url: "https://github.com/microsoft/onnxruntime/releases/download/v1.31.1/onnxruntime-win-x64-1.31.1.zip",
        relative_path: "runtime/onnxruntime.zip",
        bytes: 82_929_944,
        unzip_into: Some("onnx"),
        marker: "onnxruntime.dll",
        pick: &[],
        vram_gb: None,
        note: "The tempo model runs on this, on the processor; it is loaded at run time, not linked in.",
    },
    #[cfg(target_os = "macos")]
    Asset {
        id: "onnxruntime",
        label: "ONNX Runtime 1.31.1",
        kind: AssetKind::Runtime,
        url: "https://github.com/microsoft/onnxruntime/releases/download/v1.31.1/onnxruntime-osx-arm64-1.31.1.tgz",
        relative_path: "runtime/onnxruntime.tgz",
        bytes: 36_936_435,
        unzip_into: Some("onnx"),
        marker: "libonnxruntime",
        pick: &[],
        vram_gb: None,
        note: "The tempo model runs on this, on the processor; it is loaded at run time, not linked in.",
    },
    #[cfg(target_os = "linux")]
    Asset {
        id: "onnxruntime",
        label: "ONNX Runtime 1.31.1",
        kind: AssetKind::Runtime,
        url: "https://github.com/microsoft/onnxruntime/releases/download/v1.31.1/onnxruntime-linux-x64-1.31.1.tgz",
        relative_path: "runtime/onnxruntime.tgz",
        bytes: 11_413_655,
        unzip_into: Some("onnx"),
        marker: "libonnxruntime.so",
        pick: &[],
        vram_gb: None,
        note: "The tempo model runs on this, on the processor; it is loaded at run time, not linked in.",
    },
];

/// The file name of this platform's ONNX Runtime.
#[cfg(target_os = "macos")]
mod libraries {
    pub const ORT: &str = "libonnxruntime.dylib";
}

#[cfg(windows)]
mod libraries {
    pub const ORT: &str = "onnxruntime.dll";
}

#[cfg(target_os = "linux")]
mod libraries {
    pub const ORT: &str = "libonnxruntime.so";
}

/// Which build of the ONNX Runtime to load.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnnxFlavour {
    /// The graphics card if its runtime is installed, otherwise the processor.
    Auto,
    /// The default: this studio exists for machines with an NVIDIA card, and
    /// the processor path is minutes where the card is seconds.
    #[default]
    Cuda,
    Cpu,
}

impl OnnxFlavour {
    /// Whether work runs on the card through CUDA: only an NVIDIA card with a
    /// driver that runs CUDA does. An AMD card was sent to the CUDA provider
    /// and failed, after being offered gigabytes of CUDA libraries.
    pub fn uses_cuda(self) -> bool {
        !matches!(self, OnnxFlavour::Cpu) && crate::hardware::hardware().runs_cuda
    }
}

/// The words Parakeet hears in one track, each with the second it starts. `language` is a hint the model takes
/// only when it knows it; none, or one it does not, leaves the language to it.
fn parakeet_transcribe(asr: &std::sync::Mutex<dub_tcpp::TcppAsr>, model: &dub_tcpp::Model, audio: &Path, language: Option<&str>) -> Result<Vec<(f64, String)>> {
    let samples = crate::audio_pcm::decode_mono_16k(audio).with_context(|| format!("decode {} for recognition", audio.display()))?;
    let language = language.filter(|language| dub_tcpp::knows(model, language)).unwrap_or("");
    let heard = asr.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).words(&samples, language).map_err(|error| anyhow!("Parakeet transcription failed: {error}"))?;
    let words: Vec<(f64, String)> = heard
        .into_iter()
        .filter_map(|word| {
            let text = word.word.trim().to_string();
            (!text.is_empty()).then_some((word.start, text))
        })
        .collect();
    if words.is_empty() {
        bail!(NO_WORDS);
    }
    Ok(words)
}

fn point_ort_at(runtime: &Path) {
    unsafe { std::env::set_var("ORT_DYLIB_PATH", runtime) };
    #[cfg(windows)]
    if let Some(directory) = runtime.parent() {
        let existing = std::env::var("PATH").unwrap_or_default();
        unsafe { std::env::set_var("PATH", format!("{};{existing}", directory.display())) };
    }
}

/// The dropdown's Parakeet ids, as settings have held them since the ONNX builds: the v3 model, Ultra, and the
/// fp32 choice of before, which is the v3 model now that full weights are not kept.
pub const PARAKEET_V3: &str = "parakeet-tdt-int8";
pub const PARAKEET_ULTRA: &str = "parakeet-ultra-int8";
pub const PARAKEET_FP32: &str = "parakeet-tdt-fp32";

/// Where each transcribe.cpp build lands in the karaoke folder: `dub_tcpp::engine_dir` and its library.
#[cfg(windows)]
const TCPP_CUDA: &str = "runtime/transcribe-cuda.zip";
#[cfg(windows)]
const TCPP_VULKAN: &str = "runtime/transcribe-vulkan.zip";
#[cfg(windows)]
const TCPP_CPU: &str = "runtime/transcribe-cpu.zip";
#[cfg(target_os = "linux")]
const TCPP_CUDA: &str = "runtime/transcribe-cuda.tar.gz";
#[cfg(target_os = "linux")]
const TCPP_VULKAN: &str = "runtime/transcribe-vulkan.tar.gz";
#[cfg(target_os = "linux")]
const TCPP_CPU: &str = "runtime/transcribe-cpu.tar.gz";
#[cfg(target_os = "macos")]
const TCPP_CPU: &str = "runtime/transcribe-engine-cpu/libtranscribe_engine.dylib";

/// The transcribe.cpp model the dropdown names.
pub fn parakeet_model(variant: Option<&str>) -> &'static dub_tcpp::Model {
    let id = if variant == Some(PARAKEET_ULTRA) { "parakeet-ultra" } else { "parakeet-v3" };
    dub_tcpp::model(id).expect("the shared binding carries both Parakeets")
}

/// The asset of the model the dropdown names.
pub fn parakeet_model_asset(variant: Option<&str>) -> &'static str {
    if variant == Some(PARAKEET_ULTRA) { PARAKEET_ULTRA } else { PARAKEET_V3 }
}

/// The engine build this machine runs Parakeet on: the processor when chosen, CUDA on a card and driver CUDA 13
/// runs on (the build is CUDA 13; on Linux only when the system has its cuBLAS), Vulkan on any other card, the
/// processor without one. A Mac has one build, in the processor's place: Metal and the processor on Apple silicon,
/// the processor on Intel.
pub fn parakeet_build(runtime: OnnxFlavour) -> Option<dub_tcpp::Build> {
    if cfg!(target_os = "macos") {
        return Some(dub_tcpp::Build::Cpu);
    }
    if !cfg!(any(windows, target_os = "linux")) {
        return None;
    }
    let hardware = crate::hardware::hardware();
    Some(if matches!(runtime, OnnxFlavour::Cpu) || hardware.gpu_name.is_none() {
        dub_tcpp::Build::Cpu
    } else if hardware.card_cuda == Some(crate::hardware::CudaBuild::Cuda13) {
        if cfg!(target_os = "linux") && !system_library("libcublas.so.13") { dub_tcpp::Build::Vulkan } else { dub_tcpp::Build::Cuda }
    } else {
        dub_tcpp::Build::Vulkan
    })
}

/// Whether Linux's loader finds `name` without help: the CUDA build of transcribe.cpp looks for cuBLAS beside itself
/// and then where the system keeps libraries, as the music engine does, and the studio downloads none for Linux.
fn system_library(name: &str) -> bool {
    static FOUND: std::sync::OnceLock<std::collections::HashMap<String, bool>> = std::sync::OnceLock::new();
    let probe = |name: &str| {
        let listed = std::env::var_os("LD_LIBRARY_PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()));
        let cached = fs::read("/etc/ld.so.cache").is_ok_and(|cache| cache.windows(name.len()).any(|bytes| bytes == name.as_bytes()));
        listed || cached || ["/lib", "/usr/lib", "/lib64", "/usr/lib64", "/usr/lib/x86_64-linux-gnu"].iter().any(|dir| Path::new(dir).join(name).is_file())
    };
    FOUND.get_or_init(|| ["libcublas.so.13"].into_iter().map(|library| (library.to_string(), probe(library))).collect()).get(name).copied().unwrap_or_else(|| probe(name))
}

/// The files Parakeet runs from on this machine: its engine build and the model the dropdown names.
pub fn parakeet_assets(runtime: OnnxFlavour, variant: Option<&str>) -> Vec<&'static str> {
    parakeet_build(runtime).map(|build| vec![build.dir(), parakeet_model_asset(variant)]).unwrap_or_default()
}

pub fn asset(id: &str) -> Option<&'static Asset> {
    ASSETS.iter().find(|asset| asset.id == id)
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncStatus {
    pub enabled: bool,
    pub provider: AsrProvider,
    pub root: String,
    /// True when the selected provider can actually run right now.
    pub ready: bool,
    pub whisper_binary: Option<String>,
    pub whisper_model: Option<String>,
    pub openrouter_model: Option<String>,
    /// What the recogniser runs on, so the page that downloads it can show the
    /// same choice that decides which files it fetches.
    pub runtime: OnnxFlavour,
    pub installed_models: Vec<String>,
    pub assets: Vec<crate::downloads::AssetStatus>,
    pub active_download: Option<crate::downloads::DownloadProgress>,
}

pub struct LyricsSync {
    downloader: Downloader,
    /// Where the music engine's cuBLAS 13 is, which transcribe.cpp's CUDA build imports.
    cuda_libraries: std::sync::OnceLock<PathBuf>,
}

impl LyricsSync {
    pub fn new(data_root: &Path) -> Self {
        Self { downloader: Downloader::new(data_root.join("karaoke")), cuda_libraries: std::sync::OnceLock::new() }
    }

    /// The folder of the cuBLAS 13 the music engine runs on: transcribe.cpp's CUDA build takes links to it.
    pub fn set_cuda_libraries(&self, folder: PathBuf) {
        self.cuda_libraries.set(folder).ok();
    }

    /// Windows looks for a library's imports beside it: the CUDA build gets links to the engine's cuBLAS 13, no
    /// copies, unless the files are on PATH already.
    fn link_tcpp_cuda(&self, engine_dir: &Path) -> Result<()> {
        if !cfg!(windows) {
            return Ok(());
        }
        let on_path = |name: &str| std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()));
        for name in crate::engine_runtime::CUDA13_LIBRARIES {
            let target = engine_dir.join(name);
            if target.is_file() || on_path(name) {
                continue;
            }
            let source = self.cuda_libraries.get().map(|folder| folder.join(name)).filter(|source| source.is_file());
            let Some(source) = source else {
                bail!("transcribe.cpp on the card needs {name}, which comes with the music engine's CUDA 13 runtime: download it in Settings - Local engine, or choose the processor");
            };
            if fs::hard_link(&source, &target).is_err() {
                fs::copy(&source, &target).with_context(|| format!("copy {} beside transcribe.cpp", source.display()))?;
            }
        }
        Ok(())
    }

    pub fn downloader(&self) -> &Downloader {
        &self.downloader
    }

    /// The CUDA build first: on a machine that has one, the CPU build would be
    /// a silent downgrade.
    pub fn whisper_binary(&self) -> Option<PathBuf> {
        crate::downloads::locate_binary(self.downloader.root(), &[WHISPER_RUNTIME_DIR], "whisper-faster")
    }

    /// What `--model_dir` is given: the binary looks inside it for a directory
    /// called `faster-whisper-<size>`.
    fn whisper_model_dir(&self) -> PathBuf {
        self.downloader.root().join("models").join("whisper")
    }

    /// The size a model id stands for - `whisper-large-v3` is `large-v3`, which
    /// is the name the binary is given.
    fn whisper_size(id: &str) -> Option<&str> {
        let size = id.strip_prefix("whisper-")?;
        WHISPER_SIZES.contains(&size).then_some(size)
    }

    pub fn installed_models(&self) -> Vec<&'static Asset> {
        ASSETS
            .iter()
            .filter(|asset| asset.kind == AssetKind::Model && self.downloader.is_installed(asset))
            .collect()
    }

    /// Parakeet runs here: this machine's engine build and the model the dropdown names are on disk.
    pub fn parakeet_ready(&self, runtime: OnnxFlavour, model: Option<&str>) -> bool {
        let wanted = parakeet_assets(runtime, model);
        !wanted.is_empty() && wanted.iter().all(|id| asset(id).is_some_and(|asset| self.downloader.is_installed(asset)))
    }

    /// Whether the Whisper model the configuration names is actually on disk.
    /// A faster-whisper model is a directory, not a file - and
    /// `whisper_model_path` has already checked that every file inside it
    /// arrived. Asking whether that path is a file said no to a complete
    /// installation, which is how a finished download still refused to run.
    pub fn whisper_model_ready(&self, config: &LyricsSyncConfig) -> bool {
        self.whisper_model_path(config).is_some_and(|path| path.is_dir())
    }

    /// Any Parakeet whole on disk, whichever build and variant.
    pub fn parakeet_any_ready(&self) -> bool {
        let installed = |id: &str| asset(id).is_some_and(|asset| self.downloader.is_installed(asset));
        dub_tcpp::Build::ALL.iter().any(|build| installed(build.dir())) && [PARAKEET_V3, PARAKEET_ULTRA].iter().any(|id| installed(id))
    }

    /// A Parakeet whole on disk for this machine's build, v3 first: the dropdown id to choose when the one chosen is
    /// not there.
    pub fn installed_parakeet(&self, runtime: OnnxFlavour) -> Option<&'static str> {
        [PARAKEET_V3, PARAKEET_ULTRA].into_iter().find(|model| self.parakeet_ready(runtime, Some(model)))
    }

    /// The loaded Parakeet of this machine's build and the dropdown's model: one load serves every track of a run;
    /// the run unloads it after, so the card is the music engine's again.
    fn load_parakeet(&self, runtime: OnnxFlavour, variant: Option<&str>) -> Result<(std::sync::Arc<std::sync::Mutex<dub_tcpp::TcppAsr>>, &'static dub_tcpp::Model)> {
        let build = parakeet_build(runtime).ok_or_else(|| anyhow!("transcribe.cpp has no build for this system: choose Whisper"))?;
        let root = self.downloader.root().join("models");
        let model = parakeet_model(variant);
        let engine = self.downloader.runtime_dir(build.dir());
        if build == dub_tcpp::Build::Cuda {
            self.link_tcpp_cuda(&engine)?;
        }
        let asr = dub_tcpp::shared(&engine, &dub_tcpp::model_path(&root, model)).map_err(|error| anyhow!("load Parakeet: {error}"))?;
        Ok((asr, model))
    }

    /// `ort` loads this at run time; linking it would tie the build to one
    /// toolchain and one machine's libraries. The processor build at today's
    /// pin: an earlier one left on disk is not loaded.
    pub fn onnxruntime_library(&self) -> Option<PathBuf> {
        let cpu = self.downloader.runtime_dir("onnx").join(libraries::ORT);
        (cpu.is_file() && self.pinned(&["onnxruntime"])).then_some(cpu)
    }

    /// Whether every one of these downloads is installed at today's pin.
    fn pinned(&self, ids: &[&str]) -> bool {
        ids.iter().all(|id| asset(id).is_some_and(|asset| self.downloader.is_installed(asset)))
    }

    /// Binds `ort` to the studio's processor runtime - once per process, before
    /// anything touches `ort`, or it binds to whatever `onnxruntime.dll` the
    /// system happens to have. Every piece of work on ONNX asks here first. The
    /// library is loaded here, not on first use: one this process cannot load
    /// (another system's build, a signature macOS refuses) is then an error of
    /// the work that needed it, where `ort` would panic and stay unusable until
    /// the studio restarts.
    pub fn bind_ort(&self) -> Result<()> {
        static BOUND: std::sync::OnceLock<()> = std::sync::OnceLock::new();
        if BOUND.get().is_some() {
            return Ok(());
        }
        let library = self.onnxruntime_library().ok_or_else(|| anyhow!("the ONNX Runtime library is not installed"))?;
        point_ort_at(&library);
        // the process's one environment, telemetry off, over the library pointed at above
        dub_core::runtime::ort_env().map_err(|error| anyhow!(error))?;
        BOUND.get_or_init(|| ());
        Ok(())
    }

    /// A model counts as present only with every one of its files: a directory
    /// missing its tokenizer loads exactly as far as an error message.
    fn whisper_model_path(&self, config: &LyricsSyncConfig) -> Option<PathBuf> {
        let size = Self::whisper_size(config.whisper_model.as_deref()?)?;
        let prefix = format!("models/whisper/faster-whisper-{size}/");
        let parts: Vec<&'static Asset> = ASSETS.iter().filter(|asset| asset.relative_path.starts_with(&prefix)).collect();
        if parts.is_empty() || !parts.iter().all(|asset| self.downloader.is_installed(asset)) {
            return None;
        }
        Some(self.whisper_model_dir().join(format!("faster-whisper-{size}")))
    }

    pub async fn status(&self, config: &LyricsSyncConfig) -> SyncStatus {
        let whisper_binary = self.whisper_binary();
        let ready = match config.provider {
            AsrProvider::None => false,
            AsrProvider::Whisper => whisper_binary.is_some() && self.whisper_model_path(config).is_some(),
            AsrProvider::Parakeet => self.parakeet_ready(config.runtime, config.whisper_model.as_deref()),
            AsrProvider::OpenRouter => config.openrouter_model.as_deref().is_some_and(|model| !model.trim().is_empty()),
        };
        SyncStatus {
            enabled: config.enabled,
            provider: config.provider,
            runtime: config.runtime,
            root: self.downloader.root().display().to_string(),
            ready: config.enabled && ready,
            whisper_binary: whisper_binary.map(|path| path.display().to_string()),
            whisper_model: config.whisper_model.clone(),
            openrouter_model: config.openrouter_model.clone(),
            installed_models: self.installed_models().iter().map(|asset| asset.id.to_string()).collect(),
            assets: self.downloader.status_of(ASSETS),
            active_download: self.downloader.active_for("karaoke").await,
        }
    }

    /// Runs Parakeet in this process and returns the words it hears, each with the second it starts: transcribe.cpp
    /// on this machine's build, as Dub Studio and Speech Studio run it.
    pub fn parakeet_words(&self, runtime: OnnxFlavour, variant: Option<&str>, audio: &Path, language: Option<&str>) -> Result<Vec<(f64, String)>> {
        let (asr, model) = self.load_parakeet(runtime, variant)?;
        let words = parakeet_transcribe(&asr, model, audio, language);
        drop(asr);
        dub_tcpp::unload();
        words
    }

    /// The words of several tracks from one load of the recogniser: Parakeet
    /// loaded once and run over each, Whisper started once with every file.
    /// `heard` gets each track's answer, with its index, the moment it is
    /// there, so a long dataset shows its lyrics song by song.
    pub fn words_many(
        &self,
        config: &LyricsSyncConfig,
        audio: &[PathBuf],
        language: Option<&str>,
        heard: &mut dyn FnMut(usize, Result<Vec<(f64, String)>>),
        cancel: &AtomicBool,
    ) -> Recognised {
        let failed = |heard: &mut dyn FnMut(usize, Result<Vec<(f64, String)>>), error: anyhow::Error| {
            for index in 0..audio.len() {
                heard(index, Err(anyhow!("{error:#}")));
            }
        };
        match config.provider {
            AsrProvider::Parakeet => {
                let (asr, model) = match self.load_parakeet(config.runtime, config.whisper_model.as_deref()) {
                    Ok(loaded) => loaded,
                    Err(error) => {
                        failed(heard, error);
                        return Recognised::OnDevice;
                    }
                };
                for (index, path) in audio.iter().enumerate() {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    heard(index, parakeet_transcribe(&asr, model, path, language));
                }
                // the card is the music engine's between recognitions
                drop(asr);
                dub_tcpp::unload();
                Recognised::OnDevice
            }
            AsrProvider::Whisper => match self.whisper_many(config, audio, language, heard, cancel) {
                Ok(recognised) => recognised,
                Err(error) => {
                    failed(heard, error);
                    Recognised::OnDevice
                }
            },
            _ => {
                failed(heard, anyhow!("this recogniser does not run on this computer"));
                Recognised::OnDevice
            }
        }
    }

    /// One Whisper run over several tracks; the JSON it writes for each is
    /// named after the file it was given, and handed on as soon as it is whole.
    fn whisper_many(
        &self,
        config: &LyricsSyncConfig,
        audio: &[PathBuf],
        language: Option<&str>,
        heard: &mut dyn FnMut(usize, Result<Vec<(f64, String)>>),
        cancel: &AtomicBool,
    ) -> Result<Recognised> {
        let binary = self.whisper_binary().ok_or_else(|| anyhow!("the Whisper runtime is not installed"))?;
        let size = config
            .whisper_model
            .as_deref()
            .and_then(Self::whisper_size)
            .ok_or_else(|| anyhow!("no Whisper model is downloaded and selected"))?;
        if self.whisper_model_path(config).is_none() {
            bail!("the Whisper model {size} is not completely downloaded");
        }
        let work = self.downloader.root().join("work").join(format!("batch-{}", uuid::Uuid::now_v7()));
        let out_dir = work.join("out");
        fs::create_dir_all(&out_dir).with_context(|| format!("create {}", out_dir.display()))?;
        let mut wavs: Vec<(usize, PathBuf)> = Vec::with_capacity(audio.len());
        let mut answered = vec![false; audio.len()];
        for (index, path) in audio.iter().enumerate() {
            let wav = work.join(format!("{index}.wav"));
            match crate::audio_pcm::write_wav16k_mono(path, &wav) {
                Ok(()) => wavs.push((index, wav)),
                Err(error) => {
                    answered[index] = true;
                    heard(index, Err(error.context(format!("decode {} for recognition", path.display()))));
                }
            }
        }
        // A track's JSON is taken once it parses: Whisper writes it whole when
        // that track is done, and a half-written one is simply read next time
        let mut take = |answered: &mut Vec<bool>| {
            for index in 0..audio.len() {
                if answered[index] {
                    continue;
                }
                let Ok(text) = fs::read_to_string(out_dir.join(format!("{index}.json"))) else { continue };
                if serde_json::from_str::<serde_json::Value>(&text).is_err() {
                    continue;
                }
                answered[index] = true;
                let words = whisper_words_from_json(&text);
                heard(index, if words.is_empty() { Err(anyhow!(NO_WORDS)) } else { Ok(words) });
            }
        };
        let on_card = config.runtime.uses_cuda();
        let all: Vec<PathBuf> = wavs.iter().map(|(_, wav)| wav.clone()).collect();
        let mut outcome = self.run_whisper_many(&binary, size, &all, &out_dir, language, on_card, &mut || take(&mut answered), cancel);
        let mut recognised = Recognised::OnDevice;
        if outcome.as_ref().is_err_and(|error| error.to_string() != "cancelled") && on_card {
            // the card refused: the processor takes only what is still unheard,
            // and the page is told it ran there
            take(&mut answered);
            let refused = outcome.unwrap_err();
            let left: Vec<PathBuf> = wavs.iter().filter(|(index, _)| !answered[*index]).map(|(_, wav)| wav.clone()).collect();
            outcome = if left.is_empty() {
                Ok(())
            } else {
                recognised = Recognised::OnProcessor;
                self.run_whisper_many(&binary, size, &left, &out_dir, language, false, &mut || take(&mut answered), cancel)
                    .with_context(|| format!("the card was tried first and refused: {refused}"))
            };
        }
        take(&mut answered);
        let run_error = outcome.err().map(|error| format!("{error:#}"));
        for (index, done) in answered.iter().enumerate() {
            if !done {
                heard(index, Err(anyhow!("{}", run_error.clone().unwrap_or_else(|| "whisper wrote no JSON for this track".into()))));
            }
        }
        fs::remove_dir_all(&work).ok();
        Ok(recognised)
    }

    /// Runs faster-whisper over one track and returns the words it heard.
    ///
    /// Purfview's standalone build, asked for JSON with word timestamps - the
    /// same recogniser Dub Studio uses. It replaced whisper.cpp, which was
    /// asked for an LRC file: that file is written line by line, word times had
    /// to be guessed out of it, and when the run failed the binary exited zero
    /// and wrote nothing at all, so the only thing anyone ever saw was
    /// "whisper-cli produced no LRC file".
    pub fn whisper_words(&self, config: &LyricsSyncConfig, audio: &Path, language: Option<&str>, _lyrics: &str) -> Result<Vec<(f64, String)>> {
        let binary = self.whisper_binary().ok_or_else(|| anyhow!("the Whisper runtime is not installed"))?;
        let size = config
            .whisper_model
            .as_deref()
            .and_then(Self::whisper_size)
            .ok_or_else(|| anyhow!("no Whisper model is downloaded and selected"))?;
        if self.whisper_model_path(config).is_none() {
            bail!("the Whisper model {size} is not completely downloaded");
        }

        let work = self.downloader.root().join("work");
        fs::create_dir_all(&work).with_context(|| format!("create {}", work.display()))?;
        let stem = work.join(format!("sync-{}", uuid::Uuid::now_v7()));
        let wav = stem.with_extension("wav");
        crate::audio_pcm::write_wav16k_mono(audio, &wav)
            .with_context(|| format!("decode {} for recognition", audio.display()))?;
        let out_dir = stem.with_extension("out");
        fs::remove_dir_all(&out_dir).ok();
        fs::create_dir_all(&out_dir).with_context(|| format!("create {}", out_dir.display()))?;

        let on_card = config.runtime.uses_cuda();
        let mut outcome = self.run_whisper(&binary, size, &wav, &out_dir, language, on_card);
        // CTranslate2 fails inside itself on a machine without usable CUDA, so
        // the card is tried and the processor is the answer to its refusal -
        // once, and only in that direction.
        if outcome.is_err() && on_card {
            let refused = outcome.unwrap_err();
            fs::remove_dir_all(&out_dir).ok();
            fs::create_dir_all(&out_dir).ok();
            outcome = self
                .run_whisper(&binary, size, &wav, &out_dir, language, false)
                .with_context(|| format!("the card was tried first and refused: {refused}"));
        }
        fs::remove_file(&wav).ok();

        if let Err(error) = outcome {
            fs::remove_dir_all(&out_dir).ok();
            return Err(error);
        }
        let json = fs::read_dir(&out_dir)
            .with_context(|| format!("read {}", out_dir.display()))?
            .flatten()
            .map(|entry| entry.path())
            .find(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
            .ok_or_else(|| anyhow!("whisper wrote no JSON into {}", out_dir.display()))?;
        let text = fs::read_to_string(&json).with_context(|| format!("read {}", json.display()))?;
        let words = whisper_words_from_json(&text);
        fs::remove_dir_all(&out_dir).ok();
        if words.is_empty() {
            bail!(NO_WORDS);
        }
        Ok(words)
    }

    /// One run of the recogniser, with its complaints kept: a failure here is
    /// the only place that ever says why nothing was recognised.
    fn run_whisper(&self, binary: &Path, size: &str, wav: &Path, out_dir: &Path, language: Option<&str>, on_card: bool) -> Result<()> {
        self.run_whisper_many(binary, size, &[wav.to_path_buf()], out_dir, language, on_card, &mut || {}, &AtomicBool::new(false))
    }

    /// Whisper over every file in one process; `poll` is called while it
    /// works, to pick up what it has written, and `cancel` stops it between polls.
    #[allow(clippy::too_many_arguments)]
    fn run_whisper_many(
        &self,
        binary: &Path,
        size: &str,
        wavs: &[PathBuf],
        out_dir: &Path,
        language: Option<&str>,
        on_card: bool,
        poll: &mut dyn FnMut(),
        cancel: &AtomicBool,
    ) -> Result<()> {
        let mut command = Command::new(binary);
        if let Some(path) = music_core::process::own_libraries_first(binary) {
            command.env("LD_LIBRARY_PATH", path);
        }
        dub_core::proc::outside_appimage(&mut command);
        command
            .args(wavs)
            .arg("--model")
            .arg(size)
            .arg("--model_dir")
            .arg(self.whisper_model_dir())
            .arg("--task")
            .arg("transcribe")
            .arg("--output_format")
            .arg("json")
            .arg("--output_dir")
            .arg(out_dir)
            .arg("--word_timestamps")
            .arg("True")
            .arg("--compute_type")
            .arg(if on_card { "int8_float16" } else { "int8" })
            .arg("--device")
            .arg(if on_card { "cuda" } else { "cpu" })
            .arg("--beep_off")
            // A hallucination fed back as the next window's prompt is how one
            // subtitler's credit becomes eight; lyrics lose nothing by it
            .args(["--condition_on_previous_text", "False"]);
        // A language it was told beats one it has to guess, and "auto" is not a
        // language code - passing it as one is how a run comes back empty.
        if let Some(code) = language.map(str::trim).filter(|code| !code.is_empty() && *code != "auto") {
            command.arg("--language").arg(code);
        }
        command
            // The weights are on this disk; a recogniser that goes looking for
            // them on the network is one that fails without one.
            .env("HF_HUB_OFFLINE", "1")
            .env("TRANSFORMERS_OFFLINE", "1")
            .stdin(Stdio::null());
        // Its output goes to a file: a pipe nobody reads while it works through
        // a whole dataset fills up and stalls it.
        let log_path = out_dir.with_extension("log");
        let log = fs::File::create(&log_path).with_context(|| format!("create {}", log_path.display()))?;
        command.stdout(log.try_clone()?).stderr(log);
        // CTranslate2 and the CUDA libraries sit beside the binary, and that is
        // where they are found from.
        if let Some(directory) = binary.parent() {
            command.current_dir(directory);
            // a Linux loader does not look beside the program
            #[cfg(target_os = "linux")]
            command.env(
                "LD_LIBRARY_PATH",
                std::env::join_paths(std::iter::once(directory.to_path_buf()).chain(std::env::var_os("LD_LIBRARY_PATH").iter().flat_map(std::env::split_paths)))
                    .unwrap_or_else(|_| directory.as_os_str().to_owned()),
            );
            // CTranslate2 in this build asks for an executable stack, which glibc 2.41 and later refuse unless told
            #[cfg(target_os = "linux")]
            command.env(
                "GLIBC_TUNABLES",
                std::env::var("GLIBC_TUNABLES").ok().filter(|tunables| !tunables.is_empty()).map_or_else(|| "glibc.rtld.execstack=2".to_string(), |tunables| format!("{tunables}:glibc.rtld.execstack=2")),
            );
        }
        hide_console(&mut command);

        let mut child = command.spawn().with_context(|| format!("run {}", binary.display()))?;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if cancel.load(Ordering::Relaxed) {
                child.kill().ok();
                child.wait().ok();
                bail!("cancelled");
            }
            poll();
            std::thread::sleep(std::time::Duration::from_millis(500));
        };
        let output = fs::read_to_string(&log_path).unwrap_or_default();
        fs::remove_file(&log_path).ok();
        if status.success() {
            return Ok(());
        }
        let mut tail: Vec<&str> = output.lines().filter(|line| !line.trim().is_empty()).rev().take(8).collect();
        tail.reverse();
        bail!("whisper exited with {}: {}", status, tail.join(" | "))
    }
}

/// Groups a word stream into karaoke lines: a new line at a noticeable pause,
/// at sentence-ending punctuation, or once a line grows past comfortable
/// reading length. Ported from the segmentation Dub Studio uses for subtitles.
pub fn group_words(words: &[(f64, String)]) -> Vec<(f64, String)> {
    const PAUSE: f64 = 0.6;
    const MAX_CHARS: usize = 42;

    let mut lines: Vec<(f64, String)> = Vec::new();
    let mut start = 0.0;
    let mut current = String::new();
    let mut previous: Option<f64> = None;

    for (at, word) in words {
        let pause = previous.is_some_and(|last| at - last > PAUSE);
        let too_long = current.chars().count() + word.chars().count() + 1 > MAX_CHARS;
        if !current.is_empty() && (pause || too_long) {
            lines.push((start, std::mem::take(&mut current)));
        }
        if current.is_empty() {
            start = *at;
        } else {
            current.push(' ');
        }
        current.push_str(word);
        if word.ends_with(['.', '!', '?', '…']) {
            lines.push((start, std::mem::take(&mut current)));
        }
        previous = Some(*at);
    }
    if !current.is_empty() {
        lines.push((start, current));
    }
    merge_flashing_lines(lines)
}

/// ACE Step Studio merged LRC lines that begin less than two seconds apart so
/// they do not flash past unread. Karaoke here follows the same rule.
fn merge_flashing_lines(lines: Vec<(f64, String)>) -> Vec<(f64, String)> {
    const MIN_DISPLAY_SECONDS: f64 = 2.0;
    let mut merged: Vec<(f64, String)> = Vec::with_capacity(lines.len());
    for (start, text) in lines {
        match merged.last_mut() {
            Some((previous_start, previous_text)) if start - *previous_start < MIN_DISPLAY_SECONDS => {
                previous_text.push(' ');
                previous_text.push_str(&text);
            }
            _ => merged.push((start, text)),
        }
    }
    merged
}

/// The timed segments in an OpenAI-compatible verbose transcription. A model
/// that answered with plain text yields nothing, and the caller reports that
/// rather than inventing timings.
pub fn segments_from_verbose_json(body: &serde_json::Value) -> Vec<(f64, String)> {
    let mut words = Vec::new();
    if let Some(list) = body.get("words").and_then(|value| value.as_array()) {
        for entry in list {
            let (Some(start), Some(text)) = (entry.get("start").and_then(serde_json::Value::as_f64), entry.get("word").or_else(|| entry.get("text")).and_then(serde_json::Value::as_str)) else {
                continue;
            };
            words.push((start, text.trim().to_string()));
        }
    }
    if !words.is_empty() {
        return group_words(&words);
    }
    let mut segments = Vec::new();
    if let Some(list) = body.get("segments").and_then(|value| value.as_array()) {
        for entry in list {
            let (Some(start), Some(text)) = (entry.get("start").and_then(serde_json::Value::as_f64), entry.get("text").and_then(serde_json::Value::as_str)) else {
                continue;
            };
            segments.push((start, text.trim().to_string()));
        }
    }
    segments
}

/// One written line, with a time for every word in it.
#[derive(Debug, Clone, PartialEq)]
pub struct TimedLine {
    pub start: f64,
    /// The words of the written line, each with the moment it is sung.
    pub words: Vec<(f64, String)>,
}

/// Enhanced LRC - the A2 format every karaoke player understands: a line time
/// followed by a time before each word. Without per-word times a player has
/// nothing to do but sweep the highlight linearly, which drifts away from the
/// singing within a line.
pub fn enhanced_lrc(lines: &[TimedLine]) -> String {
    let mut out = String::new();
    for line in lines {
        if line.words.is_empty() {
            continue;
        }
        out.push_str(&format!("[{}]", stamp(line.start)));
        for (at, word) in &line.words {
            out.push_str(&format!("<{}>{} ", stamp(*at), word));
        }
        out.pop();
        out.push('\n');
    }
    out
}

fn stamp(seconds: f64) -> String {
    let total = seconds.max(0.0);
    let minutes = (total as u64) / 60;
    let rest = total - (minutes * 60) as f64;
    format!("{minutes:02}:{rest:05.2}")
}

/// Puts the track's own lyrics on the recogniser's clock, word by word.
///
/// The line anchoring is the same as before; inside a line each written word
/// takes the time of the recognised word it matches, and words nobody matched
/// are spread across the gap in proportion to their length. That is what makes
/// a karaoke highlight land on the syllable instead of drifting through it.
pub fn align_lyrics_words(words: &[(f64, String)], lyrics: &str) -> Vec<TimedLine> {
    let anchored = align_lyrics(words, lyrics);
    if anchored.is_empty() {
        return Vec::new();
    }
    let heard: Vec<(f64, String)> = words.iter().map(|(at, word)| (*at, normalise(word))).collect();
    let mut timed: Vec<TimedLine> = Vec::with_capacity(anchored.len());

    for (index, (start, text)) in anchored.iter().enumerate() {
        let end = anchored.get(index + 1).map(|(next, _)| *next).unwrap_or_else(|| {
            // The last line runs to the last thing anyone heard.
            heard.last().map(|(at, _)| *at + 2.0).unwrap_or(start + 4.0)
        });
        let written: Vec<&str> = text.split_whitespace().collect();
        if written.is_empty() {
            continue;
        }

        // The recognised words that fall inside this line's span.
        let inside: Vec<&(f64, String)> = heard.iter().filter(|(at, _)| *at >= *start - 0.01 && *at < end).collect();
        let mut placed: Vec<Option<f64>> = vec![None; written.len()];
        let mut cursor = 0usize;
        for (position, word) in written.iter().enumerate() {
            let expected = normalise(word);
            if expected.is_empty() {
                continue;
            }
            if let Some(found) = inside[cursor..].iter().position(|(_, heard_word)| {
                // Compare by characters, never by bytes: slicing "неон" at a
                // byte index lands inside a letter and panics.
                heard_word == &expected || (expected.chars().count() > 3 && heard_word.starts_with(&stem(&expected)))
            }) {
                placed[position] = Some(inside[cursor + found].0);
                cursor += found + 1;
            }
        }

        // Anything unmatched is spread by length across the gap it sits in.
        let mut previous_time = *start;
        let mut position = 0usize;
        while position < written.len() {
            if let Some(at) = placed[position] {
                previous_time = at;
                position += 1;
                continue;
            }
            let gap_start = position;
            while position < written.len() && placed[position].is_none() {
                position += 1;
            }
            let next_time = placed.get(position).copied().flatten().unwrap_or(end);
            let span = (next_time - previous_time).max(0.05);
            let weight: usize = written[gap_start..position].iter().map(|word| word.chars().count().max(1)).sum();
            let mut used = 0usize;
            for offset in gap_start..position {
                let length = written[offset].chars().count().max(1);
                // Centred in its own share of the gap: a word placed exactly on
                // the previous word's time would highlight two words at once.
                let share = (used as f64 + length as f64 / 2.0) / weight as f64;
                placed[offset] = Some(previous_time + span * share);
                used += length;
            }
            previous_time = next_time;
        }

        timed.push(TimedLine {
            start: *start,
            words: written
                .iter()
                .zip(placed)
                .map(|(word, at)| (at.unwrap_or(*start), (*word).to_string()))
                .collect(),
        });
    }
    timed
}

/// Puts the track's own lyrics on the recogniser's clock.
///
/// The words are already known - the model sang what it was given - so the
/// recogniser is used for *timing only*, which is what every karaoke aligner
/// worth the name does. Its text is mistrusted: sung vocals are mis-heard
/// constantly, and printing that back as lyrics is how karaoke ends up
/// showing nonsense. All lines are placed at once by a monotonic alignment
/// that maximises how well they match overall: a line matched greedily to a
/// later repeat of itself would strand every line sung in between, so a line
/// that was not heard clearly is left out and filled in between its
/// neighbours instead. A repeated chorus consumes its occurrences in order.
pub fn align_lyrics(words: &[(f64, String)], lyrics: &str) -> Vec<(f64, String)> {
    if words.is_empty() {
        return Vec::new();
    }
    let (heard_lines, edges) = heard_lines(words, lyrics);
    let lines: Vec<&str> = heard_lines.iter().map(|(line, _)| line.as_str()).collect();
    let mut placed: Vec<Option<f64>> = heard_lines.iter().map(|(_, at)| *at).collect();
    interpolate(&lines, &mut placed, &edges);
    lines
        .iter()
        .zip(placed)
        .filter_map(|(line, at)| at.map(|at| (at, (*line).to_string())))
        .collect()
}

/// Every sung line of the lyrics with the moment the recogniser heard it start,
/// or None for a line it did not hear clearly enough to place.
pub fn heard_line_starts(words: &[(f64, String)], lyrics: &str) -> Vec<(String, Option<f64>)> {
    heard_lines(words, lyrics).0
}

/// The sung lines with the moments they start, and what the recogniser heard outside the placed ones.
fn heard_lines(words: &[(f64, String)], lyrics: &str) -> (Vec<(String, Option<f64>)>, Edges) {
    let lines: Vec<&str> = lyrics
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !(line.starts_with('[') && line.ends_with(']')))
        .collect();
    if lines.is_empty() || words.is_empty() {
        let edges = Edges { before: 0, after: 0, first: 0.0, last: 0.0 };
        return (lines.iter().map(|line| ((*line).to_string(), None)).collect(), edges);
    }

    let heard: Vec<(f64, String)> = words.iter().map(|(at, word)| (*at, normalise(word))).collect();
    let count = heard.len();
    // How well each line matches when it starts at each recognised word; below
    // the threshold a start is not a candidate at all.
    let spans: Vec<usize> = lines
        .iter()
        .map(|line| line.split_whitespace().filter(|word| !normalise(word).is_empty()).count())
        .collect();
    let scores: Vec<Vec<f64>> = lines
        .iter()
        .zip(&spans)
        .map(|(line, &span)| {
            let written: String = line.split_whitespace().map(normalise).collect();
            (0..count)
                .map(|start| {
                    if span == 0 {
                        return 0.0;
                    }
                    // A little slack in length, because the recogniser splits
                    // words differently from the page; each window is scored by
                    // what it misses and what it adds, so starting a word early
                    // costs as much as starting a word late.
                    let score = (span.saturating_sub(1).max(1)..=span + 2)
                        .filter(|length| start + length <= count)
                        .map(|length| {
                            let spoken: String = heard[start..start + length].iter().map(|(_, word)| word.as_str()).collect();
                            dice(&written, &spoken)
                        })
                        .fold(0.0, f64::max);
                    if score >= 0.45 { score } else { 0.0 }
                })
                .collect()
        })
        .collect();

    // best[p]: the best (total, sum of placed line numbers, sum of starts) so
    // far with the next line free to start at word p or later. Each line is
    // either placed at some start >= p, which moves p past it, or left out. A
    // clearly recognised line counts the same wherever it is heard best.
    // Between equal totals the earlier lines of the text win - a verse written
    // out more times than the song sings it is heard as its first copies, so
    // the copies past the song stay unplaced - and then the earlier placement:
    // a chorus the model sang twice in a row shows its lines as they are first
    // sung, not where the recogniser heard them best.
    let credit = |score: f64| if score >= 0.7 { 1.0 } else { score };
    let better = |left: (f64, usize, usize), right: (f64, usize, usize)| {
        left.0 > right.0 + 1e-9 || ((left.0 - right.0).abs() <= 1e-9 && (left.1, left.2) < (right.1, right.2))
    };
    let mut best: Vec<Option<(f64, usize, usize)>> = vec![None; count + 1];
    best[0] = Some((0.0, 0, 0));
    let mut back: Vec<Vec<(usize, Option<usize>)>> = Vec::with_capacity(lines.len());
    for (index, &span) in spans.iter().enumerate() {
        let mut next: Vec<Option<(f64, usize, usize)>> = vec![None; count + 1];
        let mut choice = vec![(0usize, None); count + 1];
        for from in 0..=count {
            let Some(so_far) = best[from] else { continue };
            if next[from].is_none_or(|kept| better(so_far, kept)) {
                next[from] = Some(so_far);
                choice[from] = (from, None);
            }
            if span == 0 {
                continue;
            }
            for start in from..count {
                let score = scores[index][start];
                if score <= 0.0 {
                    continue;
                }
                let after = (start + span).min(count);
                let total = (so_far.0 + credit(score), so_far.1 + index, so_far.2 + start);
                if next[after].is_none_or(|kept| better(total, kept)) {
                    next[after] = Some(total);
                    choice[after] = (from, Some(start));
                }
            }
        }
        best = next;
        back.push(choice);
    }

    let mut starts: Vec<Option<usize>> = vec![None; lines.len()];
    let mut position = (0..=count).fold(0, |kept, candidate| match (best[candidate], best[kept]) {
        (Some(offered), Some(held)) if better(offered, held) => candidate,
        (Some(_), None) => candidate,
        _ => kept,
    });
    for index in (0..lines.len()).rev() {
        let (from, start) = back[index][position];
        starts[index] = start;
        position = from;
    }

    // A clearly recognised line was credited in full, so its start may sit as
    // early as the first word it still matches from - a few sounds the
    // recogniser heard before it count too; settle it where it matches best,
    // between its neighbours and within its own length.
    for index in 0..lines.len() {
        let Some(start) = starts[index] else { continue };
        let floor = starts[..index].iter().rev().flatten().next().map(|previous| previous + 1).unwrap_or(0);
        let ceiling = starts[index + 1..].iter().flatten().next().copied().unwrap_or(count);
        let low = start.saturating_sub(2).max(floor);
        let high = (start + spans[index].max(2)).min(ceiling.saturating_sub(1)).min(count.saturating_sub(1));
        let settled = (low..=high).fold(start, |kept, candidate| if scores[index][candidate] > scores[index][kept] { candidate } else { kept });
        starts[index] = Some(settled);
    }

    // the words heard before the first placed line and after the last one: an
    // edge gap is timed only when the recogniser heard enough there to sing it
    let first_start = starts.iter().flatten().next().copied().unwrap_or(count);
    let after_last = starts
        .iter()
        .zip(&spans)
        .filter_map(|(start, &span)| start.map(|start| (start + span).min(count)))
        .last()
        .unwrap_or(0);
    let edges = Edges {
        before: first_start,
        after: count - after_last.min(count),
        first: heard.first().map(|(at, _)| *at).unwrap_or(0.0),
        last: heard.last().map(|(at, _)| *at).unwrap_or(0.0),
    };
    (lines.iter().zip(starts).map(|(line, start)| ((*line).to_string(), start.map(|start| heard[start].0))).collect(), edges)
}

/// Lines the recogniser could not place are spread evenly between the ones it
/// could, so a karaoke file has no silent holes. A gap before the first placed
/// line or after the last one is timed only when the recogniser heard at least
/// as many unmatched words there as the gap has lines - a first line sung
/// unclearly keeps its place - and otherwise is not in the song as heard (a
/// text longer than what was sung) and gets no time.
fn interpolate(lines: &[&str], placed: &mut [Option<f64>], edges: &Edges) {
    let mut index = 0;
    while index < lines.len() {
        if placed[index].is_some() {
            index += 1;
            continue;
        }
        let gap_start = index;
        while index < lines.len() && placed[index].is_none() {
            index += 1;
        }
        let gap = index - gap_start;
        let before = gap_start.checked_sub(1).and_then(|previous| placed[previous]);
        let after = placed.get(index).copied().flatten();
        let (from, to, leading) = match (before, after) {
            (Some(before), Some(after)) => (before, after, false),
            (None, Some(after)) if edges.before >= gap => (edges.first, after, true),
            (Some(before), None) if edges.after >= gap => (before, edges.last.max(before), false),
            (None, None) if edges.before >= gap => (edges.first, edges.last, true),
            _ => continue,
        };
        // a leading gap starts at the first word heard; any other one starts after the line before it
        let (offset, steps) = if leading { (0.0, gap as f64) } else { (1.0, (gap + 1) as f64) };
        for (position, slot) in placed[gap_start..index].iter_mut().enumerate() {
            *slot = Some(from + (to - from) * ((position as f64 + offset) / steps));
        }
    }
}

/// What the recogniser heard outside the placed lines: how many words before
/// the first one and after the last one, and the times of the first and last word.
struct Edges {
    before: usize,
    after: usize,
    first: f64,
    last: f64,
}

/// A word without its last character, for tolerating inflected endings.
fn stem(word: &str) -> String {
    let count = word.chars().count();
    word.chars().take(count.saturating_sub(1)).collect()
}

fn normalise(word: &str) -> String {
    word.chars().filter(|character| character.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The characters two strings share in order, over both their lengths: 1 for
/// the same text, lower for what either one misses or adds.
fn dice(expected: &str, heard: &str) -> f64 {
    let (left, right) = (expected.chars().count(), heard.chars().count());
    if left + right == 0 {
        return 0.0;
    }
    2.0 * similarity(expected, heard) * left as f64 / (left + right) as f64
}

/// How much of the written line the recogniser heard, compared character by
/// character rather than word by word.
///
/// Sung vocals come back mangled - "on the glass" as "arms the grass" - and a
/// word-level comparison scores that as a miss even though the line is plainly
/// the right one. Comparing characters in order tolerates the mangling, which
/// is what character-level aligners do for exactly this reason.
fn similarity(expected: &str, heard: &str) -> f64 {
    if expected.is_empty() || heard.is_empty() {
        return 0.0;
    }
    let left: Vec<char> = expected.chars().collect();
    let right: Vec<char> = heard.chars().collect();
    let mut previous = vec![0usize; right.len() + 1];
    let mut current = vec![0usize; right.len() + 1];
    for l in 0..left.len() {
        for r in 0..right.len() {
            current[r + 1] = if left[l] == right[r] { previous[r] + 1 } else { current[r].max(previous[r + 1]) };
        }
        std::mem::swap(&mut previous, &mut current);
        current.iter_mut().for_each(|value| *value = 0);
    }
    previous[right.len()] as f64 / left.len() as f64
}

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// A model is fetched from a pinned commit: a repository's latest files can be other builds under the same name.
    #[test]
    fn every_model_is_pinned_to_a_commit() {
        for asset in ASSETS.iter().filter(|asset| asset.url.contains("huggingface.co/")) {
            assert!(asset.url.contains("/resolve/") && !asset.url.contains("/resolve/main/"), "{} is not pinned", asset.url);
        }
    }

    #[test]
    fn whisper_fillers_are_dropped_and_sung_lines_kept() {
        assert!(is_hallucination(" Субтитры создавал DimaTorzok"));
        assert!(is_hallucination("Продолжение следует..."));
        assert!(is_hallucination("Thanks for watching!"));
        assert!(is_hallucination("ご視聴ありがとうございました"));
        assert!(is_hallucination("ВЕСЕЛАЯ МУЗЫКА"));
        assert!(is_hallucination("[Music]"));
        assert!(is_hallucination(" 1."));
        assert!(!is_hallucination("Если б мне платили каждый раз,"));
        assert!(!is_hallucination("Спасибо, что ты рядом со мной"));
        assert!(!is_hallucination("Поехали!"));
        let json = r#"{"segments":[{"start":1.0,"text":" Субтитры создавал DimaTorzok","words":[{"start":1.0,"word":" Субтитры"}]},{"start":5.0,"text":" Тьма во мне","words":[{"start":5.0,"word":" Тьма"},{"start":5.4,"word":" во"},{"start":5.6,"word":" мне"}]}]}"#;
        let words = whisper_words_from_json(json);
        assert_eq!(words.iter().map(|(_, word)| word.as_str()).collect::<Vec<_>>(), ["Тьма", "во", "мне"]);
    }

    // The releases every runtime download is pinned to.
    #[cfg(not(target_os = "linux"))]
    const WHISPER_BUILD: &str = "Whisper-Faster_r192.3";
    #[cfg(target_os = "linux")]
    const WHISPER_BUILD: &str = "Whisper-Faster_r189.1";
    const WHISPER_CUBLAS_BUILD: &str = "11.11.3.6";
    const WHISPER_CUDNN_BUILD: &str = "8.9.7.29";
    const CUBLAS_BUILD: &str = "12.9.2.10";
    const CUDART_BUILD: &str = "12.9.79";
    const CUFFT_BUILD: &str = "11.4.1.4";
    const CUDNN_BUILD: &str = "9.25.0.15";
    const CURAND_BUILD: &str = "10.3.10.19";
    const ONNXRUNTIME_BUILD: &str = "v1.31.1";
    const ONNXRUNTIME_DIRECTML_BUILD: &str = "onnxruntime.directml/1.24.4/";
    const DIRECTML_BUILD: &str = "ai.directml/1.15.4/";

    impl TimedLine {
        fn text(&self) -> String {
            self.words.iter().map(|(_, word)| word.as_str()).collect::<Vec<_>>().join(" ")
        }
    }

    #[test]
    fn words_become_lines_at_pauses_and_never_flash_past() {
        let words = vec![
            (0.0, "neon".into()),
            (0.4, "on".into()),
            (0.8, "the".into()),
            (1.2, "glass".into()),
            // A long pause would start a new line, but it lands inside the two
            // second window ACE used, so it joins the line before it.
            (2.9, "driving".into()),
            (3.3, "home".into()),
            (12.0, "the".into()),
            (12.4, "engine".into()),
            (12.9, "dies".into()),
        ];
        let lines = group_words(&words);
        assert_eq!(
            lines,
            vec![
                (0.0, "neon on the glass".to_string()),
                (2.9, "driving home".to_string()),
                (12.0, "the engine dies".to_string()),
            ]
        );
    }

    #[test]
    fn a_verbose_transcription_yields_timed_lines_and_plain_text_yields_none() {
        let verbose = serde_json::json!({
            "text": "neon on the glass",
            "segments": [{ "start": 1.5, "text": " neon on the glass" }, { "start": 9.0, "text": "driving home" }]
        });
        assert_eq!(
            segments_from_verbose_json(&verbose),
            vec![(1.5, "neon on the glass".to_string()), (9.0, "driving home".to_string())]
        );
        assert!(segments_from_verbose_json(&serde_json::json!({ "text": "no timings here" })).is_empty());
    }

    #[test]
    fn the_written_lyrics_are_kept_and_only_the_timing_is_borrowed() {
        // What the recogniser heard: two words wrong, as sung vocals go.
        let heard = vec![
            (1.0, "neon".into()),
            (1.4, "arms".into()),   // "on" misheard
            (1.8, "the".into()),
            (2.2, "grass".into()),  // "glass" misheard
            (9.0, "driving".into()),
            (9.6, "home".into()),
        ];
        let lyrics = "[verse]
Neon on the glass
Driving home
";
        let lines = align_lyrics(&heard, lyrics);

        // The user's words, on the recogniser's clock - never the mishearing.
        assert_eq!(lines, vec![(1.0, "Neon on the glass".to_string()), (9.0, "Driving home".to_string())]);
    }

    #[test]
    fn every_word_gets_its_own_time_and_the_file_says_so() {
        let heard = vec![
            (1.0, "neon".into()),
            (1.4, "arms".into()),
            (1.8, "the".into()),
            (2.2, "glass".into()),
            (9.0, "driving".into()),
            (9.6, "home".into()),
        ];
        let lines = align_lyrics_words(&heard, "Neon on the glass
Driving home");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text(), "Neon on the glass");
        // "Neon", "the" and "glass" were heard; "on" was not, so it lands
        // between the words either side of it rather than on the line start.
        assert_eq!(lines[0].words[0].0, 1.0);
        assert_eq!(lines[0].words[2].0, 1.8);
        assert_eq!(lines[0].words[3].0, 2.2);
        assert!(lines[0].words[1].0 > 1.0 && lines[0].words[1].0 < 1.8, "got {}", lines[0].words[1].0);

        let lrc = enhanced_lrc(&lines);
        assert!(lrc.starts_with("[00:01.00]<00:01.00>Neon <"), "{lrc}");
        assert!(lrc.contains("<00:09.00>Driving <00:09.60>home"), "{lrc}");
    }

    #[test]
    fn russian_lyrics_align_without_slicing_a_letter_in_half() {
        let heard = vec![
            (1.0, "неон".into()),
            (1.5, "дрожит".into()),
            (2.0, "на".into()),
            (2.4, "коже".into()),
        ];
        let lines = align_lyrics_words(&heard, "Неон дрожит на мокрой коже");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].words.len(), 5);
        assert_eq!(lines[0].words[0].0, 1.0);
        assert_eq!(lines[0].words[1].0, 1.5);
        assert!(enhanced_lrc(&lines).contains("<00:01.50>дрожит"));
    }

    #[test]
    fn a_line_nobody_could_place_is_filled_in_between_its_neighbours() {
        let heard = vec![(0.0, "first".into()), (10.0, "third".into())];
        let lines = align_lyrics(&heard, "First
Something entirely unheard
Third");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].0, 0.0);
        assert!(lines[1].0 > 0.0 && lines[1].0 < 10.0, "the middle line got {}", lines[1].0);
        assert_eq!(lines[2].0, 10.0);
    }

    /// A text that repeats its verse more times than the song sings it: the sung lines get their own times from the
    /// first copies, and the copies past the song get no time at all instead of one shared point.
    #[test]
    fn a_text_longer_than_the_song_times_only_what_was_sung() {
        let verse = ["neon fades along the lane", "footsteps keep the time of rain", "fold the night and leave it here", "morning has a sky to clear"];
        let lyrics = (0..4).flat_map(|_| verse.iter()).copied().collect::<Vec<_>>().join("\n");
        // the song sings the verse once and a half
        let mut words: Vec<(f64, String)> = Vec::new();
        let mut at = 10.0;
        for line in verse.iter().chain(&verse[..2]) {
            for word in line.split_whitespace() {
                words.push((at, word.to_string()));
                at += 0.5;
            }
            at += 1.0;
        }
        let lines = align_lyrics(&words, &lyrics);
        assert_eq!(lines.len(), 6, "only the sung lines are timed: {lines:?}");
        assert!(lines.windows(2).all(|pair| pair[0].0 < pair[1].0), "every sung line has its own time: {lines:?}");
        assert_eq!(lines[0].0, 10.0);
        let timed = align_lyrics_words(&words, &lyrics);
        assert_eq!(timed.len(), 6);
    }

    /// A first line the recogniser heard but could not match keeps its place, from the first word heard.
    #[test]
    fn a_first_line_heard_unclearly_starts_at_the_first_word() {
        let lyrics = "Neon fades along the lane\nFootsteps keep the time of rain\nFold the night and leave it here";
        let mut words: Vec<(f64, String)> = vec![(5.0, "mmm".into()), (5.6, "ahh".into()), (6.2, "ooh".into())];
        let mut at = 10.0;
        for line in ["footsteps keep the time of rain", "fold the night and leave it here"] {
            for word in line.split_whitespace() {
                words.push((at, word.to_string()));
                at += 0.5;
            }
            at += 1.0;
        }
        let lines = align_lyrics(&words, lyrics);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert_eq!((lines[0].0, lines[0].1.as_str()), (5.0, "Neon fades along the lane"));
        assert_eq!(lines[1].0, 10.0, "{lines:?}");
    }

    /// Lines past the song stay without a time when nothing was heard after the last sung line, and when what was
    /// heard there is fewer words than the lines.
    #[test]
    fn lines_past_the_song_get_no_time_from_a_few_stray_words() {
        let sung = "neon fades along the lane";
        let lyrics = std::iter::once(sung.to_string()).chain((0..68).map(|line| format!("extra line number {line} of the text"))).collect::<Vec<_>>().join("\n");
        let mut words: Vec<(f64, String)> = sung.split_whitespace().enumerate().map(|(index, word)| (10.0 + index as f64 * 0.5, word.to_string())).collect();
        assert_eq!(align_lyrics(&words, &lyrics).len(), 1, "no words after the sung line");
        words.push((20.0, "yeah".into()));
        words.push((21.0, "oh".into()));
        assert_eq!(align_lyrics(&words, &lyrics).len(), 1, "two stray words do not time 68 lines");
    }

    #[test]
    fn the_whisper_runtime_is_pinned_and_every_asset_is_distinct() {
        for entry in ASSETS {
            assert!(entry.bytes > 0, "{} has no size", entry.id);
            assert!(entry.url.starts_with("https://"), "{} is not fetched over https", entry.id);
            assert_eq!(ASSETS.iter().filter(|other| other.id == entry.id).count(), 1);
            if entry.kind == AssetKind::Runtime {
                // Every runtime is pinned to an exact release - whisper.cpp to
                // its tag, ONNX Runtime to its version - so a working setup
                // keeps working.
                // NVIDIA's libraries are pinned by their own version in the
                // archive name, the same way the others are.
                let pinned = entry.url.contains(WHISPER_BUILD)
                    || entry.url.contains(WHISPER_CUBLAS_BUILD)
                    || entry.url.contains(WHISPER_CUDNN_BUILD)
                    || entry.url.contains(ONNXRUNTIME_BUILD)
                    || entry.url.contains(ONNXRUNTIME_DIRECTML_BUILD)
                    || entry.url.contains(DIRECTML_BUILD)
                    || entry.url.contains(CUBLAS_BUILD)
                    || entry.url.contains(CUDART_BUILD)
                    || entry.url.contains(CUDNN_BUILD)
                    || entry.url.contains(CUFFT_BUILD)
                    || entry.url.contains(CURAND_BUILD)
                    || entry.url.contains("/transcribe.cpp/releases/download/engine-");
                assert!(pinned, "{} is not pinned to a release", entry.id);
                // a single library is downloaded as it is, not unpacked
                if entry.unzip_into.is_some() {
                    assert!(!entry.marker.is_empty(), "{} has no proof of extraction", entry.id);
                }
            }
        }
    }

    #[test]
    fn karaoke_stays_off_until_a_provider_is_chosen() {
        let mut config = LyricsSyncConfig::default();
        assert!(!config.available());
        config.enabled = true;
        assert!(!config.available());
        config.provider = AsrProvider::Whisper;
        assert!(config.available());
    }
}

#[cfg(test)]
mod live_recognition {
    use super::*;

    /// The whole recognition path against a real installation, when one is
    /// pointed at: decode, run the recogniser, read its JSON, get words with
    /// times. Checking the download and the binary separately is what let a
    /// finished installation still refuse to run.
    #[test]
    fn recognising_a_real_track_end_to_end() {
        let (Some(root), Some(track)) = (std::env::var_os("YUE_DATA_ROOT"), std::env::var_os("YUE_TEST_TRACK")) else { return };
        let sync = LyricsSync::new(std::path::Path::new(&root));
        let config = LyricsSyncConfig {
            enabled: true,
            provider: AsrProvider::Whisper,
            whisper_model: Some("whisper-large-v3".into()),
            runtime: OnnxFlavour::Cuda,
            ..Default::default()
        };
        assert!(sync.whisper_binary().is_some(), "the recogniser is not installed");
        assert!(sync.whisper_model_ready(&config), "the model is not considered ready");
        let words = sync
            .whisper_words(&config, std::path::Path::new(&track), Some("ru"), "")
            .expect("recognition");
        eprintln!("words: {}", words.len());
        for (at, word) in words.iter().take(8) {
            eprintln!("  {at:.2}s {word}");
        }
        assert!(!words.is_empty(), "nothing was recognised");
    }

    /// A chorus sung twice, heard worse the first time, and one of its lines
    /// not heard at all: nothing may jump to the second chorus and strand the
    /// verse sung in between.
    #[test]
    fn a_line_heard_better_later_does_not_strand_the_verse_before_it() {
        let heard = |from: f64, text: &str| -> Vec<(f64, String)> {
            text.split_whitespace().enumerate().map(|(i, w)| (from + i as f64 * 0.5, w.to_string())).collect()
        };
        let mut words = Vec::new();
        words.extend(heard(1.0, "walking down the road tonight"));
        words.extend(heard(10.0, "my truck talks back"));
        words.extend(heard(13.0, "fast down the dusty track"));
        words.extend(heard(16.0, "ive been part of it"));
        words.extend(heard(20.0, "took my girl out on a friday"));
        words.extend(heard(30.0, "my truck talks back"));
        words.extend(heard(33.0, "down the dusty track"));
        words.extend(heard(36.0, "feeling right tonight"));
        let lyrics = "[Verse 1]\nWalking down the road tonight\n\n[Chorus]\nMy truck talks back\nDown the dusty track\nFeeling right tonight\n\n[Verse 2]\nTook my girl out on a Friday\n\n[Chorus]\nMy truck talks back\nDown the dusty track\nFeeling right tonight";
        let lines = align_lyrics(&words, lyrics);
        let at = |index: usize| lines[index].0;
        assert_eq!(lines.len(), 8);
        assert!((at(2) - 13.5).abs() < 0.6, "the first chorus keeps its own line: {lines:?}");
        assert!(at(3) > at(2) && at(3) < at(4), "an unheard line sits between its neighbours: {lines:?}");
        assert!((at(4) - 20.0).abs() < 0.6, "the second verse is where it was sung: {lines:?}");
        assert!((at(6) - 33.0).abs() < 0.6 && (at(7) - 36.0).abs() < 0.6, "the second chorus is its own: {lines:?}");
    }

    #[test]
    fn karaoke_files_land_where_the_shared_binding_looks() {
        let root = Path::new("karaoke-root").join("models");
        for (variant, id) in [(None, PARAKEET_V3), (Some(PARAKEET_ULTRA), PARAKEET_ULTRA), (Some(PARAKEET_FP32), PARAKEET_V3)] {
            let model = parakeet_model(variant);
            assert_eq!(parakeet_model_asset(variant), id);
            let asset = asset(id).unwrap();
            assert_eq!(Path::new("karaoke-root").join(asset.relative_path), dub_tcpp::model_path(&root, model));
            assert_eq!((asset.url, asset.bytes), (model.download.url, model.download.size));
        }
        #[cfg(target_os = "macos")]
        {
            assert_eq!(parakeet_build(OnnxFlavour::Auto), Some(dub_tcpp::Build::Cpu));
            let asset = asset("transcribe-engine-cpu").unwrap();
            // the Mac fetches the single dylib straight into the build's folder under runtime/, where the loader opens it
            assert_eq!(
                Path::new("karaoke-root").join(asset.relative_path),
                Path::new("karaoke-root").join("runtime").join(dub_tcpp::Build::Cpu.dir()).join(dub_tcpp::LIBRARY),
            );
        }
        // Windows and Linux fetch the engine-0.2.0 archive and unpack it into the build's folder under runtime/, where the
        // loader (runtime_dir(build.dir())) opens LIBRARY; the studio ships the CUDA 13, Vulkan and processor builds.
        #[cfg(any(windows, target_os = "linux"))]
        for build in [dub_tcpp::Build::Cuda, dub_tcpp::Build::Vulkan, dub_tcpp::Build::Cpu] {
            let asset = asset(build.dir()).unwrap();
            assert_eq!(asset.unzip_into, Some(build.dir()));
            assert_eq!(asset.marker, dub_tcpp::LIBRARY);
            assert_eq!((asset.url, asset.bytes), (build.download().url, build.download().size));
        }
    }

    /// The karaoke of a real track: Parakeet's words aligned to the lyrics in
    /// `YUE_TEST_LYRICS`, printed line by line with their start times.
    #[test]
    fn parakeet_karaoke_of_a_real_track() {
        let (Some(root), Some(track), Some(lyrics)) =
            (std::env::var_os("YUE_DATA_ROOT"), std::env::var_os("YUE_TEST_TRACK"), std::env::var_os("YUE_TEST_LYRICS"))
        else {
            return;
        };
        let sync = LyricsSync::new(std::path::Path::new(&root));
        let words = sync.parakeet_words(OnnxFlavour::Auto, None, std::path::Path::new(&track), None).expect("recognition");
        let lyrics = std::fs::read_to_string(lyrics).expect("lyrics file");
        for (at, word) in &words {
            eprintln!("W {at:7.2} {word}");
        }
        for line in align_lyrics_words(&words, &lyrics) {
            eprintln!("{:7.1} {}", line.start, line.words.iter().map(|(_, word)| word.as_str()).collect::<Vec<_>>().join(" "));
        }
    }

    /// The same against Parakeet: what the recogniser actually heard, printed
    /// whole, so a generated song can be checked against its own lyrics, and
    /// how long loading and hearing took. `YUE_TEST_RUNTIME=cpu` keeps it on
    /// the processor, to hold the card's time against.
    #[test]
    fn parakeet_hears_a_real_track() {
        let (Some(root), Some(track)) = (std::env::var_os("YUE_DATA_ROOT"), std::env::var_os("YUE_TEST_TRACK")) else { return };
        let runtime = if std::env::var("YUE_TEST_RUNTIME").is_ok_and(|runtime| runtime == "cpu") { OnnxFlavour::Cpu } else { OnnxFlavour::Auto };
        let sync = LyricsSync::new(std::path::Path::new(&root));
        assert!(sync.parakeet_ready(runtime, None), "Parakeet is not installed");
        let started = std::time::Instant::now();
        let (asr, model) = sync.load_parakeet(runtime, None).expect("load Parakeet");
        let loaded = started.elapsed();
        let words = parakeet_transcribe(&asr, model, std::path::Path::new(&track), None).expect("recognition");
        eprintln!("on {:?}: loaded in {:.1} s, heard in {:.1} s", parakeet_build(runtime), loaded.as_secs_f64(), (started.elapsed() - loaded).as_secs_f64());
        let heard: Vec<&str> = words.iter().map(|(_, word)| word.as_str()).collect();
        eprintln!("heard {} words: {}", words.len(), heard.join(" "));
        assert!(!words.is_empty(), "nothing was recognised");
    }
}


#[cfg(all(test, windows))]
mod parakeet_live {
    use super::*;
    use std::time::Instant;

    fn env_path(name: &str) -> PathBuf {
        PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name} is not set")))
    }

    /// Parakeet on each transcribe.cpp build, from the karaoke folder of `YUE_DATA_ROOT`: how much of the song's own
    /// lyrics (`YUE_TEST_LYRICS`) each hears in order, and how long it takes. `GGML_VK_VISIBLE_DEVICES` picks the card
    /// Vulkan runs on.
    #[test]
    #[ignore = "needs YUE_DATA_ROOT with the three engine builds, YUE_TEST_TRACK and YUE_TEST_LYRICS"]
    fn parakeet_hears_the_lyrics_on_every_build() {
        let root = env_path("YUE_DATA_ROOT").join("karaoke").join("models");
        let track = env_path("YUE_TEST_TRACK");
        let lyrics = std::fs::read_to_string(env_path("YUE_TEST_LYRICS")).expect("the lyrics");
        let sung = words_of(lyrics.lines().filter(|line| !line.trim_start().starts_with('[')).flat_map(str::split_whitespace));
        let model = parakeet_model(None);
        for build in dub_tcpp::Build::ALL {
            let started = Instant::now();
            let asr = dub_tcpp::shared(&dub_tcpp::engine_dir(&root, build), &dub_tcpp::model_path(&root, model)).expect("load Parakeet");
            let words = parakeet_transcribe(&asr, model, &track, None).expect("recognition");
            let heard = words_of(words.iter().map(|(_, word)| word.as_str()));
            let share = in_order(&sung, &heard);
            eprintln!("Parakeet on {build:?}: {:.0}% of the lyrics in order, {:.1} s\n  {}", share * 100.0, started.elapsed().as_secs_f64(), heard.join(" "));
            assert!(share > 0.5, "{build:?} hears too little of the lyrics");
        }
    }

    /// Words without punctuation, in lower case.
    fn words_of<'a>(words: impl Iterator<Item = &'a str>) -> Vec<String> {
        words
            .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
            .filter(|word| !word.is_empty())
            .collect()
    }

    /// The share of `reference` that `heard` has in the same order: their
    /// longest common subsequence over the reference's length.
    fn in_order(reference: &[String], heard: &[String]) -> f64 {
        let mut row = vec![0usize; heard.len() + 1];
        for word in reference {
            let mut diagonal = 0;
            for (index, other) in heard.iter().enumerate() {
                let above = row[index + 1];
                row[index + 1] = if word == other { diagonal + 1 } else { row[index + 1].max(row[index]) };
                diagonal = above;
            }
        }
        row[heard.len()] as f64 / reference.len().max(1) as f64
    }
}
