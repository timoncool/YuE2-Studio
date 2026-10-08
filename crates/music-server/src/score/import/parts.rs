//! A MIDI file's parts: what each one is, and which of them the voice and the
//! instrument take.
//!
//! A part is a track, or one channel of a track that holds several (every
//! format-0 file does). Only parts with notes count, numbered from 1 in file
//! order. Channel 10 is drums and is never sung.
//!
//! Left to choose, the voice is the part the karaoke syllables fall on, then a
//! part named as sung or as the melody, then the highest melodic line: not a
//! bass, carrying at least a quarter as many notes as the busiest part, and
//! marked down by up to an octave for the notes it strikes in chords. The
//! instrument is a part named for it, or the busiest remaining part above G3
//! that strikes at most half its notes in chords, or nothing. A part named
//! `Chords`, the harmony "Save as MIDI" writes, is neither.
//!
//! Names are bytes in no stated encoding: UTF-8 is tried first, then text whose
//! letters are mostly bytes from 0xC0 up is read as Windows-1251 and anything
//! else as Latin-1; Windows-1251 letters a converter encoded as Latin-1 into
//! UTF-8 are read back to the Cyrillic. Ported from YuE2-ComfyUI's `midi/tracks.py`.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;

use super::super::smf;

const FAMILIES: [&str; 16] = [
    "Piano",
    "Chromatic Percussion",
    "Organ",
    "Guitar",
    "Bass",
    "Strings",
    "Ensemble",
    "Brass",
    "Reed",
    "Pipe",
    "Synth Lead",
    "Synth Pad",
    "Synth Effects",
    "Ethnic",
    "Percussive",
    "Sound Effects",
];

/// A part whose middle pitch is below C3 is a bass line whatever its program.
const LOWEST_MELODY: f64 = 48.0;
const INSTRUMENT_LOWEST: f64 = 55.0;
const BUSY_SHARE: f64 = 0.25;
const CHORD_LINE: f64 = 0.5;
/// Semitones a part's middle pitch is marked down for striking all of its notes in chords.
const CHORD_PENALTY: f64 = 12.0;
/// The share of karaoke syllables that must fall on a part's onsets for the words to name it the voice.
const KARAOKE_SHARE: f64 = 0.5;

fn voice_name() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new("(?i)vocal|voice|vox|melod|sing|lead|вокал|голос|мелод").expect("a valid pattern"))
}

fn instrument_name() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"(?i)^ins\b|instrument|solo").expect("a valid pattern"))
}

fn chords_name() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"(?i)^\s*chords?\s*$").expect("a valid pattern"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Cp1251,
    Latin1,
    /// Windows-1251 letters a converter wrote into UTF-8 as if they were Latin-1.
    DoubleEncoded,
}

fn without_nulls(data: &[u8]) -> Vec<u8> {
    data.iter().copied().filter(|byte| *byte != 0).collect()
}

/// Windows-1251 as Python reads it: the one byte the code page leaves undefined is a replacement character.
fn cp1251(raw: &[u8]) -> (String, bool) {
    let mut text = String::new();
    let mut clean = true;
    for chunk in raw.split_inclusive(|byte| *byte == 0x98) {
        let (body, undefined) = match chunk.split_last() {
            Some((0x98, body)) => (body, true),
            _ => (chunk, false),
        };
        text.push_str(&encoding_rs::WINDOWS_1251.decode_without_bom_handling(body).0);
        if undefined {
            text.push('\u{fffd}');
            clean = false;
        }
    }
    (text, clean)
}

