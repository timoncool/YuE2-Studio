//! A song as an Ableton Live project folder, zipped: a Live set with the
//! generated master on an active track, each separated stem on a track that is
//! switched off, and one track per part of the song's score, at the score's
//! tempo and meter, with the audio and MIDI files beside it in the folders
//! Live's own "Collect All and Save" uses.
//!
//! The project is made from what the song has now: nothing is separated or
//! transcribed for it, and what it answers says which of the stems and MIDI
//! the song had. Every file is on its track as a clip at bar 1, written from
//! the clips of a set Live 12 saved (see `ableton`). Audio clips are warped
//! with two markers, the sample's first and last frame, at the set's tempo, so
//! they play at their own rate. The read-me says which file is on which track
//! as well, so the project can be put back together by hand if Live does not
//! take a clip.
//!
//! The zip waits in the studio's `exports` folder until the window has saved
//! it, as an export's file does; a caller that names a folder gets it there.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result};
use axum::extract::{Path as UrlPath, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::ableton::{self, AudioClip, Clip, ClipNote, MidiClip, SetPlan, TrackPlan};
use crate::score::smf;
use crate::{api_error, song_midi, ApiError, AppState};

/// Live's clip colours for each stem, so a set reads the same way every time.
fn stem_color(stem: &str) -> u32 {
    match stem {
        "drums" => 1,
        "bass" => 13,
        "vocals" => 9,
        "guitar" => 5,
        "piano" => 16,
        _ => 21,
    }
}

fn label(stem: &str) -> String {
    let mut chars = stem.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

fn safe(name: &str) -> String {
    let cleaned: String = name.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '_' } else { c }).collect();
    let trimmed = cleaned.trim().trim_end_matches('.').to_string();
    if trimmed.is_empty() { "Untitled".into() } else { trimmed }
}

/// What goes into a project: the files, and the set that names them.
pub struct Contents {
    pub title: String,
    pub bpm: f64,
    pub meter: (u32, u32),
    pub mix: PathBuf,
    /// Stem name and its file.
    pub stems: Vec<(String, PathBuf)>,
    /// Track name, file name and bytes of each MIDI part.
    pub midi: Vec<(String, String, Vec<u8>)>,
    /// Whether the tempo came from the song's score; without one it is Live's default and says so.
    pub tempo_from_score: bool,
}

/// Live 12 Lite, the smallest edition, refuses a ninth audio or MIDI track
/// (returns are not counted): a set made for it opens in every edition.
pub const LIVE_LITE_TRACKS: usize = 8;

/// What goes on a track and what only goes in the folder. The master always
/// has the first track and plays: it is the song. Stems come next, on tracks
/// that are switched off (played with the master they would sound every part
/// twice, and they are estimates of it). The score's parts fill the tracks
/// that are left.
fn track_choice(stems: usize, midi: usize) -> (usize, usize) {
    let stems_on = stems.min(LIVE_LITE_TRACKS - 1);
    let midi_on = midi.min(LIVE_LITE_TRACKS - 1 - stems_on);
    (stems_on, midi_on)
}

fn read_me(contents: &Contents, files: &[(String, String)], left_off: &[String]) -> String {
    let mut text = format!(
        "{title} - YuE2 Studio project for Ableton Live 12\n\n\
         Open \"{title}.als\". The set is at {bpm} BPM in {num}/{den}{tempo_note}.\n\n\
         Each file is on its track as a clip at bar 1 in Arrangement view, named for the file.\n\
         The Master track is the song exactly as it was generated, and it is the one that plays.\n\
         The separated stems are on tracks that are switched off: turn a track's activator on (and the Master off)\n\
         to work from the stems instead.\n\
         Every audio file starts at the song's first sample and has the song's exact length, so at bar 1 they line up.\n\
         Audio clips are warped in Re-Pitch with a marker at the first and last sample: at this tempo they play at\n\
         their own speed. Changing the set's tempo changes their speed and pitch together.\n\
         If Live asks for a file, it is in Samples/Imported or MIDI below; if a clip is missing, drag the file\n\
         onto its track at bar 1.\n\n",
        title = contents.title,
        bpm = contents.bpm,
        num = contents.meter.0,
        den = contents.meter.1,
        tempo_note = if contents.tempo_from_score { ", the tempo of the song's score" } else { " (the song has no score, so this is Live's default tempo, not the song's)" },
    );
    text.push_str("Track -> file\n");
    for (track, file) in files {
        text.push_str(&format!("  {track} -> {file}\n"));
    }
    if !left_off.is_empty() {
        text.push_str(&format!("\nIn the folder but not on a track (Live 12 Lite holds {LIVE_LITE_TRACKS} tracks):\n"));
        for file in left_off {
            text.push_str(&format!("  {file}\n"));
        }
    }
    text.push_str(
        "\nWhat the files are\n\
         - Master: the song as generated, byte for byte.\n\
         - (separated): stems made from the mix by the studio's stem separator. Each is the model's estimate of that part,\n\
           not an original recording; parts bleed into each other.\n\
         - (from score): the notes of the score the song was generated with - what the model was asked to play.\n\
           How closely the recording follows them is not measured. The MIDI tracks have no instrument: add one in Live.\n\
         - (transcribed): notes a transcription model heard in the master - what was played, as far as it can tell.\n",
    );
    text
}

