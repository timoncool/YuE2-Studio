//! Audio to MIDI: MuScriptor (Kyutai & Mirelo, arXiv:2607.08168) run by
//! audio.cpp's server, which reads a size's safetensors folder as it is.
//!
//! Nothing of it comes with the studio. The server is audio.cpp's release,
//! and the weights come from an open mirror of the gated official repositories
//! (the same files, byte for byte); both are fetched the first time a track is
//! turned into MIDI, or from the tools page before that. The code is MIT; the
//! weights are CC BY-NC 4.0, which the tools page says.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};

use crate::downloads::{Asset, Downloader};

/// One size of the model.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Size {
    pub id: &'static str,
    /// audio.cpp's family: `muscriptor` hears every instrument, `rosvot` the sung melody.
    pub family: &'static str,
    pub params: &'static str,
    /// The mirror's revision the files were measured at.
    #[serde(skip)]
    revision: &'static str,
    #[serde(skip)]
    config_bytes: u64,
    pub bytes: u64,
}

pub const SIZES: [Size; 4] = [
    Size { id: "small", family: "muscriptor", params: "103M", revision: "31a8f75d6a8b5383fd71ad1371dc1620389ab722", config_bytes: 124, bytes: 411_888_600 },
    Size { id: "medium", family: "muscriptor", params: "307M", revision: "27246ba68bd4d8f98bdec10a6edf8d7cf42a8826", config_bytes: 126, bytes: 1_228_144_472 },
    Size { id: "large", family: "muscriptor", params: "1.4B", revision: "87f4bf981f56f90fb5043153b3f54af3c3053da9", config_bytes: 125, bytes: 5_465_642_136 },
    Size { id: "vocal", family: "rosvot", params: "114M", revision: "ded36295f789661e590e4bf6ad49a25470cf830d", config_bytes: 0, bytes: 456_486_848 },
];

impl Size {
    /// MuScriptor, which hears every instrument; the others hear one part.
    pub fn every_instrument(&self) -> bool {
        self.family == "muscriptor"
    }
}

/// What a track is transcribed with when nobody chose: the middle size hears
/// most of what the large one does for a quarter of the download.
pub const DEFAULT_SIZE: &str = "medium";

pub fn size(id: &str) -> Option<&'static Size> {
    SIZES.iter().find(|size| size.id == id)
}

/// ROSVOT as audio.cpp's server carries it: the sung melody of a recording to MIDI.
pub fn sung_entry(model: &Path) -> acpp_server::ModelEntry {
    acpp_server::ModelEntry {
        id: "rosvot".to_string(),
        family: "rosvot".to_string(),
        path: model.to_path_buf(),
        task: "midi".to_string(),
        mode: acpp_server::Mode::Offline,
        load_options: Default::default(),
        session_options: Default::default(),
        default_request_options: Default::default(),
        busy_timeout_ms: None,
        output_rate: None,
    }
}

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// A size's two files, from the open mirror at a fixed revision.
pub(crate) fn weight_assets(size: &'static Size) -> &'static [Asset] {
    static ASSETS: OnceLock<HashMap<&'static str, Vec<Asset>>> = OnceLock::new();
    ASSETS.get_or_init(|| {
        SIZES
            .iter()
            .map(|size| {
                if !size.every_instrument() {
                    return (size.id, vec![*crate::components::required(size.family)]);
                }
                let file = |part: &str| *crate::components::required(&format!("muscriptor-{}-{part}", size.id));
                (size.id, vec![file("config"), file("model")])
            })
            .collect()
    })[size.id]
        .as_slice()
}


/// Microseconds per quarter of a file with no Set Tempo: 120 BPM.
const DEFAULT_MICROS: u32 = 500_000;

/// The chunks of a standard MIDI file after its header: (is a track, the whole chunk, its body).
fn chunks(smf: &[u8]) -> anyhow::Result<(usize, Vec<(bool, &[u8], &[u8])>)> {
    anyhow::ensure!(smf.len() >= 14 && &smf[..4] == b"MThd", "not a standard MIDI file");
    let header_len = u32::from_be_bytes([smf[4], smf[5], smf[6], smf[7]]) as usize;
    let division = u16::from_be_bytes([smf[12], smf[13]]);
    anyhow::ensure!(division & 0x8000 == 0, "a SMPTE-timed MIDI file has no tempo to set");
    let mut found = Vec::new();
    let mut at = 8 + header_len;
    while at + 8 <= smf.len() {
        let length = u32::from_be_bytes([smf[at + 4], smf[at + 5], smf[at + 6], smf[at + 7]]) as usize;
        let body = smf.get(at + 8..at + 8 + length).ok_or_else(|| anyhow::anyhow!("a track runs past the end of the file"))?;
        found.push((&smf[at..at + 4] == b"MTrk", &smf[at..at + 8 + length], body));
        at += 8 + length;
    }
    Ok((8 + header_len, found))
}

/// A standard MIDI file timed at `bpm`: one Set Tempo at the start of the first track in place of the tempo it had (the
/// first Set Tempo, or 120 BPM without one), and every tick scaled so each event keeps the second it sounded at.
pub fn with_tempo(smf: &[u8], bpm: u32) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(bpm > 0, "no tempo");
    let (header, chunks) = chunks(smf)?;
    let mut source = None;
    for (track, _, body) in &chunks {
        if *track && source.is_none() {
            source = track_events(body)?.into_iter().find_map(|event| event.tempo);
        }
    }
    let target = (60_000_000.0 / f64::from(bpm)).round() as u32;
    let factor = f64::from(source.unwrap_or(DEFAULT_MICROS)) / f64::from(target);
    let mut out = smf[..header].to_vec();
    let mut first = true;
    for (track, whole, body) in chunks {
        if !track {
            out.extend_from_slice(whole);
            continue;
        }
        let retimed = retimed_track(body, factor, first.then_some(target))?;
        out.extend_from_slice(b"MTrk");
        out.extend_from_slice(&(retimed.len() as u32).to_be_bytes());
        out.extend_from_slice(&retimed);
        first = false;
    }
    Ok(out)
}