/// How to read these bytes.
pub fn encoding_of(data: &[u8]) -> Encoding {
    let raw = without_nulls(data);
    let Ok(value) = std::str::from_utf8(&raw) else {
        let high = raw.iter().filter(|byte| **byte >= 0xC0).count();
        let letters = high + raw.iter().filter(|byte| byte.is_ascii_alphabetic()).count();
        return if high >= 3 && high * 2 >= letters { Encoding::Cp1251 } else { Encoding::Latin1 };
    };
    let wide: Vec<char> = value.chars().filter(|character| *character as u32 > 127).collect();
    let letters = value.chars().filter(|character| character.is_alphabetic()).count();
    if wide.len() >= 3 && wide.iter().all(|character| *character as u32 <= 0xFF) && wide.len() * 2 >= letters {
        let latin: Vec<u8> = value.chars().map(|character| character as u32 as u8).collect();
        return if cp1251(&latin).1 { Encoding::DoubleEncoded } else { Encoding::Utf8 };
    }
    Encoding::Utf8
}

/// Bytes read in an encoding `encoding_of` named, with nothing folded: a karaoke syllable keeps its spaces.
pub fn text_of(data: &[u8], encoding: Encoding) -> String {
    let raw = without_nulls(data);
    match encoding {
        Encoding::DoubleEncoded => match std::str::from_utf8(&raw) {
            Ok(value) if value.chars().all(|character| character as u32 <= 0xFF) => {
                let latin: Vec<u8> = value.chars().map(|character| character as u32 as u8).collect();
                match cp1251(&latin) {
                    (text, true) => text,
                    _ => String::from_utf8_lossy(&raw).into_owned(),
                }
            }
            _ => String::from_utf8_lossy(&raw).into_owned(),
        },
        Encoding::Utf8 => String::from_utf8_lossy(&raw).into_owned(),
        Encoding::Cp1251 => cp1251(&raw).0,
        Encoding::Latin1 => raw.iter().map(|byte| *byte as char).collect(),
    }
}

/// Text from a MIDI file's bytes, its encoding guessed, whitespace folded.
pub fn decode(data: &[u8]) -> String {
    text_of(data, encoding_of(data)).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One part: its number in the list, where it comes from, its name and program, and the notes it plays.
#[derive(Clone, Debug)]
pub struct Part {
    pub number: usize,
    pub channel: u8,
    pub notes: Vec<smf::Note>,
    pub program: u8,
    pub name: String,
    pub median: f64,
}

impl Part {
    fn new(number: usize, channel: u8, notes: Vec<smf::Note>, program: u8, name: String) -> Part {
        let mut pitches: Vec<u8> = notes.iter().map(|note| note.pitch).collect();
        pitches.sort_unstable();
        let middle = pitches.len() / 2;
        let median = if pitches.len() % 2 == 1 { pitches[middle] as f64 } else { (pitches[middle - 1] as f64 + pitches[middle] as f64) / 2.0 };
        Part { number, channel, notes, program, name, median }
    }

    pub fn drums(&self) -> bool {
        self.channel == smf::DRUM_CHANNEL
    }

    pub fn family(&self) -> &'static str {
        if self.drums() { "Drums" } else { FAMILIES[(self.program / 8) as usize] }
    }

    pub fn bass(&self) -> bool {
        !self.drums() && ((32..40).contains(&self.program) || self.median < LOWEST_MELODY)
    }

    /// The most notes the part sounds at once.
    pub fn polyphony(&self) -> usize {
        let mut edges: Vec<(u64, i32)> = self.notes.iter().flat_map(|note| [(note.start, 1), (note.end, -1)]).collect();
        edges.sort_unstable();
        let (mut most, mut sounding) = (0, 0);
        for (_, step) in edges {
            sounding += step;
            most = most.max(sounding);
        }
        most as usize
    }

    /// The share of notes struck together with another note of the part.
    pub fn chord_share(&self) -> f64 {
        let mut onsets: BTreeMap<u64, usize> = BTreeMap::new();
        for note in &self.notes {
            *onsets.entry(note.start).or_default() += 1;
        }
        onsets.values().filter(|count| **count > 1).sum::<usize>() as f64 / self.notes.len() as f64
    }

    pub fn label(&self) -> String {
        format!("{} {}", self.number, if self.name.is_empty() { self.family() } else { &self.name })
    }

    fn is_chords(&self) -> bool {
        chords_name().is_match(&self.name)
    }
}

