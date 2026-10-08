//! Moves sounding notes, keys and chord roots together, then reads the result
//! back. Uses the same invariant as YuE2-ComfyUI's transpose.py (Apache-2.0).
use super::{abc, notation, rebuild};

fn root(text: &str) -> Result<(i32, usize), String> {
    let letter = text.chars().next().filter(|c| ('A'..='G').contains(c)).ok_or("Invalid pitch name")?;
    let mut width = 1;
    let mut alteration = 0;
    for c in text[1..].chars().take_while(|c| *c == '#' || *c == 'b') {
        width += 1;
        alteration += if c == '#' { 1 } else { -1 };
    }
    Ok(((abc::natural(letter) + alteration).rem_euclid(12), width))
}

fn key(text: &str, step: i32) -> Result<String, String> {
    let (pitch, _) = root(text)?;
    let minor = text.ends_with('m');
    abc::keys().filter(|(name, _)| name.ends_with('m') == minor)
        .filter(|(name, _)| root(name).is_ok_and(|(p, _)| p == (pitch + step).rem_euclid(12)))
        .min_by_key(|(_, fifths)| (fifths.abs(), -*fifths))
        .map(|(name, _)| name.to_string()).ok_or_else(|| "Unsupported target key".into())
}

fn chord(text: &str, step: i32, flats: bool) -> Result<String, String> {
    if !abc::chord_pattern().is_match(text) { return Err("Unsupported chord symbol".into()); }
    let names = if flats { rebuild::FLAT_NAMES } else { rebuild::SHARP_NAMES };
    let (head, bass) = text.split_once('/').map_or((text, None), |(head, bass)| (head, Some(bass)));
    let (pitch, width) = root(head)?;
    let mut out = format!("{}{}", names[(pitch + step).rem_euclid(12) as usize], &head[width..]);
    if let Some(bass) = bass {
        out.push('/');
        out.push_str(names[(root(bass)?.0 + step).rem_euclid(12) as usize]);
    }
    Ok(out)
}

pub fn move_score(text: &str, step: i32) -> Result<String, String> {
    if !(-24..=24).contains(&step) { return Err("Transpose must be between -24 and 24 semitones".into()); }
    if step == 0 { return Ok(text.to_string()); }
    let mut sheet = notation::read(text)?;
    if sheet.bars.iter().any(|bar| !bar.editable) { return Err("This score changes key within a bar; transpose its notes in the MIDI editor".into()); }
    for part in [&mut sheet.notes.Vocal, &mut sheet.notes.Ins] {
        for note in part {
            note.pitch += i64::from(step);
            if !(0..=127).contains(&note.pitch) { return Err("Transposing would move a note outside MIDI pitches 0–127".into()); }
        }
    }
    for symbol in &mut sheet.chords {
        let source_key = sheet.bars.iter().rev().find(|bar| bar.start <= symbol.start).map(|bar| bar.key.as_str()).unwrap_or("C");
        let target = key(source_key, step)?;
        symbol.name = chord(&symbol.name, step, abc::key_fifths(&target).unwrap_or(0) < 0)?;
    }
    let mut source = String::new();
    for line in abc::split_keep(text) {
        if let Some(name) = line.strip_prefix("K:") {
            let body = name.trim_end_matches(['\r', '\n']);
            source.push_str(&format!("K:{}{}", key(body.trim(), step)?, &name[body.len()..]));
        } else { source.push_str(line); }
    }
    let desired = serde_json::to_value(&sheet).map_err(|error| error.to_string())?;
    let moved = notation::write(&source, &desired)?.abc;
    let read = notation::read(&moved)?;
    if read.notes != sheet.notes || read.chords != sheet.chords || read.bpm != sheet.bpm || read.total != sheet.total {
        return Err("The transposed score did not read back with the requested music".into());
    }
    Ok(moved)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tune() -> String {
        "X:1\nT:\nM:4/4\nL:1/16\nQ:1/4=120\nV: Vocal clef=treble name=\"Vocal Melody\" snm=\"Vocal\"\nV: Ins clef=treble name=\"Ins Melody\" snm=\"Inst.\"\nK:C\n% verse\nV: Vocal\n\"Am7/C\"C4D4E4F4|G4A4B4c4|Z2|\nV: Ins\nZ4|\n".to_string()
    }
    #[test]
    fn every_semitone_moves_notes_chords_and_key_without_moving_time() {
        let source = tune();
        let original = notation::read(&source).unwrap();
        assert!(!original.notes.Vocal.is_empty());
        for step in -24..=24 {
            let moved = move_score(&source, step).unwrap();
            let read = notation::read(&moved).unwrap();
            assert_eq!(read.total, original.total);
            assert_eq!(read.sections, original.sections);
            for (before, after) in original.notes.Vocal.iter().zip(&read.notes.Vocal) {
                assert_eq!(after.pitch, before.pitch + i64::from(step));
                assert_eq!((after.start, after.length), (before.start, before.length));
            }
            assert_eq!(root(&read.chords[0].name).unwrap().0, (9 + step).rem_euclid(12));
            assert_eq!(root(read.chords[0].name.split('/').nth(1).unwrap()).unwrap().0, step.rem_euclid(12));
            assert_eq!(root(&read.bars[0].key).unwrap().0, step.rem_euclid(12));
        }
    }
    #[test]
    fn zero_is_exact_and_impossible_moves_are_refused() {
        let source = tune();
        assert_eq!(move_score(&source, 0).unwrap(), source);
        assert!(move_score(&source, 25).is_err());
        assert!(move_score("not a score", 2).is_err());
    }
}
