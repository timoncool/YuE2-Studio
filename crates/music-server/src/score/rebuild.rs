//! Beats, chords, keys, sections and notes written out as two-voice ABC, the
//! way a score read from a MIDI file or laid out for its words is written.
//!
//! Bars come from the beats: a bar runs from one downbeat to the next, a span
//! before the first downbeat is a pickup and a span after the last one a
//! partial bar. Every beat is split into equal subbeats and everything else is
//! snapped to that grid by the midpoints between its points. Bars go out in
//! groups of at most four, and a group starts where the meter, the key or the
//! section changes, the section's name a comment above it: the layout of
//! YuE2's own scores. Chords go on the vocal voice only, a bar of rests is
//! `Z`, and notes are spelled from the key with an accidental only where the
//! bar's state changes. Ported from YuE2-ComfyUI's `sheetsage/abc_rebuild.py`.

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;

use super::abc;

const NO_CHORDS: [&str; 3] = ["N", "X", "?"];

/// Each chord quality label and the symbol the score writes for it.
pub const QUALITY_TEXT: [(&str, &str); 15] = [
    ("maj", ""),
    ("min", "m"),
    ("dim", "dim"),
    ("aug", "aug"),
    ("7", "7"),
    ("maj7", "maj7"),
    ("min7", "m7"),
    ("dim7", "dim7"),
    ("hdim7", "m7b5"),
    ("sus4", "sus4"),
    ("sus2", "sus2"),
    ("maj6", "6"),
    ("min6", "m6"),
    ("sus4(b7)", "7sus4"),
    ("minmaj7", "m(maj7)"),
];
pub const SHARP_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
pub const FLAT_NAMES: [&str; 12] = ["C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B"];

/// The spelling of each pitch class in a key with that many sharps (positive) or flats, from seven flats up.
const PITCH_NAMES: [[&str; 12]; 15] = [
    ["C", "Db", "D", "Eb", "Fb", "F", "Gb", "G", "Ab", "Bbb", "Bb", "Cb"],
    ["C", "Db", "D", "Eb", "Fb", "F", "Gb", "G", "Ab", "A", "Bb", "Cb"],
    ["C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "Cb"],
    ["C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B"],
    ["C", "Db", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"],
    ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"],
    ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "G#", "A", "Bb", "B"],
    ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "Bb", "B"],
    ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"],
    ["C", "C#", "D", "D#", "E", "E#", "F#", "G", "G#", "A", "A#", "B"],
    ["B#", "C#", "D", "D#", "E", "E#", "F#", "G", "G#", "A", "A#", "B"],
    ["B#", "C#", "D", "D#", "E", "E#", "F#", "F##", "G#", "A", "A#", "B"],
    ["B#", "C#", "C##", "D#", "E", "E#", "F#", "F##", "G#", "A", "A#", "B"],
    ["B#", "C#", "C##", "D#", "E", "E#", "F#", "F##", "G#", "G##", "A#", "B"],
    ["B#", "C#", "C##", "D#", "D##", "E#", "F#", "F##", "G#", "G##", "A#", "B"],
];

/// What a score is written from: times in seconds, beats as `(time, number, numerator, denominator)`,
/// notes as `(start, end, pitch, voice)` with the vocal line voice 0.
#[derive(Clone, Debug, Default)]
pub struct Rows {
    pub beats: Vec<(f64, i64, i64, i64)>,
    pub chords: Vec<(f64, f64, String)>,
    pub keys: Vec<(f64, f64, String)>,
    pub structures: Vec<(f64, f64, String)>,
    pub notes: Vec<(f64, f64, i32, usize)>,
}

fn root_parts(root: &str) -> Result<(i32, char, &str), String> {
    let mut characters = root.chars();
    let letter = characters.next().filter(|letter| ('A'..='G').contains(letter));
    let accidental = characters.as_str();
    let valid = matches!(accidental, "" | "#" | "##" | "b" | "bb");
    match letter {
        Some(letter) if valid => {
            let shift = accidental.matches('#').count() as i32 - accidental.matches('b').count() as i32;
            Ok(((abc::natural(letter) + shift).rem_euclid(12), letter, accidental))
        }
        _ => Err(format!("Invalid pitch spelling {}", abc::repr(root))),
    }
}

