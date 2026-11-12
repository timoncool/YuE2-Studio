//! Why a song failed, as the window shows it.
//!
//! Each reason has a code and fields: the window translates it by the code
//! (`app/i18n/songErrors.ts`), while the English text stays for the log, the hub
//! and agents. The hub groups failures by that text, so it is word for word the
//! text of before. A reason without a kind of its own travels as `Raw` and is
//! shown as it is. The codes are the same in every music studio; a studio adds
//! codes for its own engine and never renames these.

use std::fmt;

use serde_json::{json, Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum SongError {
    /// A reason with no kind of its own: shown as it is.
    Raw(String),
    TrainingOnCard,
    /// Lost with an engine that restarted: the card's memory ran out.
    LostVram,
    /// Lost to the card's memory again after the studio made the song once more with this smaller KV cache.
    LostVramRetried(u32),
    /// Lost with an engine that restarted: the computer's memory ran out.
    LostRam,
    /// Lost with an engine that restarted: the card failed, in the engine's own words when it gave any.
    LostDevice(Option<String>),
    /// Lost with an engine that restarted, whose last line says where it was.
    LostAtLine(String),
    Lost,
    /// Lost with an engine that ended on this exit code (`0xc0000005`), with its last line when it wrote one.
    LostExited { exit: String, line: Option<String> },
    /// Lost with an engine that could not listen on its port: `set` when YUE_ENGINE_PORT names it, else another
    /// program took the port the studio chose, which `note` says when it was not the engine's own.
    LostPort { port: u16, set: bool, note: Option<String> },
    /// The engine gave the song up while loading the models: the card's memory ran out.
    LoadVram,
    /// The engine gave the song up while loading the models: the computer's memory ran out.
    LoadRam,
    /// The engine gave the song up with this FATAL line.
    EngineFatal(String),
    EngineFailed,
    EngineUnreachable(String),
    ImportFailed(String),
    /// The request names a LoRA that is not installed.
    AdapterMissing(String),
    /// Vocals only was asked for without the stem separator installed.
    VocalsNeedSeparator,
    /// The stem separator's runtime cannot run on this computer.
    SeparatorUnavailable(String),
    /// The engine did not take the request.
    EngineRefused(String),
    /// The engine's FATAL line: the prompt and the score leave the model too
    /// few tokens for the length asked for.
    ContextShort { line: String, tokens: u64, context: u64, can_last: u64, asked: u64 },
    /// The engine's FATAL line: the score the model wrote cannot be sung;
    /// `adapters` when LoRAs were on, which the engine blames first.
    ScoreUnsingable { line: String, song: u64, problem: String, adapters: bool },
}

/// The Windows status of a process that ran an instruction the processor does not have.
const ILLEGAL_INSTRUCTION: &str = "0xc000001d";

/// What a Windows exit status of the engine means, for the statuses a person can act on.
fn exit_meaning(exit: &str) -> Option<&'static str> {
    Some(match exit {
        "0xc0000005" => "a crash: an access violation",
        ILLEGAL_INSTRUCTION => "an instruction this processor does not have",
        "0xc0000409" => "a crash: the engine aborted",
        "0xc00000fd" => "a crash: a stack overflow",
        "0xc0000374" => "a crash: corrupted memory",
        "0xc0000135" | "0xc0000139" | "0xc0000142" => "a library of the engine did not load",
        "0xc0000017" | "0xc000012d" => "the computer ran out of memory",
        _ => return None,
    })
}

const CONTEXT_SHORT: (&str, &str, &str, &str, &str) =
    ("the prompt and the score take ", " of the model's ", " tokens, so a song can last ", " s and ", " s were asked for");
const UNSINGABLE: (&str, &str) = ("the score the model wrote for song ", " cannot be sung: ");
const UNSINGABLE_SEED: &str = ". Draw another seed.";
const UNSINGABLE_ADAPTERS: &str = ". Adapters past their limits on the score half do this: lower their strengths or draw another seed.";

impl SongError {
    /// The reason the engine's FATAL line gives, with its fields where the
    /// line is one the window explains in its own language.
    pub fn from_fatal(line: String) -> SongError {
        if let Some(short) = context_short(&line) {
            return short;
        }
        if let Some(unsingable) = score_unsingable(&line) {
            return unsingable;
        }
        SongError::EngineFatal(line)
    }

