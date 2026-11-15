//! A score written out as a MIDI file, what "Save as MIDI" hands over. A
//! conductor track carries the tempo, every change of meter at the bar it
//! starts in, every key signature and a marker at each named section. A track
//! follows for each line that sounds - `Vocal` on channel 1 with a voice
//! sound, `Ins` on channel 2 with a piano - and, where the score has chord
//! symbols, `Chords` on channel 3 with strings holding each chord until the
//! next: the root between C3 and B3, the chord above it, a slash bass an
//! octave below. The track names are the ones the MIDI reader looks for, so a
//! file saved here and read again gives the same score.

use super::{abc, sections, smf, Q};

pub(super) const QUALITY_STEPS: [(&str, &[i32]); 15] = [
    ("", &[0, 4, 7]),
    ("m", &[0, 3, 7]),
    ("dim", &[0, 3, 6]),
    ("aug", &[0, 4, 8]),
    ("7", &[0, 4, 7, 10]),
    ("maj7", &[0, 4, 7, 11]),
    ("m7", &[0, 3, 7, 10]),
    ("dim7", &[0, 3, 6, 9]),
    ("m7b5", &[0, 3, 6, 10]),
    ("sus4", &[0, 5, 7]),
    ("sus2", &[0, 2, 7]),
    ("6", &[0, 4, 7, 9]),
    ("m6", &[0, 3, 7, 9]),
    ("7sus4", &[0, 5, 7, 10]),
    ("m(maj7)", &[0, 3, 7, 11]),
];

/// Each line of the score as a track: name, channel, General MIDI program (from 0), velocity.
const LINES: [(&str, usize, u8, u8, u8); 2] = [("Vocal", abc::VOCAL, 0, 53, 100), ("Ins", abc::INS, 1, 0, 90)];
const CHORDS: (&str, u8, u8, u8) = ("Chords", 2, 48, 64);
pub const CHORD_ROOT: i32 = 48;
const CONDUCTOR: &str = "YuE2 score";

fn pitch_class(name: &str) -> i32 {
    let natural = match name.as_bytes()[0] {
        b'C' => 0,
        b'D' => 2,
        b'E' => 4,
        b'F' => 5,
        b'G' => 7,
        b'A' => 9,
        _ => 11,
    };
    (natural + name.matches('#').count() as i32 - name.matches('b').count() as i32).rem_euclid(12)
}

/// A quarter-note time in ticks, rounded half to even as the reference does.
pub fn tick(quarters: Q) -> u64 {
    let numerator = quarters.num() * i64::from(smf::DIVISION);
    let denominator = quarters.den();
    let (whole, rest) = (numerator.div_euclid(denominator), numerator.rem_euclid(denominator));
    let rounded = if 2 * rest > denominator || (2 * rest == denominator && whole % 2 != 0) { whole + 1 } else { whole };
    rounded.max(0) as u64
}

fn split_symbol(symbol: &str) -> Option<(&str, &str, Option<&str>)> {
    let symbol = symbol.trim();
    let (head, bass) = match symbol.split_once('/') {
        Some((head, bass)) => (head, Some(bass)),
        None => (symbol, None),
    };
    let root_length = 1 + head.get(1..).map_or(0, |rest| {
        if rest.starts_with("##") || rest.starts_with("bb") {
            2
        } else if rest.starts_with('#') || rest.starts_with('b') {
            1
        } else {
            0
        }
    });
    let root = head.get(..root_length)?;
    if !matches!(root.as_bytes().first(), Some(b'A'..=b'G')) {
        return None;
    }
    if let Some(bass) = bass {
        let valid = matches!(bass.as_bytes().first(), Some(b'A'..=b'G')) && matches!(&bass[1..], "" | "#" | "##" | "b" | "bb");
        if !valid {
            return None;
        }
    }
    Some((root, &head[root_length..], bass))
}