/// One event of a track: its absolute tick, its bytes in the body, and the microseconds of a Set Tempo.
struct TrackEvent {
    tick: u64,
    start: usize,
    end: usize,
    tempo: Option<u32>,
}

fn track_events(body: &[u8]) -> anyhow::Result<Vec<TrackEvent>> {
    let mut events = Vec::new();
    let (mut at, mut tick, mut running) = (0usize, 0u64, 0u8);
    while at < body.len() {
        tick += read_vlq(body, &mut at)?;
        let start = at;
        let status = *body.get(at).ok_or_else(|| anyhow::anyhow!("an event runs past the track"))?;
        let end = if status == 0xFF {
            let mut cursor = at + 2;
            let length = read_vlq(body, &mut cursor)? as usize;
            cursor + length
        } else if status == 0xF0 || status == 0xF7 {
            let mut cursor = at + 1;
            let length = read_vlq(body, &mut cursor)? as usize;
            cursor + length
        } else {
            let (command, data_at) = if status & 0x80 != 0 { running = status; (status, at + 1) } else { (running, at) };
            anyhow::ensure!(command & 0x80 != 0, "a running status with no status before it");
            data_at + if matches!(command & 0xF0, 0xC0 | 0xD0) { 1 } else { 2 }
        };
        anyhow::ensure!(end <= body.len(), "an event runs past the track");
        let tempo = (status == 0xFF && body.get(at + 1) == Some(&0x51) && end >= 3)
            .then(|| u32::from_be_bytes([0, body[end - 3], body[end - 2], body[end - 1]]));
        events.push(TrackEvent { tick, start, end, tempo });
        at = end;
    }
    Ok(events)
}

fn read_vlq(bytes: &[u8], at: &mut usize) -> anyhow::Result<u64> {
    let mut value = 0u64;
    for _ in 0..4 {
        let byte = *bytes.get(*at).ok_or_else(|| anyhow::anyhow!("a delta time runs past the track"))?;
        *at += 1;
        value = (value << 7) | u64::from(byte & 0x7F);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    anyhow::bail!("a delta time longer than four bytes")
}

fn write_vlq(value: u64, out: &mut Vec<u8>) {
    let mut groups = vec![(value & 0x7F) as u8];
    let mut rest = value >> 7;
    while rest > 0 {
        groups.push((rest & 0x7F) as u8 | 0x80);
        rest >>= 7;
    }
    out.extend(groups.iter().rev());
}

/// A track with every tick multiplied by `factor` and its Set Tempo events left out; `tempo` puts one at its start.
fn retimed_track(body: &[u8], factor: f64, tempo: Option<u32>) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(body.len() + 8);
    if let Some(micros) = tempo {
        out.extend_from_slice(&[0x00, 0xFF, 0x51, 0x03]);
        out.extend_from_slice(&micros.to_be_bytes()[1..]);
    }
    let mut written = 0u64;
    for event in track_events(body)? {
        if event.tempo.is_some() {
            continue;
        }
        let scaled = ((event.tick as f64) * factor).round() as u64;
        write_vlq(scaled.saturating_sub(written), &mut out);
        written = written.max(scaled);
        out.extend_from_slice(&body[event.start..event.end]);
    }
    Ok(out)
}