/// A root spelled with at most one accidental, unless asked to keep a double.
fn portable_name(root: &str, keep_double: bool) -> Result<String, String> {
    let (pitch_class, _letter, accidental) = root_parts(root)?;
    if keep_double || accidental.chars().count() <= 1 {
        return Ok(root.to_string());
    }
    let names = if accidental.starts_with('#') { SHARP_NAMES } else { FLAT_NAMES };
    Ok(names[pitch_class as usize].to_string())
}

fn bass_pitch(root: &str, degree_text: &str) -> Result<String, String> {
    if root_parts(degree_text).is_ok() {
        return portable_name(degree_text, true);
    }
    let digits = degree_text.trim_start_matches(['#', 'b']);
    let degree_accidental = &degree_text[..degree_text.len() - digits.len()];
    let degree: Option<i32> = digits.parse().ok().filter(|value| (1..=13).contains(value) && !digits.starts_with('0') && !digits.starts_with('+'));
    let accidental_valid = matches!(degree_accidental, "" | "#" | "##" | "b" | "bb");
    let Some(degree) = degree.filter(|_| accidental_valid) else {
        return Err(format!("Invalid chord bass degree {}", abc::repr(degree_text)));
    };
    let (root_pc, root_letter, root_accidental) = root_parts(root)?;
    let mut interval = [0, 2, 4, 5, 7, 9, 11][((degree - 1) % 7) as usize] + 12 * ((degree - 1) / 7);
    interval += degree_accidental.matches('#').count() as i32 - degree_accidental.matches('b').count() as i32;
    let target = (root_pc + interval).rem_euclid(12);
    let letter = abc::LETTERS[(abc::letter_index(root_letter) + degree as usize - 1) % 7];
    let difference = (target - abc::natural(letter) + 6).rem_euclid(12) - 6;
    if (-2..=2).contains(&difference) {
        let accidental = ["bb", "b", "", "#", "##"][(difference + 2) as usize];
        return Ok(format!("{letter}{accidental}"));
    }
    let names = if format!("{root_accidental}{degree_accidental}").contains('#') { SHARP_NAMES } else { FLAT_NAMES };
    Ok(names[target as usize].to_string())
}

/// A chord label such as `A:min7/b3` as an ABC chord symbol, or None for no chord.
pub fn chord_text(chord: &str) -> Result<Option<String>, String> {
    let chord = chord.trim();
    if NO_CHORDS.contains(&chord) {
        return Ok(None);
    }
    let Some((root, descriptor)) = chord.split_once(':') else {
        return Err(format!("Chord {} is missing the ':' quality separator", abc::repr(chord)));
    };
    let (quality, bass) = match descriptor.split_once('/') {
        Some((quality, bass)) => (quality, Some(bass)),
        None => (descriptor, None),
    };
    let Some((_, text)) = QUALITY_TEXT.iter().find(|(label, _)| *label == quality) else {
        return Err(format!("Unsupported chord quality {} in {}", abc::repr(quality), abc::repr(chord)));
    };
    let mut symbol = portable_name(root, true)? + text;
    if let Some(bass) = bass.filter(|bass| !bass.is_empty()) {
        symbol.push('/');
        symbol.push_str(&bass_pitch(root, bass)?);
    }
    Ok(Some(symbol))
}

