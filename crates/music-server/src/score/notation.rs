//! A score as notes, and edited notes written back into the score.
//!
//! Only the bars that changed are written again, together with any bar a tie
//! joins to one of them; the header, the section comments, the other part and
//! every untouched bar come back character for character. A rewritten bar is
//! spelled the way the model spells one, and whatever is written is read back
//! with the dialect's reader and compared with the notes asked for before it
//! is handed on. The bar grid does not move: the tempo is the one header an
//! edit may set.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;
use serde_json::Value;

use super::abc::{self, Score, INS, VOCAL, VOICES};
use super::Q;

pub const LONGEST: usize = 200_000;
pub const MOST_NOTES: usize = 20_000;
pub const TEMPO_LINE: usize = 4;
pub const UNIT_LINE: usize = 3;
pub const HEADER_LINES: usize = 8;
pub const TEMPO_LOW: i64 = 40;
pub const TEMPO_HIGH: i64 = 200;
pub const SECTION_LONGEST: usize = 40;
pub const FINEST: i64 = 32;

const MARKS: [(i32, &str); 5] = [(-2, "__"), (-1, "_"), (0, "="), (1, "^"), (2, "^^")];

fn mark(alteration: i32) -> &'static str {
    MARKS.iter().find(|(value, _)| *value == alteration).map(|(_, text)| *text).expect("an alteration of at most two")
}

fn part_name(voice: usize) -> &'static str {
    if voice == VOCAL { "sung" } else { "instrumental" }
}

fn reason(error: &str) -> String {
    error.trim().trim_end_matches('.').to_string()
}

fn unreadable(error: &str) -> String {
    format!(
        "This score cannot be read note by note: {}.\n\nThe piano roll draws scores written in the dialect the model writes. The ABC tab still has the text, and a song can still be sung from text the roll cannot draw.",
        reason(error)
    )
}

fn not_written(error: &str) -> String {
    format!("The edit could not be written into the score: {}.\n\nNothing was changed. The score is as it was before the edit.", reason(error))
}

fn tempo_range(value: i64, low: i64, high: i64) -> String {
    format!("A tempo of {value} BPM is outside {low} to {high}.")
}

/// `\s*Z([2-4])?\s*` as a whole: how many bars a whole-bar rest piece holds.
fn full_rest(piece: &str) -> Option<usize> {
    static FULL_REST: OnceLock<Regex> = OnceLock::new();
    let pattern = FULL_REST.get_or_init(|| Regex::new(r"^\s*Z([2-4])?\s*$").expect("full rest pattern"));
    pattern.captures(piece).map(|found| found.get(1).map_or(1, |count| count.as_str().parse().expect("a digit")))
}

/// A time in quarter notes as a whole number of L: units.
fn ticks(quarters: Q, per_quarter: Q) -> Result<i64, String> {
    let value = quarters * per_quarter;
    if !value.is_whole() {
        return Err("an event falls between the steps of L:".into());
    }
    Ok(value.num())
}

/// The key in force at `time` on a voice's key timeline.
fn key_at(keys: &[(Q, String)], time: Q) -> String {
    let mut current = keys[0].1.clone();
    for (start, key) in keys {
        if *start > time {
            break;
        }
        current = key.clone();
    }
    current
}

/// Where every bar of every part sits in the text.
#[derive(Clone, Debug)]
struct Piece {
    line: usize,
    place: usize,
    bars: Vec<usize>,
}

#[derive(Clone, Debug)]
struct Parsed {
    source: String,
    score: Score,
    lines: Vec<String>,
    pieces: [Vec<Piece>; 2],
    /// Per bar: the section's position among the comments and its name.
    sections: Vec<(i64, String)>,
    /// Bars that change key inside themselves.
    inline: BTreeSet<usize>,
}

fn parsed(text: &str) -> Result<Parsed, String> {
    let source = text.trim();
    if source.is_empty() {
        return Err("There is no score yet.".into());
    }
    if source.len() > LONGEST {
        return Err("That score is far longer than any song.".into());
    }
    let inner = || -> Result<Parsed, String> {
        let score = abc::parse(source)?;
        if score.unit.den() < 4 {
            return Err(format!("L:1/{} is coarser than a quarter note", score.unit.den()));
        }
        let (lines, pieces, sections, inline) = layout(source, &score);
        Ok(Parsed { source: source.to_string(), score, lines, pieces, sections, inline })
    };
    inner().map_err(|error| unreadable(&error))
}

type Layout = (Vec<String>, [Vec<Piece>; 2], Vec<(i64, String)>, BTreeSet<usize>);