/// The model sizes on disk, and audio.cpp's server that runs them.
pub struct Transcriber {
    downloader: Downloader,
    acpp: Arc<crate::acpp::Acpp>,
}

impl Transcriber {
    pub fn new(data_root: &Path, acpp: Arc<crate::acpp::Acpp>) -> Self {
        Self { downloader: Downloader::new(data_root.join("midi")), acpp }
    }

    pub fn downloader(&self) -> &Downloader {
        &self.downloader
    }

    /// audio.cpp's server of this build is installed.
    pub fn server_installed(&self, build: acpp_server::asset::Build) -> bool {
        self.acpp.installed(build)
    }

    pub fn model_dir(&self, size: &Size) -> PathBuf {
        self.downloader.root().join("models").join(format!("muscriptor-{}", size.id))
    }

    /// The file audio.cpp's server loads for a model of one file (ROSVOT's GGUF).
    pub fn model_file(&self, size: &'static Size) -> PathBuf {
        self.downloader.root().join(weight_assets(size)[0].relative_path)
    }

    pub fn model_installed(&self, size: &'static Size) -> bool {
        weight_assets(size).iter().all(|asset| self.downloader.is_installed(asset))
    }

    /// The weights of a size not downloaded yet.
    pub fn missing(&self, size: &'static Size) -> Vec<&'static Asset> {
        weight_assets(size).iter().filter(|asset| !self.downloader.is_installed(asset)).collect()
    }

    /// The weights of a size still to download.
    pub fn missing_bytes(&self, size: &'static Size) -> u64 {
        self.missing(size).iter().map(|asset| asset.bytes.saturating_sub(self.downloader.partial_bytes(asset))).sum()
    }

    /// Deletes a size's weights; the transcriber stays.
    pub fn remove(&self, size: &'static Size) -> anyhow::Result<u64> {
        let mut freed = 0;
        for asset in weight_assets(size) {
            freed += self.downloader.remove(asset)?;
        }
        Ok(freed)
    }

    /// Where a file handed in by path is written: the studio's own folder.
    pub fn loose_output(&self, source: &Path) -> PathBuf {
        let stem = source.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_else(|| "audio".into());
        self.downloader.root().join("files").join(format!("{stem}.mid"))
    }

    pub fn work_dir(&self) -> PathBuf {
        self.downloader.root().join("work")
    }
}

/// One note in seconds, as the transcriber heard it or the MIDI editor holds it; the transcriber gives no velocity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub pitch: u8,
    pub start: f64,
    pub end: f64,
    pub instrument: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub velocity: Option<u8>,
}

/// A transcription at work, or the last one.
#[derive(Debug, Clone, Serialize)]
pub struct Run {
    /// The library track, or none for a file named by its path.
    pub song_id: Option<String>,
    pub title: String,
    pub size: &'static str,
    /// preparing, downloading, reading, transcribing, done.
    pub stage: &'static str,
    pub chunks_done: u32,
    pub chunks_total: u32,
    pub notes: usize,
    /// When this track's run began, in milliseconds since the Unix epoch.
    pub started_ms: u64,
    pub done: bool,
    pub error: Option<String>,
    /// The MIDI file on this computer, once written.
    pub file: Option<String>,
    /// The set this track belongs to, when several were asked for at once.
    pub batch: Option<Batch>,
}

/// Several library tracks turned into MIDI one after another.
#[derive(Debug, Clone, Serialize)]
pub struct Batch {
    /// How many of the set are finished, made or failed.
    pub done: usize,
    pub total: usize,
    /// The tracks still waiting, in order.
    pub queued: Vec<String>,
    /// Each track that failed, by its title, with the reason.
    pub failed: Vec<String>,
}