/// A key label such as `A#:major` as an ABC key with a standard signature.
pub fn key_text(key: &str) -> Result<String, String> {
    let key = key.trim();
    let (root, minor) = if let Some((root, mode)) = key.split_once(':') {
        match mode {
            "major" => (root, false),
            "minor" => (root, true),
            _ => return Err(format!("Unsupported key mode {} in {}", abc::repr(mode), abc::repr(key))),
        }
    } else if let Some(root) = key.strip_suffix('m') {
        (root, true)
    } else {
        (key, false)
    };
    let (root_pc, _letter, accidental) = root_parts(root)?;
    let suffix = if minor { "m" } else { "" };
    let candidate = portable_name(root, false)? + suffix;
    if abc::key_fifths(&candidate).is_some() {
        return Ok(candidate);
    }
    let (names, others) = if accidental.contains('b') { (FLAT_NAMES, SHARP_NAMES) } else { (SHARP_NAMES, FLAT_NAMES) };
    for names in [names, others] {
        let candidate = format!("{}{suffix}", names[root_pc as usize]);
        if abc::key_fifths(&candidate).is_some() {
            return Ok(candidate);
        }
    }
    Err(format!("Cannot encode portable ABC key for {}", abc::repr(key)))
}

/// A MIDI pitch in ABC, spelled from the key; `bar_state` holds the bar's accidentals by letter.
fn note_text(pitch: i32, signature: &[i32; 7], bar_state: &mut HashMap<usize, i32>) -> String {
    let sharps: i32 = signature.iter().sum();
    let name = PITCH_NAMES[(sharps + 7) as usize][pitch.rem_euclid(12) as usize];
    let letter = name.chars().next().expect("a pitch name starts with its letter");
    let alteration = match &name[1..] {
        "#" => 1,
        "##" => 2,
        "b" => -1,
        "bb" => -2,
        _ => 0,
    };
    let mut octave = (pitch - 60).div_euclid(12);
    if pitch.rem_euclid(12) == 11 && alteration == -1 {
        octave += 1;
    } else if pitch.rem_euclid(12) == 0 && alteration == 1 {
        octave -= 1;
    }
    let index = abc::letter_index(letter);
    let mut text = String::new();
    if *bar_state.get(&index).unwrap_or(&signature[index]) != alteration {
        bar_state.insert(index, alteration);
        text.push_str(["__", "_", "=", "^", "^^"][(alteration + 2) as usize]);
    }
    if octave > 0 {
        text.push(letter.to_ascii_lowercase());
        text.push_str(&"'".repeat((octave - 1) as usize));
    } else {
        text.push(letter);
        text.push_str(&",".repeat((-octave).max(0) as usize));
    }
    text
}

#[derive(Clone, Copy, Debug)]
struct Beat {
    time: f64,
    number: i64,
    numerator: i64,
    denominator: i64,
}

#[derive(Clone, Copy, Debug)]
struct Bar {
    index: usize,
    start: usize,
    end: usize,
    numerator: i64,
    denominator: i64,
    written_numerator: i64,
    written_denominator: Option<i64>,
    pad_before: bool,
    subbeats: usize,
}

impl Bar {
    fn start_t(&self) -> usize {
        self.start * self.subbeats
    }

    fn end_t(&self) -> usize {
        self.end * self.subbeats
    }

    fn abc_numerator(&self) -> i64 {
        self.written_numerator
    }

    fn abc_denominator(&self) -> i64 {
        self.written_denominator.unwrap_or(self.denominator)
    }
}

fn first_most_common(values: &[i64]) -> i64 {
    let mut counts: HashMap<i64, usize> = HashMap::new();
    for value in values {
        *counts.entry(*value).or_default() += 1;
    }
    let top = counts.values().copied().max().unwrap_or(0);
    *values.iter().find(|value| counts[value] == top).expect("a bar holds at least one beat")
}