/// A file's length in frames and its rate from its header, in the formats that keep them there (WAV, FLAC).
fn header_length(path: &Path) -> Option<(u64, u32)> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "wav" => {
            let reader = hound::WavReader::open(path).ok()?;
            Some((u64::from(reader.duration()), reader.spec().sample_rate))
        }
        "flac" => {
            use std::io::Read;
            let mut head = [0u8; 26];
            std::fs::File::open(path).ok()?.read_exact(&mut head).ok()?;
            if &head[..4] != b"fLaC" || head[4] & 0x7f != 0 {
                return None;
            }
            // STREAMINFO: 20 bits of rate, 3 of channels, 5 of depth, 36 of total samples
            let packed = u64::from_be_bytes(head[18..26].try_into().ok()?);
            let (rate, frames) = ((packed >> 44) as u32, packed & 0xF_FFFF_FFFF);
            (rate > 0 && frames > 0).then_some((frames, rate))
        }
        _ => None,
    }
}

/// An audio file as Live's clip: its length in frames and rate from its header, or from decoding it where the
/// header does not say.
fn audio_clip(name: &str, relative_path: String, path: &Path) -> Result<AudioClip> {
    let (frames, sample_rate) = match header_length(path) {
        Some(length) => length,
        None => {
            let audio = crate::audio_pcm::decode_stereo(path)?;
            (audio.frames() as u64, audio.rate)
        }
    };
    let meta = std::fs::metadata(path).with_context(|| format!("read {}", path.display()))?;
    let modified = meta.modified().ok().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |since| since.as_secs());
    Ok(AudioClip { name: name.into(), relative_path, frames, sample_rate, file_size: meta.len(), modified })
}

/// A MIDI file as Live's clip: every note of every track, in quarter notes,
/// as long as the file (and never shorter than its last note).
fn midi_clip(name: &str, bytes: &[u8]) -> Result<MidiClip> {
    let song = smf::read(bytes).map_err(|reason| anyhow::anyhow!("{name}: {reason}"))?;
    let division = f64::from(song.division);
    let notes: Vec<ClipNote> = song
        .tracks
        .iter()
        .flat_map(|track| track.notes.iter())
        .map(|note| ClipNote { key: note.pitch, time: note.start as f64 / division, duration: note.end.saturating_sub(note.start) as f64 / division, velocity: note.velocity })
        .collect();
    let last = song.tracks.iter().flat_map(|track| track.notes.iter()).map(|note| note.end).max().unwrap_or(0);
    let beats = song.end().max(last) as f64 / division;
    Ok(MidiClip { name: name.into(), beats, notes })
}

fn stem_of(file_name: &str) -> &str {
    file_name.rsplit_once('.').map_or(file_name, |(stem, _)| stem)
}

/// What a written project holds: its files, the tracks in its set, and the
/// files that are in the folder but not on a track.
#[derive(Debug)]
pub struct Project {
    pub files: Vec<String>,
    pub tracks: Vec<String>,
    pub left_off: Vec<String>,
}