impl Run {
    pub fn progress(&self) -> f64 {
        if self.done && self.error.is_none() {
            1.0
        } else if self.chunks_total == 0 {
            0.0
        } else {
            f64::from(self.chunks_done) / f64::from(self.chunks_total)
        }
    }
}

/// What a song's MIDI says about itself, kept beside the .mid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sidecar {
    pub size: String,
    /// The tempo the file is written at: the track's score or its measured beat; none leaves 120 BPM.
    #[serde(default)]
    pub bpm: Option<u32>,
    pub made_at: String,
    pub instruments: Vec<String>,
    pub notes: Vec<Note>,
}

/// Collects the transcriber's events into notes.
#[derive(Default)]
pub struct Events {
    pub notes: Vec<Note>,
    open: HashMap<i64, usize>,
    pub finished: bool,
}

impl Events {
    /// Takes the event JSON audio.cpp's MuScriptor answers with (`text`): `start` events with a pitch, a time, an
    /// index and an instrument, and `end` events naming the start they close. An event of another type is passed over.
    pub fn take_all(&mut self, text: &str) -> Result<(), String> {
        let events: Vec<serde_json::Value> = serde_json::from_str(text).map_err(|error| format!("the transcriber's events are not JSON: {error}"))?;
        for event in &events {
            self.take(event);
        }
        self.finished = true;
        Ok(())
    }

    /// One event of the transcriber, as `take_all` reads each of its list.
    fn take(&mut self, event: &serde_json::Value) {
        match event["type"].as_str() {
            Some("start") => {
                let start = event["start_time"].as_f64().unwrap_or(0.0);
                self.notes.push(Note {
                    pitch: event["pitch"].as_u64().unwrap_or(0).min(127) as u8,
                    start,
                    end: start,
                    instrument: event["instrument"].as_str().unwrap_or("unknown").to_string(),
                    velocity: None,
                });
                if let Some(index) = event["index"].as_i64() {
                    self.open.insert(index, self.notes.len() - 1);
                }
            }
            Some("end") => {
                if let (Some(index), Some(time)) = (event["start_event_index"].as_i64(), event["end_time"].as_f64()) {
                    if let Some(at) = self.open.remove(&index) {
                        self.notes[at].end = time.max(self.notes[at].start);
                    }
                }
            }
            _ => {}
        }
    }

    /// One event as the server streams it while the transcriber works (the payload of an artifact, the JSON of one
    /// event): a note's start or end goes into the notes, as in `take_all`; a `progress` event gives the chunks done
    /// and the chunks in all.
    pub fn take_streamed(&mut self, payload: &[u8]) -> Result<Option<(usize, usize)>, String> {
        let event: serde_json::Value = serde_json::from_slice(payload).map_err(|error| format!("a streamed event is not JSON: {error}"))?;
        if event["type"].as_str() == Some("progress") {
            let count = |field: &str| event[field].as_u64().map(|value| value as usize);
            return Ok(count("completed").zip(count("total")));
        }
        self.take(&event);
        Ok(None)
    }

    /// Takes ROSVOT's notes (`text`: `{"notes":[{"pitch","start","end","word_index"}]}`); a pitch of zero is a rest.
    pub fn take_sung(&mut self, text: &str) -> Result<(), String> {
        let answer: serde_json::Value = serde_json::from_str(text).map_err(|error| format!("ROSVOT's notes are not JSON: {error}"))?;
        let notes = answer["notes"].as_array().ok_or("ROSVOT's answer has no notes")?;
        for note in notes {
            let pitch = note["pitch"].as_f64().unwrap_or(0.0).round();
            if pitch <= 0.0 {
                continue;
            }
            let start = note["start"].as_f64().unwrap_or(0.0);
            self.notes.push(Note {
                pitch: pitch.min(127.0) as u8,
                start,
                end: note["end"].as_f64().unwrap_or(start).max(start),
                instrument: "vocals".to_string(),
                velocity: None,
            });
        }
        self.finished = true;
        Ok(())
    }