fn infer_bars(beats: &[Beat], subbeats: usize) -> Result<Vec<Bar>, String> {
    let downbeats: Vec<usize> = beats.iter().enumerate().filter(|(_, beat)| beat.number == 1).map(|(index, _)| index).collect();
    let (Some(&first), Some(&last)) = (downbeats.first(), downbeats.last()) else {
        return Err("no beat is numbered 1, so no bar has a downbeat".into());
    };
    let mut spans = Vec::new();
    if first > 0 {
        spans.push((0, first, false));
    }
    spans.extend(downbeats.windows(2).map(|pair| (pair[0], pair[1], false)));
    if last < beats.len() - 1 {
        spans.push((last, beats.len() - 1, true));
    }
    if spans.is_empty() {
        return Err("the downbeats leave no bar between them".into());
    }
    let mut bars = Vec::new();
    for (index, (start, end, partial)) in spans.into_iter().enumerate() {
        let members = &beats[start..end];
        let count = members.len() as i64;
        let denominators: Vec<i64> = members.iter().map(|beat| beat.denominator).collect();
        let numerators: Vec<i64> = members.iter().map(|beat| beat.numerator).collect();
        let denominator = first_most_common(&denominators);
        let declared = first_most_common(&numerators);
        let pad_final = partial && numerators.iter().all(|value| *value == numerators[0]) && denominators.iter().all(|value| *value == denominator) && declared >= count;
        bars.push(Bar { index, start, end, numerator: count, denominator, written_numerator: if pad_final { declared } else { count }, written_denominator: None, pad_before: false, subbeats });
    }
    if bars.len() >= 2 {
        let second = bars[1];
        let first = &mut bars[0];
        if first.numerator * second.abc_denominator() < second.abc_numerator() * first.denominator {
            first.written_numerator = second.abc_numerator();
            first.written_denominator = Some(second.abc_denominator());
            first.pad_before = true;
        }
    }
    Ok(bars)
}

struct Grid {
    subbeats: usize,
    times: Vec<f64>,
    denominators: Vec<i64>,
    quarters: Vec<f64>,
    boundaries: Vec<f64>,
}

impl Grid {
    fn new(beats: &[Beat], bars: &[Bar], subbeats: usize) -> Result<Grid, String> {
        let mut interval_denominators = vec![0; beats.len() - 1];
        for bar in bars {
            for slot in &mut interval_denominators[bar.start..bar.end] {
                *slot = bar.denominator;
            }
        }
        if interval_denominators.contains(&0) {
            return Err("a beat lies in no bar".into());
        }
        let mut times = Vec::new();
        let mut denominators = Vec::new();
        let mut quarters = vec![0.0];
        let mut quarter = 0.0;
        for index in 0..beats.len() - 1 {
            let (start, end) = (beats[index].time, beats[index + 1].time);
            let step = (end - start) / subbeats as f64;
            let denominator = interval_denominators[index];
            for k in 0..subbeats {
                times.push(k as f64 * step + start);
                denominators.push(denominator);
                quarter += 4.0 / denominator as f64 / subbeats as f64;
                quarters.push(quarter);
            }
        }
        times.push(beats[beats.len() - 1].time);
        denominators.push(*interval_denominators.last().expect("two beats make one interval"));
        let boundaries = times.windows(2).map(|pair| (pair[0] + pair[1]) / 2.0).collect();
        Ok(Grid { subbeats, times, denominators, quarters, boundaries })
    }

    /// The grid point a time belongs to.
    fn at(&self, seconds: f64) -> usize {
        self.boundaries.partition_point(|boundary| *boundary < seconds)
    }

    fn clamp(&self, index: usize) -> usize {
        index.min(self.times.len() - 1)
    }
}

fn fill(rows: &[(f64, f64, String)], grid: &Grid, default: &str) -> Vec<String> {
    let mut values = vec![default.to_string(); grid.times.len()];
    for (start, end, value) in rows {
        let (a, b) = (grid.clamp(grid.at(*start)), grid.clamp(grid.at(*end)));
        for slot in values.iter_mut().take(b).skip(a) {
            *slot = value.clone();
        }
    }
    if values.len() > 1 {
        let last = values.len() - 1;
        values[last] = values[last - 1].clone();
    }
    values
}

/// A voice's notes on the grid as sustain values, onsets odd; a note too short for a subbeat is dropped.
fn voice_values(notes: &[(f64, f64, i32)], grid: &Grid, name: &str) -> Result<Vec<i32>, String> {
    let mut values = vec![0; grid.times.len()];
    let mut ordered = notes.to_vec();
    ordered.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)));
    for (start, end, pitch) in ordered {
        let (a, b) = (grid.clamp(grid.at(start)), grid.clamp(grid.at(end)));
        if b <= a {
            continue;
        }
        if values[a..b].iter().any(|value| *value != 0) {
            return Err(format!("{name} notes overlap at subbeats {a} to {b} once quantized"));
        }
        let sustain = pitch * 2 + 2;
        values[a..b].fill(sustain);
        values[a] = sustain + 1;
    }
    Ok(values)
}

