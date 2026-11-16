//! Ableton Live sets, written from a set Live itself saved.
//!
//! The `.als` format is gzipped XML that Ableton does not document, so nothing
//! here is written from a guess at it: a set saved by Live 12 is the template,
//! and the set made for a song is that file with its MIDI and audio tracks
//! copied, renamed and given fresh ids, and its tempo and time signature set.
//! Everything else (devices, returns, the main track, Live's own settings) is
//! left exactly as Live wrote it.
//!
//! Live numbers two kinds of things. Automation and modulation targets and
//! pointees share one counter for the whole set (`NextPointeeId` is the next
//! free one), so every copy gets new ones; tracks have ids of their own. Clip
//! slots, sends and lanes are numbered within their track and stay as they are.

use std::io::{Read, Write};

use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;

/// The set Live Lite 12.4.6 saved, empty: one MIDI track, one audio track,
/// the Reverb and Delay returns and the main track.
pub const LIVE_12_TEMPLATE: &[u8] = include_bytes!("../assets/ableton/live12-default.als");

/// An audio clip and a MIDI clip as Live 12.4.6 saved them in Arrangement
/// view, with what belongs to that set taken out: the sample's name and paths,
/// its size, checksum and date, its warp markers, and the notes and clip
/// envelopes of the MIDI file it came from.
const AUDIO_CLIP: &str = include_str!("../assets/ableton/live12-audio-clip.xml");
const MIDI_CLIP: &str = include_str!("../assets/ableton/live12-midi-clip.xml");

/// One track to make, named as it shows in Live.
#[derive(Debug, Clone)]
pub struct TrackPlan {
    pub name: String,
    /// One of Live's clip colour indices (0..=69).
    pub color: u32,
    /// What the track holds at bar 1, if anything.
    pub clip: Option<Clip>,
    /// Live's track activator: an inactive track is in the set and silent until switched on.
    pub active: bool,
}

/// A clip at the start of the arrangement.
#[derive(Debug, Clone)]
pub enum Clip {
    Audio(AudioClip),
    Midi(MidiClip),
}

#[derive(Debug, Clone)]
pub struct AudioClip {
    pub name: String,
    /// The sample's path from the folder the set is in, with `/` between folders.
    pub relative_path: String,
    pub frames: u64,
    pub sample_rate: u32,
    pub file_size: u64,
    /// When the sample was last changed, in seconds since 1970.
    pub modified: u64,
}

#[derive(Debug, Clone)]
pub struct MidiClip {
    pub name: String,
    /// The clip's length in beats (quarter notes).
    pub beats: f64,
    pub notes: Vec<ClipNote>,
}

/// A note, timed in beats (quarter notes) from the clip's start.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipNote {
    pub key: u8,
    pub time: f64,
    pub duration: f64,
    pub velocity: u8,
}

#[derive(Debug, Clone)]
pub struct SetPlan {
    pub bpm: f64,
    pub meter: (u32, u32),
    /// MIDI tracks first, as Live's default set has them, then audio.
    pub midi: Vec<TrackPlan>,
    pub audio: Vec<TrackPlan>,
}

pub fn gunzip(data: &[u8]) -> Result<String> {
    let mut text = String::new();
    flate2::read::GzDecoder::new(data).read_to_string(&mut text).context("an .als is gzipped XML")?;
    Ok(text)
}

pub fn gzip(text: &str) -> Result<Vec<u8>> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(text.as_bytes())?;
    Ok(encoder.finish()?)
}

/// Where the first element opening with `open` starts and where its closing tag ends.
fn element(xml: &str, open: &str, close: &str) -> Option<(usize, usize)> {
    let start = xml.find(open)?;
    let end = xml[start..].find(close)? + start + close.len();
    Some((start, end))
}

/// Live's encoding of a time signature: 99 for each doubling of the denominator, plus the numerator less one.
pub fn time_signature_value(numerator: u32, denominator: u32) -> Result<u32> {
    if !(1..=99).contains(&numerator) || !denominator.is_power_of_two() || !(1..=16).contains(&denominator) {
        bail!("Live has no time signature {numerator}/{denominator}");
    }
    Ok(99 * denominator.trailing_zeros() + numerator - 1)
}

