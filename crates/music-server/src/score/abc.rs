//! The limited two-voice ABC dialect YuE2 and SheetSage2 write, read into
//! sounding notes. Not a general ABC reader: whatever the dialect does not
//! hold is refused, so a score that reads is one the model reads the same way.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

use regex::Regex;

use super::Q;

pub const VOICES: [&str; 2] = ["Vocal", "Ins"];
pub const VOCAL: usize = 0;
pub const INS: usize = 1;

/// The lengths one token may have, in L: units.
pub const DURATIONS: [i64; 11] = [1, 2, 3, 4, 6, 8, 12, 16, 24, 32, 48];

pub const QUALITIES: [&str; 15] = ["", "m", "dim", "aug", "7", "maj7", "m7", "dim7", "m7b5", "sus4", "sus2", "6", "m6", "7sus4", "m(maj7)"];

pub const LETTERS: [char; 7] = ['C', 'D', 'E', 'F', 'G', 'A', 'B'];

pub const VOICE_LINES: [&str; 2] = ["V: Vocal clef=treble name=\"Vocal Melody\" snm=\"Vocal\"", "V: Ins clef=treble name=\"Ins Melody\" snm=\"Inst.\""];

const MAJOR_KEYS: [&str; 15] = ["Cb", "Gb", "Db", "Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E", "B", "F#", "C#"];
const MINOR_KEYS: [&str; 15] = ["Abm", "Ebm", "Bbm", "Fm", "Cm", "Gm", "Dm", "Am", "Em", "Bm", "F#m", "C#m", "G#m", "D#m", "A#m"];

/// The pitch class of a natural letter.
pub fn natural(letter: char) -> i32 {
    match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => panic!("{letter} is not a note letter"),
    }
}

pub fn letter_index(letter: char) -> usize {
    LETTERS.iter().position(|each| *each == letter).expect("a note letter")
}

/// The sharps (positive) or flats (negative) of a key the dialect knows.
pub fn key_fifths(key: &str) -> Option<i32> {
    MAJOR_KEYS.iter().chain(MINOR_KEYS.iter()).position(|each| *each == key).map(|index| (index % 15) as i32 - 7)
}

/// Every key the dialect knows, major then minor, with its signature.
pub fn keys() -> impl Iterator<Item = (&'static str, i32)> {
    MAJOR_KEYS.iter().chain(MINOR_KEYS.iter()).enumerate().map(|(index, key)| (*key, (index % 15) as i32 - 7))
}

/// The alteration the signature of `key` gives each letter, C to B.
pub fn key_accidentals(key: &str) -> Result<[i32; 7], String> {
    let Some(count) = key_fifths(key) else {
        return Err(format!("Unsupported key {}; use a standard major or minor K: field", repr(key)));
    };
    let mut result = [0; 7];
    let order: &[char] = if count > 0 { &['F', 'C', 'G', 'D', 'A', 'E', 'B'] } else { &['B', 'E', 'A', 'D', 'G', 'C', 'F'] };
    for letter in &order[..count.unsigned_abs() as usize] {
        result[letter_index(*letter)] = if count > 0 { 1 } else { -1 };
    }
    Ok(result)
}

pub fn meter_value(text: &str) -> Result<(u32, u32), String> {
    static METER: OnceLock<Regex> = OnceLock::new();
    let pattern = METER.get_or_init(|| Regex::new(r"^([1-9][0-9]*)/([1-9][0-9]*)$").expect("meter pattern"));
    let Some(found) = pattern.captures(text) else {
        return Err(format!("Unsupported meter {}; write an explicit fraction", repr(text)));
    };
    let numerator: u32 = found[1].parse().map_err(|_| format!("Unsupported meter {}; write an explicit fraction", repr(text)))?;
    let denominator: u32 = found[2].parse().map_err(|_| format!("Unsupported meter {}; write an explicit fraction", repr(text)))?;
    if denominator > 1024 || !denominator.is_power_of_two() {
        return Err(format!("Unsupported meter denominator {denominator}"));
    }
    Ok((numerator, denominator))
}