/// Every part of the song that plays a note, numbered from 1.
pub fn parts(song: &smf::Song) -> Vec<Part> {
    let mut programs: BTreeMap<u8, u8> = BTreeMap::new();
    for track in &song.tracks {
        for (channel, program) in &track.programs {
            programs.entry(*channel).or_insert(*program);
        }
    }
    let mut found: Vec<Part> = Vec::new();
    for track in &song.tracks {
        let mut channels: Vec<u8> = track.notes.iter().map(|note| note.channel).collect();
        channels.sort_unstable();
        channels.dedup();
        let name = decode(&track.name);
        for channel in &channels {
            let notes: Vec<smf::Note> = track.notes.iter().filter(|note| note.channel == *channel).copied().collect();
            let label = if channels.len() > 1 {
                format!("{}channel {}", if name.is_empty() { String::new() } else { format!("{name} ") }, channel + 1)
            } else {
                name.clone()
            };
            let program = track.programs.get(channel).or_else(|| programs.get(channel)).copied().unwrap_or(0);
            found.push(Part::new(found.len() + 1, *channel, notes, program, label));
        }
    }
    found
}

/// The parts as a short list for a message: `1 Lead (Synth Lead), 2 Bass (Bass)`.
pub fn listing(found: &[Part]) -> String {
    found.iter().map(|part| format!("{} ({})", part.label(), part.family())).collect::<Vec<_>>().join(", ")
}

/// Which part a line takes: picked by the file's own judgement, by number, or left empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Auto,
    None,
    Number(usize),
}

impl Pick {
    pub fn parse(value: Option<&str>) -> Result<Pick, String> {
        match value.map(str::trim).unwrap_or("auto") {
            "" | "auto" => Ok(Pick::Auto),
            "none" => Ok(Pick::None),
            other => other.parse::<usize>().map(Pick::Number).map_err(|_| format!("'{other}' is not 'auto' or a track number")),
        }
    }
}

fn numbered(found: &[Part], pick: Pick, line: &str) -> Result<Option<usize>, String> {
    let Pick::Number(number) = pick else {
        return Ok(None);
    };
    match found.iter().position(|part| part.number == number) {
        Some(index) if found[index].drums() => Err(format!("The {line} is track {number}, which is drums: drums have no tune to sing. The file's tracks: {}", listing(found))),
        Some(index) => Ok(Some(index)),
        None => Err(format!(
            "The {line} is track {number}, but this file has {} with notes: {}",
            if found.len() == 1 { "one track".to_string() } else { format!("{} tracks", found.len()) },
            listing(found)
        )),
    }
}

fn onset_share(part: &Part, ticks: &[u64], tolerance: u64) -> f64 {
    let mut onsets: Vec<u64> = part.notes.iter().map(|note| note.start).collect();
    onsets.sort_unstable();
    let hits = ticks
        .iter()
        .filter(|tick| {
            let at = onsets.partition_point(|onset| *onset < tick.saturating_sub(tolerance));
            at < onsets.len() && onsets[at] <= **tick + tolerance
        })
        .count();
    if ticks.is_empty() { 0.0 } else { hits as f64 / ticks.len() as f64 }
}

/// The first part with the greatest value, the way Python's `max` picks.
fn first_max(candidates: &[usize], value: impl Fn(usize) -> f64) -> usize {
    let mut best = candidates[0];
    for &index in &candidates[1..] {
        if value(index) > value(best) {
            best = index;
        }
    }
    best
}

