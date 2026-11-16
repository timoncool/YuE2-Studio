//! A song's MIDI for a DAW, each file saying where its notes come from.
//!
//! Two sources, never mixed and never passed off as each other:
//!
//! - **From the score**: the score the song was generated with, written out
//!   by the score exporter - the whole score, or one line of it (lead, voice,
//!   chords) - at the score's tempo, meter and key, with its section markers.
//!   These are the notes the model was asked to play; the recording follows
//!   them as closely as the model did, which nothing here measures.
//! - **The track's own MIDI**: what the audio-to-MIDI transcriber heard in the
//!   recording (an estimate), or a file edited in the MIDI editor.
//!
//! A song generated without a score has no score MIDI: none is made up.

use std::path::Path;

use axum::extract::{Path as UrlPath, State};
use axum::http::{header, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::score::{abc, edits, export, notation};
use crate::{api_error, ApiError, AppState};

/// One file a song's score can be written as: which of the score's lines it keeps.
pub struct Part {
    pub id: &'static str,
    pub label: &'static str,
    lines: &'static [&'static str],
}

pub const PARTS: [Part; 4] = [
    Part { id: "score", label: "Full score", lines: &["Vocal", "Ins", "Chords"] },
    Part { id: "lead", label: "Lead", lines: &["Ins"] },
    Part { id: "voice", label: "Voice line", lines: &["Vocal"] },
    Part { id: "chords", label: "Chords", lines: &["Chords"] },
];

/// The score a song was generated with, as the exporter reads it; none when it was made without one.
pub fn song_score(generation_settings: &Value, replay_request: Option<&Value>) -> Option<String> {
    let text = generation_settings
        .get("abc")
        .and_then(Value::as_str)
        .or_else(|| replay_request.and_then(|replay| replay.get("abc")).and_then(Value::as_str))
        .filter(|text| !text.trim().is_empty())?;
    let score = edits::read(text).score;
    Some(notation::whole_groups(&score).unwrap_or(score))
}

#[derive(Debug, Serialize, PartialEq)]
pub struct ScoreFacts {
    pub bpm: u32,
    pub meter: String,
    pub key: String,
    /// The parts this score has notes for, with how many notes each.
    pub parts: Vec<PartFacts>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct PartFacts {
    pub id: &'static str,
    pub label: &'static str,
    /// Notes of the melodic lines.
    pub notes: usize,
    /// Chord symbols, each held until the next.
    pub chords: usize,
}

/// What a score holds, part by part; an error when it does not parse.
pub fn facts(score: &str) -> Result<ScoreFacts, String> {
    let parsed = abc::parse(score)?;
    let vocal = &parsed.voices[abc::VOCAL];
    let notes = |line: &str| match line {
        "Vocal" => parsed.voices[abc::VOCAL].notes.len(),
        "Ins" => parsed.voices[abc::INS].notes.len(),
        _ => 0,
    };
    let parts = PARTS
        .iter()
        .map(|part| PartFacts {
            id: part.id,
            label: part.label,
            notes: part.lines.iter().map(|line| notes(line)).sum(),
            chords: if part.lines.contains(&"Chords") { vocal.chords.len() } else { 0 },
        })
        .filter(|part| part.notes + part.chords > 0)
        .collect();
    let meter = vocal.bars.first().map(|bar| bar.meter).unwrap_or(vocal.meter);
    let key = vocal.keys.first().map(|(_, key)| key.clone()).unwrap_or_else(|| vocal.key.clone());
    Ok(ScoreFacts { bpm: parsed.bpm, meter: format!("{}/{}", meter.0, meter.1), key, parts })
}

/// One part of a score as a MIDI file.
pub fn part_midi(score: &str, part: &str) -> Result<Vec<u8>, String> {
    let part = PARTS.iter().find(|candidate| candidate.id == part).ok_or_else(|| format!("no part {part}"))?;
    export::midi_with(score, |line| part.lines.contains(&line))
}

/// A file name that says what the file is, safe on every filesystem.
pub fn file_name(title: &str, what: &str) -> String {
    let title: String = title.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '_' } else { c }).collect();
    let title = title.trim();
    format!("{} - {what}.mid", if title.is_empty() { "track" } else { title })
}

/// What the track's own MIDI is: transcribed from the audio, or edited by hand.
pub fn own_midi_origin(sidecar: Option<&Path>) -> &'static str {
    let size = sidecar
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| value.get("size").and_then(Value::as_str).map(str::to_owned));
    match size.as_deref() {
        Some("edited") => "edited",
        _ => "transcribed",
    }
}

fn song_or_404(state: &AppState, id: &str) -> Result<crate::library::Song, (StatusCode, Json<ApiError>)> {
    state
        .library
        .get_song(id)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))
}