    /// The code the window finds the translation by; `None` for a reason shown as it is.
    pub fn code(&self) -> Option<&'static str> {
        Some(match self {
            SongError::Raw(_) => return None,
            SongError::TrainingOnCard => "training_on_card",
            SongError::LostVram => "lost_vram",
            SongError::LostVramRetried(_) => "lost_vram_retried",
            SongError::LostRam => "lost_ram",
            SongError::LostDevice(Some(_)) => "lost_device",
            SongError::LostDevice(None) => "lost_device_silent",
            SongError::LostAtLine(_) => "lost_at_line",
            SongError::Lost => "lost",
            SongError::LostExited { exit, .. } if exit == ILLEGAL_INSTRUCTION => "lost_illegal_instruction",
            SongError::LostExited { .. } => "lost_exited",
            SongError::LostPort { set: true, .. } => "lost_port_set",
            SongError::LostPort { set: false, .. } => "lost_port_taken",
            SongError::LoadVram => "load_vram",
            SongError::LoadRam => "load_ram",
            SongError::EngineFatal(_) => "engine_fatal",
            SongError::EngineFailed => "engine_failed",
            SongError::EngineUnreachable(_) => "engine_unreachable",
            SongError::ImportFailed(_) => "import_failed",
            SongError::AdapterMissing(_) => "adapter_missing",
            SongError::VocalsNeedSeparator => "vocals_only_no_separator",
            SongError::SeparatorUnavailable(_) => "separator_unavailable",
            SongError::EngineRefused(_) => "engine_refused",
            SongError::ContextShort { .. } => "context_short",
            SongError::ScoreUnsingable { adapters: false, .. } => "score_unsingable",
            SongError::ScoreUnsingable { adapters: true, .. } => "score_unsingable_adapters",
        })
    }

    /// The fields put into the translation: counts, and the engine's or the network's own words.
    pub fn params(&self) -> Map<String, Value> {
        let mut params = Map::new();
        let mut put = |key: &str, value: Value| {
            params.insert(key.to_string(), value);
        };
        match self {
            SongError::LostDevice(Some(detail))
            | SongError::EngineUnreachable(detail)
            | SongError::ImportFailed(detail)
            | SongError::SeparatorUnavailable(detail)
            | SongError::EngineRefused(detail) => put("detail", json!(detail)),
            SongError::LostAtLine(line) | SongError::EngineFatal(line) => put("line", json!(line)),
            SongError::AdapterMissing(name) => put("name", json!(name)),
            SongError::LostExited { exit, .. } => put("exit", json!(exit)),
            SongError::LostPort { port, .. } => put("port", json!(port)),
            SongError::LostVramRetried(max_seq) => put("n", json!(max_seq)),
            SongError::ContextShort { tokens, context, can_last, asked, .. } => {
                put("tokens", json!(tokens));
                put("context", json!(context));
                put("can_last", json!(can_last));
                put("asked", json!(asked));
            }
            SongError::ScoreUnsingable { song, problem, .. } => {
                put("song", json!(song));
                put("problem", json!(problem));
            }
            _ => {}
        }
        params
    }
}

fn context_short(line: &str) -> Option<SongError> {
    let (start, of, tokens_so, s_and, asked_for) = CONTEXT_SHORT;
    let rest = line.strip_prefix(start)?.strip_suffix(asked_for)?;
    let (tokens, rest) = rest.split_once(of)?;
    let (context, rest) = rest.split_once(tokens_so)?;
    let (can_last, asked) = rest.split_once(s_and)?;
    Some(SongError::ContextShort {
        line: line.to_string(),
        tokens: tokens.parse().ok()?,
        context: context.parse().ok()?,
        can_last: can_last.parse().ok()?,
        asked: asked.parse().ok()?,
    })
}

fn score_unsingable(line: &str) -> Option<SongError> {
    let (start, cannot) = UNSINGABLE;
    let (song, rest) = line.strip_prefix(start)?.split_once(cannot)?;
    let (problem, adapters) = match rest.strip_suffix(UNSINGABLE_ADAPTERS) {
        Some(problem) => (problem, true),
        None => (rest.strip_suffix(UNSINGABLE_SEED)?, false),
    };
    Some(SongError::ScoreUnsingable { line: line.to_string(), song: song.parse().ok()?, problem: problem.to_string(), adapters })
}

