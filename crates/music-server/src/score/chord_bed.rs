//! A chord bed: a score the studio writes at once from a tempo, a key and a
//! meter, so a song can be sung without the planning stage. Every section of
//! the lyrics gets bars of chords over rests in the Vocal voice and an empty
//! Ins voice; the model writes the melody and the arrangement over that
//! harmony. The chords come from progressions common in pop, rock, dance, jazz
//! and film music, one for the verses, another for the choruses and a third
//! for a bridge, and no two neighbouring bars share a chord: it is the changes
//! that hold the song to the tempo. After gary4juce's yuey scaffold.

use super::{phrasing, rebuild, spelling};

/// One chord of a progression: semitones above the tonic and its quality label.
type Step = (i32, &'static str);

const MAJOR: [&[Step]; 15] = [
    &[(0, "maj"), (7, "maj"), (9, "min"), (5, "maj")],
    &[(0, "maj"), (9, "min"), (5, "maj"), (7, "maj")],
    &[(9, "min"), (5, "maj"), (0, "maj"), (7, "maj")],
    &[(0, "maj"), (5, "maj"), (7, "maj"), (5, "maj")],
    &[(2, "min7"), (7, "7"), (0, "maj7"), (9, "min7")],
    &[(0, "maj"), (7, "maj"), (9, "min"), (4, "min"), (5, "maj"), (0, "maj"), (5, "maj"), (7, "maj")],
    &[(0, "maj"), (5, "maj")],
    &[(0, "maj"), (10, "maj")],
    &[(0, "maj7"), (5, "maj7")],
    &[(5, "maj"), (0, "maj"), (7, "maj"), (9, "min")],
    &[(0, "maj"), (4, "min"), (5, "maj"), (7, "maj")],
    &[(0, "maj"), (7, "maj"), (5, "maj"), (7, "maj")],
    &[(9, "min"), (7, "maj"), (5, "maj"), (7, "maj")],
    &[(0, "maj"), (10, "maj"), (5, "maj"), (0, "maj")],
    &[(5, "maj"), (7, "maj"), (4, "min"), (9, "min")],
];

const MINOR: [&[Step]; 16] = [
    &[(0, "min"), (8, "maj"), (3, "maj"), (10, "maj")],
    &[(0, "min"), (5, "min"), (10, "maj"), (3, "maj")],
    &[(0, "min"), (10, "maj"), (8, "maj"), (7, "maj")],
    &[(2, "hdim7"), (7, "7"), (0, "min7"), (8, "maj7")],
    &[(0, "min"), (5, "maj")],
    &[(0, "min7"), (5, "min7")],
    &[(0, "min"), (8, "maj"), (10, "maj"), (8, "maj")],
    &[(0, "min"), (5, "min"), (7, "min"), (5, "min")],
    &[(0, "min"), (10, "maj"), (8, "maj"), (10, "maj")],
    &[(8, "maj"), (10, "maj"), (0, "min"), (10, "maj")],
    &[(0, "min"), (5, "min"), (8, "maj"), (7, "maj")],
    &[(0, "min"), (3, "maj"), (10, "maj"), (8, "maj")],
    &[(0, "min"), (7, "min"), (8, "maj"), (5, "min")],
    &[(0, "min"), (8, "maj"), (5, "min"), (7, "maj")],
    &[(5, "min"), (7, "min"), (0, "min"), (10, "maj")],
    &[(0, "min"), (10, "maj"), (5, "min"), (0, "min")],
];

/// The default note length every bed is written in.
const UNITS_PER_WHOLE: i32 = 16;
/// Bars a line of lyrics gets, and the bars of a section are a whole number of fours.
const BARS_PER_LINE: usize = 2;
const BARS_PER_ROW: usize = 4;
const LONGEST_SECTION: usize = 16;
const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BedRequest {
    pub bpm: u32,
    /// A tonic and `m` for minor: `Em`, `F#m`, `Bb`.
    pub key: String,
    /// `4/4`, `3/4`, `6/8` or `2/4`.
    pub meter: String,
    pub lyrics: String,
    /// Picks the progressions; the same seed writes the same bed.
    pub seed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Bed {
    pub abc: String,
    pub bars: usize,
    /// The progression of the verses, the choruses and a bridge, as chord symbols.
    pub progressions: Vec<Vec<String>>,
}

/// A small deterministic generator: the seed picks the progressions, nothing else draws.
struct Draw(u64);

impl Draw {
    fn next(&mut self, below: usize) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) % below as u64) as usize
    }
}