/// The MIDI a song has: from its score, and its own, each labelled with its source.
pub async fn list(State(state): State<AppState>, UrlPath(id): UrlPath<String>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let song = song_or_404(&state, &id)?;
    let score = song_score(&song.generation_settings, song.replay_request.as_ref());
    let score_view = match score.as_deref().map(facts) {
        Some(Ok(facts)) => {
            let files: Vec<Value> = facts
                .parts
                .iter()
                .map(|part| json!({ "part": part.id, "label": part.label, "notes": part.notes, "chords": part.chords, "file_name": file_name(&song.title, &format!("{} (from score)", part.label)) }))
                .collect();
            json!({ "bpm": facts.bpm, "meter": facts.meter, "key": facts.key, "files": files })
        }
        Some(Err(reason)) => json!({ "error": reason }),
        None => Value::Null,
    };
    let own = crate::midi_path(&state, &id);
    let own_view = if own.is_file() {
        let origin = own_midi_origin(Some(&crate::midi_sidecar(&own)));
        let what = if origin == "edited" { "MIDI (edited)" } else { "audio to MIDI (estimated)" };
        json!({ "origin": origin, "file_name": file_name(&song.title, what) })
    } else {
        Value::Null
    };
    Ok(Json(json!({ "song_id": id, "score": score_view, "own": own_view })))
}

/// One of a song's MIDI files and the name it goes by: a part of its score, or `own` for the track's own MIDI.
async fn part_file(state: &AppState, id: &str, part: &str) -> Result<(Vec<u8>, String), (StatusCode, Json<ApiError>)> {
    let song = song_or_404(state, id)?;
    if part == "own" {
        let own = crate::midi_path(state, id);
        let bytes = tokio::fs::read(&own).await.map_err(|_| api_error(StatusCode::NOT_FOUND, "this track has no MIDI of its own".into()))?;
        let what = if own_midi_origin(Some(&crate::midi_sidecar(&own))) == "edited" { "MIDI (edited)" } else { "audio to MIDI (estimated)" };
        return Ok((bytes, file_name(&song.title, what)));
    }
    let score = song_score(&song.generation_settings, song.replay_request.as_ref())
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "this song was generated without a score, so it has no score MIDI".into()))?;
    let label = PARTS.iter().find(|candidate| candidate.id == part).map(|candidate| candidate.label).ok_or_else(|| api_error(StatusCode::BAD_REQUEST, format!("no part {part}")))?;
    let bytes = part_midi(&score, part).map_err(|reason| api_error(StatusCode::UNPROCESSABLE_ENTITY, reason))?;
    Ok((bytes, file_name(&song.title, &format!("{label} (from score)"))))
}

/// One of a song's MIDI files, for the window's Save dialog.
pub async fn file(State(state): State<AppState>, UrlPath((id, part)): UrlPath<(String, String)>) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let (bytes, name) = part_file(&state, &id, &part).await?;
    let disposition = format!("attachment; filename=\"score.mid\"; filename*=UTF-8''{}", crate::mcp::segment(&name));
    axum::response::Response::builder()
        .header(header::CONTENT_TYPE, "audio/midi")
        .header(header::CONTENT_DISPOSITION, disposition)
        .body(axum::body::Body::from(bytes))
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

#[derive(Debug, Deserialize)]
pub struct Into {
    /// The folder of the person's to write the file into, a full path.
    pub folder: String,
}