/// The project as a zip at `target`: the set, the audio, the MIDI and a read-me.
pub fn write_project(contents: &Contents, target: &Path) -> Result<Project> {
    let title = safe(&contents.title);
    let root = format!("{title} Project");
    let mut files: Vec<(String, String)> = Vec::new();
    let mut audio_tracks = Vec::new();
    let mix_extension = contents.mix.extension().and_then(|value| value.to_str()).unwrap_or("wav").to_ascii_lowercase();
    let mix_name = format!("{title} - Master.{mix_extension}");
    let mut audio_files = vec![("Master".to_string(), 23, mix_name, contents.mix.clone())];
    for (stem, path) in &contents.stems {
        let track = format!("{} (separated)", label(stem));
        let name = format!("{title} - {track}.wav");
        audio_files.push((track, stem_color(stem), name, path.clone()));
    }
    let (stems_on, midi_on) = track_choice(contents.stems.len(), contents.midi.len());
    let mut left_off = Vec::new();
    for (index, (track, color, name, path)) in audio_files.iter().enumerate() {
        let relative = format!("Samples/Imported/{name}");
        if index > stems_on {
            left_off.push(relative);
            continue;
        }
        let clip = audio_clip(stem_of(name), relative.clone(), path)?;
        // the master plays; a stem is there to switch on
        audio_tracks.push(TrackPlan { name: track.clone(), color: *color, clip: Some(Clip::Audio(clip)), active: index == 0 });
        files.push((track.clone(), relative));
    }
    let mut midi_tracks = Vec::new();
    for (index, (track, name, bytes)) in contents.midi.iter().enumerate() {
        if index >= midi_on {
            left_off.push(format!("MIDI/{name}"));
            continue;
        }
        midi_tracks.push(TrackPlan { name: track.clone(), color: 14, clip: Some(Clip::Midi(midi_clip(stem_of(name), bytes)?)), active: true });
        files.push((track.clone(), format!("MIDI/{name}")));
    }
    let set = ableton::build(ableton::LIVE_12_TEMPLATE, &SetPlan { bpm: contents.bpm, meter: contents.meter, midi: midi_tracks, audio: audio_tracks })?;
    let audio_files: Vec<(String, PathBuf)> = audio_files.into_iter().map(|(_, _, name, path)| (name, path)).collect();

    let partial = target.with_extension("zip.part");
    let mut listed = Vec::new();
    let written = (|| -> Result<()> {
        let file = std::fs::File::create(&partial).with_context(|| format!("create {}", partial.display()))?;
        let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored).large_file(true);
        let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut add = |zip: &mut zip::ZipWriter<_>, name: String, options, bytes: &mut dyn std::io::Read| -> Result<()> {
            zip.start_file(format!("{root}/{name}"), options)?;
            std::io::copy(bytes, zip)?;
            listed.push(name);
            Ok(())
        };
        // the set is already gzipped; storing it keeps it byte for byte what was built
        add(&mut zip, format!("{title}.als"), stored, &mut set.as_slice())?;
        for (name, path) in &audio_files {
            let mut source = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
            add(&mut zip, format!("Samples/Imported/{name}"), stored, &mut source)?;
        }
        for (_, name, bytes) in &contents.midi {
            add(&mut zip, format!("MIDI/{name}"), deflated, &mut bytes.as_slice())?;
        }
        add(&mut zip, "READ ME.txt".into(), deflated, &mut read_me(contents, &files, &left_off).as_bytes())?;
        zip.finish()?.flush()?;
        std::fs::rename(&partial, target).with_context(|| format!("publish {}", target.display()))
    })();
    if let Err(error) = written {
        let _ = std::fs::remove_file(&partial);
        return Err(error);
    }
    Ok(Project { files: listed, tracks: files.into_iter().map(|(track, _)| track).collect(), left_off })
}

fn made() -> &'static Mutex<HashMap<String, PathBuf>> {
    static MADE: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();
    MADE.get_or_init(Default::default)
}

#[derive(Debug, Deserialize)]
pub struct Start {
    pub song_id: String,
    /// A folder of the person's to write the zip into, for a caller without the window's Save dialog (an agent).
    #[serde(default)]
    pub folder: Option<String>,
}

/// The parts of a song a project is made from, as they are now.
struct Parts {
    title: String,
    mix: PathBuf,
    stems: Vec<(String, PathBuf)>,
    midi: Vec<(String, String, Vec<u8>)>,
    bpm: f64,
    meter: (u32, u32),
    tempo_from_score: bool,
}

/// The score's tempo and meter, when the song has a score.
fn score_tempo(song: &crate::library::Song) -> Result<Option<(f64, (u32, u32))>, String> {
    let Some(score) = song_midi::song_score(&song.generation_settings, song.replay_request.as_ref()) else { return Ok(None) };
    let facts = song_midi::facts(&score).map_err(|reason| format!("the song's score could not be read: {reason}"))?;
    let meter = facts.meter.split_once('/').and_then(|(num, den)| Some((num.parse().ok()?, den.parse().ok()?))).unwrap_or((4, 4));
    Ok(Some((f64::from(facts.bpm), meter)))
}