fn parse_key(key: &str) -> Result<(i32, bool), String> {
    let key = key.trim();
    let minor = key.ends_with('m');
    let tonic = key.strip_suffix('m').unwrap_or(key);
    let mut characters = tonic.chars();
    let natural: i32 = match characters.next() {
        Some('C') => 0,
        Some('D') => 2,
        Some('E') => 4,
        Some('F') => 5,
        Some('G') => 7,
        Some('A') => 9,
        Some('B') => 11,
        _ => return Err(format!("'{key}' is not a key: write a tonic from A to G, an optional # or b, and m for minor, as in Em or Bb")),
    };
    let alteration = match characters.as_str() {
        "" => 0,
        "#" => 1,
        "b" => -1,
        _ => return Err(format!("'{key}' is not a key: write a tonic from A to G, an optional # or b, and m for minor, as in Em or Bb")),
    };
    Ok(((natural + alteration).rem_euclid(12), minor))
}

/// The bar of a meter in sixteenths.
fn bar_units(meter: &str) -> Result<i32, String> {
    let (beats, unit) = meter.trim().split_once('/').ok_or_else(|| format!("'{meter}' is not a meter: use 4/4, 3/4, 6/8 or 2/4"))?;
    match (beats.trim().parse::<i32>(), unit.trim().parse::<i32>()) {
        (Ok(beats @ 1..=12), Ok(unit @ (2 | 4 | 8))) => Ok(beats * UNITS_PER_WHOLE / unit),
        _ => Err(format!("'{meter}' is not a meter: use 4/4, 3/4, 6/8 or 2/4")),
    }
}

/// How many bars a section gets: its lines of words, or the length of a section without words.
fn section_bars(label: &str, lines: usize) -> usize {
    if lines > 0 {
        return ((lines * BARS_PER_LINE).div_ceil(BARS_PER_ROW).max(1) * BARS_PER_ROW).min(LONGEST_SECTION);
    }
    match label {
        "interlude" | "instrumental" | "solo" | "development" => 8,
        _ => 4,
    }
    .min(LONGEST_SECTION)
}

/// Which progression a section plays: verses and everything else the first,
/// choruses the second, a bridge the third.
fn progression_of(label: &str) -> usize {
    match label {
        "chorus" | "post-chorus" | "pre-chorus and chorus" | "loop" => 1,
        "bridge" | "pre-outro" | "development" => 2,
        _ => 0,
    }
}

