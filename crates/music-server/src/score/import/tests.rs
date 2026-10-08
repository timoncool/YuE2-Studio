//! The scores YuE2-ComfyUI's Load MIDI wrote from four files, written again here:
//! a band with a named vocal, markers and drums; a .kar file in Windows-1251
//! with paragraphs and a repeated chorus; a score saved as MIDI by the score
//! editor, chords track and all; and a file played in by hand whose tempo and
//! meter change. `fixtures/expected.json` is the reference's own output.

use serde_json::Value;

use super::super::smf;
use super::{convert, parts::Pick, Mode};
use super::{convert_with, Options};

#[test]
fn manual_grid_and_octaves_keep_the_parts_and_move_every_note() {
    let song = smf::read(FILES[0].1).unwrap();
    let original = convert_with(&song, Mode::Full, Pick::Auto, Pick::Number(2), Options { grid: Some(32), ..Options::default() }).unwrap();
    let moved = convert_with(&song, Mode::Full, Pick::Auto, Pick::Number(2), Options { grid: Some(32), vocal_octaves: 1, instrument_octaves: -1 }).unwrap();
    assert_eq!(moved.facts.grid, 32);
    let before = super::super::notation::read(&original.abc).unwrap();
    let after = super::super::notation::read(&moved.abc).unwrap();
    assert!(!before.notes.Vocal.is_empty());
    assert!(!before.notes.Ins.is_empty());
    for (old, new, step) in [(&before.notes.Vocal, &after.notes.Vocal, 12), (&before.notes.Ins, &after.notes.Ins, -12)] {
        assert_eq!(old.len(), new.len());
        for (old, new) in old.iter().zip(new) {
            assert_eq!((old.start, old.length), (new.start, new.length));
            assert_eq!(new.pitch, old.pitch + step);
        }
    }
    assert_eq!(before.chords, after.chords);
    for invalid in [Options { grid: Some(64), ..Options::default() }, Options { vocal_octaves: 4, ..Options::default() }] {
        assert!(convert_with(&song, Mode::Full, Pick::Auto, Pick::Auto, invalid).is_err());
    }
}

const FILES: [(&str, &[u8]); 4] = [
    ("band.mid", include_bytes!("fixtures/band.mid")),
    ("karaoke.kar", include_bytes!("fixtures/karaoke.kar")),
    ("roundtrip.mid", include_bytes!("fixtures/roundtrip.mid")),
    ("tempo.mid", include_bytes!("fixtures/tempo.mid")),
];

fn expected() -> Value {
    serde_json::from_str(include_str!("fixtures/expected.json")).expect("the reference output is JSON")
}

#[test]
fn every_file_makes_the_score_the_reference_makes() {
    let expected = expected();
    for (name, data) in FILES {
        let song = smf::read(data).unwrap_or_else(|error| panic!("{name}: {error}"));
        for (mode, key) in [(Mode::Melody, "melody"), (Mode::Full, "full")] {
            let wanted = &expected[name][key];
            let made = convert(&song, mode, Pick::Auto, Pick::Auto).unwrap_or_else(|error| panic!("{name} {key}: {error}"));
            assert_eq!(made.abc, wanted["abc"].as_str().unwrap(), "{name} {key}: the score");
            assert_eq!(made.lyrics, wanted["lyrics"].as_str().unwrap(), "{name} {key}: the lyrics");
            let texts: Vec<String> = made.notices.iter().map(|notice| notice.text()).collect();
            assert_eq!(serde_json::to_value(&texts).unwrap(), wanted["notices"], "{name} {key}: the notices");
            assert_eq!(serde_json::to_value(&made.parts).unwrap(), wanted["parts"], "{name} {key}: the parts");
            let mut facts = serde_json::to_value(&made.facts).unwrap();
            let mut wanted_facts = wanted["facts"].clone();
            let seconds = (facts["seconds"].as_f64().unwrap(), wanted_facts["seconds"].as_f64().unwrap());
            assert!((seconds.0 - seconds.1).abs() < 0.051, "{name} {key}: {seconds:?} seconds");
            facts["seconds"] = Value::Null;
            wanted_facts["seconds"] = Value::Null;
            assert_eq!(facts, wanted_facts, "{name} {key}: the facts");
        }
    }
}

#[test]
fn a_track_the_file_does_not_have_or_drums_is_refused_with_the_list() {
    let expected = expected();
    for (name, data) in FILES {
        let song = smf::read(data).unwrap();
        let error = convert(&song, Mode::Melody, Pick::Number(4), Pick::Number(4)).unwrap_err();
        let listing = expected[name]["same_track"].as_str().unwrap().split(": ").last().unwrap().to_string();
        assert!(error.ends_with(&listing), "{name}: {error}");
    }
}

