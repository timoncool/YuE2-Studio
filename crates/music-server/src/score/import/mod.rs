//! A MIDI file as a score YuE2 sings, and the lyrics to go with it.
//!
//! Timing: a score has one tempo, so the file's tempo map is averaged over its
//! length and every beat is placed at that tempo; notes are placed by tick on
//! the same beats. Bars come from the file's meters (4/4 when it states none),
//! and the song runs to its last note or the end of its longest track, filled
//! out to a whole bar.
//!
//! The grid: sixteenths when every note starts and ends on one, thirty-seconds
//! when nine note edges in ten lie on one, and sixteenths for a file played in
//! by hand. A note shorter than one step is held for one.
//!
//! The two lines are the parts `parts::choose` picks, each one note at a time:
//! of notes struck together the top one is kept, a lower note starting under a
//! higher one is dropped unless most of it sounds after the higher one ends,
//! and a line whose middle pitch lies outside where YuE2's scores keep it is
//! moved by whole octaves. The key is the file's when it fits the notes, or is
//! estimated with the Krumhansl-Kessler profiles. Sections come from markers,
//! else from the karaoke paragraphs. With chords, a track named Chords is read
//! chord by chord, else the harmony is guessed, and every chord is named from
//! the key it sounds in. Ported from YuE2-ComfyUI's `midi/score.py`.

pub mod harmony;
pub mod karaoke;
pub mod parts;

use std::collections::BTreeSet;

use serde::Serialize;

use super::{abc, rebuild, sections, smf, spelling, Q};

const VOICE_WINDOW: (f64, f64) = (60.0, 82.0);
const INSTRUMENT_WINDOW: (f64, f64) = (60.0, 88.0);
const ON_GRID: f64 = 0.9;
const KEY_SLACK: f64 = 0.05;
const TEMPO_SLACK: f64 = 0.01;
const SHOWN_BARS: usize = 8;
/// Beats a score may run to: an hour at 330 BPM.
const MOST_BEATS: usize = 20_000;

const MAJOR_PROFILE: [f64; 12] = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
const MINOR_PROFILE: [f64; 12] = [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];
const MAJOR_KEYS: [&str; 15] = ["Cb", "Gb", "Db", "Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E", "B", "F#", "C#"];
const MINOR_KEYS: [&str; 15] = ["Abm", "Ebm", "Bbm", "Fm", "Cm", "Gm", "Dm", "Am", "Em", "Bm", "F#m", "C#m", "G#m", "D#m", "A#m"];

/// Whether the score gets chord symbols: `Melody` for a score sung with 'cot' at melody, `Full` with chords.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Melody,
    Full,
}

impl Mode {
    pub fn parse(value: Option<&str>) -> Result<Mode, String> {
        match value.unwrap_or("melody") {
            "melody" => Ok(Mode::Melody),
            "full" => Ok(Mode::Full),
            other => Err(format!("'mode' is '{other}'; it must be melody or full")),
        }
    }
}

/// What the import shows about the score it wrote.
#[derive(Clone, Debug, Serialize)]
pub struct Facts {
    pub bpm: i64,
    pub meter: String,
    pub bars: usize,
    pub seconds: f64,
    pub key: String,
    pub key_source: &'static str,
    pub grid: u32,
    pub voice: Option<usize>,
    pub instrument: Option<usize>,
    pub voice_shift: i32,
    pub instrument_shift: i32,
    pub karaoke: bool,
}

/// Something to say about a score written from a file, with the values a translation fills in.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum Notice {
    TempoChanges { low: f64, high: f64, bpm: f64 },
    PlayedIn,
    Moved { line: &'static str, track: usize, semitones: i32 },
    Struck { track: usize, count: usize },
    ChordsRead { track: usize, name: String },
    Unnamed { track: usize, bars: Vec<usize> },
    ChordsGuessed,
}