pub fn write(request: &BedRequest) -> Result<Bed, String> {
    if !(40..=240).contains(&request.bpm) {
        return Err("the tempo must be between 40 and 240 BPM".into());
    }
    let (tonic, minor) = parse_key(&request.key)?;
    let units = bar_units(&request.meter)?;
    let mode = if minor { "minor" } else { "major" };
    let key_label = spelling::key_name(&format!("{}:{mode}", NAMES[tonic as usize]));
    let key_text = format!("{}{}", key_label.split(':').next().unwrap_or("C"), if minor { "m" } else { "" });

    let pool: &[&[Step]] = if minor { &MINOR } else { &MAJOR };
    let mut draw = Draw(request.seed ^ 0x9e37_79b9_7f4a_7c15);
    let mut picked: Vec<usize> = Vec::new();
    while picked.len() < 3 {
        let index = draw.next(pool.len());
        if !picked.contains(&index) {
            picked.push(index);
        }
    }
    let symbol = |step: &Step| -> Result<String, String> {
        let label = format!("{}:{}", NAMES[((tonic + step.0).rem_euclid(12)) as usize], step.1);
        rebuild::chord_text(&spelling::chord_name(&label, &key_label))?.ok_or_else(|| format!("no chord symbol for {label}"))
    };
    let progressions: Vec<Vec<String>> =
        picked.iter().map(|index| pool[*index].iter().map(symbol).collect::<Result<Vec<_>, _>>()).collect::<Result<_, _>>()?;

    let mut sections = phrasing::section_lines(&request.lyrics);
    if sections.is_empty() {
        sections = vec![("verse".to_string(), 2), ("chorus".to_string(), 2), ("verse".to_string(), 2), ("chorus".to_string(), 2)];
    }
    let mut abc = format!(
        "X:1\nT:\nM:{}\nL:1/{UNITS_PER_WHOLE}\nQ:1/4={}\nV: Vocal clef=treble name=\"Vocal Melody\" snm=\"Vocal\"\nV: Ins clef=treble name=\"Ins Melody\" snm=\"Inst.\"\nK:{key_text}\n",
        request.meter.trim(),
        request.bpm
    );
    let mut previous: Option<&str> = None;
    let mut total = 0;
    for (label, lines) in &sections {
        let chords = &progressions[progression_of(label)];
        let bars = section_bars(label, *lines);
        abc.push_str(&format!("% {label}\n"));
        let mut at = 0;
        let mut row: Vec<String> = Vec::new();
        for _ in 0..bars {
            let mut chord = chords[at % chords.len()].as_str();
            if previous == Some(chord) {
                at += 1;
                chord = chords[at % chords.len()].as_str();
            }
            at += 1;
            row.push(format!("\"{chord}\"z{units}"));
            previous = Some(chord);
            if row.len() == BARS_PER_ROW {
                abc.push_str(&format!("V: Vocal\n{}|\nV: Ins\nZ{}|\n", row.join("|"), row.len()));
                row.clear();
            }
        }
        if !row.is_empty() {
            abc.push_str(&format!("V: Vocal\n{}|\nV: Ins\nZ{}|\n", row.join("|"), row.len()));
        }
        total += bars;
    }
    Ok(Bed { abc, bars: total, progressions })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(key: &str, meter: &str, lyrics: &str) -> BedRequest {
        BedRequest { bpm: 96, key: key.into(), meter: meter.into(), lyrics: lyrics.into(), seed: 7 }
    }

    fn chords(abc: &str) -> Vec<String> {
        abc.lines().filter(|line| !line.starts_with("V:")).flat_map(|line| line.split('"').skip(1).step_by(2).map(str::to_owned).collect::<Vec<_>>()).collect()
    }

    #[test]
    fn every_lyrics_section_gets_its_bars_and_no_chord_repeats_its_neighbour() {
        let bed = write(&request("Em", "4/4", "[Intro]\n[Verse 1]\nwalk on\nrun home\nsing loud\n[Chorus]\nhold on\nfire\n[Bridge]\nagain\n[Outro]")).unwrap();
        let sections: Vec<&str> = bed.abc.lines().filter(|line| line.starts_with("% ")).collect();
        assert_eq!(sections, vec!["% intro", "% verse", "% chorus", "% bridge", "% outro"]);
        assert_eq!(bed.bars, 4 + 8 + 4 + 4 + 4);
        let all = chords(&bed.abc);
        assert_eq!(all.len(), bed.bars);
        assert!(all.windows(2).all(|pair| pair[0] != pair[1]), "{all:?}");
        assert!(bed.abc.contains("K:Em\n") && bed.abc.contains("M:4/4\nL:1/16\nQ:1/4=96\n"));
        assert!(bed.abc.contains("z16|") && bed.abc.contains("V: Ins\nZ4|"));
        assert_eq!(bed.progressions.len(), 3);
        assert!(bed.progressions[0] != bed.progressions[1], "verse and chorus play different progressions");
    }

    #[test]
    fn the_key_names_its_chords_and_the_meter_its_bars() {
        let bed = write(&request("A#m", "3/4", "[Verse]\nwalking down")).unwrap();
        assert!(bed.abc.contains("K:Bbm\n"), "{}", bed.abc);
        assert!(chords(&bed.abc).iter().all(|chord| !chord.contains('#')), "flats in B-flat minor: {:?}", chords(&bed.abc));
        assert!(bed.abc.contains("z12|"));
        assert!(write(&request("F", "6/8", "")).unwrap().abc.contains("z12|"));
    }

    #[test]
    fn the_same_seed_writes_the_same_bed_and_a_planned_score_reads_it() {
        assert_eq!(write(&request("C", "4/4", "[Verse]\nwalk on")).unwrap(), write(&request("C", "4/4", "[Verse]\nwalk on")).unwrap());
        let other = BedRequest { seed: 8, ..request("C", "4/4", "[Verse]\nwalk on\n[Chorus]\nhold on") };
        assert!(write(&other).is_ok());
        let bed = write(&request("D", "4/4", "")).unwrap();
        assert_eq!(bed.bars, 16, "no lyrics: verse, chorus, verse, chorus");
        assert!(crate::score::abc::parse(&bed.abc).is_ok(), "{}", bed.abc);
        assert!(crate::score::notation::read(&bed.abc).is_ok(), "{}", bed.abc);
    }

    #[test]
    fn a_bad_tempo_key_or_meter_is_named() {
        assert!(write(&BedRequest { bpm: 20, ..request("C", "4/4", "") }).unwrap_err().contains("tempo"));
        assert!(write(&request("H", "4/4", "")).unwrap_err().contains("not a key"));
        assert!(write(&request("C", "5/3", "")).unwrap_err().contains("not a meter"));
    }
}