struct Score {
    subbeats: usize,
    bars: Vec<Bar>,
    grid: Grid,
    keys: Vec<String>,
    chords: Vec<String>,
    sections: Vec<(usize, String)>,
    voices: [Vec<i32>; 2],
}

fn parse_rows(rows: &[(f64, f64, String)], what: &str) -> Result<Vec<(f64, f64, String)>, String> {
    let mut parsed = Vec::new();
    let mut previous_end: Option<f64> = None;
    for (start, end, value) in rows {
        if end <= start {
            return Err(format!("{what} end must be after start"));
        }
        if previous_end.is_some_and(|previous| *start < previous - 1e-6) {
            return Err(format!("overlapping {what} intervals"));
        }
        parsed.push((*start, *end, value.trim().to_string()));
        previous_end = Some(*end);
    }
    Ok(parsed)
}

fn score(rows: &Rows, melody_only: bool, subbeats: usize) -> Result<Score, String> {
    let mut beats: Vec<Beat> = Vec::new();
    for &(time, number, numerator, denominator) in &rows.beats {
        if number < 1 || numerator < 1 {
            return Err("beat IDs and meter numerators must be positive".into());
        }
        if denominator < 1 || denominator & (denominator - 1) != 0 {
            return Err("meter denominator must be a positive power of two".into());
        }
        if beats.last().is_some_and(|last| time <= last.time) {
            return Err("beat times must be strictly increasing".into());
        }
        beats.push(Beat { time, number, numerator, denominator });
    }
    if beats.len() < 2 {
        return Err("at least two beat events are required".into());
    }
    let mut keys = parse_rows(&rows.keys, "key")?;
    for (_, _, key) in &mut keys {
        *key = key_text(key)?;
    }
    if keys.is_empty() {
        return Err("at least one key interval is required".into());
    }
    let sections = parse_rows(&rows.structures, "structure")?;
    let chords = if melody_only { Vec::new() } else { parse_rows(&rows.chords, "chord")? };
    for (_, _, chord) in &chords {
        chord_text(chord)?;
    }
    let bars = infer_bars(&beats, subbeats)?;
    let grid = Grid::new(&beats, &bars, subbeats)?;
    let mut voices: [Vec<i32>; 2] = [Vec::new(), Vec::new()];
    for (index, name) in abc::VOICES.iter().enumerate() {
        let notes: Vec<(f64, f64, i32)> = rows.notes.iter().filter(|note| note.3 == index).map(|note| (note.0, note.1, note.2)).collect();
        voices[index] = voice_values(&notes, &grid, name)?;
    }
    let key_values = fill(&keys, &grid, &keys[0].2);
    let chord_values = if melody_only { vec!["N".to_string(); grid.times.len()] } else { fill(&chords, &grid, "N") };
    let section_starts = sections.iter().map(|(start, _, label)| (grid.clamp(grid.at(*start)), label.clone())).collect();
    Ok(Score { subbeats: grid.subbeats, bars, grid, keys: key_values, chords: chord_values, sections: section_starts, voices })
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// The ABC unit length that expresses every subbeat of every meter exactly.
fn unit_denominator(score: &Score) -> Result<i64, String> {
    let mut denominator = 1;
    for bar in &score.bars {
        for value in [bar.denominator, bar.abc_denominator()] {
            let value = value * score.subbeats as i64;
            denominator = denominator / gcd(denominator, value) * value;
        }
    }
    if denominator > 1024 {
        return Err(format!("the meters want a unit note of 1/{denominator}, too short to write"));
    }
    Ok(denominator)
}

fn units(score: &Score, start: usize, end: usize, unit: i64) -> Result<i64, String> {
    let mut total = 0;
    for denominator in &score.grid.denominators[start..end] {
        let divisor = denominator * score.subbeats as i64;
        if unit % divisor != 0 {
            return Err(format!("a unit note of 1/{unit} does not divide a 1/{divisor} subbeat"));
        }
        total += unit / divisor;
    }
    Ok(total)
}

/// Quarter notes per minute over the whole grid.
fn tempo(score: &Score) -> Result<f64, String> {
    let grid = &score.grid;
    let seconds = grid.times[grid.times.len() - 1] - grid.times[0];
    let quarters = grid.quarters[grid.quarters.len() - 1] - grid.quarters[0];
    if seconds <= 0.0 || quarters <= 0.0 {
        return Err("the score takes no time, so it has no tempo".into());
    }
    Ok(quarters / seconds * 60.0)
}

fn sustain_of(value: i32) -> i32 {
    (value / 2 - 1) * 2 + 2
}

fn same_segment(value: i32, following: i32) -> bool {
    if value == 0 { following == 0 } else { following == sustain_of(value) }
}

fn continues(value: i32, following: i32) -> bool {
    value > 0 && following == sustain_of(value)
}

/// A length as note values strict ABC readers accept, largest first.
fn split_units(duration: i64) -> Result<Vec<i64>, String> {
    if duration <= 0 {
        return Err(format!("a length of {duration} units cannot be written"));
    }
    let mut pieces = Vec::new();
    let mut remaining = duration;
    while remaining > 0 {
        if abc::DURATIONS.contains(&remaining) {
            pieces.push(remaining);
            break;
        }
        let Some(largest) = abc::DURATIONS.iter().copied().filter(|value| *value < remaining).max() else {
            return Err(format!("a length of {duration} units is not a sum of note values"));
        };
        pieces.push(largest);
        remaining -= largest;
    }
    Ok(pieces)
}

fn tokens(prefix: &str, text: &str, duration: i64, tie_out: bool) -> Result<Vec<String>, String> {
    let pieces = split_units(duration)?;
    let count = pieces.len();
    Ok(pieces
        .into_iter()
        .enumerate()
        .map(|(index, piece)| {
            let tied = text != "z" && (index + 1 < count || tie_out);
            format!("{}{text}{}{}", if index == 0 { prefix } else { "" }, if piece == 1 { String::new() } else { piece.to_string() }, if tied { "-" } else { "" })
        })
        .collect())
}

fn padding(bar: &Bar, unit: i64) -> i64 {
    bar.abc_numerator() * unit / bar.abc_denominator() - bar.numerator * unit / bar.denominator
}

fn bar_text(score: &Score, voice: usize, bar: &Bar, unit: i64) -> Result<String, String> {
    let values = &score.voices[voice];
    let with_chords = voice == abc::VOCAL;
    let mut bar_state: HashMap<usize, i32> = HashMap::new();
    let mut current_key = score.keys[bar.start_t()].clone();
    let mut signature = abc::key_accidentals(&current_key)?;
    let pad = padding(bar, unit);
    if pad < 0 {
        return Err(format!("bar {} holds more beats than its written meter", bar.index));
    }
    let mut leading = if bar.pad_before { pad } else { 0 };
    let mut trailing = if bar.pad_before { 0 } else { pad };
    let mut parts: Vec<String> = Vec::new();
    let mut t = bar.start_t();
    while t < bar.end_t() {
        let mut changes = vec![bar.end_t()];
        if let Some(probe) = (t + 1..bar.end_t()).find(|probe| !same_segment(values[t], values[*probe])) {
            changes.push(probe);
        }
        if let Some(probe) = (t + 1..bar.end_t()).find(|probe| score.keys[*probe] != score.keys[probe - 1]) {
            changes.push(probe);
        }
        if with_chords {
            if let Some(probe) = (t + 1..bar.end_t()).find(|probe| score.chords[*probe] != score.chords[probe - 1]) {
                changes.push(probe);
            }
        }
        let following = *changes.iter().min().expect("the bar's end is always a change");
        let mut prefix = String::new();
        if t > bar.start_t() && score.keys[t] != current_key {
            current_key = score.keys[t].clone();
            signature = abc::key_accidentals(&current_key)?;
            bar_state.clear();
            prefix.push_str(&format!("[K:{current_key}]"));
        }
        if with_chords && (t == bar.start_t() || score.chords[t] != score.chords[t - 1]) {
            if let Some(symbol) = chord_text(&score.chords[t])? {
                prefix.push_str(&format!("\"{symbol}\""));
            }
        }
        let value = values[t];
        let text = if value == 0 { "z".to_string() } else { note_text(value / 2 - 1, &signature, &mut bar_state) };
        let mut duration = units(score, t, following, unit)?;
        if t == bar.start_t() && leading != 0 {
            if value == 0 && prefix.is_empty() {
                duration += leading;
            } else {
                parts.extend(tokens("", "z", leading, false)?);
            }
            leading = 0;
        }
        if value == 0 && following == bar.end_t() && trailing != 0 {
            duration += trailing;
            trailing = 0;
        }
        let tie_out = value > 0 && following < values.len() && continues(value, values[following]);
        parts.extend(tokens(&prefix, &text, duration, tie_out)?);
        t = following;
    }
    if trailing != 0 {
        parts.extend(tokens("", "z", trailing, false)?);
    }
    Ok(parts.concat())
}

fn music_element() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r#""(?P<quoted>[^"]*)"|\[K:(?P<key>[^\]]+)\]|(?P<note>[_=^]*[A-Ga-gz][,']*)(?P<duration>\d*)(?P<tie>-?)"#).expect("a valid pattern"))
}