impl Notice {
    pub fn text(&self) -> String {
        match self {
            Notice::TempoChanges { low, high, bpm } => format!(
                "The file's tempo moves between {low:.0} and {high:.0} BPM. A score keeps one tempo, so it is written at the average, {bpm:.0} BPM, and the song will not speed up or slow down where the file does."
            ),
            Notice::PlayedIn => "The notes of the file do not sit on a grid of sixteenths or thirty-seconds -- it was probably played in by hand -- so they were rounded to the nearest sixteenth.".into(),
            Notice::Moved { line, track, semitones } => {
                let octaves = semitones.abs() / 12;
                format!(
                    "The {line} line (track {track}) sat {} for YuE2's scores, so it was moved {} {}.",
                    if *semitones > 0 { "low" } else { "high" },
                    if *semitones > 0 { "up" } else { "down" },
                    if octaves == 1 { "an octave".to_string() } else { format!("{octaves} octaves") }
                )
            }
            Notice::Struck { track, count } => {
                format!("Track {track} strikes {count} notes together with a higher one. A line of the score holds one note at a time, so the top note of each chord was kept.")
            }
            Notice::ChordsRead { track, name } => format!("The chord symbols were read from track {track}, '{name}', chord by chord."),
            Notice::Unnamed { track, bars } => format!(
                "Track {track} sounds notes that make no chord the score can name at {}, so the nearest chord was written there, or the one before it kept.",
                bar_list(bars)
            ),
            Notice::ChordsGuessed => "The chord symbols were guessed from what the file's parts play together: a harmony to follow, not a transcription of it.".into(),
        }
    }
}

/// A file's score and lyrics for one mode and one choice of parts, with what to say about them.
#[derive(Clone, Debug, Serialize)]
pub struct Converted {
    pub abc: String,
    pub lyrics: String,
    pub parts: Vec<parts::Row>,
    pub facts: Facts,
    pub notices: Vec<Notice>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    pub grid: Option<u32>,
    /// Additional octaves after fitting the line to the singing window.
    pub vocal_octaves: i32,
    pub instrument_octaves: i32,
}

/// Seconds from the start of the song to `tick`, through its tempo map.
fn seconds_at(song: &smf::Song, tick: Q) -> f64 {
    let division = song.division as f64;
    let mut seconds = 0.0;
    let mut last_tick: u64 = 0;
    let mut tempo = smf::DEFAULT_TEMPO;
    for &(change, value) in &song.tempos {
        if Q::int(change as i64) >= tick {
            break;
        }
        seconds += (change - last_tick) as f64 * tempo as f64 / 1e6 / division;
        last_tick = change;
        tempo = value;
    }
    seconds + (tick - Q::int(last_tick as i64)).to_f64() * tempo as f64 / 1e6 / division
}

fn meters(song: &smf::Song) -> Vec<(u64, i64, i64)> {
    let mut rows: Vec<(u64, i64, i64)> = Vec::new();
    for &(tick, numerator, denominator) in &song.meters {
        let row = (tick, numerator as i64, denominator as i64);
        match rows.last_mut() {
            Some(last) if last.0 == tick => *last = row,
            Some(last) if (last.1, last.2) == (row.1, row.2) => {}
            _ => rows.push(row),
        }
    }
    if rows.first().is_none_or(|first| first.0 > 0) {
        rows.insert(0, (0, 4, 4));
    }
    rows
}

/// `(tick, number, numerator, denominator)` for every beat up to the first downbeat at or after `end`.
fn beats(song: &smf::Song, end: u64) -> Result<Vec<(Q, i64, i64, i64)>, String> {
    let meters = meters(song);
    let mut rows = Vec::new();
    let mut tick = Q::ZERO;
    let mut number = 1;
    let mut index = 0;
    let (mut numerator, mut denominator) = (meters[0].1, meters[0].2);
    loop {
        while index + 1 < meters.len() && Q::int(meters[index + 1].0 as i64) <= tick {
            index += 1;
            (numerator, denominator) = (meters[index].1, meters[index].2);
            number = 1;
        }
        rows.push((tick, number, numerator, denominator));
        if number == 1 && tick >= Q::int(end as i64) && rows.len() > 1 {
            return Ok(rows);
        }
        if rows.len() > MOST_BEATS {
            return Err(format!("the file runs past {MOST_BEATS} beats, longer than any song"));
        }
        let step = Q::new(song.division as i64 * 4, denominator);
        let following = meters.get(index + 1).map(|meter| Q::int(meter.0 as i64));
        tick = match following {
            Some(following) if tick < following && following < tick + step => following,
            _ => tick + step,
        };
        number = number % numerator + 1;
    }
}

fn on_grid(tick: u64, division: u32, per_quarter: u64) -> bool {
    (tick * per_quarter) % division as u64 == 0
}

/// 16 or 32: sixteenths when every edge lies on one, thirty-seconds when nine in ten do, else sixteenths.
fn grid_of(notes: &[smf::Note], division: u32) -> u32 {
    let edges: Vec<u64> = notes.iter().flat_map(|note| [note.start, note.end]).collect();
    if edges.iter().all(|tick| on_grid(*tick, division, 4)) {
        return 16;
    }
    if edges.iter().filter(|tick| on_grid(**tick, division, 8)).count() as f64 >= ON_GRID * edges.len() as f64 {
        return 32;
    }
    16
}

