//! "Match sections to the score" for a cover (after HOT-Step's Cover Studio): each lyric block
//! is retagged with the score section it is sung in, found from where the recogniser heard the
//! block's first line in the source recording.
//!
//! Words never change. A block keeps its tag when it already names the same kind of section; a
//! block the recogniser did not hear, or one that starts in the last quarter of a section, keeps
//! its tag and place and is reported as unsure. A sung score section with no block gets a copy of
//! the last block of the same kind (a chorus the lyrics wrote once) unless an unsure block sits in
//! that gap, since it may be the one sung there. Sections the voice does not sing get an empty tag,
//! and the lyrics' own empty tags give way to them.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use super::{abc, schedule, sections};

/// A first line heard up to a bar before a section starts is a pickup into it.
const PICKUP_BARS: f64 = 1.0;
/// A block opening a section must start in its first three quarters: SheetSage's section starts are
/// approximate, but a block that starts later is more likely misplaced than late.
const MAX_SECTION_FRACTION: f64 = 0.75;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BlockReport {
    /// 1-based, in the original lyrics.
    pub index: usize,
    pub tag: Option<String>,
    pub new_tag: Option<String>,
    /// 1-based score section, none when the block was left where it was.
    pub section: Option<usize>,
    pub first_line_seconds: Option<f64>,
    /// kept, renamed, merged, unsure or dropped.
    pub status: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Filled {
    pub section: usize,
    pub tag: String,
    pub copied_from: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Proposal {
    pub lyrics: String,
    pub blocks: Vec<BlockReport>,
    pub filled: Vec<Filled>,
    pub unchanged: bool,
}

struct Block {
    tag: Option<String>,
    body: String,
    /// How many sung lines of the lyrics come before this block's first one.
    first_line: usize,
    lines: usize,
}

fn tag_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    trimmed.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')).filter(|inner| !inner.is_empty() && !inner.contains(']')).map(|inner| inner.trim().to_string())
}

fn split_blocks(lyrics: &str) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut sung = 0usize;
    for line in lyrics.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
        if let Some(tag) = tag_line(line) {
            blocks.push(Block { tag: Some(tag), body: String::new(), first_line: sung, lines: 0 });
            continue;
        }
        if blocks.is_empty() {
            blocks.push(Block { tag: None, body: String::new(), first_line: sung, lines: 0 });
        }
        let block = blocks.last_mut().expect("a block was just made");
        if !block.body.is_empty() || !line.trim().is_empty() {
            block.body.push_str(if block.body.is_empty() { "" } else { "\n" });
            block.body.push_str(line);
        }
        if !line.trim().is_empty() {
            block.lines += 1;
            sung += 1;
        }
    }
    for block in &mut blocks {
        block.body = block.body.trim_end_matches('\n').to_string();
        block.body = block.body.trim_end().to_string();
    }
    blocks.retain(|block| block.tag.is_some() || !block.body.trim().is_empty());
    blocks
}

/// A score tag in the lyrics' own style: lower case when most of their tags are.
fn styled(tag: &str, lower: bool) -> String {
    if lower { tag.to_lowercase() } else { tag.to_string() }
}

struct Span {
    tag: &'static str,
    label: String,
    start: f64,
    sung: bool,
}