/// One MIDI file per line of the song's score; the full score would double them.
fn score_midi(song: &crate::library::Song) -> Result<Vec<(String, String, Vec<u8>)>, String> {
    let Some(score) = song_midi::song_score(&song.generation_settings, song.replay_request.as_ref()) else { return Ok(vec![]) };
    let facts = song_midi::facts(&score).map_err(|reason| format!("the song's score could not be read: {reason}"))?;
    let mut midi = Vec::new();
    for part in facts.parts.iter().filter(|part| part.id != "score") {
        let bytes = song_midi::part_midi(&score, part.id)?;
        let track = format!("{} (from score)", part.label);
        midi.push((track.clone(), song_midi::file_name(&song.title, &track), bytes));
    }
    Ok(midi)
}

fn stems_of(state: &AppState, song_id: &str) -> Vec<(String, PathBuf)> {
    crate::stems_on_disk(state, song_id).into_iter().map(|stem| { let path = crate::stem_path(state, song_id, &stem); (stem, path) }).collect()
}

/// A transcription of the song kept beside it, from the MIDI tools.
fn transcribed_midi(state: &AppState, song: &crate::library::Song) -> Option<(String, String, Vec<u8>)> {
    let bytes = std::fs::read(crate::midi_path(state, &song.id)).ok()?;
    let track = "Master (transcribed)".to_string();
    Some((track.clone(), song_midi::file_name(&song.title, &track), bytes))
}

/// Writes the project under a name no earlier one has: into the folder the caller named, where it is theirs,
/// or into the studio's `exports` folder, where it waits for the window to save it.
async fn package(parts: Parts, named: Option<PathBuf>) -> Result<Value, String> {
    let contents = Contents { title: parts.title, bpm: parts.bpm, meter: parts.meter, mix: parts.mix, stems: parts.stems, midi: parts.midi, tempo_from_score: parts.tempo_from_score };
    let folder = match &named {
        Some(folder) => folder.clone(),
        None => crate::studio_data_root().map(|root| root.join("exports")).ok_or("the studio has no data folder")?,
    };
    std::fs::create_dir_all(&folder).map_err(|error| format!("create {}: {error}", folder.display()))?;
    // a new name every time: an earlier project is never overwritten
    let base = format!("{} Project", safe(&contents.title));
    let target = (1..)
        .map(|n| folder.join(if n == 1 { format!("{base}.zip") } else { format!("{base} ({n}).zip") }))
        .find(|path| !path.exists())
        .expect("a free name");
    let written = target.clone();
    let summary = (contents.stems.len(), contents.midi.len(), contents.tempo_from_score, contents.bpm, contents.meter);
    let project = tokio::task::spawn_blocking(move || write_project(&contents, &written))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| format!("{error:#}"))?;
    let token = named.is_none().then(|| uuid::Uuid::now_v7().to_string());
    if let Some(token) = &token {
        made().lock().expect("made").insert(token.clone(), target.clone());
    }
    Ok(json!({
        "id": token,
        "path": named.is_some().then(|| target.display().to_string()),
        "file_name": target.file_name().and_then(|name| name.to_str()),
        "files": project.files,
        "tracks": project.tracks,
        "left_off": project.left_off,
        "stems": summary.0,
        "midi_parts": summary.1,
        "tempo_from_score": summary.2,
        "bpm": summary.3,
        "meter": format!("{}/{}", summary.4 .0, summary.4 .1),
        "clips_placed": true,
        "master_active": true,
    }))
}

fn song_or_404(state: &AppState, song_id: &str) -> Result<crate::library::Song, (StatusCode, Json<ApiError>)> {
    state
        .library
        .get_song(song_id)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))
}

/// Builds a song's Live project from what it has now, making nothing new, and answers with what is in it.
pub async fn start(State(state): State<AppState>, Json(start): Json<Start>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let named = start.folder.as_deref().map(str::trim).filter(|folder| !folder.is_empty()).map(PathBuf::from);
    if named.as_ref().is_some_and(|folder| !folder.is_absolute()) {
        return Err(api_error(StatusCode::BAD_REQUEST, "folder must be a full path".into()));
    }
    let song = song_or_404(&state, &start.song_id)?;
    let mix = state.library.media_path_for_song(&song).ok_or_else(|| api_error(StatusCode::NOT_FOUND, "the song's audio is not in the library".into()))?;
    let tempo = score_tempo(&song).map_err(|reason| api_error(StatusCode::UNPROCESSABLE_ENTITY, reason))?;
    let mut midi = score_midi(&song).map_err(|reason| api_error(StatusCode::UNPROCESSABLE_ENTITY, reason))?;
    midi.extend(transcribed_midi(&state, &song));
    let (bpm, meter) = tempo.unwrap_or((120.0, (4, 4)));
    let parts = Parts { title: song.title.clone(), mix, stems: stems_of(&state, &song.id), midi, bpm, meter, tempo_from_score: tempo.is_some() };
    package(parts, named).await.map(Json).map_err(|reason| api_error(StatusCode::INTERNAL_SERVER_ERROR, reason))
}