/// The tick of every grid point: `subbeats` equal steps inside each beat, and the last beat.
fn points_of(rows: &[(Q, i64, i64, i64)], subbeats: i64) -> Vec<Q> {
    let mut points = Vec::new();
    for pair in rows.windows(2) {
        let step = (pair[1].0 - pair[0].0) / Q::int(subbeats);
        points.extend((0..subbeats).map(|k| pair[0].0 + Q::int(k) * step));
    }
    points.push(rows[rows.len() - 1].0);
    points
}

/// The index of the grid point nearest `tick`; halfway goes to the earlier one.
fn snap(points: &[Q], tick: u64) -> usize {
    let tick = Q::int(tick as i64);
    let at = points.partition_point(|point| *point < tick);
    if at == 0 {
        return 0;
    }
    if at >= points.len() {
        return points.len() - 1;
    }
    if points[at] - tick < tick - points[at - 1] { at } else { at - 1 }
}

/// Semitones, whole octaves, that bring a middle pitch inside `window`; 0 when it is inside.
fn octave_shift(median: f64, window: (f64, f64)) -> i32 {
    let (low, high) = window;
    if median < low {
        return 12 * ((low - median) / 12.0).ceil() as i32;
    }
    if median > high {
        return -12 * ((median - high) / 12.0).ceil() as i32;
    }
    0
}

/// A line of `(start, end, pitch)` on grid indices, one note at a time, and how many chord notes it dropped.
fn skyline(notes: &[(usize, usize, i32)]) -> (Vec<(usize, usize, i32)>, usize) {
    let mut ordered = notes.to_vec();
    ordered.sort_by_key(|(start, end, pitch)| (*start, -pitch, *end));
    let mut kept: Vec<(usize, usize, i32)> = Vec::new();
    let mut struck = 0;
    for (start, end, pitch) in ordered {
        let Some(last) = kept.last_mut() else {
            kept.push((start, end, pitch));
            continue;
        };
        if start >= last.1 {
            kept.push((start, end, pitch));
        } else if start == last.0 {
            struck += 1;
        } else if pitch > last.2 {
            last.1 = start;
            kept.push((start, end, pitch));
        } else if end - last.1 > last.1 - start {
            let from = last.1;
            kept.push((from, end, pitch));
        }
    }
    (kept, struck)
}

fn held(notes: &[smf::Note], points: &[Q]) -> Vec<(usize, usize, i32)> {
    notes
        .iter()
        .filter_map(|note| {
            let start = snap(points, note.start);
            let end = snap(points, note.end).max(start + 1).min(points.len() - 1);
            (end > start).then_some((start, end, note.pitch as i32))
        })
        .collect()
}

/// A part as a line on the grid, moved into `window` by the middle pitch of the notes the line keeps.
fn line(part: &parts::Part, points: &[Q], window: (f64, f64)) -> (Vec<(usize, usize, i32)>, i32, usize) {
    let (kept, struck) = skyline(&held(&part.notes, points));
    if kept.is_empty() {
        return (Vec::new(), 0, struck);
    }
    let mut pitches: Vec<i32> = kept.iter().map(|note| note.2).collect();
    pitches.sort_unstable();
    let count = pitches.len();
    let shift = octave_shift((pitches[(count - 1) / 2] + pitches[count / 2]) as f64 / 2.0, window);
    let moved = kept.into_iter().filter(|note| (0..=127).contains(&(note.2 + shift))).map(|(start, end, pitch)| (start, end, pitch + shift)).collect();
    (moved, shift, struck)
}

fn correlation(weights: &[f64; 12], profile: &[f64; 12], root: usize) -> f64 {
    let rotated: Vec<f64> = (0..12).map(|step| weights[(root + step) % 12]).collect();
    let mean_a = rotated.iter().sum::<f64>() / 12.0;
    let mean_b = profile.iter().sum::<f64>() / 12.0;
    let top: f64 = rotated.iter().zip(profile).map(|(a, b)| (a - mean_a) * (b - mean_b)).sum();
    let bottom = (rotated.iter().map(|a| (a - mean_a).powi(2)).sum::<f64>() * profile.iter().map(|b| (b - mean_b).powi(2)).sum::<f64>()).sqrt();
    if bottom != 0.0 { top / bottom } else { 0.0 }
}