/// A chord symbol the dialect writes: a root, one of its qualities and an
/// optional bass after a slash.
pub fn chord_pattern() -> &'static Regex {
    static CHORD: OnceLock<Regex> = OnceLock::new();
    CHORD.get_or_init(|| {
        let qualities: Vec<String> = QUALITIES.iter().map(|quality| regex::escape(quality)).collect();
        let pitch = "[A-G](?:bb|##|b|#)?";
        Regex::new(&format!("^{pitch}(?:{})(?:/{pitch})?$", qualities.join("|"))).expect("chord pattern")
    })
}

fn token_pattern() -> &'static Regex {
    static TOKEN: OnceLock<Regex> = OnceLock::new();
    TOKEN.get_or_init(|| {
        Regex::new(r#"^(?:"(?P<chord>[^"\n]*)"|\[K:(?P<key>[^\]\n]+)\]|(?P<acc>\^\^|__|\^|_|=)?(?P<note>[A-Ga-gz])(?P<oct>[,']*)(?P<duration>[0-9]*)(?P<tie>-?))"#).expect("token pattern")
    })
}

/// A string as Python's repr writes it, which the dialect's messages quote.
pub fn repr(text: &str) -> String {
    if text.contains('\'') && !text.contains('"') {
        format!("\"{text}\"")
    } else {
        format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
    }
}

/// Lines as Python's `str.splitlines` cuts them, without their endings.
pub fn split_lines(text: &str) -> Vec<&str> {
    split_keep(text).into_iter().map(|line| line.trim_end_matches(['\r', '\n'])).collect()
}