/// None when every part left is named an instrument: the score then keeps its voice silent.
fn auto_voice(found: &[Part], pitched: &[usize], syllables: &[u64], division: u32) -> Option<(usize, &'static str)> {
    let named_chords: Vec<usize> = pitched.iter().copied().filter(|index| !found[*index].is_chords()).collect();
    let usable = if named_chords.is_empty() { pitched.to_vec() } else { named_chords };
    if !syllables.is_empty() {
        let tolerance = (division as u64 / 8).max(1);
        let best = first_max(&usable, |index| onset_share(&found[index], syllables, tolerance));
        if onset_share(&found[best], syllables, tolerance) >= KARAOKE_SHARE {
            return Some((best, "karaoke"));
        }
    }
    let named: Vec<usize> = usable.iter().copied().filter(|index| voice_name().is_match(&found[*index].name) && !found[*index].bass()).collect();
    if !named.is_empty() {
        return Some((first_max(&named, |index| found[index].notes.len() as f64), "name"));
    }
    if usable.iter().all(|index| instrument_name().is_match(&found[*index].name)) {
        return None;
    }
    let busiest = usable.iter().map(|index| found[*index].notes.len()).max().unwrap_or(0);
    let melodic: Vec<usize> = usable.iter().copied().filter(|index| !found[*index].bass() && found[*index].notes.len() as f64 >= BUSY_SHARE * busiest as f64).collect();
    let pool = if melodic.is_empty() { usable } else { melodic };
    Some((first_max(&pool, |index| found[index].median - CHORD_PENALTY * found[index].chord_share()), "highest"))
}

fn auto_instrument(found: &[Part], rest: &[usize]) -> Option<(usize, &'static str)> {
    let usable: Vec<usize> = rest.iter().copied().filter(|index| !found[*index].is_chords()).collect();
    let named: Vec<usize> = usable.iter().copied().filter(|index| instrument_name().is_match(&found[*index].name)).collect();
    if !named.is_empty() {
        return Some((first_max(&named, |index| found[index].notes.len() as f64), "name"));
    }
    let busiest = usable.iter().map(|index| found[*index].notes.len()).max()?;
    let lines: Vec<usize> = usable
        .iter()
        .copied()
        .filter(|index| {
            let part = &found[*index];
            !part.bass() && part.median >= INSTRUMENT_LOWEST && part.chord_share() <= CHORD_LINE && part.notes.len() as f64 >= BUSY_SHARE * busiest as f64
        })
        .collect();
    if lines.is_empty() {
        return None;
    }
    Some((first_max(&lines, |index| found[index].notes.len() as f64 * (1.0 - found[index].chord_share())), "busiest"))
}

/// The parts the voice and the instrument take, as indices into the parts, and how each was chosen.
#[derive(Clone, Debug)]
pub struct Chosen {
    pub voice: Option<usize>,
    pub instrument: Option<usize>,
    pub voice_why: &'static str,
    pub instrument_why: &'static str,
}

/// The parts the two lines take, or why they cannot: a number the file does not have, drums, one part
/// asked to be both, or a file with nothing but drums.
pub fn choose(found: &[Part], vocal: Pick, instrument: Pick, syllables: &[u64], division: u32) -> Result<Chosen, String> {
    let pitched: Vec<usize> = (0..found.len()).filter(|index| !found[*index].drums()).collect();
    if pitched.is_empty() {
        return Err(format!("the file has no part with pitched notes{}", if found.is_empty() { String::new() } else { format!(", only drums: {}", listing(found)) }));
    }
    let (voice, voice_why) = match numbered(found, vocal, "voice track")? {
        Some(index) => (Some(index), "chosen"),
        None => auto_voice(found, &pitched, syllables, division).map_or((None, ""), |(index, why)| (Some(index), why)),
    };
    let (instrument, instrument_why) = if instrument == Pick::None {
        (None, "")
    } else {
        match numbered(found, instrument, "instrument track")? {
            Some(index) if Some(index) == voice => {
                return Err(format!("The voice and the instrument are both track {}; one part cannot be both lines of the score", found[index].number));
            }
            Some(index) => (Some(index), "chosen"),
            None => {
                let rest: Vec<usize> = pitched.iter().copied().filter(|index| Some(*index) != voice).collect();
                match auto_instrument(found, &rest) {
                    Some((index, why)) => (Some(index), why),
                    None => (None, ""),
                }
            }
        }
    };
    Ok(Chosen { voice, instrument, voice_why, instrument_why })
}