fn full_rest(text: &str) -> bool {
    let mut cursor = 0;
    let mut saw = false;
    for found in music_element().captures_iter(text) {
        let whole = found.get(0).expect("a match has its whole");
        if whole.start() != cursor {
            return false;
        }
        cursor = whole.end();
        if found.name("quoted").is_some() || found.name("key").is_some() {
            return false;
        }
        saw = true;
        if found.name("note").map(|note| note.as_str()) != Some("z") || found.name("tie").is_some_and(|tie| !tie.as_str().is_empty()) {
            return false;
        }
    }
    saw && cursor == text.len()
}

fn voice_line(score: &Score, voice: usize, bars: &[Bar], unit: i64) -> Result<String, String> {
    let texts = bars.iter().map(|bar| bar_text(score, voice, bar, unit)).collect::<Result<Vec<_>, _>>()?;
    let mut line = String::new();
    let mut index = 0;
    while index < texts.len() {
        if !full_rest(&texts[index]) {
            line.push_str(&texts[index]);
            line.push('|');
            index += 1;
            continue;
        }
        let mut end = index + 1;
        while end < texts.len() && full_rest(&texts[end]) {
            end += 1;
        }
        line.push('Z');
        if end - index > 1 {
            line.push_str(&(end - index).to_string());
        }
        line.push('|');
        index = end;
    }
    Ok(line)
}

