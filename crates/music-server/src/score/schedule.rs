//! The lyric schedule tied to the score clock (HOT-Step's C6): each sung lyric
//! block is tied to its score section's first Vocal note, so the engine keeps
//! a section's words out of the composer's sight until the score reaches them.
//! HOT-Step's listening test took the on-time sections of score-driven renders
//! from 2 of 11 to 11 of 11 with the soft bias used here.
//!
//! Score sections and lyric blocks are matched in order by kind, as HOT-Step's
//! drift metric does; spans are codepoints into the exact lyrics sent.

use std::sync::OnceLock;

use regex::Regex;

use super::{abc, sections};

/// The bias on the rows of a section not yet due: soft, HOT-Step's default.
pub const BIAS: f64 = -4.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Timed {
    pub start_sec: f64,
    /// Codepoints of the block in the lyrics, its tag line included.
    pub lyric: (usize, usize),
}

struct Block {
    label: String,
    start: usize,
    end: usize,
    sung: bool,
}

fn kind(label: &str) -> String {
    let normalized: String = label.to_lowercase().chars().map(|character| if matches!(character, '_' | '–' | '—') { '-' } else { character }).collect();
    static PRE: OnceLock<Regex> = OnceLock::new();
    static KIND: OnceLock<Regex> = OnceLock::new();
    if PRE.get_or_init(|| Regex::new(r"\bpre[ -]?chorus\b").expect("pre-chorus pattern")).is_match(&normalized) {
        return "pre-chorus".into();
    }
    KIND.get_or_init(|| Regex::new(r"\b(verse|chorus|bridge|intro|outro|interlude)\b").expect("kind pattern"))
        .captures(&normalized)
        .map(|found| found[1].to_string())
        .unwrap_or_else(|| normalized.trim().to_string())
}

fn instrumental(label: &str) -> bool {
    static PLAYED: OnceLock<Regex> = OnceLock::new();
    PLAYED.get_or_init(|| Regex::new(r"(?i)\b(instrumental|solo|break|interlude)\b").expect("instrumental pattern")).is_match(label)
}

fn blocks(lyrics: &str) -> Vec<Block> {
    static TAG: OnceLock<Regex> = OnceLock::new();
    let tag = TAG.get_or_init(|| Regex::new(r"^\[\s*([^\]]*?)\s*\]$").expect("tag pattern"));
    let length = lyrics.chars().count();
    let mut found: Vec<Block> = Vec::new();
    let mut offset = 0;
    for line in lyrics.split('\n') {
        if let Some(label) = tag.captures(line.trim()) {
            if let Some(last) = found.last_mut() {
                last.end = offset;
            }
            found.push(Block { label: label[1].to_string(), start: offset, end: length, sung: false });
        } else if let Some(last) = found.last_mut() {
            last.sung |= line.chars().any(char::is_alphanumeric);
        }
        offset += line.chars().count() + 1;
    }
    found
}

/// The schedule of a score and the lyrics sung from it, or None when there is
/// nothing to time: no sections, no sung block matching a timed section, or a
/// score outside YuE2's dialect.
pub fn schedule(score: &str, lyrics: &str) -> Option<Vec<Timed>> {
    if lyrics.contains('\r') {
        return None;
    }
    let parsed = abc::parse(score).ok()?;
    let found = sections::sections(score).ok()?;
    let vocal = &parsed.voices[abc::VOCAL];
    let tags = blocks(lyrics);
    let characters: Vec<char> = lyrics.chars().collect();
    let mut next = 0;
    let mut timed = Vec::new();
    for (index, section) in found.iter().enumerate() {
        if instrumental(&section.label) {
            continue;
        }
        let Some(matched) = (next..tags.len()).find(|&at| tags[at].sung && !instrumental(&tags[at].label) && kind(&tags[at].label) == kind(&section.label)) else {
            continue;
        };
        next = matched + 1;
        let end = found.get(index + 1).map(|following| following.start);
        let Some(first) = vocal.notes.iter().find(|note| note.start >= section.start && end.is_none_or(|end| note.start < end)) else {
            continue;
        };
        let block = &tags[matched];
        let mut close = block.end;
        while close > block.start && characters[close - 1].is_whitespace() {
            close -= 1;
        }
        let start_sec = (first.start.to_f64() * 60.0 / parsed.bpm as f64 * 1000.0).round() / 1000.0;
        timed.push(Timed { start_sec, lyric: (block.start, close) });
    }
    (!timed.is_empty()).then_some(timed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCORE: &str = "X:1\nT:\nM:4/4\nL:1/16\nQ:1/4=120\nV: Vocal clef=treble name=\"Vocal Melody\" snm=\"Vocal\"\nV: Ins clef=treble name=\"Ins Melody\" snm=\"Inst.\"\nK:C\n% intro\nV: Vocal\nz16|z16|\nV: Ins\nC16|C16|\n% verse\nV: Vocal\nz8C8|D16|\nV: Ins\nZ2|\n% chorus\nV: Vocal\nE16|F16|\nV: Ins\nZ2|\n";

    #[test]
    fn each_sung_block_waits_for_its_sections_first_vocal_note() {
        let lyrics = "[Intro]\n\n[Verse 1]\nline one\nline two\n\n[Chorus]\nsing it\n";
        let timed = schedule(SCORE, lyrics).expect("a schedule");
        assert_eq!(timed.len(), 2);
        // verse: bar 3 at 120 BPM is 4 s in, its first note half a bar later
        assert_eq!(timed[0].start_sec, 5.0);
        assert_eq!(timed[1].start_sec, 8.0);
        let text: Vec<char> = lyrics.chars().collect();
        let verse: String = text[timed[0].lyric.0..timed[0].lyric.1].iter().collect();
        assert_eq!(verse, "[Verse 1]\nline one\nline two");
        let chorus: String = text[timed[1].lyric.0..timed[1].lyric.1].iter().collect();
        assert_eq!(chorus, "[Chorus]\nsing it");
    }

    #[test]
    fn spans_count_codepoints_and_unmatched_blocks_stay_visible() {
        let lyrics = "[Куплет]\nпервая строка\n[Chorus]\nприпев";
        let timed = schedule(SCORE, lyrics).expect("the chorus is timed");
        assert_eq!(timed.len(), 1);
        let text: Vec<char> = lyrics.chars().collect();
        let chorus: String = text[timed[0].lyric.0..timed[0].lyric.1].iter().collect();
        assert_eq!(chorus, "[Chorus]\nприпев");
        assert!(schedule(SCORE, "no tags at all").is_none());
        assert!(schedule("X:1\nnot a score", "[Verse]\nwords").is_none());
    }
}