/// The pitches a chord symbol is held as, or why it is not one the score format knows.
pub fn chord_pitches(symbol: &str) -> Result<Vec<i32>, String> {
    let unknown = || format!("'{symbol}' is not a chord symbol the score format knows");
    let (root, quality, bass) = split_symbol(symbol).ok_or_else(unknown)?;
    let steps = QUALITY_STEPS.iter().find(|(name, _)| *name == quality).map(|(_, steps)| *steps).ok_or_else(unknown)?;
    let base = CHORD_ROOT + pitch_class(root);
    let mut pitches: Vec<i32> = steps.iter().map(|step| base + step).collect();
    if let Some(bass) = bass {
        pitches.insert(0, CHORD_ROOT - 12 + pitch_class(bass));
    }
    Ok(pitches)
}

/// The score as a format-1 MIDI file, or why it cannot be read.
pub fn midi_of(text: &str) -> Result<Vec<u8>, String> {
    midi_with(text, |_| true)
}

/// The score as a MIDI file holding the conductor track and only the lines `keep` names
/// (`Vocal`, `Ins`, `Chords`), each exactly as the whole file has it.
pub fn midi_with(text: &str, keep: impl Fn(&str) -> bool) -> Result<Vec<u8>, String> {
    let score = abc::parse(text)?;
    let vocal = &score.voices[abc::VOCAL];
    let end = tick(vocal.time);
    let mut conductor = vec![(0, smf::text(smf::NAME, CONDUCTOR)), (0, smf::tempo((60e6 / f64::from(score.bpm)).round() as u32))];
    let mut meter = None;
    for bar in &vocal.bars {
        if Some(bar.meter) != meter {
            conductor.push((tick(bar.start), smf::meter(bar.meter.0, bar.meter.1)));
            meter = Some(bar.meter);
        }
    }
    let mut key: Option<&str> = None;
    for (start, name) in &vocal.keys {
        if Some(name.as_str()) != key {
            let sharps = abc::key_fifths(name).ok_or_else(|| format!("the key {name} is not one the score format knows"))?;
            conductor.push((tick(*start), smf::key(sharps, name.ends_with('m'))));
            key = Some(name);
        }
    }
    for section in sections::sections(text)? {
        if !section.label.is_empty() {
            conductor.push((tick(section.start), smf::text(smf::MARKER, &section.label)));
        }
    }
    let mut tracks = vec![conductor];
    for (name, voice, channel, program, velocity) in LINES {
        let notes = &score.voices[voice].notes;
        if notes.is_empty() || !keep(name) {
            continue;
        }
        let mut events = vec![(0, smf::text(smf::NAME, name)), (0, smf::program(channel, program))];
        for note in notes {
            events.push((tick(note.start), smf::note_on(channel, note.pitch as u8, velocity)));
            events.push((tick(note.start + note.duration), smf::note_off(channel, note.pitch as u8)));
        }
        tracks.push(events);
    }
    if !vocal.chords.is_empty() && keep(CHORDS.0) {
        let (name, channel, program, velocity) = CHORDS;
        let mut events = vec![(0, smf::text(smf::NAME, name)), (0, smf::program(channel, program))];
        for (index, (onset, symbol)) in vocal.chords.iter().enumerate() {
            let stop = vocal.chords.get(index + 1).map_or(vocal.time, |next| next.0);
            if stop <= *onset {
                continue;
            }
            for pitch in chord_pitches(symbol)? {
                events.push((tick(*onset), smf::note_on(channel, pitch as u8, velocity)));
                events.push((tick(stop), smf::note_off(channel, pitch as u8)));
            }
        }
        tracks.push(events);
    }
    smf::write(&tracks, smf::DIVISION, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chord_symbols_are_held_the_way_the_reader_names_them() {
        assert_eq!(chord_pitches("C").unwrap(), vec![48, 52, 55]);
        assert_eq!(chord_pitches("Bbm7").unwrap(), vec![58, 61, 65, 68]);
        assert_eq!(chord_pitches("F#m(maj7)/C#").unwrap(), vec![37, 54, 57, 61, 65]);
        assert!(chord_pitches("H7").is_err());
        assert!(chord_pitches("Cadd9").is_err());
    }

    #[test]
    fn ticks_round_half_to_even() {
        assert_eq!(tick(Q::new(1, 1)), 480);
        assert_eq!(tick(Q::new(1, 3)), 160);
        assert_eq!(tick(Q::new(1, 960)), 0);
        assert_eq!(tick(Q::new(3, 960)), 2);
    }
}