/// The first part named as the chords, the way "Save as MIDI" names them; drums are never it.
pub fn chords_part(found: &[Part]) -> Option<usize> {
    found.iter().position(|part| !part.drums() && part.is_chords())
}

/// One part as the list of the file's tracks shows it.
#[derive(Clone, Debug, Serialize)]
pub struct Row {
    pub number: usize,
    pub name: String,
    pub family: &'static str,
    pub notes: usize,
    pub low: u8,
    pub high: u8,
    pub drums: bool,
    pub polyphony: usize,
    pub role: &'static str,
    pub why: &'static str,
}

pub fn describe(found: &[Part], chosen: Option<&Chosen>) -> Vec<Row> {
    found
        .iter()
        .enumerate()
        .map(|(index, part)| {
            let (role, why) = match chosen {
                Some(chosen) if chosen.voice == Some(index) => ("voice", chosen.voice_why),
                Some(chosen) if chosen.instrument == Some(index) => ("instrument", chosen.instrument_why),
                _ => ("", ""),
            };
            Row {
                number: part.number,
                name: part.name.clone(),
                family: part.family(),
                notes: part.notes.len(),
                low: part.notes.iter().map(|note| note.pitch).min().unwrap_or(0),
                high: part.notes.iter().map(|note| note.pitch).max().unwrap_or(0),
                drums: part.drums(),
                polyphony: part.polyphony(),
                role,
                why,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_1251_is_read_as_python_reads_it() {
        let expected = "\u{0402}\u{0403}\u{201a}\u{0453}\u{201e}\u{2026}\u{2020}\u{2021}\u{20ac}\u{2030}\u{0409}\u{2039}\u{040a}\u{040c}\u{040b}\u{040f}\u{0452}\u{2018}\u{2019}\u{201c}\u{201d}\u{2022}\u{2013}\u{2014}\u{fffd}\u{2122}\u{0459}\u{203a}\u{045a}\u{045c}\u{045b}\u{045f}\u{00a0}\u{040e}\u{045e}\u{0408}\u{00a4}\u{0490}\u{00a6}\u{00a7}\u{0401}\u{00a9}\u{0404}\u{00ab}\u{00ac}\u{00ad}\u{00ae}\u{0407}\u{00b0}\u{00b1}\u{0406}\u{0456}\u{0491}\u{00b5}\u{00b6}\u{00b7}\u{0451}\u{2116}\u{0454}\u{00bb}\u{0458}\u{0405}\u{0455}\u{0457}\u{0410}\u{0411}\u{0412}\u{0413}\u{0414}\u{0415}\u{0416}\u{0417}\u{0418}\u{0419}\u{041a}\u{041b}\u{041c}\u{041d}\u{041e}\u{041f}\u{0420}\u{0421}\u{0422}\u{0423}\u{0424}\u{0425}\u{0426}\u{0427}\u{0428}\u{0429}\u{042a}\u{042b}\u{042c}\u{042d}\u{042e}\u{042f}\u{0430}\u{0431}\u{0432}\u{0433}\u{0434}\u{0435}\u{0436}\u{0437}\u{0438}\u{0439}\u{043a}\u{043b}\u{043c}\u{043d}\u{043e}\u{043f}\u{0440}\u{0441}\u{0442}\u{0443}\u{0444}\u{0445}\u{0446}\u{0447}\u{0448}\u{0449}\u{044a}\u{044b}\u{044c}\u{044d}\u{044e}\u{044f}";
        let bytes: Vec<u8> = (0x80..=0xFF).collect();
        assert_eq!(cp1251(&bytes).0, expected);
    }

    #[test]
    fn names_are_read_in_the_encoding_they_were_written_in() {
        assert_eq!(decode("Вокал".as_bytes()), "Вокал");
        let windows: Vec<u8> = encoding_rs::WINDOWS_1251.encode("Вокал партия").0.into_owned();
        assert_eq!(encoding_of(&windows), Encoding::Cp1251);
        assert_eq!(decode(&windows), "Вокал партия");
        let converted: String = windows.iter().map(|byte| *byte as char).collect();
        assert_eq!(encoding_of(converted.as_bytes()), Encoding::DoubleEncoded);
        assert_eq!(decode(converted.as_bytes()), "Вокал партия");
        assert_eq!(decode(b"Caf\xe9  Lead"), "Caf\u{e9} Lead");
    }

    fn note(start: u64, end: u64, pitch: u8, channel: u8) -> smf::Note {
        smf::Note { start, end, pitch, velocity: 100, channel }
    }

    fn song(tracks: Vec<(&str, Vec<smf::Note>, u8)>) -> smf::Song {
        let tracks = tracks
            .into_iter()
            .map(|(name, notes, program)| {
                let mut track = smf::Track { name: name.as_bytes().to_vec(), notes, ..smf::Track::default() };
                if let Some(channel) = track.notes.first().map(|note| note.channel) {
                    track.programs.insert(channel, program);
                }
                track
            })
            .collect();
        smf::Song { division: 480, tracks, ..smf::Song::default() }
    }

    #[test]
    fn the_voice_is_the_named_melody_and_the_instrument_the_busiest_line_above_g3() {
        let melody: Vec<smf::Note> = (0..8).map(|index| note(index * 480, index * 480 + 480, 72 + (index % 3) as u8, 0)).collect();
        let pad: Vec<smf::Note> = (0..4).flat_map(|index| [note(index * 960, index * 960 + 960, 76, 1), note(index * 960, index * 960 + 960, 79, 1)]).collect();
        let lead: Vec<smf::Note> = (0..16).map(|index| note(index * 240, index * 240 + 240, 67 + (index % 5) as u8, 2)).collect();
        let bass: Vec<smf::Note> = (0..8).map(|index| note(index * 480, index * 480 + 480, 36, 3)).collect();
        let drums: Vec<smf::Note> = (0..8).map(|index| note(index * 480, index * 480 + 120, 36, 9)).collect();
        let song = song(vec![("Melody", melody, 0), ("Pad", pad, 88), ("Guitar", lead, 25), ("Bass", bass, 33), ("Drums", drums, 0)]);
        let found = parts(&song);
        let chosen = choose(&found, Pick::Auto, Pick::Auto, &[], 480).unwrap();
        assert_eq!((found[chosen.voice.unwrap()].number, chosen.voice_why), (1, "name"));
        assert_eq!(chosen.instrument.map(|index| (found[index].number, chosen.instrument_why)), Some((3, "busiest")));
        assert!(choose(&found, Pick::Number(5), Pick::Auto, &[], 480).unwrap_err().contains("drums"));
        assert!(choose(&found, Pick::Number(2), Pick::Number(2), &[], 480).unwrap_err().contains("both track 2"));
        assert!(choose(&found, Pick::Number(9), Pick::Auto, &[], 480).unwrap_err().contains("5 tracks with notes"));
    }

    #[test]
    fn a_file_of_instrument_lines_keeps_the_voice_silent() {
        let line: Vec<smf::Note> = (0..8).map(|index| note(index * 480, index * 480 + 480, 72 + (index % 3) as u8, 0)).collect();
        let chords: Vec<smf::Note> = (0..2).flat_map(|index| [note(index * 1920, index * 1920 + 1920, 60, 2), note(index * 1920, index * 1920 + 1920, 64, 2), note(index * 1920, index * 1920 + 1920, 67, 2)]).collect();
        let file = song(vec![("Ins", line.clone(), 0), ("Chords", chords, 48)]);
        let found = parts(&file);
        let chosen = choose(&found, Pick::Auto, Pick::Auto, &[], 480).unwrap();
        assert_eq!(chosen.voice, None);
        assert_eq!(chosen.instrument.map(|index| found[index].number), Some(1));
        let named = song(vec![("Vocal", line, 0)]);
        assert!(choose(&parts(&named), Pick::Auto, Pick::Auto, &[], 480).unwrap().voice.is_some());
    }
}