/// `(correlation, root pitch class, minor)` of the key whose profile best fits twelve pitch-class weights.
fn estimate_key(weights: &[f64; 12]) -> (f64, usize, bool) {
    let mut best = (f64::NEG_INFINITY, 0, false);
    for root in 0..12 {
        for (profile, minor) in [(&MAJOR_PROFILE, false), (&MINOR_PROFILE, true)] {
            let value = correlation(weights, profile, root);
            if value > best.0 || (value == best.0 && (root, minor) > (best.1, best.2)) {
                best = (value, root, minor);
            }
        }
    }
    best
}

fn signature_name(sharps: i32, minor: bool) -> &'static str {
    (if minor { MINOR_KEYS } else { MAJOR_KEYS })[(sharps + 7) as usize]
}

fn keys(song: &smf::Song, weights: &[f64; 12]) -> Result<(Vec<(Q, String)>, &'static str), String> {
    let mut distinct: Vec<(u64, i32, bool)> = Vec::new();
    for &(tick, sharps, minor) in &song.keys {
        if distinct.last().is_none_or(|last| (last.1, last.2) != (sharps, minor)) {
            distinct.push((tick, sharps, minor));
        }
    }
    let best = estimate_key(weights);
    if distinct.len() >= 2 {
        let starts = distinct.iter().enumerate().map(|(index, (tick, sharps, minor))| (if index == 0 { Q::ZERO } else { Q::int(*tick as i64) }, signature_name(*sharps, *minor).to_string())).collect();
        return Ok((starts, "file"));
    }
    if let Some(&(_, sharps, minor)) = distinct.first() {
        let root = (7 * sharps + if minor { 9 } else { 0 }).rem_euclid(12) as usize;
        if correlation(weights, if minor { &MINOR_PROFILE } else { &MAJOR_PROFILE }, root) >= best.0 - KEY_SLACK {
            return Ok((vec![(Q::ZERO, signature_name(sharps, minor).to_string())], "file"));
        }
    }
    let name = rebuild::key_text(&format!("{}{}", rebuild::SHARP_NAMES[best.1], if best.2 { ":minor" } else { ":major" }))?;
    Ok((vec![(Q::ZERO, name)], "estimated"))
}

/// `(start, end, value)` from `(tick, value)` starts, each ending where the next begins or at `last`.
fn intervals<T: Clone>(starts: &[(Q, T)], last: Q) -> Vec<(Q, Q, T)> {
    let mut rows = Vec::new();
    for (index, (tick, value)) in starts.iter().enumerate() {
        let end = starts.get(index + 1).map(|next| next.0).unwrap_or(last);
        if *tick < end {
            rows.push((*tick, end, value.clone()));
        }
    }
    rows
}

/// `bar 5` or `bars 5, 9 and 12`, the first few of them and how many more.
fn bar_list(numbers: &[usize]) -> String {
    let mut shown: Vec<String> = numbers.iter().take(SHOWN_BARS).map(usize::to_string).collect();
    if numbers.len() > SHOWN_BARS {
        shown.push(format!("{} more", numbers.len() - SHOWN_BARS));
    }
    if shown.len() == 1 {
        return format!("bar {}", shown[0]);
    }
    let last = shown.pop().expect("two or more bars");
    format!("bars {} and {last}", shown.join(", "))
}

/// An ABC key such as `F#m` as the key label the chord speller reads: `F#:minor`.
fn key_label(name: &str) -> String {
    match name.strip_suffix('m') {
        Some(root) => format!("{root}:minor"),
        None => format!("{name}:major"),
    }
}

fn bars(rows: &[(Q, i64, i64, i64)]) -> Vec<(Q, Q, i64)> {
    let downbeats: Vec<(Q, i64)> = rows.iter().filter(|row| row.1 == 1).map(|row| (row.0, row.2)).collect();
    downbeats.windows(2).map(|pair| (pair[0].0, pair[1].0, pair[0].1)).collect()
}

fn section_starts(song: &smf::Song, words: Option<&karaoke::Words>, bars: &[(Q, Q, i64)]) -> Vec<(Q, String)> {
    let marked = karaoke::marker_sections(song);
    if !marked.is_empty() {
        return marked.into_iter().map(|(tick, label)| (Q::int(tick as i64), label.to_string())).collect();
    }
    let Some(words) = words else {
        return Vec::new();
    };
    let mut starts: Vec<(Q, String)> = words.sections.iter().map(|section| (Q::int(section.tick as i64), section.label.clone())).collect();
    if let (Some(first_bar), Some(first)) = (bars.first(), starts.first()) {
        if first.0 >= first_bar.1 {
            starts.insert(0, (Q::ZERO, "intro".to_string()));
        }
    }
    starts
}

