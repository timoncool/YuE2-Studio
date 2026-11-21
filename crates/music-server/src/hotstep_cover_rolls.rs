//! YuE2 lead sheets as the version-1 HOT-Step MuLaCover rolls.
use crate::score::{Q, abc, export::chord_pitches};
use anyhow::Result;
use serde::Serialize;

const MAX_STEPS: usize = 5_000;

#[derive(Debug, Serialize)]
pub(super) struct Rolls {
    version: u8,
    dtype: &'static str,
    bpm: u32,
    pub length: usize,
    pianoroll: Vec<u8>,
    chord: Vec<u8>,
    drum_pianoroll: Vec<u8>,
    context_mask: Vec<u8>,
}

fn position(quarter: Q, sixteenths: bool) -> Result<usize> {
    let quarters = quarter.to_f64();
    let rows = if sixteenths {
        (quarters * 4.0).round()
    } else {
        quarters.round_ties_even() * 4.0
    };
    anyhow::ensure!(
        (0.0..=MAX_STEPS as f64).contains(&rows),
        "The lead sheet exceeds {MAX_STEPS} sixteenth notes. Shorten it before generating the cover."
    );
    Ok(rows as usize)
}

pub(super) fn from_abc(text: &str) -> Result<Rolls> {
    let score = abc::parse(text).map_err(anyhow::Error::msg)?;
    anyhow::ensure!(
        (1..=1000).contains(&score.bpm),
        "The lead sheet needs a valid tempo."
    );
    let vocal = &score.voices[abc::VOCAL];
    anyhow::ensure!(
        !vocal.notes.is_empty(),
        "The lead sheet needs a vocal melody."
    );
    let harmony = &vocal.chords;
    anyhow::ensure!(!harmony.is_empty(), "The lead sheet needs chord symbols.");

    let notes = vocal
        .notes
        .iter()
        .map(|note| {
            let start = position(note.start, true)?;
            let stop = position(note.start + note.duration, true)?.max(start + 1);
            let pitch = usize::try_from(note.pitch)
                .ok()
                .filter(|pitch| *pitch <= 127)
                .ok_or_else(|| {
                    anyhow::anyhow!("The lead sheet contains a note outside the MIDI range.")
                })?;
            anyhow::ensure!(
                stop <= MAX_STEPS,
                "The lead sheet exceeds {MAX_STEPS} sixteenth notes."
            );
            Ok((start, stop, pitch))
        })
        .collect::<Result<Vec<_>>>()?;
    let length = notes
        .iter()
        .map(|(_, stop, _)| *stop)
        .max()
        .unwrap_or_default()
        .max(position(vocal.time, true)?);
    let mut roll = Rolls {
        version: 1,
        dtype: "float32",
        bpm: score.bpm,
        length,
        pianoroll: vec![0; length * 256],
        chord: vec![0; length * 12],
        drum_pianoroll: vec![0; length * 128],
        context_mask: vec![0; length],
    };
    for (start, stop, pitch) in notes {
        roll.pianoroll[start * 256 + pitch * 2] = 1;
        for row in start + 1..stop {
            roll.pianoroll[row * 256 + pitch * 2 + 1] = 1;
        }
    }
    for (index, (start, name)) in harmony.iter().enumerate() {
        let start = position(*start, false)?.min(length);
        let stop = match harmony.get(index + 1) {
            Some((next, _)) => position(*next, false)?.min(length),
            None => length,
        };
        if matches!(
            name.trim().to_ascii_uppercase().as_str(),
            "N" | "NC" | "N.C." | "NONE"
        ) {
            continue;
        }
        for pitch in chord_pitches(name).map_err(anyhow::Error::msg)? {
            let chroma = pitch.rem_euclid(12) as usize;
            for row in start..stop {
                roll.chord[row * 12 + chroma] = 1;
            }
        }
    }
    anyhow::ensure!(
        roll.chord.contains(&1),
        "The lead sheet needs at least one sounding chord."
    );
    Ok(roll)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotstep_cover_rolls_encode_melody_attacks_holds_and_chroma() {
        let text = sheet("\"C\"C E G c|\"F\"F A c a|", "Z|Z|");
        let roll = from_abc(&text).unwrap();
        assert_eq!((roll.bpm, roll.length), (120, 32));
        assert_eq!(roll.pianoroll[60 * 2], 1);
        assert_eq!(roll.pianoroll[256 + 60 * 2 + 1], 1);
        assert_eq!(&roll.chord[..12], &[1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0]);
        assert_eq!(
            &roll.chord[16 * 12..17 * 12],
            &[1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0]
        );
        assert_eq!(roll.drum_pianoroll.len(), 32 * 128);
        assert_eq!(roll.context_mask.len(), 32);
    }

    fn sheet(vocal: &str, ins: &str) -> String {
        let [voice, instrument] = abc::VOICE_LINES;
        format!(
            "X:1\nT:\nM:4/4\nL:1/4\nQ:1/4=120\n{voice}\n{instrument}\nK:C\nV: Vocal\n{vocal}\nV: Ins\n{ins}\n"
        )
    }

    #[test]
    fn hotstep_cover_rolls_refuse_unconditioned_lead_sheets() {
        let no_chords = from_abc(&sheet("C E G c|", "Z|")).unwrap_err();
        assert!(no_chords.to_string().contains("chord symbols"));
        let no_melody = from_abc(&sheet("Z|", "C E G c|")).unwrap_err();
        assert!(no_melody.to_string().contains("vocal melody"));
    }
}
