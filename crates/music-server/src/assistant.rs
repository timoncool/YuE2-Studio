//! The writing assistant: style prompts, lyrics and score edits for YuE2.
//!
//! YuE2's own language model writes a score and audio codes, not words. MAP's
//! model card leaves the text to a separate model: an agent writes the style
//! and lyrics, finds the words for a cover, and edits the ABC score in
//! response to musical feedback. This module is that agent's contract.
//!
//! Two providers, both optional, because the manual form is the primary way in:
//!
//! * a local OpenAI-compatible server (llama.cpp, LM Studio, Ollama), or a
//!   GGUF the studio runs itself;
//! * OpenRouter, chosen from the live catalogue.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The extras every whole-song draft carries.
const EXTRA: &str = "title: a short song title, two to five words, no quotation marks, in the language of the lyrics. cover_prompt: one sentence describing a cover image for this track - a scene, not a poster; no text, no lettering, no logos. duration_seconds: how long this song, as written, runs when sung at its tempo, in seconds, between 30 and 360.";

const VALIDATION: &str = "Before answering, check your own draft: every explicit user constraint kept, vocal gender not contradicted, every section opened by an English tag in square brackets on a line of its own, the vocal language named in the style, and no sentence copied from a reference. Fix what fails, then answer.";

/// How YuE2 reads a style prompt, from its checkpoint and the official demo
/// requests: the text reaches the model verbatim under a `[Tags]` header.
const STYLE_CONTRACT: &str = r#"style: the style prompt YuE2 reads verbatim under its [Tags] header. Write it in English as comma-separated descriptors, 12-50 words, in this order, as the model's authors write it: the language the vocals are sung in first ("English", "Mandarin", "Russian"); genre and subgenre; the lead vocal (gender, timbre, delivery) or "instrumental"; the key instruments and textures; the mood and melodic character; the tempo as "<n> BPM" last. Example: "English, warm piano pop, expressive female voice, acoustic piano, rounded bass and light drums, lyrical memorable melody, unhurried phrasing, 88 BPM". Be concrete and musical - instruments and textures, not adjectives about quality. The request has no tempo, key, negative-prompt or instruction field, so everything about the sound lives in these descriptors: never write instructions to the model ("make it...", "the song should...") or anything that is not a descriptor. Keep every explicit user constraint: a required vocal gender, instrument, tempo or exclusion is never reversed. Never put lyric lines, a song title or section instructions in the style."#;

/// The lyric rules. YuE2 sings the words it is given and plans the song's
/// length around them, so structure and density decide the result.
const LYRICS_RULES: &str = r#"lyrics: the words YuE2 sings, and nothing else, organised into sections. Every section starts with its tag in square brackets, in English, on a line of its own, and its lines follow below it, with a blank line between sections. The tags are [Intro], [Verse 1], [Verse 2], [Pre-Chorus], [Chorus], [Bridge], [Instrumental Break] and [Outro]; never write a section name in words or in brackets of another kind, and never in the language of the song. The shape looks like this:
[Verse 1]
first line of the verse
second line of the verse

[Chorus]
first line of the chorus
second line of the chorus
Size the song to its intended length: about 2 to 3 sung words per second, a verse of 4-8 lines, a chorus repeated where a real song repeats it. Keep neighbouring lines close in syllable count so none is sung rushed. The lyrics carry no implementation notes: stage directions, singer cues, instruments, tempo and pronunciation marks all stay out - the model sings whatever text it is given. Write the sung lines in the language the user wrote their request in: a Russian idea gets Russian lines, a Japanese one Japanese; the tags and the style stay English, and the style names that language first."#;

/// How a recognised recording becomes a lyric sheet: the words stay the
/// singer's, only the layout is the assistant's.
const TRANSCRIPT_RULES: &str = r#"The transcript comes from speech recognition run on the vocals of a finished recording: one line per sung phrase, each after its start time, with the recogniser's mistakes. Write the lyric sheet of that recording exactly as it is sung. Keep the singer's words, in their order, their language and their alphabet - Cyrillic stays Cyrillic, never transliterate; correct a word only where the recognition is plainly wrong and the right word is certain from the line; never invent, rewrite, translate or complete lines, and drop fragments the recogniser picked up in instrumental passages. Leave the times out. Organise the lines into sections: a block of lines that returns is the [Chorus], written out every time it is sung; the blocks between choruses are verses numbered in order ([Verse 1], [Verse 2], [Verse 3]); a block sung once that is neither is the [Bridge]; a block that leads into the chorus every time is the [Pre-Chorus]; lines before the first verse are the [Intro] and after the last chorus the [Outro]. Every section starts with its tag in square brackets, in English, on a line of its own, its lines follow below it, and a blank line separates sections. Use no other tags and no section names in words."#;


