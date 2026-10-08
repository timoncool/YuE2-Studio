//! Describing a song by ear, as HOT-Step's Training Studio does.
//!
//! MOSS-Music-8B hears the recording through `ace-caption` (HOT-Step's native
//! GGML port) and writes a plain caption of it. It is unreliable on numbers,
//! so the tempo it states is replaced with the one `audio_facts` measured. The YuE2 style sentence is then written from what MOSS heard by
//! the writing assistant, with HOT-Step's YuE2 caption prompt.
//!
//! Prompts, sampling and the caption clean-up follow HOT-Step-CPP 3e7a0778:
//! server/src/services/training/{captionPrompt,mossCaption}.ts and
//! server/src/services/lireek/prompts.ts.

use std::{collections::HashMap, path::Path, process::Command};

use anyhow::{bail, Context, Result};
use regex::Regex;

use crate::audio_facts::Facts;

/// captionPrompt.ts CAPTION_INSTRUCTIONS, with the genre line first: named
/// before any prose exists, the genre stays what the model heard.
pub const PROSE_PROMPT: &str = "Write music dataset metadata grounded in the song's audible content. If audio is attached, describe what you actually HEAR and use title, artist, and lyrics only as weak secondary context.

Return EXACTLY 5 lines in plain text and nothing else. Each field must start at the beginning of its own new line. Never place two fields on the same line.

Use this exact output template:
genre: <comma-separated genre/style tags, most specific first>
caption: <2 to 4 sentences on one line>
bpm: <estimated BPM as integer, e.g. 120>
key: <note plus lowercase mode, e.g. 'C minor' or 'F# major'>
signature: <numerator only — one of 2, 3, 4, 6>

Caption rules:
- Line 1 is REQUIRED and must begin with `genre:`. Line 2 is REQUIRED and must begin with `caption:` followed by the description. Never omit it, never leave it blank, and never answer with the metadata fields alone. If you are unsure of everything else, still write the caption.
- The caption is 2 to 4 sentences, roughly 25 to 60 words, on a single line. Reference captions average about 30 words; a longer caption is not a better one, and padding it with invented detail is worse than stopping.
- Cover these, woven into flowing description rather than listed:
    - genre and subgenre, named plainly
    - the instruments actually present, named concretely
    - vocal character (or state that the track is instrumental and name what carries the lead line)
    - mood and atmosphere
    - production style and sonic character
- NEVER state BPM, key, or time signature in the caption text. They have dedicated fields below, and repeating them in the caption does not match how this model was trained.
- The genre you name in the caption MUST agree with the `genre:` field. Contradicting yourself between the two is worse than naming neither.
- Describe the WHOLE track, weighted by how much of it each part occupies. Intros, outros, interludes and breakdowns are often unrepresentative: a quiet piano or percussion intro before a heavy song does not make the song ambient or classical. Base the genre and caption on the style that dominates most of the running time; mention a contrasting intro or outro only as a secondary detail.
- Name things concretely: `808 bass`, `brushed snare`, `detuned saw lead`, `palm-muted guitar`, `upright piano` — not `interesting textures` or `lush soundscapes`.
- No vague imagery or stacked adjectives ('neon skies, electric hearts'), no marketing copy, and no listener-reaction language ('keeps you moving', 'emotionally resonant').
- Avoid generic openings like 'This track is' when more specific wording can be used immediately.
- If the track is instrumental, say so and name the instrument carrying the lead line.
- Start `genre:` on line 1, `caption:` on line 2, `bpm:` on line 3, `key:` on line 4, and `signature:` on line 5.
- Do not merge fields together. For example, do not output `genre: ... bpm: ... key: ...` on one line.
- Do not use markdown, bullets, numbering, code fences, labels before the template, or commentary after the template.
- Do not mention the artist name or song title in the caption.
- If audio is not attached or a field cannot be determined from available evidence, write N/A for that field instead of guessing.";

/// lireek/prompts.ts YUE2_CAPTION_SYSTEM_PROMPT.
pub const YUE2_SYSTEM_PROMPT: &str = "You write the style caption for the YuE2 music planner. The planner reads ONE descriptive sentence and writes the song's lead sheet from it, so the caption decides genre, voice, arrangement and tempo.