struct Group {
    bars: Vec<Bar>,
    labels: Vec<String>,
    meter_changed: bool,
    key_changed: bool,
}

fn groups(score: &Score) -> Vec<Group> {
    let first = &score.bars[0];
    let mut meter = (first.abc_numerator(), first.abc_denominator());
    let mut key = score.keys[first.start_t()].clone();
    let mut section = String::new();
    let mut found: Vec<Group> = Vec::new();
    for bar in &score.bars {
        let bar_meter = (bar.abc_numerator(), bar.abc_denominator());
        let bar_key = &score.keys[bar.start_t()];
        let meter_changed = bar_meter != meter;
        let key_changed = *bar_key != key;
        let mut labels = Vec::new();
        for (t, label) in &score.sections {
            if !(bar.start_t() <= *t && *t < bar.end_t()) {
                continue;
            }
            let clean = label.split_whitespace().collect::<Vec<_>>().join(" ");
            if !clean.is_empty() && clean != section {
                labels.push(clean.clone());
                section = clean;
            }
        }
        match found.last_mut() {
            Some(group) if group.bars.len() < 4 && !meter_changed && !key_changed && labels.is_empty() => group.bars.push(*bar),
            _ => found.push(Group { bars: vec![*bar], labels, meter_changed, key_changed }),
        }
        meter = bar_meter;
        key = score.keys[bar.end_t() - 1].clone();
    }
    found
}

