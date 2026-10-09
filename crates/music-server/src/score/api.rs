//! The score's MIDI routes: a score as a MIDI file for the MIDI editor, and a
//! MIDI file read back into a score. A problem with a score or a file is
//! something the person needs to read, so it answers 200 with `ok: false` and
//! the reason; a request that is not what the studio sends is a 400.

use axum::http::StatusCode;
use axum::Json;
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{edits, export, import, instrumental, notation, phrasing, smf, transpose};

type Answer = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn refused(reason: &str) -> (StatusCode, Json<Value>) {
    (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": reason })))
}

fn problem(reason: String) -> Answer {
    Ok(Json(json!({ "ok": false, "error": reason })))
}

fn too_long(text: &str) -> Result<(), (StatusCode, Json<Value>)> {
    if text.len() > notation::LONGEST {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, Json(json!({ "ok": false, "error": "That is far longer than any score." }))));
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct MidiRequest {
    abc: String,
}

#[derive(Deserialize)]
pub struct MarkRequest {
    abc: String,
    style: String,
    lyrics: String,
    cot: String,
    #[serde(default)]
    keep: bool,
}

pub async fn mark(Json(request): Json<MarkRequest>) -> Answer {
    too_long(&request.abc)?;
    if !matches!(request.cot.as_str(), "full" | "melody" | "off") { return Err(refused("cot must be full, melody or off")); }
    let score = edits::read(&request.abc).score;
    let words = edits::mark(&request.style, &request.lyrics, &request.cot);
    Ok(Json(json!({ "ok": true, "abc": edits::attach(&score, Some(&words), request.keep) })))
}

/// The largest MIDI file read: a long song is tens of kilobytes.
const LARGEST_MIDI: usize = 8 * 1024 * 1024;

#[derive(Deserialize)]
pub struct FromMidiRequest {
    /// The file, as base64.
    data: String,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    vocal: Option<String>,
    #[serde(default)]
    instrument: Option<String>,
    /// Keep the sections the file names; without them the lyrics are laid along the tune when it is sung.
    #[serde(default)]
    sections: bool,
    #[serde(default)]
    grid: Option<u32>,
    #[serde(default)]
    vocal_octaves: i32,
    #[serde(default)]
    instrument_octaves: i32,
}

/// A MIDI file as a score YuE2 sings and its lyrics. A file that reads but whose choice of tracks
/// makes no score answers with the reason and the file's tracks, so another choice can be made.
pub async fn from_midi(Json(request): Json<FromMidiRequest>) -> Answer {
    let data = base64::engine::general_purpose::STANDARD.decode(request.data.trim()).map_err(|_| refused("'data' is not base64."))?;
    if data.len() > LARGEST_MIDI {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, Json(json!({ "ok": false, "error": "That file is larger than 8 MB, far more than the MIDI of any song." }))));
    }
    let mode = import::Mode::parse(request.mode.as_deref()).map_err(|error| refused(&error))?;
    let vocal = import::parts::Pick::parse(request.vocal.as_deref()).map_err(|error| refused(&error))?;
    let instrument = import::parts::Pick::parse(request.instrument.as_deref()).map_err(|error| refused(&error))?;
    if vocal == import::parts::Pick::None {
        return Err(refused("The voice takes a track, 'auto' or a number."));
    }
    let song = match smf::read(&data) {
        Ok(song) => song,
        Err(reason) => return problem(format!("This file could not be read as a MIDI file: {}.", reason.trim_end_matches('.'))),
    };
    match import::convert_with(&song, mode, vocal, instrument, import::Options { grid: request.grid, vocal_octaves: request.vocal_octaves, instrument_octaves: request.instrument_octaves }) {
        Ok(made) => {
            let abc = if request.sections { phrasing::labelled(&made.abc, "verse") } else { phrasing::bare(&made.abc) };
            let notices: Vec<Value> = made
                .notices
                .iter()
                .map(|notice| {
                    let mut value = serde_json::to_value(notice).unwrap_or_else(|_| json!({}));
                    value["text"] = json!(notice.text());
                    value
                })
                .collect();
            Ok(Json(json!({ "ok": true, "abc": abc, "lyrics": made.lyrics, "parts": made.parts, "facts": made.facts, "notices": notices })))
        }
        Err(reason) => Ok(Json(json!({
            "ok": false,
            "error": format!("No score could be written from this file: {}.", reason.trim_end_matches('.')),
            "parts": import::parts::describe(&import::parts::parts(&song), None),
        }))),
    }
}

/// The score as a MIDI file, handed back as base64: voice, instrument and chords on tracks of their own.
pub async fn midi(Json(request): Json<MidiRequest>) -> Answer {
    too_long(&request.abc)?;
    let score = edits::read(&request.abc).score;
    let whole = notation::whole_groups(&score).unwrap_or(score);
    match export::midi_of(&whole) {
        Ok(data) => Ok(Json(json!({ "ok": true, "data": base64::engine::general_purpose::STANDARD.encode(data) }))),
        Err(reason) => problem(reason),
    }
}

/// The score made instrumental: the voice's notes on the instrument, Vocal left with its rests and chords.
pub async fn instrumental(Json(request): Json<MidiRequest>) -> Answer {
    too_long(&request.abc)?;
    let edit = edits::read(&request.abc);
    match instrumental::transfer(&edit.score) {
        Ok(made) => Ok(Json(json!({ "ok": true, "abc": edits::attach(&made.abc, edit.words.as_deref(), edit.keep), "moved": made.moved, "trimmed": made.trimmed, "dropped": made.dropped }))),
        Err(reason) => problem(reason),
    }
}

#[derive(serde::Deserialize)]
pub struct VocalOctaveRequest {
    abc: String,
    #[serde(default)]
    octaves: i32,
}

/// The vocal line moved by whole octaves (none: only measured), with where its middle now sits
/// against the range YuE2's own scores keep the voice in.
pub async fn vocal_octave(Json(request): Json<VocalOctaveRequest>) -> Answer {
    too_long(&request.abc)?;
    let edit = edits::read(&request.abc);
    let moved = match transpose::move_vocal_octaves(&edit.score, request.octaves) {
        Ok(moved) => moved,
        Err(reason) => return problem(reason),
    };
    let middle = match transpose::vocal_middle(&moved) {
        Ok(middle) => middle,
        Err(reason) => return problem(reason),
    };
    let (low, high) = transpose::VOCAL_RANGE;
    let abc = if request.octaves == 0 { request.abc.clone() } else { edits::attach(&moved, edit.words.as_deref(), edit.keep) };
    Ok(Json(json!({ "ok": true, "abc": abc, "middle": middle, "in_range": middle.is_none_or(|pitch| (low..=high).contains(&pitch)), "range": [low, high] })))
}