/// Lets a project go once the window has saved it, or was closed without: its zip leaves the `exports` folder.
pub async fn forget(UrlPath(id): UrlPath<String>) -> StatusCode {
    if let Some(path) = made().lock().expect("made").remove(&id) {
        let _ = std::fs::remove_file(path);
    }
    StatusCode::NO_CONTENT
}

pub async fn file(UrlPath(id): UrlPath<String>, request: Request) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let path = made().lock().expect("made").get(&id).cloned().ok_or_else(|| api_error(StatusCode::NOT_FOUND, "no such project".into()))?;
    let mut response = crate::serve_audio_file(&path, request).await;
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/zip"));
    if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
        let ascii: String = name.chars().map(|c| if c.is_ascii() && c != '"' { c } else { '_' }).collect();
        if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{}", crate::mcp::segment(name))) {
            response.headers_mut().insert(header::CONTENT_DISPOSITION, value);
        }
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn a_project_holds_the_set_the_files_and_a_read_me() {
        let dir = std::env::temp_dir().join(format!("ableton-project-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = |path: &Path, frames: u32| {
            let spec = hound::WavSpec { channels: 2, sample_rate: 48_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
            let mut writer = hound::WavWriter::create(path, spec).unwrap();
            for _ in 0..frames * 2 {
                writer.write_sample(0i16).unwrap();
            }
            writer.finalize().unwrap();
        };
        let mix = dir.join("mix.wav");
        wav(&mix, 96_000);
        let drums = dir.join("drums.wav");
        wav(&drums, 96_000);
        // two beats of D, then two of F
        let lead = smf::write(
            &[vec![], vec![(0, smf::note_on(0, 62, 90)), (960, smf::note_off(0, 62)), (960, smf::note_on(0, 65, 80)), (1920, smf::note_off(0, 65))]],
            smf::DIVISION,
            1920,
        )
        .unwrap();
        let contents = Contents {
            title: "Night: drive".into(),
            bpm: 92.0,
            meter: (4, 4),
            mix,
            stems: vec![("drums".into(), drums)],
            midi: vec![("Lead (from score)".into(), "Night_ drive - Lead (from score).mid".into(), lead)],
            tempo_from_score: true,
        };
        let target = dir.join("out.zip");
        let project = write_project(&contents, &target).unwrap();
        assert_eq!(project.tracks, ["Master", "Drums (separated)", "Lead (from score)"], "the master always has a track");
        assert!(project.left_off.is_empty());
        assert_eq!(project.files, [
            "Night_ drive.als",
            "Samples/Imported/Night_ drive - Master.wav",
            "Samples/Imported/Night_ drive - Drums (separated).wav",
            "MIDI/Night_ drive - Lead (from score).mid",
            "READ ME.txt",
        ]);
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&target).unwrap()).unwrap();
        let mut set = Vec::new();
        archive.by_name("Night_ drive Project/Night_ drive.als").unwrap().read_to_end(&mut set).unwrap();
        let xml = ableton::gunzip(&set).unwrap();
        for name in ["Master", "Drums (separated)", "Lead (from score)"] {
            assert!(xml.contains(&format!(r#"<UserName Value="{name}" />"#)), "{name} track");
        }
        assert!(xml.contains(r#"<Manual Value="92" />"#));
        // each file is on its track: 2 s at 92 BPM is 3.0666… beats
        let document = roxmltree::Document::parse(&xml).unwrap();
        let paths: Vec<&str> = document.descendants().filter(|node| node.has_tag_name("RelativePath")).filter_map(|node| node.attribute("Value")).filter(|value| value.starts_with("Samples/")).collect();
        assert_eq!(paths, ["Samples/Imported/Night_ drive - Master.wav", "Samples/Imported/Night_ drive - Drums (separated).wav"]);
        // the master plays and the stem waits to be switched on
        let activator = |name: &str| {
            let track = document.descendants().filter(|node| node.has_tag_name("AudioTrack")).find(|track| track.descendants().any(|node| node.has_tag_name("UserName") && node.attribute("Value") == Some(name))).unwrap();
            let speaker = track.descendants().find(|node| node.has_tag_name("Speaker")).unwrap();
            speaker.children().find(|node| node.has_tag_name("Manual")).unwrap().attribute("Value").unwrap().to_string()
        };
        assert_eq!(activator("Master"), "true");
        assert_eq!(activator("Drums (separated)"), "false");
        let audio = document.descendants().find(|node| node.has_tag_name("AudioClip")).unwrap();
        let end: f64 = audio.descendants().find(|node| node.has_tag_name("CurrentEnd")).unwrap().attribute("Value").unwrap().parse().unwrap();
        assert!((end - 2.0 * 92.0 / 60.0).abs() < 1e-9);
        let lead = document.descendants().find(|node| node.has_tag_name("MidiClip")).unwrap();
        let notes: Vec<(&str, &str, &str)> = lead
            .descendants()
            .filter(|node| node.has_tag_name("MidiNoteEvent"))
            .map(|node| (node.attribute("Time").unwrap(), node.attribute("Duration").unwrap(), node.attribute("Velocity").unwrap()))
            .collect();
        assert_eq!(notes, [("0", "2", "90"), ("2", "2", "80")]);
        for (inside, original) in [("Samples/Imported/Night_ drive - Drums (separated).wav", "drums.wav"), ("Samples/Imported/Night_ drive - Master.wav", "mix.wav")] {
            let mut back = Vec::new();
            archive.by_name(&format!("Night_ drive Project/{inside}")).unwrap().read_to_end(&mut back).unwrap();
            assert_eq!(back, std::fs::read(dir.join(original)).unwrap(), "the audio goes in byte for byte");
        }
        let mut readme = String::new();
        archive.by_name("Night_ drive Project/READ ME.txt").unwrap().read_to_string(&mut readme).unwrap();
        assert!(readme.contains("Drums (separated) -> Samples/Imported/Night_ drive - Drums (separated).wav"));
        assert!(readme.contains("Master -> Samples/Imported/Night_ drive - Master.wav"));
        assert!(readme.contains("as a clip at bar 1"));
        assert!(readme.contains("switched off"));
        assert!(readme.contains("not an original recording"));
        assert!(!target.with_extension("zip.part").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_set_never_has_more_tracks_than_live_lite_takes() {
        // (stems on, MIDI parts on); the master always has the first track
        assert_eq!(track_choice(0, 3), (0, 3));
        assert_eq!(track_choice(6, 3), (6, 1));
        assert_eq!(track_choice(4, 2), (4, 2));
        assert_eq!(track_choice(0, 9), (0, 7));
        assert_eq!(track_choice(9, 2), (7, 0));
        for stems in 0..=10 {
            for midi in 0..=10 {
                let (on, parts) = track_choice(stems, midi);
                assert!(1 + on + parts <= LIVE_LITE_TRACKS);
            }
        }
    }

    #[test]
    fn without_a_score_the_read_me_says_the_tempo_is_not_the_songs() {
        let contents = Contents { title: "x".into(), bpm: 120.0, meter: (4, 4), mix: PathBuf::from("m.wav"), stems: vec![], midi: vec![], tempo_from_score: false };
        assert!(read_me(&contents, &[], &[]).contains("not the song's"));
    }


    #[test]
    fn a_files_length_is_read_from_its_header_and_is_what_decoding_gives() {
        let folder = tempfile::tempdir().unwrap();
        let audio = audio_post::Stereo::new(vec![0.25; 12_345], vec![-0.25; 12_345], 44_100);
        let wav = folder.path().join("stem.wav");
        crate::audio_pcm::write_wav_f32(&wav, &audio).unwrap();
        let flac = folder.path().join("master.flac");
        std::fs::write(&flac, audio_post::encode::flac(&audio).unwrap()).unwrap();
        for path in [&wav, &flac] {
            let decoded = crate::audio_pcm::decode_stereo(path).unwrap();
            assert_eq!(header_length(path), Some((decoded.frames() as u64, decoded.rate)), "{}", path.display());
            let clip = audio_clip("clip", "Samples/Imported/clip".into(), path).unwrap();
            assert_eq!((clip.frames, clip.sample_rate), (12_345, 44_100));
        }
        assert_eq!(header_length(&folder.path().join("song.mp3")), None, "a format without it in the header is decoded");
    }
}