fn layout(text: &str, score: &Score) -> Layout {
    let lines: Vec<String> = abc::split_keep(text).into_iter().map(str::to_string).collect();
    let mut pieces: [Vec<Piece>; 2] = [Vec::new(), Vec::new()];
    let mut counters = [0usize; 2];
    let mut sections = Vec::new();
    let mut inline = BTreeSet::new();
    let mut section: (i64, String) = (-1, String::new());
    for (index, raw) in lines.iter().enumerate() {
        let body = raw.trim_end_matches(['\r', '\n']);
        if let Some(name) = body.strip_prefix("% ") {
            section = (section.0 + 1, name.trim().to_string());
            continue;
        }
        let Some(&voice) = score.music_lines.get(&index) else { continue };
        for (place, piece) in body[..body.len() - 1].split('|').enumerate() {
            let count = full_rest(piece).unwrap_or(1);
            let bars: Vec<usize> = (counters[voice]..counters[voice] + count).collect();
            counters[voice] += count;
            if voice == VOCAL {
                sections.extend(std::iter::repeat_n(section.clone(), count));
            }
            if piece.contains("[K:") {
                inline.extend(bars.iter().copied());
            }
            pieces[voice].push(Piece { line: index, place, bars });
        }
    }
    (lines, pieces, sections, inline)
}

fn bar_count(body: &str) -> usize {
    body[..body.len() - 1].split('|').map(|piece| full_rest(piece).unwrap_or(1)).sum()
}

fn rest_text(bars: usize) -> String {
    if bars > 1 { format!("Z{bars}") } else { "Z".into() }
}

/// One music line as two, the second beginning `at` bars into it.
fn cut_music(body: &str, at: usize) -> (String, String) {
    let (mut head, mut tail, mut seen) = (Vec::new(), Vec::new(), 0);
    for piece in body[..body.len() - 1].split('|') {
        let bars = full_rest(piece).unwrap_or(1);
        if seen + bars <= at {
            head.push(piece.to_string());
        } else if seen >= at {
            tail.push(piece.to_string());
        } else {
            head.push(rest_text(at - seen));
            tail.push(rest_text(bars - (at - seen)));
        }
        seen += bars;
    }
    (head.join("|") + "|", tail.join("|") + "|")
}

#[derive(Clone, Debug)]
struct VoiceBlock {
    head: Vec<String>,
    music: String,
}

#[derive(Clone, Debug)]
struct Block {
    names: Vec<String>,
    voices: [VoiceBlock; 2],
}

/// The groups of a score that parsed: the names above each, each part's lines.
fn blocks(bodies: &[String]) -> Vec<Block> {
    let mut found = Vec::new();
    let mut cursor = HEADER_LINES;
    while cursor < bodies.len() {
        let mut names = Vec::new();
        while bodies[cursor].starts_with("% ") {
            names.push(bodies[cursor][2..].trim().to_string());
            cursor += 1;
        }
        let mut voices = Vec::new();
        for _ in VOICES {
            let mut head = vec![bodies[cursor].clone()];
            cursor += 1;
            while bodies[cursor].starts_with("M:") || bodies[cursor].starts_with("K:") {
                head.push(bodies[cursor].clone());
                cursor += 1;
            }
            voices.push(VoiceBlock { head, music: bodies[cursor].clone() });
            cursor += 1;
        }
        let ins = voices.pop().expect("two voices");
        let vocal = voices.pop().expect("two voices");
        found.push(Block { names, voices: [vocal, ins] });
    }
    found
}

/// A group cut in two on the same bar of both parts; a meter or key it carries
/// stays with its first half.
fn split_block(block: &Block, at: usize) -> [Block; 2] {
    let halves: Vec<(VoiceBlock, VoiceBlock)> = (0..2)
        .map(|voice| {
            let (head, tail) = cut_music(&block.voices[voice].music, at);
            (VoiceBlock { head: block.voices[voice].head.clone(), music: head }, VoiceBlock { head: vec![format!("V: {}", VOICES[voice])], music: tail })
        })
        .collect();
    [
        Block { names: block.names.clone(), voices: [halves[0].0.clone(), halves[1].0.clone()] },
        Block { names: Vec::new(), voices: [halves[0].1.clone(), halves[1].1.clone()] },
    ]
}

fn line_ending(lines: &[String]) -> String {
    let first = &lines[0];
    let ending = &first[first.trim_end_matches(['\r', '\n']).len()..];
    if ending.is_empty() { "\n".into() } else { ending.to_string() }
}

fn bodies_of(lines: &[String]) -> Vec<String> {
    lines.iter().map(|line| line.trim_end_matches(['\r', '\n']).to_string()).collect()
}

fn ends_line(text: &str) -> bool {
    text.ends_with('\n') || text.ends_with('\r')
}