impl fmt::Display for SongError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SongError::Raw(text) => f.write_str(text),
            SongError::TrainingOnCard => f.write_str("a training run has the card; songs can be made once it finishes or is stopped"),
            SongError::LostVram => f.write_str("The graphics card ran out of memory on this song and the engine restarted, so the song was lost. Choose a smaller model set in the model manager or a shorter song, and close whatever else uses the card; a card below the smallest set cannot make songs."),
            SongError::LostVramRetried(max_seq) => write!(f, "The graphics card ran out of memory on this song, and again when it was made with a {max_seq} KV cache. Choose a smaller model set or a shorter song, and close whatever else uses the card."),
            SongError::LostRam => f.write_str("The computer ran out of memory (RAM) on this song and the engine restarted, so the song was lost. Close other programs that take memory, or choose a smaller model set in the model manager or a shorter song."),
            SongError::LostDevice(Some(detail)) => write!(f, "The graphics card failed during this song ({detail}) and the engine restarted, so the song was lost. The engine log is in Settings, Engine."),
            SongError::LostDevice(None) => f.write_str("The graphics card failed during this song (a CUDA or driver error) and the engine restarted, so the song was lost. The engine log is in Settings, Engine."),
            SongError::LostAtLine(line) => write!(f, "The engine stopped during this song and restarted, so the song was lost; its last line was {line}. The engine log is in Settings, Engine."),
            SongError::Lost => f.write_str("The engine stopped during this song and restarted, so the song was lost. The engine log is in Settings, Engine."),
            SongError::LostExited { exit, line } => {
                let how = match exit_meaning(exit) {
                    Some(meaning) => format!("{meaning}, exit code {exit}"),
                    None => format!("exit code {exit}"),
                };
                match line {
                    Some(line) => write!(f, "The engine stopped during this song ({how}) and restarted, so the song was lost; its last line was {line}. The engine log is in Settings, Engine."),
                    None => write!(f, "The engine stopped during this song ({how}) and restarted, so the song was lost. The engine log is in Settings, Engine."),
                }
            }
            SongError::LostPort { port, set: true, .. } => write!(f, "The engine could not listen on port {port}, which YUE_ENGINE_PORT sets, so the song was lost: another program holds it. Close that program or set another port."),
            SongError::LostPort { port, set: false, note } => {
                let chosen = note.as_deref().map(|note| format!(" (At start {note}.)")).unwrap_or_default();
                write!(f, "The engine could not listen on port {port}, so the song was lost: another program took the port after the studio chose it.{chosen} Restart the studio: it chooses a free port at start.")
            }
            SongError::LoadVram => f.write_str("The graphics card ran out of memory while the engine was loading the models. Choose a smaller quantisation in the model manager, or close whatever else is using the card - the writing assistant holds several gigabytes of its own."),
            SongError::LoadRam => f.write_str("The computer ran out of memory (RAM) while the engine was loading the models. Close other programs that take memory, or choose a smaller quantisation in the model manager."),
            SongError::EngineFatal(line) | SongError::ContextShort { line, .. } | SongError::ScoreUnsingable { line, .. } => f.write_str(line),
            SongError::EngineFailed => f.write_str("The engine reported a failed job; its log has the reason."),
            SongError::EngineUnreachable(detail) => write!(f, "The engine stopped answering about this job: {detail}"),
            SongError::ImportFailed(detail) => write!(f, "The engine finished the job, but the studio could not import its result: {detail}"),
            SongError::AdapterMissing(name) => write!(f, "adapter {name} is not installed; add it again on the LoRA page"),
            SongError::VocalsNeedSeparator => f.write_str("Install the stem separator in Tools before requesting vocals only."),
            SongError::SeparatorUnavailable(detail) => write!(f, "The stem separator runtime is unavailable: {detail}"),
            SongError::EngineRefused(detail) => write!(f, "the engine refused the job: {detail}"),
        }
    }
}

impl From<String> for SongError {
    fn from(text: String) -> Self {
        SongError::Raw(text)
    }
}