/// The proposal for a cover's lyrics against its score. `line_starts` are the lyrics' sung lines in
/// order, each with the second the recogniser heard it start (`lyrics_sync::heard_line_starts`).
pub fn match_sections(score: &str, lyrics: &str, line_starts: &[(String, Option<f64>)]) -> Result<Proposal, String> {
    let parsed = abc::parse(score)?;
    let found = sections::sections(score)?;
    if found.iter().all(|section| section.label.is_empty()) {
        return Err("The score has no section labels.".into());
    }
    let seconds = |quarters: f64| quarters * 60.0 / parsed.bpm.max(1) as f64;
    let vocal = &parsed.voices[abc::VOCAL];
    let first_bar = vocal.bars.first().ok_or("The score has no bars.")?;
    let bar_seconds = seconds(first_bar.length.to_f64());
    let song_end = vocal.bars.last().map(|bar| seconds(bar.start.to_f64() + bar.length.to_f64())).unwrap_or(0.0);
    let spans: Vec<Span> = found
        .iter()
        .map(|section| Span { tag: section.tag, label: section.label.clone(), start: seconds(section.start.to_f64()), sung: section.notes > 0 })
        .collect();
    let span_end = |index: usize| spans.get(index + 1).map(|next| next.start).unwrap_or(song_end);
    let containing = |at: f64| spans.iter().rposition(|span| span.start <= at).unwrap_or(0);

    let blocks = split_blocks(lyrics);
    let tags: Vec<&String> = blocks.iter().filter_map(|block| block.tag.as_ref()).collect();
    let lower = tags.iter().filter(|tag| tag.chars().next().is_some_and(char::is_lowercase)).count() * 2 > tags.len();
    let tag_of = |index: usize| styled(spans[index].tag, lower);

    // Where each block is sung: its first line as heard, when it was.
    let mut last: Option<usize> = None;
    let placed: Vec<(Option<f64>, Option<usize>)> = blocks
        .iter()
        .map(|block| {
            let heard = (block.lines > 0).then(|| line_starts.get(block.first_line).and_then(|(_, at)| *at)).flatten();
            let mut section = heard.map(|at| {
                let index = containing(at + PICKUP_BARS * bar_seconds);
                let late = (at - spans[index].start) / (span_end(index) - spans[index].start).max(1e-6) > MAX_SECTION_FRACTION;
                (Some(index) == last || !late).then_some(index)
            }).flatten();
            // The recogniser is monotonic; a block placed before its predecessor is unsure, not moved.
            if section.is_some_and(|index| last.is_some_and(|previous| index < previous)) {
                section = None;
            }
            if section.is_some() {
                last = section;
            }
            (heard, section)
        })
        .collect();

    let mut out: Vec<String> = Vec::new();
    let mut report: Vec<BlockReport> = Vec::new();
    let mut filled: Vec<Filled> = Vec::new();
    let mut last_of_kind: HashMap<String, usize> = HashMap::new();
    let mut emitted: HashSet<usize> = HashSet::new();
    let mut current: Option<usize> = None;
    let mut unsure_since = false;
    let mut last_was_placed = false;

    let mut fill = |up_to: usize, instrumental_only: bool, current: Option<usize>, unsure_since: bool, last_of_kind: &HashMap<String, usize>, out: &mut Vec<String>, last_was_placed: &mut bool| {
        let from = current.map_or(0, |index| index + 1);
        for index in from..up_to {
            if emitted.contains(&index) || (instrumental_only && spans[index].sung) {
                continue;
            }
            let source = if spans[index].sung && !unsure_since { last_of_kind.get(&schedule::kind(&spans[index].label)).copied() } else { None };
            if spans[index].sung && source.is_none() {
                continue;
            }
            let body = source.map(|block| blocks[block].body.clone()).unwrap_or_default();
            out.push(if body.is_empty() { format!("[{}]", tag_of(index)) } else { format!("[{}]\n{}", tag_of(index), body) });
            emitted.insert(index);
            *last_was_placed = false;
            filled.push(Filled { section: index + 1, tag: tag_of(index), copied_from: source.map(|block| block + 1) });
        }
    };

    // Sections before the first one the voice sings belong to no block: they open the lyrics.
    if let Some(first_sung) = spans.iter().position(|span| span.sung) {
        fill(first_sung, true, current, unsure_since, &last_of_kind, &mut out, &mut last_was_placed);
    }
    for (position, (block, (heard, section))) in blocks.iter().zip(&placed).enumerate() {
        if block.body.trim().is_empty() {
            report.push(BlockReport { index: position + 1, tag: block.tag.clone(), new_tag: None, section: None, first_line_seconds: None, status: "dropped" });
            continue;
        }
        let mergeable = section.is_some() && *section == current && last_was_placed;
        if section.is_none() || (*section == current && !mergeable) {
            if let Some(at) = heard {
                fill(containing(*at), true, current, unsure_since, &last_of_kind, &mut out, &mut last_was_placed);
            }
            out.push(match &block.tag {
                Some(tag) => format!("[{tag}]\n{}", block.body),
                None => block.body.clone(),
            });
            report.push(BlockReport { index: position + 1, tag: block.tag.clone(), new_tag: block.tag.clone(), section: None, first_line_seconds: *heard, status: "unsure" });
            unsure_since = true;
            last_was_placed = false;
            continue;
        }
        let index = section.expect("placed");
        if mergeable {
            if let Some(previous) = out.last_mut() {
                previous.push_str("\n\n");
                previous.push_str(&block.body);
            }
            report.push(BlockReport { index: position + 1, tag: block.tag.clone(), new_tag: None, section: Some(index + 1), first_line_seconds: *heard, status: "merged" });
            continue;
        }
        fill(index, false, current, unsure_since, &last_of_kind, &mut out, &mut last_was_placed);
        let keep = block.tag.as_ref().is_some_and(|tag| schedule::kind(tag) == schedule::kind(&spans[index].label) || schedule::kind(tag) == schedule::kind(spans[index].tag));
        let new_tag = if keep { block.tag.clone().expect("kept") } else { tag_of(index) };
        out.push(format!("[{new_tag}]\n{}", block.body));
        report.push(BlockReport { index: position + 1, tag: block.tag.clone(), new_tag: Some(new_tag), section: Some(index + 1), first_line_seconds: *heard, status: if keep { "kept" } else { "renamed" } });
        last_of_kind.insert(schedule::kind(&spans[index].label), position);
        current = Some(index);
        unsure_since = false;
        last_was_placed = true;
    }
    fill(spans.len(), false, current, unsure_since, &last_of_kind, &mut out, &mut last_was_placed);

    let result = out.join("\n\n");
    let unchanged = result == lyrics.replace("\r\n", "\n").trim();
    Ok(Proposal { lyrics: result, blocks: report, filled, unchanged })
}