/// One of a song's MIDI files written into a folder, for a caller without the window's Save dialog (an agent).
/// The file never replaces another: a taken name gets a number.
pub async fn write(State(state): State<AppState>, UrlPath((id, part)): UrlPath<(String, String)>, Json(into): Json<Into>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let folder = std::path::PathBuf::from(into.folder.trim());
    if !folder.is_absolute() {
        return Err(api_error(StatusCode::BAD_REQUEST, "folder must be a full path".into()));
    }
    let (bytes, name) = part_file(&state, &id, &part).await?;
    tokio::fs::create_dir_all(&folder).await.map_err(|error| api_error(StatusCode::BAD_REQUEST, format!("create {}: {error}", folder.display())))?;
    let stem = name.trim_end_matches(".mid");
    let path = std::iter::once(folder.join(&name))
        .chain((2..).map(|n| folder.join(format!("{stem} ({n}).mid"))))
        .find(|path| !path.exists())
        .expect("some number is free");
    tokio::fs::write(&path, &bytes).await.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("write {}: {error}", path.display())))?;
    Ok(Json(json!({ "path": path.display().to_string(), "bytes": bytes.len() })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::score::smf;

    /// A short instrumental score: lead on Ins, chords on the Vocal line's rests, at 92 BPM in D minor.
    pub const SCORE: &str = "X:1\nT:\nM:4/4\nL:1/16\nQ:1/4=92\nV: Vocal clef=treble name=\"Vocal Melody\" snm=\"Vocal\"\nV: Ins clef=treble name=\"Ins Melody\" snm=\"Inst.\"\nK:Dm\n% intro\nV: Vocal\n\"Dm\"z16|\"Bb\"z16|\nV: Ins\nD4F4A4d4|c8B8|\n";

    #[test]
    fn the_score_midi_carries_the_scores_notes_and_tempo() {
        let parsed = abc::parse(SCORE).unwrap();
        let song = smf::read(&part_midi(SCORE, "score").unwrap()).unwrap();
        // tempo: 92 BPM exactly as microseconds per quarter
        assert_eq!(song.tempos, vec![(0, (60e6f64 / 92.0).round() as u32)]);
        assert_eq!(song.meters, vec![(0, 4, 4)]);
        assert_eq!(song.keys.first().map(|key| (key.1, key.2)), Some((-1, true)));
        assert_eq!(song.markers.first().map(|marker| marker.1.clone()), Some(b"intro".to_vec()));
        // every lead note at its score position and length, in ticks of 480 per quarter
        let lead = song.tracks.iter().find(|track| track.name == b"Ins").expect("an Ins track");
        let expected: Vec<(u64, u64, u8)> = parsed.voices[abc::INS]
            .notes
            .iter()
            .map(|note| (export::tick(note.start), export::tick(note.start + note.duration), note.pitch as u8))
            .collect();
        let written: Vec<(u64, u64, u8)> = lead.notes.iter().map(|note| (note.start, note.end, note.pitch)).collect();
        assert_eq!(written, expected);
        assert_eq!(written.first(), Some(&(0, 480, 62)), "D4 is the first sixteenth-unit quarter");
        // the chords held bar by bar: Dm then Bb, each a bar of 1920 ticks
        let chords = song.tracks.iter().find(|track| track.name == b"Chords").expect("a Chords track");
        let mut held: Vec<(u64, u64, u8)> = chords.notes.iter().map(|note| (note.start, note.end, note.pitch)).collect();
        held.sort();
        let dm: Vec<u8> = export::chord_pitches("Dm").unwrap().into_iter().map(|pitch| pitch as u8).collect();
        let bb: Vec<u8> = export::chord_pitches("Bb").unwrap().into_iter().map(|pitch| pitch as u8).collect();
        let mut expected_chords: Vec<(u64, u64, u8)> = dm.iter().map(|pitch| (0, 1920, *pitch)).chain(bb.iter().map(|pitch| (1920, 3840, *pitch))).collect();
        expected_chords.sort();
        assert_eq!(held, expected_chords);
        assert_eq!(export::tick(parsed.voices[abc::VOCAL].time), 3840);
    }

    #[test]
    fn a_part_keeps_only_its_line_and_the_conductor() {
        let lead = smf::read(&part_midi(SCORE, "lead").unwrap()).unwrap();
        let names: Vec<Vec<u8>> = lead.tracks.iter().map(|track| track.name.clone()).collect();
        assert_eq!(names, vec![b"YuE2 score".to_vec(), b"Ins".to_vec()]);
        assert_eq!(lead.tempos.len(), 1, "every part keeps the tempo");
        let chords = smf::read(&part_midi(SCORE, "chords").unwrap()).unwrap();
        assert_eq!(chords.tracks.iter().map(|track| track.name.clone()).collect::<Vec<_>>(), vec![b"YuE2 score".to_vec(), b"Chords".to_vec()]);
        assert!(part_midi(SCORE, "bass").is_err(), "no part is made up");
    }

    #[test]
    fn facts_list_only_the_parts_with_notes() {
        let facts = facts(SCORE).unwrap();
        assert_eq!((facts.bpm, facts.meter.as_str(), facts.key.as_str()), (92, "4/4", "Dm"));
        let ids: Vec<&str> = facts.parts.iter().map(|part| part.id).collect();
        assert_eq!(ids, vec!["score", "lead", "chords"], "the voice line has only rests here");
        assert_eq!((facts.parts[0].notes, facts.parts[0].chords), (6, 2));
        assert_eq!((facts.parts[1].notes, facts.parts[1].chords), (6, 0));
        assert_eq!((facts.parts[2].notes, facts.parts[2].chords), (0, 2));
    }

    #[test]
    fn the_score_is_found_where_a_song_keeps_it_and_never_invented() {
        assert!(song_score(&json!({}), None).is_none());
        assert!(song_score(&json!({ "abc": "  " }), None).is_none());
        assert!(song_score(&json!({}), Some(&json!({ "abc": SCORE }))).is_some());
        assert!(song_score(&json!({ "abc": SCORE }), None).unwrap().contains("Q:1/4=92"));
    }

    #[test]
    fn file_names_say_what_the_file_is() {
        assert_eq!(file_name("Night: drive", "Lead (from score)"), "Night_ drive - Lead (from score).mid");
        assert_eq!(file_name("  ", "audio to MIDI (estimated)"), "track - audio to MIDI (estimated).mid");
        let dir = std::env::temp_dir().join(format!("song-midi-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let edited = dir.join("a.json");
        std::fs::write(&edited, br#"{"size":"edited"}"#).unwrap();
        let heard = dir.join("b.json");
        std::fs::write(&heard, br#"{"size":"base"}"#).unwrap();
        assert_eq!(own_midi_origin(Some(&edited)), "edited");
        assert_eq!(own_midi_origin(Some(&heard)), "transcribed");
        std::fs::remove_dir_all(&dir).ok();
    }
}