    pub fn instruments(&self) -> Vec<String> {
        let mut names: Vec<String> = self.notes.iter().map(|note| note.instrument.clone()).collect();
        names.sort();
        names.dedup();
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_transcribers_events_become_notes() {
        let mut events = Events::default();
        events
            .take_all(
                r#"[{"type":"start","pitch":60,"start_time":0.5,"index":0,"instrument":"acoustic_piano"},
                    {"type":"start","pitch":36,"start_time":0.5,"index":1,"instrument":"drums"},
                    {"type":"end","end_time":0.52,"start_event_index":1},
                    {"type":"end","end_time":1.25,"start_event_index":0}]"#,
            )
            .unwrap();
        assert_eq!(events.notes.len(), 2);
        assert_eq!((events.notes[0].pitch, events.notes[0].start, events.notes[0].end), (60, 0.5, 1.25));
        assert_eq!(events.notes[1].instrument, "drums");
        assert!(events.finished);
        assert_eq!(events.instruments(), ["acoustic_piano", "drums"]);
        assert!(Events::default().take_all("[ace-midi] a log line").is_err());
    }

/// The events the server streams while it works give the notes the final answer gives, and its progress the chunks.
    #[test]
    fn the_streamed_events_give_the_notes_of_the_answer() {
        let events = [
            r#"{"type":"start","pitch":65,"start_time":0.32,"index":0,"instrument":"acoustic_piano"}"#,
            r#"{"type":"start","pitch":36,"start_time":0.5,"index":1,"instrument":"drums"}"#,
            r#"{"type":"progress","completed":1,"total":6}"#,
            r#"{"type":"end","end_time":0.52,"start_event_index":1}"#,
            r#"{"type":"end","end_time":1.25,"start_event_index":0}"#,
        ];
        let mut streamed = Events::default();
        let mut progress = Vec::new();
        for event in events {
            if let Some(chunks) = streamed.take_streamed(event.as_bytes()).unwrap() {
                progress.push(chunks);
            }
        }
        assert_eq!(progress, [(1, 6)]);
        let mut answered = Events::default();
        answered.take_all(&format!("[{}]", events.join(","))).unwrap();
        let pairs = |events: &Events| events.notes.iter().map(|note| (note.pitch, note.start, note.end, note.instrument.clone())).collect::<Vec<_>>();
        assert_eq!(pairs(&streamed), pairs(&answered));
        assert_eq!(pairs(&streamed)[0], (65, 0.32, 1.25, "acoustic_piano".to_string()));
        assert!(streamed.take_streamed(b"not json").is_err());
    }

    #[test]
    fn every_size_has_its_files_on_the_mirror() {
        for size in &SIZES {
            let assets = weight_assets(size);
            if size.every_instrument() {
                assert_eq!(assets.len(), 2);
                assert!(assets.iter().all(|asset| asset.url.contains(size.revision) && asset.url.contains("cocktailpeanut/muscriptor-")));
            } else {
                assert_eq!(assets.len(), 1);
                assert!(assets[0].url.contains(size.revision) && assets[0].bytes == size.bytes);
            }
        }
        assert!(size(DEFAULT_SIZE).is_some_and(Size::every_instrument));
    }

    #[test]
    fn a_new_tempo_keeps_every_note_at_its_second() {
        use crate::score::smf;
        let meta = vec![(0, smf::tempo(500_000))];
        let notes = vec![(480, smf::note_on(0, 64, 100)), (960, smf::note_off(0, 64))];
        let file = smf::write(&[meta, notes], 480, 960).unwrap();
        let retimed = with_tempo(&file, 60).unwrap();
        let song = smf::read(&retimed).unwrap();
        assert_eq!(song.tempos, [(0, 1_000_000)]);
        let note = song.tracks.iter().flat_map(|track| &track.notes).next().unwrap();
        assert_eq!((note.start, note.end, note.pitch), (240, 480, 64));
        assert!(with_tempo(b"not midi", 90).is_err());
    }

    #[test]
    fn the_sung_notes_become_vocal_notes_without_rests() {
        let mut events = Events::default();
        events.take_sung(r#"{"notes":[{"pitch":0,"start":0.0,"end":0.4,"word_index":0},{"pitch":64,"start":0.4,"end":0.9,"word_index":0}]}"#).unwrap();
        assert_eq!(events.notes.len(), 1);
        assert_eq!((events.notes[0].pitch, events.notes[0].start, events.notes[0].end), (64, 0.4, 0.9));
        assert_eq!(events.instruments(), ["vocals"]);
        assert!(Events::default().take_sung("[]").is_err());
    }
}