#[cfg(test)]
mod tests {
    use super::*;

    // 120 BPM, 4/4: a bar is 2 s. intro bars 1-2 (no voice), verse 3-4, chorus 5-6, verse 7-8, chorus 9-10.
    const SCORE: &str = "X:1\nT:\nM:4/4\nL:1/16\nQ:1/4=120\nV: Vocal clef=treble name=\"Vocal Melody\" snm=\"Vocal\"\nV: Ins clef=treble name=\"Ins Melody\" snm=\"Inst.\"\nK:C\n% intro\nV: Vocal\nz16|z16|\nV: Ins\nC16|C16|\n% verse\nV: Vocal\nC16|D16|\nV: Ins\nZ2|\n% chorus\nV: Vocal\nE16|F16|\nV: Ins\nZ2|\n% verse\nV: Vocal\nC16|D16|\nV: Ins\nZ2|\n% chorus\nV: Vocal\nE16|F16|\nV: Ins\nZ2|\n";

    fn heard(lines: &[(&str, Option<f64>)]) -> Vec<(String, Option<f64>)> {
        lines.iter().map(|(line, at)| ((*line).to_string(), *at)).collect()
    }

    #[test]
    fn blocks_take_the_tag_of_the_section_they_are_sung_in_and_a_chorus_written_once_is_copied() {
        let lyrics = "[Intro]\n\n[Verse]\nfirst line\nsecond line\n\n[Hook]\nsing it\n\n[Verse 2]\nthird line";
        let lines = heard(&[("first line", Some(4.1)), ("second line", Some(6.0)), ("sing it", Some(8.2)), ("third line", Some(12.3))]);
        let proposal = match_sections(SCORE, lyrics, &lines).expect("a proposal");
        assert_eq!(
            proposal.lyrics,
            "[Intro]\n\n[Verse]\nfirst line\nsecond line\n\n[Chorus]\nsing it\n\n[Verse 2]\nthird line\n\n[Chorus]\nsing it"
        );
        assert_eq!(proposal.blocks[0].status, "dropped");
        assert_eq!(proposal.blocks[1].status, "kept");
        assert_eq!(proposal.blocks[2].status, "renamed");
        assert_eq!(proposal.blocks[3].status, "kept");
        assert_eq!(proposal.filled.last().map(|fill| fill.copied_from), Some(Some(3)));
        assert!(!proposal.unchanged);
    }

    #[test]
    fn a_block_not_heard_keeps_its_tag_and_place_and_blocks_copying_into_its_gap() {
        let lyrics = "[Verse]\nfirst line\n\n[Chorus]\nsing it\n\n[Bridge]\nlost words";
        let lines = heard(&[("first line", Some(4.0)), ("sing it", Some(8.0)), ("lost words", None)]);
        let proposal = match_sections(SCORE, lyrics, &lines).expect("a proposal");
        assert_eq!(proposal.blocks[2].status, "unsure");
        assert!(proposal.lyrics.contains("[Bridge]\nlost words"));
        assert!(!proposal.filled.iter().any(|fill| fill.copied_from.is_some()), "{:?}", proposal.filled);
    }

    #[test]
    fn an_intro_before_any_singing_opens_the_lyrics_even_when_no_block_was_heard() {
        let lyrics = "[Verse]\nfirst line\n\n[Chorus]\nsing it";
        let lines = heard(&[("first line", None), ("sing it", None)]);
        let proposal = match_sections(SCORE, lyrics, &lines).expect("a proposal");
        assert!(proposal.lyrics.starts_with("[Intro]\n\n[Verse]\nfirst line"), "{}", proposal.lyrics);
    }

    #[test]
    fn words_never_change_and_a_score_without_labels_is_refused() {
        let lyrics = "[Verse]\nfirst line\n\n[Chorus]\nsing it";
        let lines = heard(&[("first line", Some(4.0)), ("sing it", Some(8.0))]);
        let proposal = match_sections(SCORE, lyrics, &lines).expect("a proposal");
        let words = |text: &str| text.lines().filter(|line| tag_line(line).is_none()).map(str::to_string).collect::<Vec<_>>().join(" ");
        assert!(words(&proposal.lyrics).contains("first line") && words(&proposal.lyrics).contains("sing it"));
        assert!(match_sections("X:1\nM:4/4\nL:1/16\nQ:1/4=120\nV: Vocal\nV: Ins\nK:C\nV: Vocal\nC16|\nV: Ins\nC16|\n", lyrics, &lines).is_err());
    }
}