pub fn convert_with(song: &smf::Song, mode: Mode, vocal: parts::Pick, instrument: parts::Pick, options: Options) -> Result<Converted, String> {
    if options.grid.is_some_and(|grid| !matches!(grid, 16 | 32)) { return Err("The MIDI grid must be 16 or 32".into()); }
    if !(-3..=3).contains(&options.vocal_octaves) || !(-3..=3).contains(&options.instrument_octaves) { return Err("Additional octave shifts must be between -3 and 3".into()); }
    let found = parts::parts(song);
    if found.is_empty() {
        return Err("the file has no notes".into());
    }
    let words = karaoke::read(song);
    let syllables = words.as_ref().map(|words| words.syllables.clone()).unwrap_or_default();
    let chosen = parts::choose(&found, vocal, instrument, &syllables, song.division)?;
    if chosen.voice.is_none() && chosen.instrument.is_none() {
        return Err("the file has no melody to write: no part for the voice or the instrument".into());
    }
    let last_note = found.iter().flat_map(|part| part.notes.iter().map(|note| note.end)).max().unwrap_or(0);
    let rows = beats(song, song.end().max(last_note))?;
    // the line of each part: 0 the voice, 1 the instrument
    let mut lines: Vec<(usize, (f64, f64), &str, usize)> = Vec::new();
    if let Some(voice) = chosen.voice {
        lines.push((voice, VOICE_WINDOW, "voice", 0));
    }
    if let Some(instrument) = chosen.instrument {
        lines.push((instrument, INSTRUMENT_WINDOW, "instrument", 1));
    }
    let line_notes: Vec<smf::Note> = lines.iter().flat_map(|(index, _, _, _)| found[*index].notes.iter().copied()).collect();
    let grid = options.grid.unwrap_or_else(|| grid_of(&line_notes, song.division));
    let subbeats = (grid as i64 / rows.iter().map(|row| row.3).min().expect("a song has beats")).max(1);
    let points = points_of(&rows, subbeats);
    let last = rows[rows.len() - 1].0;
    let length = seconds_at(song, last);
    let per_tick = length / last.to_f64();
    let bpm = last.to_f64() / song.division as f64 * 60.0 / length;
    let at = |tick: Q| tick.to_f64() * per_tick;

    let mut notices = Vec::new();
    let mut tempos: Vec<f64> = song.tempos.iter().filter(|(tick, _)| Q::int(*tick as i64) < last).map(|(_, value)| 60e6 / *value as f64).collect();
    if tempos.is_empty() {
        tempos.push(60e6 / smf::DEFAULT_TEMPO as f64);
    }
    let (low, high) = tempos.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), value| (low.min(*value), high.max(*value)));
    if high > low * (1.0 + TEMPO_SLACK) {
        notices.push(Notice::TempoChanges { low, high, bpm });
    }
    let edges: Vec<u64> = line_notes.iter().flat_map(|note| [note.start, note.end]).collect();
    if grid == 16 && !edges.is_empty() && (edges.iter().filter(|tick| on_grid(**tick, song.division, 4)).count() as f64) < ON_GRID * edges.len() as f64 {
        notices.push(Notice::PlayedIn);
    }
    let mut notes: Vec<(f64, f64, i32, usize)> = Vec::new();
    let (mut voice_shift, mut instrument_shift) = (0, 0);
    for &(index, window, name, voice) in &lines {
        let part = &found[index];
        let (mut kept, mut shift, struck) = line(part, &points, window);
        let additional = 12 * if voice == 0 { options.vocal_octaves } else { options.instrument_octaves };
        shift += additional;
        for note in &mut kept {
            note.2 += additional;
            if !(0..=127).contains(&note.2) { return Err(format!("The {name} octave shift would move notes outside MIDI pitches 0–127")); }
        }
        if voice == 0 { voice_shift = shift } else { instrument_shift = shift }
        notes.extend(kept.iter().map(|(start, stop, pitch)| (at(points[*start]), at(points[*stop]), *pitch, voice)));
        if shift != 0 {
            notices.push(Notice::Moved { line: name, track: part.number, semitones: shift });
        }
        if struck > 0 {
            notices.push(Notice::Struck { track: part.number, count: struck });
        }
    }
    let mut weights = [0.0; 12];
    for part in found.iter().filter(|part| !part.drums()) {
        for note in &part.notes {
            weights[(note.pitch % 12) as usize] += (note.end - note.start) as f64;
        }
    }
    let (key_starts, key_source) = keys(song, &weights)?;
    let bars = bars(&rows);
    let starts: Vec<(Q, String)> = section_starts(song, words.as_ref(), &bars).into_iter().filter(|(tick, _)| *tick < last).collect();
    let structure = intervals(&starts, last);
    let mut rows_of_parts = parts::describe(&found, Some(&chosen));
    let key_rows = intervals(&key_starts, last);
    let mut harmony: Vec<(Q, Q, String)> = Vec::new();
    if mode == Mode::Full {
        let signature = abc::key_fifths(&key_starts[0].1).ok_or_else(|| format!("the key {} has no signature", key_starts[0].1))?;
        let tonic = (7 * signature).rem_euclid(12);
        let scale: BTreeSet<i32> = [0, 2, 4, 5, 7, 9, 11].iter().map(|step| (tonic + step) % 12).collect();
        if let Some(written) = parts::chords_part(&found) {
            let part = &found[written];
            let (read, unnamed) = harmony::read(&held(&part.notes, &points), points.len() - 1, Some(&scale));
            harmony = read.into_iter().map(|(start, stop, chord)| (points[start], points[stop], chord)).collect();
            notices.push(Notice::ChordsRead { track: part.number, name: part.name.clone() });
            if !unnamed.is_empty() {
                let downbeats: Vec<Q> = bars.iter().map(|bar| bar.0).collect();
                let numbers: Vec<usize> = unnamed.iter().map(|start| downbeats.partition_point(|tick| *tick <= points[*start]).max(1)).collect::<BTreeSet<usize>>().into_iter().collect();
                notices.push(Notice::Unnamed { track: part.number, bars: numbers });
            }
            if let Some(row) = rows_of_parts.iter_mut().find(|row| row.number == part.number && row.role.is_empty()) {
                row.role = "chords";
            }
        } else {
            let heard: Vec<(Q, Q, i32, f64)> = found
                .iter()
                .enumerate()
                .filter(|(_, part)| !part.drums())
                .flat_map(|(index, part)| {
                    let weight = if Some(index) == chosen.voice { harmony::VOICE_WEIGHT } else { 1.0 };
                    part.notes.iter().map(move |note| (Q::int(note.start as i64), Q::int(note.end as i64), note.pitch as i32, weight))
                })
                .collect();
            harmony = harmony::guess(&heard, &harmony::spans(&bars), signature < 0, Some(&scale));
            notices.push(Notice::ChordsGuessed);
        }
        let labelled: Vec<(Q, Q, String)> = key_rows.iter().map(|(start, stop, name)| (*start, *stop, key_label(name))).collect();
        harmony = spelling::respelled(&harmony, &labelled);
    }
    notes.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)).then(a.3.cmp(&b.3)));
    let built = rebuild::Rows {
        beats: rows.iter().map(|(tick, number, numerator, denominator)| (at(*tick), *number, *numerator, *denominator)).collect(),
        chords: harmony.iter().map(|(start, stop, chord)| (at(*start), at(*stop), chord.clone())).collect(),
        keys: key_rows.iter().map(|(start, stop, name)| (at(*start), at(*stop), name.clone())).collect(),
        structures: structure.iter().map(|(start, stop, label)| (at(*start), at(*stop), label.clone())).collect(),
        notes,
    };
    let abc = rebuild::build(&built, mode == Mode::Melody, subbeats as usize)?;
    let lyrics = match &words {
        Some(words) => words.lyrics(),
        None => sections::skeleton(&sections::sections(&abc)?),
    };
    let facts = Facts {
        bpm: bpm.round_ties_even() as i64,
        meter: format!("{}/{}", rows[0].2, rows[0].3),
        bars: bars.len(),
        seconds: (length * 10.0).round() / 10.0,
        key: key_starts[0].1.clone(),
        key_source,
        grid,
        voice: chosen.voice.map(|index| found[index].number),
        instrument: chosen.instrument.map(|index| found[index].number),
        voice_shift,
        instrument_shift,
        karaoke: words.is_some(),
    };
    Ok(Converted { abc, lyrics, parts: rows_of_parts, facts, notices })
}

#[cfg(test)]
mod tests;