/// Lines as Python's `str.splitlines(keepends=True)` cuts them.
pub fn split_keep(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\n' => {
                out.push(&text[start..=index]);
                index += 1;
                start = index;
            }
            b'\r' => {
                let end = if bytes.get(index + 1) == Some(&b'\n') { index + 2 } else { index + 1 };
                out.push(&text[start..end]);
                index = end;
                start = end;
            }
            _ => index += 1,
        }
    }
    if start < bytes.len() {
        out.push(&text[start..]);
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub start: Q,
    pub pitch: i32,
    pub duration: Q,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bar {
    pub start: Q,
    pub length: Q,
    pub meter: (u32, u32),
}

#[derive(Clone, Debug)]
pub struct Voice {
    pub meter: (u32, u32),
    pub key: String,
    pub time: Q,
    pub notes: Vec<Note>,
    pub bars: Vec<Bar>,
    pub chords: Vec<(Q, String)>,
    pub keys: Vec<(Q, String)>,
    pending: Option<(i32, i32)>,
}

impl Voice {
    fn new(meter: (u32, u32), key: &str) -> Voice {
        Voice { meter, key: key.to_string(), time: Q::ZERO, notes: Vec::new(), bars: Vec::new(), chords: Vec::new(), keys: vec![(Q::ZERO, key.to_string())], pending: None }
    }
}

#[derive(Clone, Debug)]
pub struct Score {
    pub unit: Q,
    pub bpm: u32,
    pub voices: [Voice; 2],
    /// The line index of every music line, and the voice it belongs to.
    pub music_lines: BTreeMap<usize, usize>,
}

fn fail(condition: bool, message: impl FnOnce() -> String) -> Result<(), String> {
    if condition {
        Err(message())
    } else {
        Ok(())
    }
}

fn parse_bar(body: &str, voice: &mut Voice, unit: Q, context: &str) -> Result<(), String> {
    let (numerator, denominator) = voice.meter;
    let length = Q::new(4 * numerator as i64, denominator as i64);
    let start = voice.time;
    let mut offset = Q::ZERO;
    // the dialect carries an accidental to every octave of its letter
    let mut local: HashMap<char, i32> = HashMap::new();
    if body == "Z" {
        fail(voice.pending.is_some(), || format!("{context}: tie enters a full-measure rest"))?;
        offset = length;
    } else {
        let mut cursor = 0;
        while cursor < body.len() {
            let rest = &body[cursor..];
            let first = rest.chars().next().expect("text left");
            if first.is_whitespace() {
                cursor += first.len_utf8();
                continue;
            }
            let Some(found) = token_pattern().captures(rest) else {
                let shown: String = rest.chars().take(24).collect();
                return Err(format!("{context}: unsupported token at {}", repr(&shown)));
            };
            cursor += found.get(0).expect("the whole match").end();
            fail(offset >= length, || format!("{context}: event after the measure end"))?;
            if let Some(chord) = found.name("chord") {
                let chord = chord.as_str();
                fail(!chord_pattern().is_match(chord), || format!("{context}: unsupported chord {}", repr(chord)))?;
                voice.chords.push((start + offset, chord.to_string()));
                continue;
            }
            if let Some(key) = found.name("key") {
                let key = key.as_str();
                key_accidentals(key)?;
                voice.key = key.to_string();
                voice.keys.push((start + offset, key.to_string()));
                local.clear();
                continue;
            }
            let note = found.name("note").expect("a note or a rest").as_str().chars().next().expect("one letter");
            let accidental = found.name("acc").map(|m| m.as_str()).unwrap_or("");
            let octave = found.name("oct").map(|m| m.as_str()).unwrap_or("");
            let tie = found.name("tie").is_some_and(|m| !m.as_str().is_empty());
            let digits = found.name("duration").map(|m| m.as_str()).unwrap_or("");
            let units: i64 = if digits.is_empty() { 1 } else { digits.parse().map_err(|_| format!("{context}: unsupported duration {digits}; split it into tied supported lengths"))? };
            fail(!DURATIONS.contains(&units), || format!("{context}: unsupported duration {units}; split it into tied supported lengths"))?;
            let duration = Q::int(units) * unit * Q::int(4);
            fail(offset + duration > length, || format!("{context}: note/rest exceeds meter duration"))?;
            fail(octave.contains(',') && octave.contains('\''), || format!("{context}: mixed octave marks"))?;
            if note == 'z' {
                fail(!accidental.is_empty() || !octave.is_empty() || tie, || format!("{context}: a rest cannot have accidentals, octave marks or ties"))?;
                fail(voice.pending.is_some(), || format!("{context}: tie enters a rest"))?;
            } else {
                let letter = note.to_ascii_uppercase();
                let mut written = 60 + natural(letter) + if note.is_ascii_lowercase() { 12 } else { 0 };
                written += 12 * (octave.matches('\'').count() as i32 - octave.matches(',').count() as i32);
                let mut alteration = match local.get(&letter) {
                    Some(value) => *value,
                    None => key_accidentals(&voice.key)?[letter_index(letter)],
                };
                if !accidental.is_empty() {
                    alteration = match accidental {
                        "=" => 0,
                        "_" => -1,
                        "__" => -2,
                        "^" => 1,
                        _ => 2,
                    };
                    local.insert(letter, alteration);
                }
                let mut pitch = written + alteration;
                if let Some((old_pitch, old_written)) = voice.pending {
                    // an unmarked continuation keeps its tied accidental across a
                    // barline; it does not alter later untied notes in that bar
                    if accidental.is_empty() && written == old_written {
                        pitch = old_pitch;
                    }
                    fail(pitch != old_pitch, || format!("{context}: tie changes pitch from {old_pitch} to {pitch}"))?;
                    let last = voice.notes.last_mut().expect("a tie continues a note");
                    last.duration += duration;
                } else {
                    fail(!(0..=127).contains(&pitch), || format!("{context}: pitch {pitch} is outside MIDI range"))?;
                    voice.notes.push(Note { start: start + offset, pitch, duration });
                }
                voice.pending = if tie { Some((pitch, written)) } else { None };
            }
            offset += duration;
        }
    }
    fail(offset != length, || format!("{context}: duration {offset} quarter notes != meter duration {length}"))?;
    voice.bars.push(Bar { start, length, meter: voice.meter });
    voice.time += length;
    Ok(())
}

/// A score read into sounding notes, or the first thing in it the dialect
/// does not hold.
pub fn parse(text: &str) -> Result<Score, String> {
    let lines = split_lines(text);
    fail(lines.len() < 12, || "Incomplete native two-voice ABC".into())?;
    fail(lines[0] != "X:1" || lines[1] != "T:", || "Expected native X:1 and blank T: header".into())?;
    fail(!lines[2].starts_with("M:"), || "Missing header M:".into())?;
    let meter = meter_value(&lines[2][2..])?;
    static UNIT: OnceLock<Regex> = OnceLock::new();
    let unit_found = UNIT.get_or_init(|| Regex::new(r"^L:1/([1-9][0-9]*)$").expect("unit pattern")).captures(lines[3]);
    let Some(unit_found) = unit_found else {
        return Err("Expected L:1/<power of two>, usually L:1/32".into());
    };
    let denominator: i64 = unit_found[1].parse().map_err(|_| "Unsupported L: denominator".to_string())?;
    fail(denominator > 1024 || !(denominator as u64).is_power_of_two(), || "Unsupported L: denominator".into())?;
    let unit = Q::new(1, denominator);
    static TEMPO: OnceLock<Regex> = OnceLock::new();
    let tempo_found = TEMPO.get_or_init(|| Regex::new(r"^Q:1/4=([1-9][0-9]*)$").expect("tempo pattern")).captures(lines[4]);
    let Some(tempo_found) = tempo_found else {
        return Err("Expected integer quarter-note tempo Q:1/4=<BPM>".into());
    };
    let bpm: u32 = tempo_found[1].parse().map_err(|_| "Expected integer quarter-note tempo Q:1/4=<BPM>".to_string())?;
    fail(lines[5] != VOICE_LINES[0] || lines[6] != VOICE_LINES[1], || "Preserve native Vocal and Ins voice definitions".into())?;
    fail(!lines[7].starts_with("K:"), || "Missing header K:".into())?;
    let key = &lines[7][2..];
    key_accidentals(key)?;
    let mut voices = [Voice::new(meter, key), Voice::new(meter, key)];
    let mut music_lines = BTreeMap::new();
    let mut cursor = 8;
    let mut group = 0;
    while cursor < lines.len() {
        while cursor < lines.len() && lines[cursor].starts_with("% ") {
            cursor += 1;
        }
        fail(cursor == lines.len(), || "Dangling section comment without music".into())?;
        group += 1;
        let mut counts = Vec::new();
        for (which, name) in VOICES.iter().enumerate() {
            let context = format!("group {group}, {name}");
            fail(cursor >= lines.len() || lines[cursor] != format!("V: {name}"), || format!("{context}: expected V: {name}"))?;
            cursor += 1;
            let voice = &mut voices[which];
            let mut fields = HashSet::new();
            while cursor < lines.len() && (lines[cursor].starts_with("M:") || lines[cursor].starts_with("K:")) {
                let (field, value) = lines[cursor].split_once(':').expect("a field line");
                fail(fields.contains(field), || format!("{context}: duplicate {field}: field"))?;
                fields.insert(field.to_string());
                if field == "M" {
                    voice.meter = meter_value(value)?;
                } else {
                    key_accidentals(value)?;
                    voice.key = value.to_string();
                    let time = voice.time;
                    voice.keys.push((time, value.to_string()));
                }
                cursor += 1;
            }
            fail(cursor >= lines.len(), || format!("{context}: missing music line"))?;
            let line = lines[cursor];
            fail(!line.ends_with('|'), || format!("{context}: music line must end with a plain barline"))?;
            music_lines.insert(cursor, which);
            cursor += 1;
            let mut bars: Vec<&str> = Vec::new();
            for bar in line[..line.len() - 1].split('|') {
                let bar = bar.trim();
                fail(bar.is_empty(), || format!("{context}: empty measure or unsupported double/repeat barline"))?;
                match whole_rest_bars(bar) {
                    Some(count) => bars.extend(std::iter::repeat_n("Z", count)),
                    None => bars.push(bar),
                }
            }
            fail(!(1..=4).contains(&bars.len()), || format!("{context}: expected 1\u{2013}4 measures after expanding Z rests"))?;
            counts.push(bars.len());
            for bar in bars {
                let context = format!("{context}, bar {}", voice.bars.len() + 1);
                parse_bar(bar, voice, unit, &context)?;
            }
        }
        fail(counts[0] != counts[1], || format!("group {group}: voices have different measure counts"))?;
    }
    for (which, voice) in voices.iter().enumerate() {
        fail(voice.pending.is_some(), || format!("{}: unresolved tie at end of score", VOICES[which]))?;
    }
    fail(!voices[INS].chords.is_empty(), || "Native chord symbols belong in Vocal, not Ins".into())?;
    fail(voices[VOCAL].bars != voices[INS].bars, || "Voice meter/time grids differ".into())?;
    fail(voices[VOCAL].keys != voices[INS].keys, || "Voice key-change timelines differ".into())?;
    Ok(Score { unit, bpm, voices, music_lines })
}

/// How many bars a whole-bar rest `Z` to `Z4` stands for, or None for a bar
/// with music in it.
pub fn whole_rest_bars(bar: &str) -> Option<usize> {
    match bar {
        "Z" => Some(1),
        "Z2" => Some(2),
        "Z3" => Some(3),
        "Z4" => Some(4),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "X:1\nT:\nM:4/4\nL:1/32\nQ:1/4=90\nV: Vocal clef=treble name=\"Vocal Melody\" snm=\"Vocal\"\nV: Ins clef=treble name=\"Ins Melody\" snm=\"Inst.\"\nK:G\n";

    #[test]
    fn a_small_score_reads_into_sounding_notes() {
        let text = format!("{HEADER}% verse\nV: Vocal\n\"G\"g8f8e8d8|B32-|\nV: Ins\nZ2|\n% chorus\nV: Vocal\nB16z16|Z|\nV: Ins\nG32|Z|\n");
        let score = parse(&text).expect("the score reads");
        assert_eq!(score.bpm, 90);
        assert_eq!(score.unit, Q::new(1, 32));
        let vocal = &score.voices[VOCAL];
        // f is sharp in G; the tie holds B across the barline as one note
        assert_eq!(vocal.notes[1], Note { start: Q::int(1), pitch: 78, duration: Q::int(1) });
        assert_eq!(vocal.notes[4], Note { start: Q::int(4), pitch: 71, duration: Q::int(6) });
        assert_eq!(vocal.chords, vec![(Q::ZERO, "G".to_string())]);
        assert_eq!(vocal.bars.len(), 4);
        assert_eq!(score.voices[INS].notes, vec![Note { start: Q::int(8), pitch: 67, duration: Q::int(4) }]);
    }

    #[test]
    fn what_the_dialect_does_not_hold_is_refused() {
        let text = format!("{HEADER}V: Vocal\nB16z8|\nV: Ins\nZ|\n");
        let problem = parse(&text).expect_err("a short bar");
        assert!(problem.contains("duration 3 quarter notes != meter duration 4"), "{problem}");
        let text = format!("{HEADER}V: Vocal\n\"Xmaj\"B32|\nV: Ins\nZ|\n");
        assert!(parse(&text).expect_err("a chord it does not know").contains("unsupported chord"));
    }

    #[test]
    fn keys_carry_their_signatures() {
        assert_eq!(key_accidentals("D").expect("D"), [1, 0, 0, 1, 0, 0, 0]);
        assert_eq!(key_accidentals("Fm").expect("Fm"), [0, -1, -1, 0, 0, -1, -1]);
        assert!(key_accidentals("H").is_err());
        assert_eq!(split_keep("a\r\nb\rc\n"), vec!["a\r\n", "b\r", "c\n"]);
        assert_eq!(split_lines("a\n\nb\n"), vec!["a", "", "b"]);
    }
}