/// `text` with its section comments replaced by `wanted`, `(bar, name)` each.
fn resection(text: &str, wanted: &[(usize, String)]) -> String {
    let lines: Vec<String> = abc::split_keep(text).into_iter().map(str::to_string).collect();
    let bodies = bodies_of(&lines);
    let ending = line_ending(&lines);
    let mut found = blocks(&bodies);
    let mut starts = Vec::new();
    let mut at = 0;
    for block in &found {
        starts.push(at);
        at += bar_count(&block.voices[VOCAL].music);
    }
    for (bar, _name) in wanted {
        if starts.contains(bar) {
            continue;
        }
        let index = starts.iter().enumerate().filter(|(_, start)| **start < *bar).map(|(place, _)| place).max().expect("a group before the bar");
        let halves = split_block(&found[index], bar - starts[index]);
        found.splice(index..=index, halves);
        starts.insert(index + 1, *bar);
    }
    let named: HashMap<usize, &String> = wanted.iter().map(|(bar, name)| (*bar, name)).collect();
    let mut out: Vec<String> = lines[..HEADER_LINES].to_vec();
    for (start, block) in starts.iter().zip(&found) {
        if let Some(name) = named.get(start) {
            out.push(format!("% {name}{ending}"));
        }
        for voice in &block.voices {
            out.extend(voice.head.iter().map(|line| format!("{line}{ending}")));
            out.push(format!("{}{ending}", voice.music));
        }
    }
    if !ends_line(lines.last().expect("lines")) {
        let last = out.last_mut().expect("lines out");
        *last = last.trim_end_matches(['\r', '\n']).to_string();
    }
    out.concat()
}

/// Bars as a music line, a run of whole-bar rests written as one Z of at most four.
fn squeezed(texts: &[String]) -> String {
    let mut out = Vec::new();
    let mut at = 0;
    while at < texts.len() {
        if texts[at] != "Z" {
            out.push(texts[at].clone());
            at += 1;
            continue;
        }
        let mut end = at;
        while end < texts.len() && texts[end] == "Z" && end - at < 4 {
            end += 1;
        }
        out.push(rest_text(end - at));
        at = end;
    }
    out.join("|") + "|"
}

/// One note as `(start, length, pitch)` in L: units.
pub type Tick = (i64, i64, i64);

/// A score written again on `L:1/unit`, finer than the one it came in.
fn refine(text: &str, unit: i64) -> Result<String, String> {
    let parsed = parsed(text)?;
    let sheet = read(&parsed.source)?;
    let scale = unit / sheet.unit;
    let per_quarter = Q::new(unit, 4);
    let lines = &parsed.lines;
    let bodies = bodies_of(lines);
    let ending = line_ending(lines);
    let grid: Vec<(i64, i64)> = sheet.bars.iter().map(|bar| (bar.start * scale, bar.length * scale)).collect();
    let keys: Vec<&String> = sheet.bars.iter().map(|bar| &bar.key).collect();
    let notes: [Vec<Tick>; 2] = [0, 1].map(|voice| {
        let mut list: Vec<Tick> = sheet.notes.part(voice).iter().map(|note| (note.start * scale, note.length * scale, note.pitch)).collect();
        list.sort();
        list
    });
    let chords: BTreeMap<i64, String> = sheet.chords.iter().map(|chord| (chord.start * scale, chord.name.clone())).collect();
    let mut out: Vec<String> = lines[..HEADER_LINES].to_vec();
    out[UNIT_LINE] = format!("L:1/{unit}{}", &lines[UNIT_LINE][bodies[UNIT_LINE].len()..]);
    let mut number = 0;
    for block in blocks(&bodies) {
        out.extend(block.names.iter().map(|name| format!("% {name}{ending}")));
        let first = number;
        for (voice, part) in block.voices.iter().enumerate() {
            number = first;
            out.extend(part.head.iter().map(|line| format!("{line}{ending}")));
            let mut written = Vec::new();
            for _ in 0..bar_count(&part.music) {
                let (start, length) = grid[number];
                let inside = inside(&notes[voice], start, length);
                let mut spelled = HashMap::new();
                for note in &inside {
                    spelled.insert(*note, spell(note.2 as i32, &key_at(&parsed.score.voices[voice].keys, Q::int(note.0) / per_quarter))?);
                }
                let in_bar: BTreeMap<i64, String> = if voice == VOCAL { chords.range(start..start + length).map(|(tick, name)| (*tick, name.clone())).collect() } else { BTreeMap::new() };
                written.push(bar_text(start, length, &inside, &in_bar, keys[number], &spelled)?);
                number += 1;
            }
            out.push(format!("{}{ending}", squeezed(&written)));
        }
    }
    if !ends_line(lines.last().expect("lines")) {
        let last = out.last_mut().expect("lines out");
        *last = last.trim_end_matches(['\r', '\n']).to_string();
    }
    Ok(out.concat())
}

/// Two parses of the same song, however each is written down.
fn same_music(before: &Score, after: &Score) -> Result<(), String> {
    for voice in [VOCAL, INS] {
        if before.voices[voice].notes != after.voices[voice].notes {
            return Err(format!("the {} part does not read back as the same notes", part_name(voice)));
        }
        if before.voices[voice].bars != after.voices[voice].bars {
            return Err(format!("the bars of the {} part moved", part_name(voice)));
        }
        if before.voices[voice].keys != after.voices[voice].keys {
            return Err(format!("a key change of the {} part moved", part_name(voice)));
        }
    }
    if before.voices[VOCAL].chords != after.voices[VOCAL].chords {
        return Err("the chords moved".into());
    }
    if before.bpm != after.bpm {
        return Err("the tempo changed".into());
    }
    Ok(())
}