const DICTION_RULE: &str = r#"
Diction: the model sings the letters it is given and there is no pronunciation channel. Write every word in its ordinary spelling - in Russian write ё as ё, never е - and choose words whose stress falls naturally on the long notes of the line."#;

const DUET_RULE: &str = r#"
Two voices: describe both singers in the style ("male and female duet, warm baritone and airy soprano"). YuE2 has no singer tags, so do not write singer cues into the lyrics; let the sections themselves - a verse each, a shared chorus - carry the exchange."#;

/// The score YuE2 writes and reads back: ABC notation in the layout of its
/// planning stage and of SheetSage2's transcriptions.
const SCORE_CONTRACT: &str = r#"abc: the complete revised ABC score in YuE2's native dialect. Keep the source's header exactly - X:1, T:, M:, L: (keep the exported unit length), Q:1/4=<bpm>, the declarations V: Vocal and V: Ins, K: - and its layout: sections opened by comment lines ("% verse", "% chorus", "% bridge", "% interlude"), each group of one to four bars written as a "V: Vocal" block followed by a "V: Ins" block. Both voices are single melody lines; Ins is an instrumental theme or solo, never a chord staff. Harmony is written as quoted chord symbols in the Vocal voice before the note or rest they start on, including while the voice rests ("Am7"z16). Native chord qualities only: major (no suffix), m, dim, aug, 7, maj7, m7, dim7, m7b5, sus4, sus2, 6, m6, 7sus4, m(maj7), with optional slash bass (F#m7/C#); anything else (maj9, 13, alt) is not native. Durations are multiples of L from 1, 2, 3, 4, 6, 8, 12, 16, 24, 32, 48 only; any other length is tied (C8-C2), a tie joins equal pitches, a rest is never tied, and a chord change inside a held note splits it with a tie ("C"E16-"Am7"E16). Every bar adds up to the metre (with L:1/32 a 4/4 bar is 32 units, 3/4 is 24); Z, Z2, Z3 are whole resting bars counted by measures, never used across a chord or key change. Accidentals last to the barline and carry across octaves by letter (after ^F, a later f in the bar is sharp too). No tuplets, grace notes, chords of stacked notes, repeat signs, slurs, decorations or w: lyric lines. A metre or key change starts a new group with matching M: or K: in both voices. Chord symbols belong to a full-mode score only: keep them if the score has them, and do not add them to a melody-only score unless asked. Make exactly the change the request asks for - reharmonise, transpose, change the tempo, lengthen or shorten a section, write a solo - and keep every other bar of both voices note for note: same pitches, onsets and durations. When the melody changes, keep the syllable count of the lyric lines it carries. A tempo change is a new Q: and the same BPM in the style."#;

/// The topics `writing_guide` answers, each with what it covers.
pub const GUIDE_TOPICS: &[(&str, &str)] = &[
    ("song", "writing a whole song for song_create: style, lyrics, title, cover prompt"),
    ("style", "the style sentence YuE2 reads, for a new song and for a dataset song from what MOSS heard"),
    ("lyrics", "lyrics: sections, density, diction, duets"),
    ("score", "editing an ABC score in YuE2's dialect"),
    ("transcript", "turning recognised words into a lyric sheet"),
    ("sections", "marking the sections of a published lyric sheet without changing a word"),
];