Return exactly ONE sentence, plain text, no line breaks, no quotes, no label, nothing before or after it. Build it in THIS order, each part a short comma-separated phrase, and keep the order even when a part is brief:

  1. language      — the language the vocal is sung in (\"English\", \"Italian\"). For an instrumental write \"instrumental\" here and skip the vocal part.
  2. genre         — the specific style, with era words where they help (\"early 90s pop punk\", \"classic Sanremo ballad\", \"dark synth-pop\"). Never a bare umbrella like \"rock\" or \"pop\".
  3. vocal         — register, gender and delivery of the lead voice (\"nasal male tenor lead vocal with gang-vocal shouts\"), or what carries the lead line if instrumental.
  4. instruments   — the instruments actually present, named concretely (\"distorted power-chord guitars, driving eighth-note bass, punchy live drums\").
  5. mood          — two to four plain words (\"restless, sarcastic and buoyant\").
  6. production    — the mix and era character (\"tight dry mid-90s rock mix with little reverb\").
  7. BPM           — the number followed by \" BPM\" (\"168 BPM\"). This is the ONLY place a number appears.

Rules:
- One sentence. Roughly 35-70 words. Every part present, in order.
- Concrete nouns, not review copy: \"LinnDrum\", \"gated snare\", \"arpeggiated synth bass\" — never \"lush soundscapes\" or \"keeps you moving\".
- Do not name the artist, the band, the song title, the key, or the time signature. Do not quote or summarise the lyrics.
- The genre must agree with the evidence you are given; do not collapse it to an umbrella term.
- Output the sentence and NOTHING else.";

/// What MOSS heard in one recording.
#[derive(Clone, Debug, Default)]
pub struct Heard {
    /// Genre tags from the plain caption, most specific first.
    pub genre: String,
    /// The plain caption's description, facts corrected.
    pub caption: String,
}

/// One run of the captioner over several songs, the way HOT-Step labels a
/// dataset: the model loads once, and each song's caption is handed to `done`
/// as soon as it is written, with its index. `libraries` goes first on PATH:
/// the captioner's ggml-cuda imports cuBLAS from the engine's folder, and
/// without it ggml would quietly run the whole model on the processor.
pub fn hear_batch(
    exe: &Path,
    moss: &Path,
    libraries: Option<&Path>,
    audio: &[std::path::PathBuf],
    cancel: &std::sync::atomic::AtomicBool,
    mut done: impl FnMut(usize, Result<String>),
) -> Result<Vec<String>> {
    use std::io::BufRead;

    let work = tempfile::tempdir().context("make a working folder for the captioner")?;
    let prompt = work.path().join("prompt.prose.txt");
    std::fs::write(&prompt, PROSE_PROMPT)?;
    let list = work.path().join("songs.tsv");
    let listing: Vec<String> = audio.iter().enumerate().map(|(index, path)| format!("{}\t{}", path.display(), work.path().join(index.to_string()).display())).collect();
    std::fs::write(&list, listing.join("\n"))?;

    let mut command = Command::new(exe);
    command
        .arg("--models")
        .arg(moss)
        .arg("--src-list")
        .arg(&list)
        .args(["--mode", "prose", "--temperature", "0", "--rep-penalty", "1.0", "--freq-penalty", "0.3"])
        .arg("--prompt-file")
        .arg(format!("prose={}", prompt.display()))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    if let Some(libraries) = libraries {
        let mut paths = vec![std::path::PathBuf::from(libraries.as_os_str())];
        paths.extend(std::env::var_os("PATH").iter().flat_map(std::env::split_paths));
        if let Ok(path) = std::env::join_paths(paths) {
            command.env("PATH", path);
        }
    }
    quiet(&mut command);
    let mut child = command.spawn().with_context(|| format!("start {}", exe.display()))?;
    let stderr = child.stderr.take().context("the captioner's messages")?;

    // "[MOSS] prose   -> <base> (6.1s)": a single mode writes the bare base
    let written = Regex::new(r"^\[MOSS\]\s+prose\s+->\s+(.+?)\s+\([0-9.]+s\)\s*$").expect("valid regex");
    let mut reported = vec![false; audio.len()];
    let mut log: Vec<String> = Vec::new();
    for line in std::io::BufReader::new(stderr).lines() {
        let line = line.context("read the captioner's messages")?;
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            child.kill().ok();
            child.wait().ok();
            bail!("cancelled");
        }
        if let Some(found) = written.captures(&line) {
            let path = std::path::PathBuf::from(&found[1]);
            let index = path.file_name().and_then(|name| name.to_str()).and_then(|name| name.parse::<usize>().ok());
            if let Some(index) = index.filter(|&index| index < audio.len()) {
                reported[index] = true;
                done(index, std::fs::read_to_string(&path).map(|text| text.trim().to_string()).context("read the caption"));
            }
        } else if !line.starts_with("[MOSS] (") {
            log.push(line);
        }
    }
    let status = child.wait().context("wait for the captioner")?;
    let tail = log.iter().rev().take(8).rev().cloned().collect::<Vec<_>>().join(" | ");
    for (index, seen) in reported.iter().enumerate() {
        if !seen {
            done(index, Err(anyhow::anyhow!("the captioner wrote nothing for this song ({status}): {tail}")));
        }
    }
    Ok(log)
}

/// What MOSS heard in one song, its numbers replaced with the measured ones.
pub fn heard(prose: &str, facts: &Facts) -> Result<Heard> {
    let fields = parse_prose(prose);
    let genre = fields.get("genre").cloned().filter(|value| !is_na(value)).unwrap_or_default();
    let caption = fields.get("caption").cloned().filter(|value| !is_na(value)).unwrap_or_default();
    if caption.is_empty() {
        bail!("the captioner wrote no description");
    }
    Ok(Heard { caption: correct_facts_in_prose(&caption, facts), genre })
}

fn quiet(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = command;
}

fn is_na(value: &str) -> bool {
    value.trim().eq_ignore_ascii_case("n/a") || value.trim().is_empty()
}

/// `genre:` / `caption:` / ... lines, keys lowercased; a line without a
/// label continues the previous field.
fn parse_prose(text: &str) -> HashMap<String, String> {
    let label = Regex::new(r"^\s*(genre|caption|bpm|key|signature)\s*:\s*(.*)$").expect("valid regex");
    let mut fields: HashMap<String, String> = HashMap::new();
    let mut last: Option<String> = None;
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let line = line.trim_start_matches(['*', '-', ' ']);
        if let Some(found) = label.captures(&line.to_lowercase()) {
            let key = found[1].to_string();
            let value = line[line.find(':').map(|at| at + 1).unwrap_or(0)..].trim().trim_matches('*').trim().to_string();
            fields.insert(key.clone(), value);
            last = Some(key);
        } else if let Some(key) = &last {
            let entry = fields.entry(key.clone()).or_default();
            entry.push(' ');
            entry.push_str(line);
        }
    }
    fields
}

/// mossCaption.ts correctFactsInProse: a stated tempo becomes the measured one.
pub fn correct_facts_in_prose(text: &str, facts: &Facts) -> String {
    let bpm = Regex::new(r"\b(\d{2,3})(\s*)(BPM|bpm)\b").expect("valid regex");
    bpm.replace_all(text, |found: &regex::Captures| format!("{}{}{}", facts.bpm, &found[2], &found[3])).into_owned()
}

/// The user message for the YuE2 caption, as yue2CaptionJob.ts builds it.
pub fn yue2_request(heard: &Heard, facts: &Facts, language: &str, lyrics: &str, instrumental: bool) -> String {
    let mut lines = Vec::new();
    if !heard.caption.is_empty() {
        lines.push("EVIDENCE — a caption written from this recording for a different music model (source material, not a template):".to_string());
        lines.push(format!("  \"{}\"", heard.caption));
        lines.push(String::new());
    }
    if !heard.genre.is_empty() {
        lines.push(format!("Genre tags from listening: {}", heard.genre));
    }
    lines.push(format!(
        "Language: {}",
        if instrumental { "instrumental (no vocal)".to_string() } else if language.is_empty() { "unknown — infer it from the lyrics excerpt below".to_string() } else { language.to_string() }
    ));
    lines.push(format!("BPM: {} — end the sentence with exactly \"{} BPM\".", facts.bpm, facts.bpm));
    if instrumental {
        lines.push("This track is INSTRUMENTAL: write \"instrumental\" as the language part and name the lead instrument in the vocal part.".into());
    } else if !lyrics.trim().is_empty() {
        let excerpt: String = lyrics.chars().take(400).collect();
        lines.push(String::new());
        lines.push("Lyrics excerpt (evidence of the language and structure only):".into());
        lines.push(excerpt);
    }
    lines.push(String::new());
    lines.push("Write the one-sentence YuE2 caption now.".into());
    lines.join("\n")
}

/// prompts.ts normalizeYue2Caption: labels, quotes and line breaks off, and
/// the tempo tail rebuilt from the measured number.
pub fn normalize_yue2(raw: &str, bpm: u32) -> String {
    let mut text = raw.to_string();
    if let Some(found) = Regex::new(r"(?s)```(?:[a-z]*)\n(.*?)```").expect("valid regex").captures(&text) {
        text = found[1].to_string();
    }
    let text = Regex::new(r"(?i)^\s*(caption|yue2 caption|style)\s*:\s*").expect("valid regex").replace(&text, "").into_owned();
    let text = text.replace(['\r', '\n'], " ").replace("**", "");
    let text = text.trim_matches(|c: char| c.is_whitespace() || "\"'“”‘’".contains(c)).to_string();
    let text = Regex::new(r"\s+").expect("valid regex").replace_all(&text, " ").into_owned();
    let tail = Regex::new(r"(?i),?\s*(?:at\s+|around\s+|~\s*)?\d{2,3}\s*bpm\.?\s*$").expect("valid regex");
    let text = tail.replace(&text, "").into_owned();
    format!("{}, {bpm} BPM", text.trim_end_matches(|c: char| ".,; ".contains(c)))
}

/// prompts.ts validateYue2Caption: what is wrong with a caption, none if nothing.
pub fn validate_yue2(caption: &str) -> Vec<String> {
    let text = caption.trim();
    if text.is_empty() {
        return vec!["empty".into()];
    }
    let mut issues = Vec::new();
    if text.contains(['\r', '\n']) {
        issues.push("contains a line break (must be one sentence)".to_string());
    }
    let words = text.split_whitespace().count();
    if words < 20 {
        issues.push(format!("too short ({words} words; expect roughly 35-70)"));
    }
    if words > 110 {
        issues.push(format!("too long ({words} words; expect roughly 35-70)"));
    }
    let bpm = Regex::new(r"(?i)\d{2,3}\s*bpm").expect("valid regex");
    if !Regex::new(r"(?i)\d{2,3}\s*bpm\s*\.?$").expect("valid regex").is_match(text) {
        issues.push("must END with the tempo as \"<N> BPM\"".into());
    }
    if bpm.find_iter(text).count() > 1 {
        issues.push("states BPM more than once".into());
    }
    if text.contains("**") || text.starts_with('#') {
        issues.push("contains markdown".into());
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts {
        Facts { bpm: 104 }
    }

    #[test]
    fn the_guessed_numbers_give_way_to_the_measured_ones() {
        let fixed = correct_facts_in_prose("Builds up at 115 BPM in C minor over Cm and Abmaj7, then G major.", &facts());
        assert_eq!(fixed, "Builds up at 104 BPM in C minor over Cm and Abmaj7, then G major.");
    }

    #[test]
    fn the_plain_caption_splits_into_its_fields() {
        let fields = parse_prose("genre: indie pop, synth-pop\ncaption: A bright song.\nIt ends softly.\nbpm: 118\nkey: N/A");
        assert_eq!(fields["genre"], "indie pop, synth-pop");
        assert_eq!(fields["caption"], "A bright song. It ends softly.");
        assert!(is_na(&fields["key"]));
    }

    #[test]
    fn the_yue2_caption_ends_on_the_measured_tempo() {
        let raw = "\"Russian, 2010s indie pop, young female lead vocal, synths and drum machine, wistful and playful, crisp bedroom-pop mix, 120 BPM.\"";
        let caption = normalize_yue2(raw, 104);
        assert!(caption.ends_with(", 104 BPM"), "{caption}");
        assert!(!caption.contains("120"), "{caption}");
        assert!(validate_yue2(&caption).is_empty(), "{:?}", validate_yue2(&caption));
        assert!(validate_yue2("Russian indie pop, 104 BPM").iter().any(|issue| issue.starts_with("too short")));
    }
}