fn whole(value: &Value) -> Option<i64> {
    if value.is_boolean() { None } else { value.as_i64() }
}

/// The note length an edit asks the score to be rewritten on, or None.
fn wanted_unit(sheet: &Value, current: i64) -> Result<Option<i64>, String> {
    let Some(asked) = sheet.get("unit").filter(|value| !value.is_null()) else { return Ok(None) };
    let Some(unit) = whole(asked) else { return Ok(None) };
    if unit <= current {
        return Ok(None);
    }
    if unit > FINEST || unit & (unit - 1) != 0 {
        return Err(format!("A score can be rewritten on a note length of 1/{FINEST} at the most, and only on a power of two; 1/{unit} is not one."));
    }
    Ok(Some(unit))
}

fn printable(name: &str) -> bool {
    name.chars().all(|c| c == ' ' || !(c.is_control() || c.is_whitespace() || matches!(c, '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2060}'..='\u{2064}' | '\u{feff}')))
}

/// The sections an edit asks for, checked as untrusted input.
fn wanted_sections(sheet: &Value, bars: usize) -> Result<Option<Vec<(usize, String)>>, String> {
    let Some(items) = sheet.get("sections").filter(|value| !value.is_null()) else { return Ok(None) };
    let Some(items) = items.as_array().filter(|items| items.len() <= bars) else {
        return Err("The sections must be a list, at most one for each bar.".into());
    };
    let mut wanted = Vec::new();
    let mut seen = BTreeSet::new();
    for item in items {
        let (Some(bar), Some(name)) = (item.get("bar").and_then(whole), item.get("name").and_then(Value::as_str)) else {
            return Err("Every section needs a whole-number bar and a name.".into());
        };
        let clean = name.split_whitespace().collect::<Vec<_>>().join(" ");
        if clean.is_empty() {
            continue;
        }
        if clean.chars().count() > SECTION_LONGEST || !printable(&clean) {
            let shown: String = name.chars().take(60).collect();
            return Err(format!("'{}' cannot be a section name: a name is a line of text of at most {SECTION_LONGEST} characters.", shown.trim()));
        }
        if bar < 0 || bar as usize >= bars {
            return Err(format!("A section starts at bar {}, and this song has {bars} bars.", bar + 1));
        }
        if !seen.insert(bar) {
            return Err(format!("Two sections start at bar {}, and every bar belongs to one section.", bar + 1));
        }
        wanted.push((bar as usize, clean));
    }
    wanted.sort();
    Ok(Some(wanted))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SheetBar {
    pub start: i64,
    pub length: i64,
    pub meter: String,
    pub key: String,
    pub section: String,
    pub editable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SheetSection {
    pub name: String,
    pub bar: usize,
    pub bars: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SheetNote {
    pub start: i64,
    pub length: i64,
    pub pitch: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SheetChord {
    pub start: i64,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[allow(non_snake_case)]
pub struct SheetNotes {
    pub Vocal: Vec<SheetNote>,
    pub Ins: Vec<SheetNote>,
}

impl SheetNotes {
    pub fn part(&self, voice: usize) -> &[SheetNote] {
        if voice == VOCAL { &self.Vocal } else { &self.Ins }
    }
}

/// The score as the piano roll draws it: times and lengths in whole L: units
/// from the start of the song, sounding notes with ties resolved.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Sheet {
    pub unit: i64,
    pub per_quarter: i64,
    pub bpm: u32,
    pub seconds: f64,
    pub total: i64,
    pub bars: Vec<SheetBar>,
    pub sections: Vec<SheetSection>,
    pub notes: SheetNotes,
    pub chords: Vec<SheetChord>,
    pub signatures: BTreeMap<String, i32>,
}

/// The score as the piano roll draws it, or why it cannot be.
pub fn read(text: &str) -> Result<Sheet, String> {
    let parsed = parsed(text)?;
    let score = &parsed.score;
    let per_quarter = Q::new(score.unit.den(), 4);
    let vocal = &score.voices[VOCAL];
    let at = |value: Q| ticks(value, per_quarter);
    let mut bars = Vec::new();
    for (number, bar) in vocal.bars.iter().enumerate() {
        bars.push(SheetBar {
            start: at(bar.start)?,
            length: at(bar.length)?,
            meter: format!("{}/{}", bar.meter.0, bar.meter.1),
            key: key_at(&vocal.keys, bar.start),
            section: parsed.sections[number].1.clone(),
            editable: !parsed.inline.contains(&number),
        });
    }
    let mut groups: Vec<(i64, SheetSection)> = Vec::new();
    for (number, (identity, name)) in parsed.sections.iter().enumerate() {
        match groups.last_mut() {
            Some((last, group)) if last == identity => group.bars += 1,
            _ => groups.push((*identity, SheetSection { name: name.clone(), bar: number, bars: 1 })),
        }
    }
    let notes_of = |voice: usize| -> Result<Vec<SheetNote>, String> { score.voices[voice].notes.iter().map(|note| Ok(SheetNote { start: at(note.start)?, length: at(note.duration)?, pitch: note.pitch as i64 })).collect() };
    let seconds = vocal.time.to_f64() * 60.0 / score.bpm as f64;
    Ok(Sheet {
        unit: score.unit.den(),
        per_quarter: per_quarter.num(),
        bpm: score.bpm,
        seconds: format!("{seconds:.2}").parse().expect("a number"),
        total: at(vocal.time)?,
        signatures: bars.iter().map(|bar| (bar.key.clone(), abc::key_fifths(&bar.key).expect("a key the reader took"))).collect(),
        bars,
        sections: groups.into_iter().map(|(_, group)| group).collect(),
        notes: SheetNotes { Vocal: notes_of(VOCAL)?, Ins: notes_of(INS)? },
        chords: vocal.chords.iter().map(|(start, name)| Ok(SheetChord { start: at(*start)?, name: name.clone() })).collect::<Result<_, String>>()?,
    })
}

/// The tempo the sheet asks for, as a whole quarter-note BPM.
fn tempo(sheet: &Value, current: i64) -> Result<i64, String> {
    let Some(asked) = sheet.get("bpm").filter(|value| !value.is_null()) else { return Ok(current) };
    let Some(number) = asked.as_f64().filter(|_| !asked.is_boolean()) else { return Err("the tempo must be a number".into()) };
    let value = number.round_ties_even() as i64;
    let (low, high) = (TEMPO_LOW.min(current), TEMPO_HIGH.max(current));
    if !(low..=high).contains(&value) {
        return Err(tempo_range(value, low, high));
    }
    Ok(value)
}

/// The notes and chords an edit asks for, checked as untrusted input.
fn wanted(sheet: &Value, bars: &[SheetBar], total: i64) -> Result<([Vec<Tick>; 2], Vec<(i64, String)>), String> {
    let Some(parts) = sheet.get("notes").and_then(Value::as_object) else {
        return Err("Send the edit as an object with 'notes' for each part.".into());
    };
    let bar_of = |tick: i64| -> usize { bars.iter().rposition(|bar| bar.start <= tick).unwrap_or(0) + 1 };
    let mut notes: [Vec<Tick>; 2] = [Vec::new(), Vec::new()];
    for voice in [VOCAL, INS] {
        let empty = Vec::new();
        let items = match parts.get(VOICES[voice]) {
            None => &empty,
            Some(value) => match value.as_array().filter(|items| items.len() <= MOST_NOTES) {
                Some(items) => items,
                None => return Err(format!("The {} part must be a list of at most {MOST_NOTES} notes.", part_name(voice))),
            },
        };
        let mut clean = Vec::new();
        for item in items {
            let (Some(start), Some(length), Some(pitch)) = (item.get("start").and_then(whole), item.get("length").and_then(whole), item.get("pitch").and_then(whole)) else {
                return Err("Every note needs a whole-number start, length and pitch.".into());
            };
            if start < 0 || length < 1 || start + length > total {
                return Err(format!("A {} note runs outside the song, which ends after bar {}.", part_name(voice), bars.len()));
            }
            if !(0..=127).contains(&pitch) {
                return Err(format!("A {} note has pitch {pitch}, and pitches go from 0 to 127.", part_name(voice)));
            }
            clean.push((start, length, pitch));
        }
        clean.sort();
        for pair in clean.windows(2) {
            if pair[1].0 < pair[0].0 + pair[0].1 {
                return Err(format!("Two {} notes overlap in bar {}. Each part sings one note at a time, so move or shorten one of them.", part_name(voice), bar_of(pair[1].0)));
            }
        }
        notes[voice] = clean;
    }
    let empty = Vec::new();
    let items = match sheet.get("chords") {
        None => &empty,
        Some(value) => match value.as_array().filter(|items| items.len() <= MOST_NOTES) {
            Some(items) => items,
            None => return Err("The chords must be a list.".into()),
        },
    };
    let mut chords = BTreeMap::new();
    for item in items {
        let (Some(start), Some(name)) = (item.get("start").and_then(whole), item.get("name").and_then(Value::as_str)) else {
            return Err("Every chord needs a whole-number start and a name.".into());
        };
        let name = name.trim();
        if !(0..total).contains(&start) {
            return Err("A chord sits outside the song.".into());
        }
        if !abc::chord_pattern().is_match(name) {
            return Err(format!("'{name}' is not a chord symbol the score format knows. It knows a root from A to G with an optional # or b, then one of: major (nothing), m, dim, aug, 7, maj7, m7, dim7, m7b5, sus4, sus2, 6, m6, 7sus4, m(maj7) -- and an optional bass note after a slash, as in C/E."));
        }
        if chords.insert(start, name.to_string()).is_some() {
            return Err(format!("Two chords start at the same moment in bar {}.", bar_of(start)));
        }
    }
    Ok((notes, chords.into_iter().collect()))
}

/// The notes of a sorted list that sound at some point of one bar.
fn inside(items: &[Tick], start: i64, length: i64) -> Vec<Tick> {
    let end = start + length;
    items.iter().filter(|note| note.0 < end && note.0 + note.1 > start).copied().collect()
}

/// The bars of one part that have to be written again, spread along ties.
fn dirty(grid: &[(i64, i64)], old: &[Tick], new: &[Tick], old_chords: &[(i64, String)], new_chords: &[(i64, String)]) -> BTreeSet<usize> {
    let mut dirty = BTreeSet::new();
    for (number, &(start, length)) in grid.iter().enumerate() {
        let chords_in = |chords: &[(i64, String)]| -> Vec<(i64, String)> { chords.iter().filter(|chord| start <= chord.0 && chord.0 < start + length).cloned().collect() };
        if inside(old, start, length) != inside(new, start, length) || chords_in(old_chords) != chords_in(new_chords) {
            dirty.insert(number);
        }
    }
    let mut waiting: Vec<usize> = dirty.iter().copied().collect();
    while let Some(number) = waiting.pop() {
        let (start, length) = grid[number];
        for items in [old, new] {
            for note in inside(items, start, length) {
                for neighbour in [number.wrapping_sub(1), number + 1] {
                    if neighbour < grid.len() && !dirty.contains(&neighbour) && !inside(&[note], grid[neighbour].0, grid[neighbour].1).is_empty() {
                        dirty.insert(neighbour);
                        waiting.push(neighbour);
                    }
                }
            }
        }
    }
    dirty
}

/// `(letter, alteration, written)` naming a sounding pitch from `key`: a name
/// the signature gives wins, then the fewest accidentals, then a minor key
/// raises and a major key leans the way its signature does.
fn spell(pitch: i32, key: &str) -> Result<(char, i32, i32), String> {
    let signature = abc::key_accidentals(key)?;
    let minor = key.ends_with('m');
    let flats = abc::key_fifths(key).expect("a key the reader took") < 0;
    let mut best: Option<((bool, i32, bool), char, i32, i32)> = None;
    for letter in abc::LETTERS {
        let given = signature[abc::letter_index(letter)];
        for alteration in [-2, -1, 0, 1, 2] {
            let written = pitch - alteration;
            if (written - abc::natural(letter)).rem_euclid(12) != 0 {
                continue;
            }
            let leaning = if minor { alteration < given } else if flats { alteration > 0 } else { alteration < 0 };
            let cost = (alteration != given, alteration.abs(), leaning);
            if best.as_ref().is_none_or(|found| cost < found.0) {
                best = Some((cost, letter, alteration, written));
            }
        }
    }
    let (_, letter, alteration, written) = best.expect("every pitch has a name");
    Ok((letter, alteration, written))
}

/// A natural note at MIDI pitch `written`: C is 60, c 72, c' 84, C, 48.
fn pitch_text(letter: char, written: i32) -> String {
    let octave = (written - abc::natural(letter)).div_euclid(12);
    if octave >= 6 {
        format!("{}{}", letter.to_ascii_lowercase(), "'".repeat((octave - 6) as usize))
    } else {
        format!("{letter}{}", ",".repeat((5 - octave) as usize))
    }
}

/// A length in L: units as the native lengths that add up to it, longest first.
fn lengths(mut units: i64) -> Vec<i64> {
    let mut out = Vec::new();
    while units > 0 {
        let take = *abc::DURATIONS.iter().rev().find(|length| **length <= units).expect("a length of one");
        out.push(take);
        units -= take;
    }
    out
}

fn digits(units: i64) -> String {
    if units == 1 { String::new() } else { units.to_string() }
}

/// One bar of one part, written from its sounding notes and its chords.
fn bar_text(start: i64, length: i64, notes: &[Tick], chords: &BTreeMap<i64, String>, key: &str, spelled: &HashMap<Tick, (char, i32, i32)>) -> Result<String, String> {
    let end = start + length;
    if notes.is_empty() && chords.is_empty() {
        return Ok("Z".into());
    }
    let mut edges = BTreeSet::from([start, end]);
    for note in notes {
        edges.insert(note.0.max(start));
        edges.insert((note.0 + note.1).min(end));
    }
    edges.extend(chords.keys().copied());
    let edges: Vec<i64> = edges.into_iter().filter(|edge| (start..=end).contains(edge)).collect();
    let signature = abc::key_accidentals(key)?;
    let mut state = signature;
    let mut marked: HashMap<char, BTreeSet<i32>> = HashMap::new();
    let mut out = String::new();
    for pair in edges.windows(2) {
        let (left, right) = (pair[0], pair[1]);
        if let Some(chord) = chords.get(&left) {
            out.push('"');
            out.push_str(chord);
            out.push('"');
        }
        let note = notes.iter().find(|note| note.0 <= left && left < note.0 + note.1);
        let pieces = lengths(right - left);
        let Some(note) = note else {
            for units in pieces {
                out.push('z');
                out.push_str(&digits(units));
            }
            continue;
        };
        let (letter, alteration, written) = spelled[note];
        let index = abc::letter_index(letter);
        let octave = (written - abc::natural(letter)).div_euclid(12);
        let onward = note.0 + note.1 > right;
        for (place, units) in pieces.iter().enumerate() {
            let mut sign = "";
            if left == note.0 && place == 0 {
                let altered = alteration != signature[index];
                let unmarked_here = !marked.get(&letter).is_some_and(|octaves| octaves.contains(&octave));
                if state[index] != alteration || (altered && unmarked_here) {
                    if state[index] != alteration {
                        marked.insert(letter, BTreeSet::from([octave]));
                    } else {
                        marked.entry(letter).or_default().insert(octave);
                    }
                    state[index] = alteration;
                    sign = mark(alteration);
                }
            }
            let tied = place < pieces.len() - 1 || onward;
            out.push_str(sign);
            out.push_str(&pitch_text(letter, written));
            out.push_str(&digits(*units));
            if tied {
                out.push('-');
            }
        }
    }
    Ok(out)
}

/// The written score read back: the notes, chords and tempo asked for, on the old grid.
fn check(text: &str, notes: &[Vec<Tick>; 2], chords: &[(i64, String)], score: &Score, per_quarter: Q, bpm: i64) -> Result<(), String> {
    let result = abc::parse(text)?;
    let at = |value: Q| ticks(value, per_quarter);
    for voice in [VOCAL, INS] {
        let mut got = Vec::new();
        for note in &result.voices[voice].notes {
            got.push((at(note.start)?, at(note.duration)?, note.pitch as i64));
        }
        got.sort();
        if got != notes[voice] {
            return Err(format!("the {} part does not read back as the notes asked for", part_name(voice)));
        }
        if result.voices[voice].bars != score.voices[voice].bars {
            return Err(format!("the bars of the {} part moved", part_name(voice)));
        }
        if result.voices[voice].keys != score.voices[voice].keys {
            return Err(format!("a key change of the {} part moved", part_name(voice)));
        }
    }
    let mut got = Vec::new();
    for (start, name) in &result.voices[VOCAL].chords {
        got.push((at(*start)?, name.clone()));
    }
    if got != chords {
        return Err("the chords do not read back as the chords asked for".into());
    }
    if result.bpm as i64 != bpm || result.unit != score.unit {
        return Err("the tempo or the note length is not the one asked for".into());
    }
    Ok(())
}

fn checked(text: &str, notes: &[Vec<Tick>; 2], chords: &[(i64, String)], score: &Score, per_quarter: Q, bpm: i64) -> Result<(), String> {
    check(text, notes, chords, score, per_quarter, bpm).map_err(|error| not_written(&error))
}

/// What `write` hands back: the new text, and the bars written again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Written {
    pub abc: String,
    pub bars: Vec<usize>,
}

/// `text` with the notes of `sheet` in it. `sheet` is what `read` gave,
/// edited: notes per part, chords, a tempo, sections, a finer unit. An edit
/// that changes nothing hands the text back untouched.
pub fn write(text: &str, sheet: &Value) -> Result<Written, String> {
    let mut work = parsed(text)?;
    let as_it_came = work.source.clone();
    let mut source = work.source.clone();
    let mut score = work.score.clone();
    if let Some(unit) = wanted_unit(sheet, score.unit.den())? {
        let came_as = score.clone();
        source = refine(&source, unit)?;
        work = parsed(&source)?;
        score = work.score.clone();
        same_music(&came_as, &score).map_err(|error| not_written(&error))?;
    }
    let per_quarter = Q::new(score.unit.den(), 4);
    let base = read(&source)?;
    if let Some(wanted) = wanted_sections(sheet, base.bars.len())? {
        let current: Vec<(usize, String)> = base.sections.iter().filter(|group| !group.name.is_empty()).map(|group| (group.bar, group.name.clone())).collect();
        if wanted != current {
            source = resection(&source, &wanted);
            work = parsed(&source)?;
        }
    }
    let mut lines = work.lines.clone();
    let bpm = tempo(sheet, score.bpm as i64)?;
    if bpm != score.bpm as i64 {
        let raw = &lines[TEMPO_LINE];
        let body_length = raw.trim_end_matches(['\r', '\n']).len();
        lines[TEMPO_LINE] = format!("Q:1/4={bpm}{}", &raw[body_length..]);
    }
    let grid: Vec<(i64, i64)> = base.bars.iter().map(|bar| (bar.start, bar.length)).collect();
    let (notes, chords) = wanted(sheet, &base.bars, base.total)?;
    let old: [Vec<Tick>; 2] = [VOCAL, INS].map(|voice| {
        let mut list: Vec<Tick> = base.notes.part(voice).iter().map(|note| (note.start, note.length, note.pitch)).collect();
        list.sort();
        list
    });
    let old_chords: Vec<(i64, String)> = base.chords.iter().map(|chord| (chord.start, chord.name.clone())).collect();
    let marked = [dirty(&grid, &old[VOCAL], &notes[VOCAL], &old_chords, &chords), dirty(&grid, &old[INS], &notes[INS], &[], &[])];
    let touched: Vec<usize> = marked[VOCAL].union(&marked[INS]).copied().collect();
    if touched.is_empty() && bpm == score.bpm as i64 {
        if source != as_it_came {
            checked(&source, &notes, &chords, &score, per_quarter, bpm)?;
        }
        return Ok(Written { abc: source, bars: Vec::new() });
    }
    if let Some(locked) = touched.iter().find(|number| work.inline.contains(number)) {
        return Err(format!("Bar {} changes key halfway through, and the piano roll leaves such bars as they are. Edit that bar in the ABC tab.", locked + 1));
    }
    let chord_at: BTreeMap<i64, String> = chords.iter().cloned().collect();
    let mut replaced: BTreeMap<usize, BTreeMap<usize, String>> = BTreeMap::new();
    for voice in [VOCAL, INS] {
        let mut spelled = HashMap::new();
        for note in &notes[voice] {
            spelled.insert(*note, spell(note.2 as i32, &key_at(&score.voices[voice].keys, Q::int(note.0) / per_quarter))?);
        }
        for piece in &work.pieces[voice] {
            if !piece.bars.iter().any(|bar| marked[voice].contains(bar)) {
                continue;
            }
            let mut written = Vec::new();
            for &number in &piece.bars {
                if !marked[voice].contains(&number) {
                    written.push("Z".to_string());
                    continue;
                }
                let (start, length) = grid[number];
                let in_bar: BTreeMap<i64, String> = if voice == VOCAL { chord_at.range(start..start + length).map(|(tick, name)| (*tick, name.clone())).collect() } else { BTreeMap::new() };
                written.push(bar_text(start, length, &inside(&notes[voice], start, length), &in_bar, &base.bars[number].key, &spelled)?);
            }
            replaced.entry(piece.line).or_default().insert(piece.place, written.join("|"));
        }
    }
    for (index, places) in replaced {
        let raw = lines[index].clone();
        let body = raw.trim_end_matches(['\r', '\n']);
        let mut parts: Vec<String> = body[..body.len() - 1].split('|').map(str::to_string).collect();
        for (place, piece) in places {
            parts[place] = piece;
        }
        lines[index] = format!("{}|{}", parts.join("|"), &raw[body.len()..]);
    }
    let result = lines.concat();
    checked(&result, &notes, &chords, &score, per_quarter, bpm)?;
    Ok(Written { abc: result, bars: touched })
}

/// The whole groups of a score whose writing may have stopped halfway through.
fn written_blocks(bodies: &[String]) -> Vec<Block> {
    let mut found = Vec::new();
    let mut cursor = HEADER_LINES;
    while cursor < bodies.len() {
        let mut names = Vec::new();
        while cursor < bodies.len() && bodies[cursor].starts_with("% ") {
            names.push(bodies[cursor][2..].trim().to_string());
            cursor += 1;
        }
        let mut voices = Vec::new();
        for name in VOICES {
            let names_voice = cursor < bodies.len() && bodies[cursor].starts_with("V:") && bodies[cursor].split_once(':').map_or(bodies[cursor].as_str(), |(_, rest)| rest).split_whitespace().next() == Some(name);
            if !names_voice {
                return found;
            }
            let mut head = vec![bodies[cursor].clone()];
            cursor += 1;
            while cursor < bodies.len() && (bodies[cursor].starts_with("M:") || bodies[cursor].starts_with("K:")) {
                head.push(bodies[cursor].clone());
                cursor += 1;
            }
            if cursor >= bodies.len() || !bodies[cursor].trim_end().ends_with('|') {
                return found;
            }
            voices.push(VoiceBlock { head, music: bodies[cursor].trim_end().to_string() });
            cursor += 1;
        }
        let ins = voices.pop().expect("two voices");
        let vocal = voices.pop().expect("two voices");
        found.push(Block { names, voices: [vocal, ins] });
    }
    found
}

/// A score up to its last whole group when the rest of it cannot be read, as
/// when the model ran out of tokens inside a line; None otherwise.
pub fn whole_groups(text: &str) -> Option<String> {
    if parsed(text).is_ok() {
        return None;
    }
    let lines = abc::split_lines(text.trim());
    let bodies: Vec<String> = lines.iter().map(|line| line.trim_end().to_string()).collect();
    let found = written_blocks(&bodies);
    if found.is_empty() {
        return None;
    }
    let used = HEADER_LINES + found.iter().map(|block| block.names.len() + block.voices.iter().map(|voice| voice.head.len() + 1).sum::<usize>()).sum::<usize>();
    if used >= lines.len() {
        return None;
    }
    let kept = lines[..used].join("\n");
    parsed(&kept).ok()?;
    Some(kept)
}