/// The rules the studio's own assistant is prompted with, as a guide for an
/// agent connected over MCP: the same text, so an agent writes the way the
/// model expects.
pub fn writing_guide(topic: &str) -> Option<String> {
    Some(match topic {
        "song" => format!("{STYLE_CONTRACT}\n\n{LYRICS_RULES}{DICTION_RULE}{DUET_RULE}\n\n{EXTRA}\n\n{VALIDATION}"),
        "style" => format!("For a new song:\n{STYLE_CONTRACT}\n\nFor a dataset song, from what MOSS heard (its heard note: genre, caption, bpm):\n{}", crate::listen::YUE2_SYSTEM_PROMPT),
        "lyrics" => format!("{LYRICS_RULES}{DICTION_RULE}{DUET_RULE}"),
        "score" => SCORE_CONTRACT.to_string(),
        "transcript" => TRANSCRIPT_RULES.to_string(),
        "sections" => SHEET_SECTIONS_PROMPT.to_string(),
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistTarget {
    /// Write the lyrics and the style together.
    All,
    /// Rewrite only the lyrics, coherent with the current style.
    Lyrics,
    /// Rewrite only the style, coherent with the current lyrics.
    Style,
    /// Edit the ABC score as the instruction asks.
    Score,
    /// Lay out a recording's recognised words as a lyric sheet.
    Transcript,
    /// Lay out a published lyric sheet in sections, its words untouched.
    Sheet,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssistRequest {
    pub target: AssistTarget,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub instruction: String,
    #[serde(default)]
    pub lyrics: String,
    #[serde(default)]
    pub style: String,
    #[serde(default)]
    pub abc: String,
    #[serde(default = "default_duration")]
    pub duration_seconds: f64,
}

fn default_duration() -> f64 {
    120.0
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AssistDraft {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lyrics: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<u32>,
}

/// The system prompt and the JSON keys the answer must carry.
pub fn instructions(request: &AssistRequest) -> (String, &'static [&'static str]) {
    let references = references_for(request);
    let notes = craft_notes(request);
    match request.target {
        AssistTarget::Lyrics => (
            format!(
                "You write lyrics for YuE2, a model that turns a style prompt and lyrics into a complete song.\n\
                 Given a lyrics instruction, the current style and a target length, write lyrics coherent with that style.\n\
                 {LYRICS_RULES}{notes}\n\
                 Answer with ONLY a JSON object with key: lyrics."
            ),
            &["lyrics"],
        ),
        AssistTarget::Style => (
            format!(
                "You write the style prompt for YuE2, a model that turns a style prompt and lyrics into a complete song.\n\
                 Given a sound instruction and/or lyrics, write a style that fits them. {STYLE_CONTRACT}{notes}\n\
                 Also write {EXTRA}\n\
                 Answer with ONLY a JSON object with keys: style, title, cover_prompt, duration_seconds.{references}"
            ),
            &["style"],
        ),
        AssistTarget::Score => (
            format!(
                "You are a music editor working on the ABC score YuE2 planned for a song; YuE2 renders whatever score you return.\n\
                 Given the current score, its style and lyrics, and a musical request, revise the score. {SCORE_CONTRACT}\n\
                 If the request changes the tempo, the instruments or the genre, write the revised style too, so the style and the score agree, following this: {STYLE_CONTRACT}\n\
                 Answer with ONLY a JSON object with keys: abc, and style only when it should change."
            ),
            &["abc"],
        ),
        AssistTarget::Transcript => (
            format!(
                "You prepare training data for YuE2, a model that learns songs from their audio and their lyric sheets.\n\
                 {TRANSCRIPT_RULES}\n\
                 Answer with ONLY a JSON object with key: lyrics."
            ),
            &["lyrics"],
        ),
        // a sheet asks for boundaries only; see `sheet_in_sections`
        AssistTarget::Sheet => (SHEET_SECTIONS_PROMPT.to_string(), &["sections"]),
        AssistTarget::All => (
            format!(
                "You write inputs for YuE2, a model that turns a style prompt and lyrics into a complete song with vocals and accompaniment.\n\
                 Given a song description, produce:\n\
                 1. {LYRICS_RULES}\n\
                 2. {STYLE_CONTRACT}{notes}\n\
                 3-5. {EXTRA}\n\
                 Answer with ONLY a JSON object with keys: lyrics, style, title, cover_prompt, duration_seconds.\n\
                 {VALIDATION}{references}"
            ),
            &["lyrics", "style"],
        ),
    }
}

/// Rules that only apply to some songs arrive only for them: a duet rule told
/// to a solo song would invite a second voice.
fn craft_notes(request: &AssistRequest) -> String {
    let mut notes = String::new();
    if matches!(request.target, AssistTarget::All | AssistTarget::Lyrics) {
        notes.push_str(DICTION_RULE);
    }
    if wants_two_voices(request) {
        notes.push_str(DUET_RULE);
    }
    notes
}

fn wants_two_voices(request: &AssistRequest) -> bool {
    const CUES: &[&str] = &[
        "duet", "дуэт", "two voices", "два голоса", "male and female", "female and male",
        "мужской и женский", "женский и мужской", "call and response", "перекличк", "вдвоём", "вдвоем",
    ];
    let brief = format!("{} {} {} {}", request.description, request.instruction, request.style, request.lyrics).to_lowercase();
    CUES.iter().any(|cue| brief.contains(cue))
}

/// The closest official YuE2 requests, as the model was shown them.
fn references_for(request: &AssistRequest) -> String {
    let brief = format!("{} {} {}", request.description, request.instruction, request.style);
    let references = crate::skill::references(&brief);
    if references.is_empty() {
        return String::new();
    }
    let mut block = String::from(
        "\n\nReference requests from the official YuE2 demo, close to this one. Use them for the shape and level of detail of the style and the lyric layout. Do not copy their sentences, instruments or story.\n",
    );
    for (index, reference) in references.iter().enumerate() {
        block.push_str(&format!(
            "\n--- reference {} ---\nstyle: {}\nlyrics:\n{}\n",
            index + 1,
            reference.style.trim(),
            reference.lyrics.trim()
        ));
    }
    block
}

pub fn user_message(request: &AssistRequest) -> String {
    let instruction = request.instruction.trim();
    let description = request.description.trim();
    let brief = if !instruction.is_empty() { instruction } else { description };

    match request.target {
        AssistTarget::Lyrics => format!(
            "Lyrics instruction: {}\nCurrent style, keep the lyrics coherent with it:\n{}\nTarget length: about {} seconds.",
            if brief.is_empty() { "(none - write lyrics that fit the style)" } else { brief },
            request.style.trim(),
            request.duration_seconds.round() as i64,
        ),
        AssistTarget::Style => format!(
            "Sound instruction: {}\nCurrent lyrics, keep the style coherent with them:\n{}",
            if brief.is_empty() { "(none - describe a sound that fits the lyrics)" } else { brief },
            request.lyrics.trim(),
        ),
        AssistTarget::Transcript => format!("Transcript:\n{}", request.description.trim()),
        AssistTarget::Sheet => format!("Lyric sheet:\n{}", request.description.trim()),
        AssistTarget::Score => format!(
            "Request: {}\n\nStyle:\n{}\n\nLyrics:\n{}\n\nCurrent score:\n{}",
            if brief.is_empty() { "(none - tidy the score without changing the music)" } else { brief },
            request.style.trim(),
            request.lyrics.trim(),
            request.abc.trim(),
        ),
        AssistTarget::All => {
            // What the user already wrote is material to build around.
            let mut carried = String::new();
            for (label, value) in [("Lyrics", &request.lyrics), ("Style", &request.style)] {
                let value = value.trim();
                if !value.is_empty() {
                    carried.push_str(&format!("\n{label} (the user wrote this - keep it, build around it):\n{value}"));
                }
            }
            format!(
                "Song description: {}{carried}",
                if brief.is_empty() { "(none - choose something musical and specific)" } else { brief },
            )
        }
    }
}

/// Extracts the answer, tolerating a model that wraps its JSON in prose or a
/// code fence.
pub fn parse_draft(content: &str, required: &[&str]) -> Result<AssistDraft> {
    let start = content.find('{').context("the assistant returned no JSON object")?;
    let end = content.rfind('}').context("the assistant returned no JSON object")?;
    if end <= start {
        bail!("the assistant returned no JSON object");
    }
    let value: Value = serde_json::from_str(&content[start..=end]).with_context(|| {
        let sample: String = content.chars().take(220).collect();
        format!("the assistant returned invalid JSON. It answered: {sample}")
    })?;
    // A string, or very often an array of lines: both are the same text.
    let field = |key: &str| -> Option<String> {
        let text = match value.get(key)? {
            Value::String(text) => text.trim().to_owned(),
            Value::Array(items) => items.iter().filter_map(|item| item.as_str()).collect::<Vec<_>>().join("\n").trim().to_owned(),
            _ => return None,
        };
        (!text.is_empty()).then_some(text)
    };
    for key in required {
        if field(key).is_none() {
            bail!("the assistant answer is missing '{key}'");
        }
    }
    let abc = field("abc");
    if let Some(score) = &abc {
        if !score.contains("K:") || !score.contains('|') {
            bail!("the assistant returned a score that is not ABC notation");
        }
    }
    Ok(AssistDraft {
        lyrics: field("lyrics"),
        style: field("style"),
        abc,
        title: field("title"),
        cover_prompt: field("cover_prompt"),
        duration_seconds: value
            .get("duration_seconds")
            .and_then(|value| value.as_u64().or_else(|| value.as_f64().map(|seconds| seconds.round() as u64)).or_else(|| value.as_str().and_then(|text| text.trim().parse().ok())))
            .map(|seconds| seconds.clamp(10, 360) as u32),
    })
}

/// The answer's shape as a schema the server can enforce: llama-server turns it
/// into a grammar, so a local model cannot answer with prose.
pub fn draft_schema(required: &[&str]) -> Value {
    let long = serde_json::json!({ "type": "string", "minLength": 20 });
    let short = serde_json::json!({ "type": "string", "minLength": 3 });
    serde_json::json!({
        "type": "object",
        "properties": {
            "lyrics": long,
            "style": short,
            "abc": long,
            "title": short,
            "cover_prompt": short,
            "duration_seconds": { "type": "number" },
        },
        "required": required,
        "additionalProperties": false,
    })
}

/// Sampling fitted to the task: laying out a transcript is copying, not
/// writing, so it runs cold whatever the model publishes. The length is
/// bounded by what the task can need, so a model looping on one line fails in
/// seconds instead of at the request timeout.
pub fn fit_to_task(mut body: Value, target: AssistTarget) -> Value {
    if matches!(target, AssistTarget::Transcript | AssistTarget::Sheet) {
        body["temperature"] = Value::from(0.2);
        body["max_tokens"] = Value::from(4096);
    }
    body
}

/// The same for a server on this machine, which bounds nothing on its own:
/// Ollama generates without end unless the request names a limit, and a small
/// model that never closes a string under the schema lists words until the
/// connection is dropped. Every target is bounded there. OpenRouter models
/// carry their own limit, and there thinking counts against `max_tokens`.
pub fn fit_to_local_task(body: Value, target: AssistTarget) -> Value {
    let mut body = fit_to_task(body, target);
    body["max_tokens"] = Value::from(max_tokens_for(target));
    body
}

/// Room for the longest answer a target can need on a local server, thinking
/// included.
pub fn max_tokens_for(target: AssistTarget) -> u32 {
    match target {
        AssistTarget::Style | AssistTarget::Transcript | AssistTarget::Sheet => 4096,
        AssistTarget::All | AssistTarget::Lyrics | AssistTarget::Score => 8192,
    }
}

/// A Cyrillic word with Latin look-alikes in it ("Tут", "oстов"), which a
/// small model writes at the start of a line, spelled in Cyrillic. Only the
/// letters that are one letter both by sight and by sound are swapped: a
/// Latin H stands for Н as often as for Х, and is left for the eye.
pub fn cyrillic_homoglyphs(text: &str) -> String {
    let swap = |c: char| match c {
        'A' => 'А', 'C' => 'С', 'E' => 'Е', 'K' => 'К', 'M' => 'М', 'O' => 'О', 'P' => 'Р', 'T' => 'Т', 'X' => 'Х',
        'a' => 'а', 'c' => 'с', 'e' => 'е', 'o' => 'о', 'p' => 'р', 'x' => 'х', 'y' => 'у',
        other => other,
    };
    let cyrillic = |c: char| ('\u{0400}'..='\u{04FF}').contains(&c);
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if word.chars().any(cyrillic) && word.chars().any(|c| c.is_ascii_alphabetic()) {
            out.extend(word.chars().map(swap));
        } else {
            out.push_str(word);
        }
        word.clear();
    };
    for c in text.chars() {
        if c.is_alphabetic() {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// A published sheet is laid out by asking for its section boundaries only:
/// its lines go in numbered and what comes back is where each section starts
/// and what it is. The studio puts the sheet's own lines under the tags, so no
/// word can be changed, dropped or merged, whatever the model.
pub const SHEET_SECTIONS_PROMPT: &str = r#"You mark the sections of a published lyric sheet. Its lines are numbered. Do not rewrite anything; answer only which lines form each section, in order. A line marked [xN] is sung N times in the song: a block of such lines is the chorus, marked every time it returns, and the lines between two choruses are one verse, not several. A block of lines that returns is a chorus, every time it is sung; the blocks between choruses are verses; a block sung once that is neither is a bridge; a block that leads into the chorus every time is a pre-chorus; lines before the first verse are the intro and after the last chorus the outro. Every line belongs to exactly one section: the first section starts at line 1, each next one starts right after the previous one ends, and the last ends at the last line.
Answer with ONLY a JSON object: {"sections": [{"kind": "verse", "from": 1, "to": 4}, {"kind": "chorus", "from": 5, "to": 8}]}, where kind is one of intro, verse, pre-chorus, chorus, bridge, outro."#;

const SECTION_KINDS: [&str; 6] = ["intro", "verse", "pre-chorus", "chorus", "bridge", "outro"];

/// The sheet's lines as the model reads them: "1. first line".
pub fn numbered_lines(lines: &[&str]) -> String {
    // a small model does not see a returning block in a plain list; each line
    // sung more than once says how many times, so the chorus stands out
    let key = |line: &str| line.to_lowercase().replace('ё', "е").chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for line in lines {
        *counts.entry(key(line)).or_default() += 1;
    }
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| match counts[&key(line)] {
            1 => format!("{}. {line}", index + 1),
            times => format!("{}. {line} [x{times}]", index + 1),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The answer the model is held to when it runs locally.
pub fn sheet_sections_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "sections": {
                "type": "array",
                "minItems": 1,
                "items": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "enum": SECTION_KINDS },
                        "from": { "type": "integer", "minimum": 1 },
                        "to": { "type": "integer", "minimum": 1 },
                    },
                    "required": ["kind", "from", "to"],
                },
            },
        },
        "required": ["sections"],
    })
}

/// The lines of a lyric sheet without its section tags: a line that is only a
/// bracketed tag goes, the words stay as they are.
pub fn without_section_tags(text: &str) -> String {
    text.lines()
        .filter(|line| {
            let line = line.trim();
            !(line.starts_with('[') && line.ends_with(']') && !line[1..line.len() - 1].contains(['[', ']']))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The sheet in sections from where the model says each one starts: a
/// section runs to the line before the next one, so every line is kept once,
/// in order, gaps and overlaps in the answer notwithstanding. None when the
/// answer marks no section.
pub fn sheet_in_sections(answer: &str, lines: &[&str]) -> Option<String> {
    let value: Value = serde_json::from_str(answer.get(answer.find('{')?..=answer.rfind('}')?)?).ok()?;
    let mut starts: Vec<(usize, String)> = value
        .get("sections")?
        .as_array()?
        .iter()
        .filter_map(|section| {
            let kind = section.get("kind")?.as_str()?.trim().to_lowercase();
            let from = section.get("from")?.as_u64()? as usize;
            Some((from, if SECTION_KINDS.contains(&kind.as_str()) { kind } else { "verse".to_string() }))
        })
        .filter(|(from, _)| *from >= 1 && *from <= lines.len())
        .collect();
    starts.sort_by_key(|(from, _)| *from);
    starts.dedup_by_key(|(from, _)| *from);
    let first = starts.first_mut()?;
    first.0 = 1;
    let mut verse = 0;
    let blocks: Vec<String> = starts
        .iter()
        .enumerate()
        .map(|(index, (from, kind))| {
            let to = starts.get(index + 1).map_or(lines.len(), |(next, _)| next - 1);
            if kind == "verse" {
                verse += 1;
            }
            format!("[{}]\n{}", section_tag(kind, verse), lines[from - 1..to].join("\n"))
        })
        .collect();
    Some(blocks.join("\n\n"))
}

/// YuE2's tags: "[Verse 1]", "[Pre-Chorus]".
fn section_tag(kind: &str, verse: usize) -> String {
    match kind {
        "verse" => format!("Verse {verse}"),
        "pre-chorus" => "Pre-Chorus".into(),
        other => {
            let mut letters = other.chars();
            letters.next().map(|first| first.to_uppercase().chain(letters).collect()).unwrap_or_default()
        }
    }
}

/// The share of letters in a text that are Cyrillic, to tell a lyric sheet
/// that kept its alphabet from one the model transliterated.
pub fn cyrillic_share(text: &str) -> f64 {
    let (mut cyrillic, mut letters) = (0usize, 0usize);
    for c in text.chars().filter(|c| c.is_alphabetic()) {
        letters += 1;
        if ('\u{0400}'..='\u{04FF}').contains(&c) {
            cyrillic += 1;
        }
    }
    if letters == 0 { 0.0 } else { cyrillic as f64 / letters as f64 }
}

/// The request as it goes out, with the model's own sampling when it has any.
///
/// OpenRouter publishes `default_parameters` per model, and 83 of them fill it
/// in. Sending one hardcoded temperature to every model overrides what the
/// model asks for; the studio's own value is only a fallback for models that
/// publish nothing.
pub fn chat_body_full(
    model: &str,
    system: &str,
    user: &str,
    effort: Option<&str>,
    defaults: Option<&Value>,
) -> Value {
    chat_body_constrained(model, system, user, effort, defaults, None)
}

/// The same request, with the answer's shape enforced where the server can do
/// it. Asking politely for JSON in the prompt is a hope; a schema is a rule.
pub fn chat_body_constrained(
    model: &str,
    system: &str,
    user: &str,
    effort: Option<&str>,
    defaults: Option<&Value>,
    schema: Option<Value>,
) -> Value {
    let mut body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ],
        "stream": false,
    });

    let mut published = false;
    if let Some(Value::Object(map)) = defaults {
        for (key, value) in map {
            if value.is_null() {
                continue;
            }
            body[key] = value.clone();
            published = true;
        }
    }
    if !published {
        // Nothing published: a little warmth, because these are lyrics.
        body["temperature"] = Value::from(0.8);
    }
    if let Some(effort) = effort.filter(|value| !value.trim().is_empty() && *value != "off") {
        // The draft is what is wanted, not the thinking: exclude keeps the
        // response small and the parser looking in one place.
        //
        // Only for a model that says it takes this. OpenRouter publishes
        // `supported_parameters` for every model and 182 of the 468 do not
        // list reasoning; sending it to those is asking for something they
        // never offered.
        body["reasoning"] = serde_json::json!({ "effort": effort, "exclude": true });
    }
    if let Some(schema) = schema {
        // llama-server reads the schema from `json_schema.schema`, the OpenAI
        // shape; beside `type` it is ignored and any JSON object passes
        body["response_format"] = serde_json::json!({ "type": "json_schema", "json_schema": { "name": "answer", "strict": true, "schema": schema } });
    }

    body
}

/// Reads the answer out of a chat completion.
///
/// Reasoning models served by llama.cpp put their visible answer in
/// `content` and their thinking in `reasoning_content` - but with several
/// Gemma builds `content` comes back empty and everything, the JSON draft
/// included, arrives in `reasoning_content`. Reading only `content` there
/// looks exactly like a model that answered nothing.
pub fn content_of(response: &Value) -> Result<String> {
    let message = response
        .pointer("/choices/0/message")
        .context("the assistant response contained no message")?;
    // OpenRouter calls it `reasoning`, llama.cpp `reasoning_content`; both
    // appear when a model answers with its thinking and an empty content.
    for field in ["content", "reasoning_content", "reasoning"] {
        if let Some(text) = message.get(field).and_then(Value::as_str) {
            if !text.trim().is_empty() {
                return Ok(text.to_owned());
            }
        }
    }
    Err(anyhow!("the assistant response contained no message content"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_target_names_a_length_limit() {
        // Ollama generates without end when a request names none.
        for target in [AssistTarget::All, AssistTarget::Lyrics, AssistTarget::Style, AssistTarget::Score, AssistTarget::Transcript, AssistTarget::Sheet] {
            let body = super::fit_to_local_task(serde_json::json!({}), target);
            assert_eq!(body["max_tokens"].as_u64(), Some(u64::from(super::max_tokens_for(target))));
        }
    }

    #[test]
    fn section_tags_go_and_the_words_stay() {
        let sheet = "[Verse 1]\nСреди связок\n[x] в горле\n\n[Chorus]\nНо настала пора";
        assert_eq!(without_section_tags(sheet), "Среди связок\n[x] в горле\n\nНо настала пора");
    }

    #[test]
    fn a_sheet_is_cut_where_the_sections_start() {
        let lines = ["Шёл я как-то по лесу,", "Шёл по грибы", "И тут раз", "И тут два", "Конец"];
        // the answer skips line 1, starts two sections at line 2 and none at
        // line 4: the first start wins, the sheet begins at line 1, and every
        // line stays, once, in order
        let answer = r#"{"sections": [{"kind": "verse", "from": 2, "to": 2}, {"kind": "chorus", "from": 2, "to": 3}, {"kind": "verse", "from": 3, "to": 3}, {"kind": "outro", "from": 5, "to": 5}]}"#;
        assert_eq!(
            sheet_in_sections(answer, &lines).as_deref(),
            Some("[Verse 1]\nШёл я как-то по лесу,\nШёл по грибы\n\n[Verse 2]\nИ тут раз\nИ тут два\n\n[Outro]\nКонец")
        );
        assert_eq!(sheet_in_sections(r#"{"sections": []}"#, &lines), None);
        assert_eq!(numbered_lines(&lines[..2]), "1. Шёл я как-то по лесу,\n2. Шёл по грибы");
        assert_eq!(numbered_lines(&["Ой, да!", "Куплет", "ой да"]), "1. Ой, да! [x2]\n2. Куплет\n3. ой да [x2]");
    }

    fn request(target: AssistTarget) -> AssistRequest {
        AssistRequest {
            target,
            description: "a night drive synth pop song".into(),
            instruction: String::new(),
            lyrics: "[Verse 1]\nneon".into(),
            style: "synth pop, female vocal, 110 BPM".into(),
            abc: "X:1\nM:4/4\nL:1/16\nK:C\nV: Vocal\nc4d4e4f4|".into(),
            duration_seconds: 90.0,
        }
    }

    #[test]
    fn every_target_declares_the_fields_it_writes() {
        assert_eq!(instructions(&request(AssistTarget::Lyrics)).1, &["lyrics"]);
        assert_eq!(instructions(&request(AssistTarget::Style)).1, &["style"]);
        assert_eq!(instructions(&request(AssistTarget::Score)).1, &["abc"]);
        assert_eq!(instructions(&request(AssistTarget::All)).1, &["lyrics", "style"]);
    }

    #[test]
    fn the_style_contract_reaches_the_request_that_goes_out() {
        let (system, _) = instructions(&request(AssistTarget::All));
        assert!(system.contains("verbatim"));
        assert!(system.contains("[Verse 1]"));
        assert!(system.contains("style prompt YuE2 reads"));
    }

    #[test]
    fn a_whole_song_prompt_carries_official_references() {
        let mut metal = request(AssistTarget::All);
        metal.description = "heavy metal song with screamed vocals".into();
        metal.style = String::new();
        let (system, _) = instructions(&metal);
        assert!(system.contains("Reference requests from the official YuE2 demo"));
        assert!(system.len() < 20_000, "the prompt grew to {} characters", system.len());
    }

    #[test]
    fn a_score_edit_sends_the_score_and_asks_for_the_whole_revision() {
        let message = user_message(&request(AssistTarget::Score));
        assert!(message.contains("Current score:"));
        assert!(message.contains("K:C"));
        let (system, _) = instructions(&request(AssistTarget::Score));
        assert!(system.contains("keep every other bar of both voices note for note"));
        assert!(system.contains("Native chord qualities only"));
    }

    #[test]
    fn a_score_answer_must_be_abc() {
        assert!(parse_draft("{\"abc\": \"just some words about music\"}", &["abc"]).is_err());
        let draft = parse_draft("{\"abc\": \"X:1\\nK:C\\nc4|\"}", &["abc"]).unwrap();
        assert!(draft.abc.unwrap().starts_with("X:1"));
    }

    #[test]
    fn the_other_half_of_the_song_travels_as_context() {
        let lyrics_message = user_message(&request(AssistTarget::Lyrics));
        assert!(lyrics_message.contains("synth pop, female vocal"));
        assert!(lyrics_message.contains("90 seconds"));
        assert!(user_message(&request(AssistTarget::Style)).contains("[Verse 1]"));
    }

    #[test]
    fn the_lyrics_follow_the_language_of_the_request() {
        let mut russian = request(AssistTarget::All);
        russian.description = "панк-рок про ёжика в бункере".into();
        let (system, _) = instructions(&russian);
        assert!(system.contains("language the user wrote their request in"));
        assert!(system.contains("write ё as ё"));
        assert!(!system.contains("combining acute"), "YuE2 has no pronunciation channel");
    }

    #[test]
    fn the_duet_rules_arrive_only_for_two_voices() {
        let mut solo = request(AssistTarget::All);
        solo.style = String::new();
        solo.lyrics = String::new();
        assert!(!instructions(&solo).0.contains("Two voices:"));
        let mut duet = solo.clone();
        duet.description = "дуэт мужского и женского голоса, поп-баллада".into();
        let duet_system = instructions(&duet).0;
        assert!(duet_system.contains("Two voices:"));
        assert!(!duet_system.contains("[Male Vocals]"), "YuE2 has no singer tags");
    }

    #[test]
    fn json_survives_a_code_fence_and_lines_as_a_list() {
        let draft = parse_draft("Sure!\n```json\n{\"lyrics\": [\"[Verse 1]\", \"line\"], \"style\": \"pop\"}\n```", &["lyrics", "style"]).unwrap();
        assert_eq!(draft.lyrics.unwrap(), "[Verse 1]\nline");
        assert!(parse_draft("{\"lyrics\": \"x\"}", &["style"]).is_err());
        assert!(parse_draft("no json here", &["lyrics"]).is_err());
    }

    #[test]
    fn an_answer_that_arrives_as_reasoning_is_still_an_answer() {
        let response = serde_json::json!({ "choices": [{ "message": { "content": "", "reasoning_content": "{\"lyrics\": \"[Verse]\"}" } }] });
        assert_eq!(content_of(&response).unwrap(), "{\"lyrics\": \"[Verse]\"}");
        assert!(content_of(&serde_json::json!({ "choices": [{ "message": { "content": "  " } }] })).is_err());
    }

    #[test]
    fn a_model_that_published_nothing_gets_the_studio_s_own_warmth() {
        assert_eq!(chat_body_constrained("m", "s", "u", None, None, None)["temperature"], serde_json::json!(0.8));
        let published = serde_json::json!({ "temperature": 0.6 });
        assert_eq!(chat_body_constrained("m", "s", "u", None, Some(&published), None)["temperature"], serde_json::json!(0.6));
    }

    #[test]
    fn reasoning_is_only_asked_of_models_that_take_it() {
        assert_eq!(chat_body_constrained("m", "s", "u", Some("high"), None, None)["reasoning"], serde_json::json!({ "effort": "high", "exclude": true }));
        assert!(chat_body_constrained("m", "s", "u", Some("off"), None, None).get("reasoning").is_none());
    }

    #[test]
    fn the_schema_asks_for_content_not_just_a_key() {
        let schema = draft_schema(&["abc"]);
        assert_eq!(schema["properties"]["abc"]["minLength"], 20);
        assert_eq!(schema["required"][0], "abc");
    }
}