fn write(score: &Score) -> Result<String, String> {
    let unit = unit_denominator(score)?;
    let first = &score.bars[0];
    let mut lines = vec![
        "X:1".to_string(),
        "T:".to_string(),
        format!("M:{}/{}", first.abc_numerator(), first.abc_denominator()),
        format!("L:1/{unit}"),
        format!("Q:1/4={}", tempo(score)?.round_ties_even() as i64),
        abc::VOICE_LINES[0].to_string(),
        abc::VOICE_LINES[1].to_string(),
        format!("K:{}", score.keys[first.start_t()]),
    ];
    for group in groups(score) {
        lines.extend(group.labels.iter().map(|label| format!("% {label}")));
        let head = &group.bars[0];
        for (voice, name) in abc::VOICES.iter().enumerate() {
            lines.push(format!("V: {name}"));
            if group.meter_changed {
                lines.push(format!("M:{}/{}", head.abc_numerator(), head.abc_denominator()));
            }
            if group.key_changed {
                lines.push(format!("K:{}", score.keys[head.start_t()]));
            }
            lines.push(voice_line(score, voice, &group.bars, unit)?);
        }
    }
    Ok(lines.join("\n") + "\n")
}

/// The score these rows describe, as ABC in YuE2's two-voice dialect, or why it cannot be written.
pub fn build(rows: &Rows, melody_only: bool, subbeats: usize) -> Result<String, String> {
    write(&score(rows, melody_only, subbeats)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beats(count: usize, seconds_per_beat: f64, numerator: i64) -> Vec<(f64, i64, i64, i64)> {
        (0..count).map(|index| (index as f64 * seconds_per_beat, (index as i64 % numerator) + 1, numerator, 4)).collect()
    }

    #[test]
    fn chord_labels_become_symbols_with_their_bass_spelled_from_the_root() {
        assert_eq!(chord_text("A:min7/b3").unwrap().as_deref(), Some("Am7/C"));
        assert_eq!(chord_text("C:maj/3").unwrap().as_deref(), Some("C/E"));
        assert_eq!(chord_text("Bb:hdim7").unwrap().as_deref(), Some("Bbm7b5"));
        assert_eq!(chord_text("N").unwrap(), None);
        assert!(chord_text("C").unwrap_err().contains("missing the ':'"));
        assert!(chord_text("C:weird").unwrap_err().contains("Unsupported chord quality"));
    }

    #[test]
    fn keys_get_a_standard_signature() {
        assert_eq!(key_text("A#:major").unwrap(), "Bb");
        assert_eq!(key_text("F#:minor").unwrap(), "F#m");
        assert_eq!(key_text("Cbm").unwrap(), "Bm");
        assert!(key_text("C:dorian").is_err());
    }

    #[test]
    fn a_tune_is_written_in_the_dialect_with_ties_and_whole_rest_bars() {
        let rows = Rows {
            beats: beats(13, 0.5, 4),
            chords: vec![(0.0, 2.0, "C:maj".into()), (2.0, 4.0, "G:7".into())],
            keys: vec![(0.0, 6.0, "C:major".into())],
            structures: vec![(0.0, 6.0, "verse".into())],
            notes: vec![(0.0, 0.5, 60, 0), (0.5, 1.75, 64, 0), (2.0, 4.0, 67, 0), (0.0, 2.0, 48, 1)],
        };
        let text = build(&rows, false, 4).unwrap();
        assert_eq!(
            text,
            "X:1\nT:\nM:4/4\nL:1/16\nQ:1/4=120\nV: Vocal clef=treble name=\"Vocal Melody\" snm=\"Vocal\"\nV: Ins clef=treble name=\"Ins Melody\" snm=\"Inst.\"\nK:C\n% verse\nV: Vocal\n\"C\"C4E8-E2z2|\"G7\"G16|Z|\nV: Ins\nC,16|Z2|\n"
        );
    }
}
