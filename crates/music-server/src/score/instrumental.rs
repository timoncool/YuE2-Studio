//! A score turned instrumental the way m-a-p's yue2-instrumental skill does it:
//! every Vocal note moves to Ins with its pitch, start and length, Vocal keeps
//! only its rests and chords, and the instrument keeps what it played where
//! the voice was silent. Ins carries one note at a time, so where the two met
//! the voice wins and the instrument keeps only the part of its note outside it.

use serde::Serialize;
use serde_json::{json, Value};

use super::notation::{self, SheetNote};

#[derive(Debug, Serialize, PartialEq)]
pub struct Transfer {
    pub abc: String,
    /// Vocal notes now on Ins.
    pub moved: usize,
    /// Ins notes shortened or split around the voice.
    pub trimmed: usize,
    /// Ins notes the voice covered whole.
    pub dropped: usize,
}

/// The parts of `note` outside every interval of `voice`, sorted by start.
fn outside(note: &SheetNote, voice: &[SheetNote]) -> Vec<SheetNote> {
    let mut pieces = vec![(note.start, note.start + note.length)];
    for sung in voice {
        let (from, to) = (sung.start, sung.start + sung.length);
        pieces = pieces
            .into_iter()
            .flat_map(|(start, end)| {
                if to <= start || from >= end {
                    vec![(start, end)]
                } else {
                    [(start, from.min(end)), (to.max(start), end)].into_iter().filter(|(a, b)| b > a).collect()
                }
            })
            .collect();
    }
    pieces.into_iter().map(|(start, end)| SheetNote { start, length: end - start, pitch: note.pitch }).collect()
}

pub fn transfer(abc: &str) -> Result<Transfer, String> {
    let sheet = notation::read(abc)?;
    if sheet.notes.Vocal.is_empty() {
        return Err("The score has no Vocal notes to move: it is instrumental already.".into());
    }
    let voice = sheet.notes.Vocal.clone();
    let (mut trimmed, mut dropped) = (0, 0);
    let mut ins: Vec<SheetNote> = Vec::new();
    for note in &sheet.notes.Ins {
        let kept = outside(note, &voice);
        match kept.as_slice() {
            [] => dropped += 1,
            [one] if one == note => {}
            _ => trimmed += 1,
        }
        ins.extend(kept);
    }
    ins.extend(voice.iter().cloned());
    ins.sort_by_key(|note| (note.start, note.pitch));
    let mut edited = serde_json::to_value(&sheet).map_err(|error| error.to_string())?;
    edited["notes"] = json!({ "Vocal": Value::Array(Vec::new()), "Ins": ins });
    let written = notation::write(abc, &edited)?;
    Ok(Transfer { abc: written.abc, moved: voice.len(), trimmed, dropped })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCORE: &str = "X:1\nT:\nM:4/4\nL:1/16\nQ:1/4=120\nV: Vocal clef=treble name=\"Vocal Melody\" snm=\"Vocal\"\nV: Ins clef=treble name=\"Ins Melody\" snm=\"Inst.\"\nK:C\n% intro\nV: Vocal\nz16|z16|\nV: Ins\nC16|C16|\n% verse\nV: Vocal\n\"C\"z8C8|\"G\"D16|\nV: Ins\nG,16|Z|\n";

    #[test]
    fn the_voice_moves_to_the_instrument_and_keeps_its_chords() {
        let made = transfer(SCORE).unwrap();
        assert_eq!((made.moved, made.trimmed, made.dropped), (2, 1, 0));
        let sheet = notation::read(&made.abc).unwrap();
        assert!(sheet.notes.Vocal.is_empty());
        let ins: Vec<(i64, i64, i64)> = sheet.notes.Ins.iter().map(|note| (note.start, note.length, note.pitch)).collect();
        let before = notation::read(SCORE).unwrap();
        let sung: Vec<(i64, i64, i64)> = before.notes.Vocal.iter().map(|note| (note.start, note.length, note.pitch)).collect();
        for note in &sung {
            assert!(ins.contains(note), "{note:?} not on Ins: {ins:?}");
        }
        // the instrument's G, held under the voice's C, keeps the half before it
        let g = before.notes.Ins.iter().find(|note| note.pitch == 55).unwrap();
        assert!(ins.iter().any(|(start, length, pitch)| *pitch == 55 && *start == g.start && *length < g.length));
        assert_eq!(sheet.chords.iter().map(|chord| chord.name.as_str()).collect::<Vec<_>>(), vec!["C", "G"]);
        // no two Ins notes at once
        let mut sorted = sheet.notes.Ins.clone();
        sorted.sort_by_key(|note| note.start);
        assert!(sorted.windows(2).all(|pair| pair[0].start + pair[0].length <= pair[1].start));
    }

    #[test]
    fn an_instrumental_score_is_left_as_it_is() {
        let none = "X:1\nT:\nM:4/4\nL:1/16\nQ:1/4=120\nV: Vocal clef=treble name=\"Vocal Melody\" snm=\"Vocal\"\nV: Ins clef=treble name=\"Ins Melody\" snm=\"Inst.\"\nK:C\n% intro\nV: Vocal\nz16|\nV: Ins\nC16|\n";
        assert!(transfer(none).unwrap_err().contains("instrumental already"));
    }
}