fn attribute(value: &str) -> String {
    value.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

fn global_ids() -> &'static Regex {
    static PATTERN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    // any element named for a target (`AutomationTarget`, `VolumeModulationTarget`,
    // `ControllerTargets.0` …) or a pointee draws its id from the set-wide counter
    PATTERN.get_or_init(|| Regex::new(r#"<([\w.]*Target[\w.]*|Pointee) Id="(\d+)""#).expect("a valid pattern"))
}

/// A copy of a track with its own track id, fresh set-wide ids, a name and a colour.
fn copy_track(block: &str, kind: &str, track_id: u64, next: &mut u64, plan: &TrackPlan, set: &SetPlan) -> Result<String> {
    let renumbered = global_ids().replace_all(block, |captures: &regex::Captures| {
        let id = *next;
        *next += 1;
        format!(r#"<{} Id="{id}""#, &captures[1])
    });
    let mut text = renumbered.into_owned();
    // the track's own id, on its opening tag
    let head_end = text.find('>').ok_or_else(|| anyhow!("a {kind} without an opening tag"))?;
    let head = Regex::new(r#"Id="\d+""#)?.replace(&text[..head_end], format!(r#"Id="{track_id}""#)).into_owned();
    text.replace_range(..head_end, &head);
    // its name: the first Name block is the track's
    let (name_start, name_end) = element(&text, "<Name>", "</Name>").ok_or_else(|| anyhow!("a {kind} without a name"))?;
    let name = &text[name_start..name_end];
    let named = Regex::new(r#"<UserName Value="[^"]*" />"#)?
        // a name is taken as written: a `$` in a song's title is not a reference to a group
        .replace(name, regex::NoExpand(&format!(r#"<UserName Value="{}" />"#, attribute(&plan.name))))
        .into_owned();
    text.replace_range(name_start..name_end, &named);
    // and its colour, the first Color after the name
    let color_at = text[name_start..].find("<Color Value=\"").map(|at| at + name_start).ok_or_else(|| anyhow!("a {kind} without a colour"))?;
    let color_end = text[color_at..].find("/>").map(|at| at + color_at + 2).ok_or_else(|| anyhow!("a broken colour"))?;
    text.replace_range(color_at..color_end, &format!(r#"<Color Value="{}" />"#, plan.color.min(69)));
    if !plan.active {
        // the activator is the mixer's Speaker; the template saves it on
        let speaker = text.find("<Speaker>").ok_or_else(|| anyhow!("a {kind} without a track activator"))?;
        let mut rest = text[speaker..].to_string();
        let manual = rest.find("<Manual Value=\"true\" />").ok_or_else(|| anyhow!("the {kind}'s activator is not on in the template"))?;
        let close = rest.find("</Speaker>").ok_or_else(|| anyhow!("a broken track activator"))?;
        if manual > close {
            bail!("the {kind}'s activator has no value");
        }
        rest.replace_range(manual..manual + "<Manual Value=\"true\" />".len(), "<Manual Value=\"false\" />");
        text.replace_range(speaker.., &rest);
    }
    if let Some(clip) = &plan.clip {
        match (kind, clip) {
            ("MidiTrack", Clip::Midi(_)) | ("AudioTrack", Clip::Audio(_)) => {}
            _ => bail!("{} cannot go on a {kind}", clip_name(clip)),
        }
        place_clip(&mut text, kind, &clip_xml(clip, plan.color, set.bpm, set.meter)?, clip_name(clip))?;
    }
    Ok(text)
}

/// Sets the first `<tag Value="…" />` in `text`.
fn set_value(text: &mut String, tag: &str, value: &str) -> Result<()> {
    let open = format!("<{tag} Value=\"");
    let at = text.find(&open).ok_or_else(|| anyhow!("Live's clip has no {tag}"))? + open.len();
    let end = text[at..].find('"').ok_or_else(|| anyhow!("a broken {tag}"))? + at;
    text.replace_range(at..end, &attribute(value));
    Ok(())
}

fn clip_name(clip: &Clip) -> &str {
    match clip {
        Clip::Audio(audio) => &audio.name,
        Clip::Midi(midi) => &midi.name,
    }
}

/// A clip's XML, from Live's own clip with this clip's length, sample or notes put in.
fn clip_xml(clip: &Clip, color: u32, bpm: f64, meter: (u32, u32)) -> Result<String> {
    let (mut text, beats) = match clip {
        Clip::Audio(audio) => {
            if audio.frames == 0 || audio.sample_rate == 0 {
                bail!("{} has no audio", audio.name);
            }
            (AUDIO_CLIP.to_string(), audio.frames as f64 / f64::from(audio.sample_rate) * bpm / 60.0)
        }
        Clip::Midi(midi) => {
            if !(midi.beats > 0.0) {
                bail!("{} has no length", midi.name);
            }
            (MIDI_CLIP.to_string(), midi.beats)
        }
    };
    let length = format_number(beats);
    for tag in ["CurrentEnd", "LoopEnd", "OutMarker", "HiddenLoopEnd", "RightTime"] {
        set_value(&mut text, tag, &length)?;
    }
    for tag in ["CurrentStart", "LoopStart", "StartRelative", "HiddenLoopStart", "LeftTime"] {
        set_value(&mut text, tag, "0")?;
    }
    // the whole song once, not repeated if the clip is drawn longer
    set_value(&mut text, "LoopOn", "false")?;
    set_value(&mut text, "Name", clip_name(clip))?;
    set_value(&mut text, "Color", &color.min(69).to_string())?;
    set_value(&mut text, "Numerator", &meter.0.to_string())?;
    set_value(&mut text, "Denominator", &meter.1.to_string())?;
    match clip {
        Clip::Audio(audio) => {
            set_value(&mut text, "RelativePath", &audio.relative_path)?;
            set_value(&mut text, "OriginalFileSize", &audio.file_size.to_string())?;
            set_value(&mut text, "LastModDate", &audio.modified.to_string())?;
            set_value(&mut text, "DefaultDuration", &audio.frames.to_string())?;
            set_value(&mut text, "DefaultSampleRate", &audio.sample_rate.to_string())?;
            // the markers below are the clip's timing: Live is not to warp it again by ear
            set_value(&mut text, "SamplesToAutoWarp", "0")?;
            set_value(&mut text, "MarkersGenerated", "false")?;
            // Re-Pitch: at the set's tempo the sample plays back at its own rate, untouched
            set_value(&mut text, "WarpMode", "3")?;
            let seconds = audio.frames as f64 / f64::from(audio.sample_rate);
            let markers = format!(
                "<WarpMarkers>\n\t\t<WarpMarker Id=\"0\" SecTime=\"0\" BeatTime=\"0\" />\n\t\t<WarpMarker Id=\"1\" SecTime=\"{}\" BeatTime=\"{length}\" />\n\t</WarpMarkers>",
                format_number(seconds)
            );
            text = text.replacen("<WarpMarkers />", &markers, 1);
        }
        Clip::Midi(midi) => {
            let mut keys: std::collections::BTreeMap<u8, Vec<ClipNote>> = Default::default();
            for note in midi.notes.iter().filter(|note| note.duration > 0.0 && note.time >= 0.0 && note.key < 128) {
                keys.entry(note.key).or_default().push(*note);
            }
            let mut next_note = 1u64;
            let mut tracks = String::from("<KeyTracks>\n");
            for (index, (key, mut notes)) in keys.into_iter().enumerate() {
                notes.sort_by(|a, b| a.time.total_cmp(&b.time));
                tracks.push_str(&format!("\t\t\t<KeyTrack Id=\"{index}\">\n\t\t\t\t<Notes>\n"));
                for note in notes {
                    tracks.push_str(&format!(
                        "\t\t\t\t\t<MidiNoteEvent Time=\"{}\" Duration=\"{}\" Velocity=\"{}\" OffVelocity=\"64\" NoteId=\"{next_note}\" />\n",
                        format_number(note.time),
                        format_number(note.duration),
                        note.velocity.clamp(1, 127)
                    ));
                    next_note += 1;
                }
                tracks.push_str(&format!("\t\t\t\t</Notes>\n\t\t\t\t<MidiKey Value=\"{key}\" />\n\t\t\t</KeyTrack>\n"));
            }
            tracks.push_str("\t\t</KeyTracks>");
            text = text.replacen("<KeyTracks />", &tracks, 1);
            let generator = text.find("<NoteIdGenerator>").ok_or_else(|| anyhow!("Live's MIDI clip has no note ids"))?;
            let mut rest = text[generator..].to_string();
            set_value(&mut rest, "NextId", &next_note.to_string())?;
            text.replace_range(generator.., &rest);
        }
    }
    Ok(text)
}

/// Puts a clip at the start of a track's arrangement: the empty event list
/// that the track's main sequencer keeps clips in, `ClipTimeable` on a MIDI
/// track and `Sample` on an audio track, gets it.
fn place_clip(track: &mut String, kind: &str, clip: &str, name: &str) -> Result<()> {
    let holder = match kind {
        "MidiTrack" => "<ClipTimeable>",
        _ => "<Sample>",
    };
    let sequencer = track.find("<MainSequencer>").ok_or_else(|| anyhow!("a {kind} without a sequencer"))?;
    let sequencer_end = track[sequencer..].find("</MainSequencer>").ok_or_else(|| anyhow!("a broken sequencer"))? + sequencer;
    let held = track[sequencer..sequencer_end].find(holder).ok_or_else(|| anyhow!("a {kind} without {holder}"))? + sequencer;
    let events = track[held..sequencer_end].find("<Events />").ok_or_else(|| anyhow!("the {kind} already holds clips"))? + held;
    let indent = &track[track[..events].rfind('\n').map_or(0, |at| at + 1)..events];
    let inner = format!("{indent}\t");
    let body: Vec<String> = clip.trim_end().lines().map(|line| format!("{inner}{line}")).collect();
    let filled = format!("<Events>\n{}\n{indent}</Events>", body.join("\n"));
    track.replace_range(events..events + "<Events />".len(), &filled);
    // Live remembers the first clip's name on the track
    let remembered = track.find("<MemorizedFirstClipName Value=\"").ok_or_else(|| anyhow!("a {kind} without MemorizedFirstClipName"))?;
    let mut rest = track[remembered..].to_string();
    set_value(&mut rest, "MemorizedFirstClipName", name)?;
    track.replace_range(remembered.., &rest);
    Ok(())
}

/// The template with the planned tracks in place of its own, at the planned tempo and meter.
pub fn build_xml(template: &str, plan: &SetPlan) -> Result<String> {
    if !(20.0..=999.0).contains(&plan.bpm) {
        bail!("Live's tempo runs from 20 to 999 BPM, not {}", plan.bpm);
    }
    let signature = time_signature_value(plan.meter.0, plan.meter.1)?;
    let (midi_start, midi_end) = element(template, "<MidiTrack ", "</MidiTrack>").ok_or_else(|| anyhow!("the template has no MIDI track"))?;
    let (audio_start, audio_end) = element(template, "<AudioTrack ", "</AudioTrack>").ok_or_else(|| anyhow!("the template has no audio track"))?;
    if audio_start != midi_end + template[midi_end..audio_start].len() || template[midi_end..audio_start].trim() != "" {
        bail!("the template's MIDI and audio tracks are not side by side");
    }
    let midi_block = &template[midi_start..midi_end];
    let audio_block = &template[audio_start..audio_end];
    let indent = &template[template[..midi_start].rfind('\n').map_or(0, |at| at + 1)..midi_start];

    let next_pattern = Regex::new(r#"<NextPointeeId Value="(\d+)" />"#)?;
    let mut next: u64 = next_pattern.captures(template).and_then(|found| found[1].parse().ok()).ok_or_else(|| anyhow!("the template has no NextPointeeId"))?;
    // track ids: above every id any track or return already has
    let track_ids = Regex::new(r#"<(?:MidiTrack|AudioTrack|ReturnTrack|GroupTrack) Id="(\d+)""#)?;
    let mut track_id = track_ids.captures_iter(template).filter_map(|found| found[1].parse::<u64>().ok()).max().unwrap_or(0) + 1;

    let mut tracks = Vec::new();
    for (plans, block, kind) in [(&plan.midi, midi_block, "MidiTrack"), (&plan.audio, audio_block, "AudioTrack")] {
        for track in plans {
            tracks.push(copy_track(block, kind, track_id, &mut next, track, plan)?);
            track_id += 1;
        }
    }
    let joined = tracks.join(&format!("\n{indent}"));
    let mut xml = format!("{}{}{}", &template[..midi_start], joined, &template[audio_end..]);
    xml = next_pattern.replace(&xml, format!(r#"<NextPointeeId Value="{next}" />"#)).into_owned();

    // tempo and time signature live on the main track's mixer
    let (main_start, main_end) = element(&xml, "<MainTrack ", "</MainTrack>").ok_or_else(|| anyhow!("the template has no main track"))?;
    let mut main = xml[main_start..main_end].to_string();
    for (tag, value) in [("<Tempo>", format_number(plan.bpm)), ("<TimeSignature>", signature.to_string())] {
        let at = main.find(tag).ok_or_else(|| anyhow!("the main track has no {tag}"))?;
        let manual = main[at..].find("<Manual Value=\"").map(|found| found + at).ok_or_else(|| anyhow!("{tag} has no value"))?;
        let end = main[manual..].find("/>").map(|found| found + manual + 2).ok_or_else(|| anyhow!("a broken {tag}"))?;
        main.replace_range(manual..end, &format!(r#"<Manual Value="{value}" />"#));
        // the arrangement plays the main track's automation, not the manual value:
        // Live keeps one envelope each for tempo and time signature, holding a
        // single event, and that event is what the set actually runs at
        let target = Regex::new(r#"<AutomationTarget Id="(\d+)""#)?
            .captures(&main[at..])
            .map(|found| found[1].to_string())
            .ok_or_else(|| anyhow!("{tag} has no automation target"))?;
        let pointee = format!(r#"<PointeeId Value="{target}" />"#);
        if let Some(envelope) = main.find(&pointee) {
            let events_end = main[envelope..].find("</Events>").map(|found| found + envelope).ok_or_else(|| anyhow!("a broken {tag} envelope"))?;
            let events = &main[envelope..events_end];
            if Regex::new(r#"<(?:Float|Enum)Event "#)?.find_iter(events).count() != 1 {
                bail!("the template's {tag} envelope is not a single value");
            }
            let set = Regex::new(r#"(<(?:Float|Enum)Event Id="\d+" Time="[^"]*" Value=")[^"]*(")"#)?.replace(events, format!("${{1}}{value}${{2}}")).into_owned();
            main.replace_range(envelope..events_end, &set);
        }
    }
    xml.replace_range(main_start..main_end, &main);
    Ok(xml)
}

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 { format!("{}", value as i64) } else { format!("{value}") }
}

/// A gzipped set from a gzipped template.
pub fn build(template_gz: &[u8], plan: &SetPlan) -> Result<Vec<u8>> {
    gzip(&build_xml(&gunzip(template_gz)?, plan)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn plan() -> SetPlan {
        SetPlan {
            bpm: 92.0,
            meter: (6, 8),
            midi: vec![TrackPlan { name: "Lead (from score)".into(), color: 14, clip: None, active: true }, TrackPlan { name: "Chords (from score)".into(), color: 15, clip: None, active: true }],
            audio: ["Mix", "Drums (separated)", "Bass (separated)"].iter().map(|name| TrackPlan { name: (*name).into(), color: 3, clip: None, active: true }).collect(),
        }
    }

    #[test]
    fn an_inactive_track_is_in_the_set_with_its_activator_off() {
        let mut plan = plan();
        plan.audio[1].active = false;
        let built = build_xml(&gunzip(LIVE_12_TEMPLATE).unwrap(), &plan).unwrap();
        let document = roxmltree::Document::parse(&built).unwrap();
        let activators: Vec<(String, String)> = document
            .descendants()
            .filter(|node| node.has_tag_name("AudioTrack") || node.has_tag_name("MidiTrack"))
            .map(|track| {
                let name = track.descendants().find(|node| node.has_tag_name("UserName")).unwrap().attribute("Value").unwrap().to_string();
                let speaker = track.descendants().find(|node| node.has_tag_name("Speaker")).unwrap();
                (name, value(speaker, "Manual").to_string())
            })
            .collect();
        assert_eq!(activators, [
            ("Lead (from score)".to_string(), "true".to_string()),
            ("Chords (from score)".to_string(), "true".to_string()),
            ("Mix".to_string(), "true".to_string()),
            ("Drums (separated)".to_string(), "false".to_string()),
            ("Bass (separated)".to_string(), "true".to_string()),
        ]);
    }

    fn with_clips() -> SetPlan {
        let mut plan = plan();
        plan.bpm = 90.0;
        plan.meter = (4, 4);
        plan.midi[0].clip = Some(Clip::Midi(MidiClip {
            name: "Lead".into(),
            beats: 8.0,
            notes: vec![
                ClipNote { key: 62, time: 0.0, duration: 1.0, velocity: 90 },
                ClipNote { key: 65, time: 1.5, duration: 0.5, velocity: 90 },
                ClipNote { key: 62, time: 4.0, duration: 2.0, velocity: 100 },
            ],
        }));
        plan.audio[1].clip = Some(Clip::Audio(AudioClip {
            name: "Drums (separated)".into(),
            relative_path: "Samples/Imported/Song - Drums (separated).wav".into(),
            frames: 960_000,
            sample_rate: 48_000,
            file_size: 2_880_044,
            modified: 1_760_000_000,
        }));
        plan
    }

    fn child<'a>(node: roxmltree::Node<'a, 'a>, tag: &str) -> roxmltree::Node<'a, 'a> {
        node.children().find(|node| node.has_tag_name(tag)).unwrap_or_else(|| panic!("no {tag} under {}", node.tag_name().name()))
    }

    fn value<'a>(node: roxmltree::Node<'a, 'a>, tag: &str) -> &'a str {
        node.descendants().find(|node| node.has_tag_name(tag)).unwrap_or_else(|| panic!("no {tag}")).attribute("Value").unwrap()
    }

    /// The element names of a clip, leaving out the lists whose length is the clip's own.
    fn clip_shape(node: roxmltree::Node) -> Vec<String> {
        let mut names = Vec::new();
        let mut stack = vec![node];
        while let Some(node) = stack.pop() {
            names.push(node.tag_name().name().to_string());
            if ["KeyTracks", "WarpMarkers", "Envelopes", "PerNoteEventStore"].contains(&node.tag_name().name()) {
                continue;
            }
            stack.extend(node.children().filter(|node| node.is_element()).collect::<Vec<_>>().into_iter().rev());
        }
        names
    }

    /// The element names of a subtree in document order, with values blanked: the shape Live reads.
    fn shape(node: roxmltree::Node) -> Vec<String> {
        node.descendants().filter(|node| node.is_element()).map(|node| node.tag_name().name().to_string()).collect()
    }

    #[test]
    fn time_signatures_use_lives_encoding() {
        // the template's 4/4 is stored as 201
        assert_eq!(time_signature_value(4, 4).unwrap(), 201);
        assert_eq!(time_signature_value(3, 4).unwrap(), 200);
        assert_eq!(time_signature_value(6, 8).unwrap(), 302);
        assert!(time_signature_value(4, 3).is_err());
    }

    #[test]
    fn the_template_is_live_12() {
        let xml = gunzip(LIVE_12_TEMPLATE).unwrap();
        let document = roxmltree::Document::parse(&xml).unwrap();
        let root = document.root_element();
        assert_eq!(root.attribute("Creator"), Some("Ableton Live 12.4.6"));
        assert!(xml.contains(r#"<Manual Value="201" />"#));
    }

    #[test]
    fn a_built_set_has_the_planned_tracks_and_live_shaped_xml() {
        let template = gunzip(LIVE_12_TEMPLATE).unwrap();
        let built = gunzip(&build(LIVE_12_TEMPLATE, &plan()).unwrap()).unwrap();
        let document = roxmltree::Document::parse(&built).expect("well-formed XML");
        let original = roxmltree::Document::parse(&template).unwrap();
        let tracks: Vec<roxmltree::Node> = document.descendants().filter(|node| node.has_tag_name("Tracks")).next().unwrap().children().filter(|node| node.is_element()).collect();
        let kinds: Vec<&str> = tracks.iter().map(|node| node.tag_name().name()).collect();
        assert_eq!(kinds, ["MidiTrack", "MidiTrack", "AudioTrack", "AudioTrack", "AudioTrack", "ReturnTrack", "ReturnTrack"]);
        let names: Vec<&str> = tracks
            .iter()
            .take(5)
            .map(|track| track.children().find(|node| node.has_tag_name("Name")).unwrap().children().find(|node| node.has_tag_name("UserName")).unwrap().attribute("Value").unwrap())
            .collect();
        assert_eq!(names, ["Lead (from score)", "Chords (from score)", "Mix", "Drums (separated)", "Bass (separated)"]);
        // every copy has exactly the shape of the track Live saved
        let template_midi = original.descendants().find(|node| node.has_tag_name("MidiTrack")).unwrap();
        let template_audio = original.descendants().find(|node| node.has_tag_name("AudioTrack")).unwrap();
        for track in &tracks[..2] {
            assert_eq!(shape(*track), shape(template_midi));
        }
        for track in &tracks[2..5] {
            assert_eq!(shape(*track), shape(template_audio));
        }
        // set-wide ids are unique and below NextPointeeId; track ids are unique
        let next: u64 = document.descendants().find(|node| node.has_tag_name("NextPointeeId")).unwrap().attribute("Value").unwrap().parse().unwrap();
        let mut seen = HashSet::new();
        for node in document.descendants().filter(|node| node.is_element() && (node.tag_name().name().contains("Target") || node.has_tag_name("Pointee"))) {
            if let Some(id) = node.attribute("Id") {
                let id: u64 = id.parse().unwrap();
                assert!(id < next, "{} id {id} is not below NextPointeeId {next}", node.tag_name().name());
                assert!(seen.insert(id), "id {id} is used twice");
            }
        }
        let track_ids: Vec<&str> = tracks.iter().map(|track| track.attribute("Id").unwrap()).collect();
        assert_eq!(track_ids.iter().collect::<HashSet<_>>().len(), track_ids.len());
        // tempo and meter on the main track
        let main = document.descendants().find(|node| node.has_tag_name("MainTrack")).unwrap();
        let manual = |tag: &str| main.descendants().find(|node| node.has_tag_name(tag)).unwrap().children().find(|node| node.has_tag_name("Manual")).unwrap().attribute("Value").unwrap().to_string();
        assert_eq!(manual("Tempo"), "92");
        assert_eq!(manual("TimeSignature"), "302");
        // and the automation the arrangement actually plays agrees
        let envelope_value = |tag: &str| {
            let target = main.descendants().find(|node| node.has_tag_name(tag)).unwrap().children().find(|node| node.has_tag_name("AutomationTarget")).unwrap().attribute("Id").unwrap();
            let envelope = main
                .descendants()
                .filter(|node| node.has_tag_name("AutomationEnvelope"))
                .find(|node| node.descendants().any(|inner| inner.has_tag_name("PointeeId") && inner.attribute("Value") == Some(target)))
                .unwrap();
            envelope.descendants().find(|node| node.has_tag_name("FloatEvent") || node.has_tag_name("EnumEvent")).unwrap().attribute("Value").unwrap().to_string()
        };
        assert_eq!(envelope_value("Tempo"), "92");
        assert_eq!(envelope_value("TimeSignature"), "302");
        // every element Live numbers from the set-wide counter, whatever it is called, is unique
        let mut every = HashSet::new();
        for node in document.descendants().filter(|node| node.is_element()) {
            let name = node.tag_name().name();
            if let (true, Some(id)) = (name.contains("Target") || name == "Pointee", node.attribute("Id")) {
                assert!(every.insert(id), "{name} repeats id {id}");
            }
        }
        // the rest of the set is untouched: returns and main track keep their shape
        let main_before = original.descendants().find(|node| node.has_tag_name("MainTrack")).unwrap();
        assert_eq!(shape(main), shape(main_before));
    }

    #[test]
    fn names_are_escaped_and_tempo_is_checked() {
        let mut odd = plan();
        odd.midi[0].name = r#"Lead "A" <1> & 2"#.into();
        let built = build_xml(&gunzip(LIVE_12_TEMPLATE).unwrap(), &odd).unwrap();
        let document = roxmltree::Document::parse(&built).unwrap();
        assert!(document.descendants().any(|node| node.attribute("Value") == Some(r#"Lead "A" <1> & 2"#)));
        odd.midi[0].name = "$100 Bill ${x} $name".into();
        let built = build_xml(&gunzip(LIVE_12_TEMPLATE).unwrap(), &odd).unwrap();
        let document = roxmltree::Document::parse(&built).unwrap();
        assert!(document.descendants().any(|node| node.attribute("Value") == Some("$100 Bill ${x} $name")), "a dollar sign in a title is a dollar sign");
        odd.bpm = 5.0;
        assert!(build_xml(&gunzip(LIVE_12_TEMPLATE).unwrap(), &odd).is_err());
    }

    #[test]
    fn clips_sit_where_live_keeps_them_with_the_songs_timing() {
        let built = gunzip(&build(LIVE_12_TEMPLATE, &with_clips()).unwrap()).unwrap();
        let document = roxmltree::Document::parse(&built).expect("well-formed XML");
        let tracks: Vec<roxmltree::Node> = document.descendants().find(|node| node.has_tag_name("Tracks")).unwrap().children().filter(|node| node.is_element()).collect();

        let lead = tracks[0];
        let events = child(child(child(child(child(lead, "DeviceChain"), "MainSequencer"), "ClipTimeable"), "ArrangerAutomation"), "Events");
        let clip = child(events, "MidiClip");
        assert_eq!(clip.attribute("Time"), Some("0"));
        assert_eq!(value(clip, "CurrentEnd"), "8");
        assert_eq!(value(clip, "LoopOn"), "false");
        assert_eq!(value(clip, "Name"), "Lead");
        let keys: Vec<&str> = clip.descendants().filter(|node| node.has_tag_name("MidiKey")).map(|node| node.attribute("Value").unwrap()).collect();
        assert_eq!(keys, ["62", "65"]);
        let notes: Vec<(String, String)> = clip
            .descendants()
            .filter(|node| node.has_tag_name("MidiNoteEvent"))
            .map(|node| (node.attribute("Time").unwrap().to_string(), node.attribute("Duration").unwrap().to_string()))
            .collect();
        assert_eq!(notes, [("0".into(), "1".into()), ("4".into(), "2".into()), ("1.5".into(), "0.5".into())]);
        let ids: HashSet<&str> = clip.descendants().filter_map(|node| node.attribute("NoteId")).collect();
        assert_eq!(ids.len(), 3);
        assert_eq!(value(child(child(clip, "Notes"), "NoteIdGenerator"), "NextId"), "4");
        assert_eq!(value(lead, "MemorizedFirstClipName"), "Lead");

        // 20 s at 90 BPM is 30 beats, and the markers hold the sample at its own rate
        let drums = tracks[3];
        let sample = child(child(child(drums, "DeviceChain"), "MainSequencer"), "Sample");
        let clip = child(child(child(sample, "ArrangerAutomation"), "Events"), "AudioClip");
        assert_eq!(value(clip, "CurrentEnd"), "30");
        assert_eq!(value(clip, "RelativePath"), "Samples/Imported/Song - Drums (separated).wav");
        assert_eq!(value(clip, "DefaultDuration"), "960000");
        assert_eq!(value(clip, "DefaultSampleRate"), "48000");
        assert_eq!(value(clip, "WarpMode"), "3");
        let markers: Vec<(&str, &str)> = clip.descendants().filter(|node| node.has_tag_name("WarpMarker")).map(|node| (node.attribute("SecTime").unwrap(), node.attribute("BeatTime").unwrap())).collect();
        assert_eq!(markers, [("0", "0"), ("20", "30")]);

        // tracks without a clip are left empty, and nothing else about any track changed shape
        let mix = tracks[2];
        assert!(child(child(child(child(child(mix, "DeviceChain"), "MainSequencer"), "Sample"), "ArrangerAutomation"), "Events").children().all(|node| !node.is_element()));
        let template = gunzip(LIVE_12_TEMPLATE).unwrap();
        let original = roxmltree::Document::parse(&template).unwrap();
        let template_audio = original.descendants().find(|node| node.has_tag_name("AudioTrack")).unwrap();
        let without_clip = |node: roxmltree::Node| -> Vec<String> {
            node.descendants().filter(|node| node.is_element() && !node.ancestors().any(|above| above.has_tag_name("AudioClip") || above.has_tag_name("MidiClip"))).map(|node| node.tag_name().name().to_string()).collect()
        };
        assert_eq!(without_clip(drums), shape(template_audio));
    }

    #[test]
    fn a_clip_goes_only_on_its_kind_of_track() {
        let mut wrong = with_clips();
        wrong.audio[0].clip = wrong.midi[0].clip.clone();
        assert!(build_xml(&gunzip(LIVE_12_TEMPLATE).unwrap(), &wrong).is_err());
    }

    /// Our clips against a set Live saved with clips in it: the same elements,
    /// in the same place. Run with KINETIC_LIVE_CLIP_SET naming such a set
    /// (an audio clip on its first audio track, a MIDI clip on its first MIDI track).
    #[test]
    #[ignore]
    fn clips_match_a_set_live_saved() {
        let path = std::env::var("KINETIC_LIVE_CLIP_SET").expect("KINETIC_LIVE_CLIP_SET");
        let theirs = gunzip(&std::fs::read(path).unwrap()).unwrap();
        let theirs = roxmltree::Document::parse(&theirs).unwrap();
        let ours = gunzip(&build(LIVE_12_TEMPLATE, &with_clips()).unwrap()).unwrap();
        let ours = roxmltree::Document::parse(&ours).unwrap();
        for tag in ["AudioClip", "MidiClip"] {
            let mine = ours.descendants().find(|node| node.has_tag_name(tag)).unwrap();
            let live = theirs.descendants().find(|node| node.has_tag_name(tag)).unwrap();
            assert_eq!(clip_shape(mine), clip_shape(live), "{tag}");
            let path = |node: roxmltree::Node| node.ancestors().skip(1).take(6).map(|node| node.tag_name().name().to_string()).collect::<Vec<_>>();
            assert_eq!(path(mine), path(live), "{tag}");
        }
    }
}