impl From<&str> for SongError {
    fn from(text: &str) -> Self {
        SongError::Raw(text.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One reason of each code. The `match` without a wildcard does not let a new kind be forgotten here.
    fn one_of_each() -> Vec<SongError> {
        let all = vec![
            SongError::Raw("x".into()),
            SongError::TrainingOnCard,
            SongError::LostVram,
            SongError::LostVramRetried(16384),
            SongError::LostRam,
            SongError::LostDevice(Some("x".into())),
            SongError::LostDevice(None),
            SongError::LostAtLine("x".into()),
            SongError::Lost,
            SongError::LostExited { exit: "0xc0000005".into(), line: None },
            SongError::LostExited { exit: ILLEGAL_INSTRUCTION.into(), line: Some("x".into()) },
            SongError::LostPort { port: 18087, set: true, note: None },
            SongError::LostPort { port: 18088, set: false, note: Some("x".into()) },
            SongError::LoadVram,
            SongError::LoadRam,
            SongError::EngineFatal("x".into()),
            SongError::EngineFailed,
            SongError::EngineUnreachable("x".into()),
            SongError::ImportFailed("x".into()),
            SongError::AdapterMissing("x".into()),
            SongError::VocalsNeedSeparator,
            SongError::SeparatorUnavailable("x".into()),
            SongError::EngineRefused("x".into()),
            SongError::ContextShort { line: "x".into(), tokens: 9700, context: 24576, can_last: 595, asked: 600 },
            SongError::ScoreUnsingable { line: "x".into(), song: 1, problem: "x".into(), adapters: false },
            SongError::ScoreUnsingable { line: "x".into(), song: 1, problem: "x".into(), adapters: true },
        ];
        for error in &all {
            match error {
                SongError::Raw(_)
                | SongError::TrainingOnCard
                | SongError::LostVram
                | SongError::LostVramRetried(_)
                | SongError::LostRam
                | SongError::LostDevice(_)
                | SongError::LostAtLine(_)
                | SongError::Lost
                | SongError::LostExited { .. }
                | SongError::LostPort { .. }
                | SongError::LoadVram
                | SongError::LoadRam
                | SongError::EngineFatal(_)
                | SongError::EngineFailed
                | SongError::EngineUnreachable(_)
                | SongError::ImportFailed(_)
                | SongError::AdapterMissing(_)
                | SongError::VocalsNeedSeparator
                | SongError::SeparatorUnavailable(_)
                | SongError::EngineRefused(_)
                | SongError::ContextShort { .. }
                | SongError::ScoreUnsingable { .. } => {}
            }
        }
        all
    }

    /// Each code is translated into every language of the window: a code without a translation fails here.
    #[test]
    fn every_code_has_a_translation_in_every_language() {
        let dictionary = include_str!("../../../app/i18n/songErrors.ts");
        let mut missing = Vec::new();
        for error in one_of_each() {
            let Some(code) = error.code() else { continue };
            let found = dictionary.matches(&format!("\n  {code}:")).count();
            if found != 5 {
                missing.push(format!("{code}: {found} of 5"));
            }
            for key in error.params().keys() {
                let named = dictionary.matches(&format!("{{{key}}}")).count();
                if named == 0 {
                    missing.push(format!("{code}: no translation uses {{{key}}}"));
                }
            }
        }
        assert!(missing.is_empty(), "without a translation: {missing:?}");
    }

    #[test]
    fn the_english_text_stays_what_the_hub_already_groups_by() {
        assert_eq!(
            SongError::ImportFailed("error sending request for url (x)".into()).to_string(),
            "The engine finished the job, but the studio could not import its result: error sending request for url (x)"
        );
        assert_eq!(SongError::EngineRefused("yue-server returned 400".into()).to_string(), "the engine refused the job: yue-server returned 400");
        assert_eq!(SongError::AdapterMissing("rock".into()).to_string(), "adapter rock is not installed; add it again on the LoRA page");
        assert_eq!(SongError::Raw("x".into()).code(), None);
    }

    /// The engine's own lines, as yue2.cpp `pipeline.h` prints them after `FATAL:`.
    #[test]
    fn the_engine_s_lines_the_window_explains_are_read_with_their_fields() {
        let line = "the prompt and the score take 9700 of the model's 24576 tokens, so a song can last 595 s and 600 s were asked for";
        let short = SongError::from_fatal(line.into());
        assert_eq!(short.code(), Some("context_short"));
        assert_eq!(short.params()["can_last"], 595);
        assert_eq!(short.params()["asked"], 600);
        assert_eq!(short.to_string(), line);

        let line = "the score the model wrote for song 2 cannot be sung: it has no voice with bars. Draw another seed.";
        let unsingable = SongError::from_fatal(line.into());
        assert_eq!(unsingable.code(), Some("score_unsingable"));
        assert_eq!(unsingable.params()["song"], 2);
        assert_eq!(unsingable.params()["problem"], "it has no voice with bars");
        assert_eq!(unsingable.to_string(), line);

        let line = "the score the model wrote for song 1 cannot be sung: x. Adapters past their limits on the score half do this: lower their strengths or draw another seed.";
        assert_eq!(SongError::from_fatal(line.into()).code(), Some("score_unsingable_adapters"));

        let other = SongError::from_fatal("lyric_schedule needs cfg_scale 1".into());
        assert_eq!(other.code(), Some("engine_fatal"));
        assert_eq!(other.to_string(), "lyric_schedule needs cfg_scale 1");
    }
}
