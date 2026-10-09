//! Lyrics laid along a bare tune: the bars of a score each line of the words
//! is sung on.
//!
//! YuE2 learned from songs whose words and score belong together: a line on a
//! phrase of the tune, with about as many notes as it has syllables and a
//! breath before the next. A tune from a MIDI file has no words, and words
//! sung over it as it stands come out as something else: measured by the
//! reference, eight lines over the Pirates of the Caribbean theme were heard 0
//! of 46 words in order, and laid along the GTA San Andreas intro 94 to 98
//! percent. So a score that names no section is laid out for its words first:
//!
//! - the voice's notes split into phrases at a rest of an eighth or more and
//!   after a note held a half note or longer, and at their bar lines;
//! - each line takes the phrase, or up to eight pieces in a row, whose notes
//!   come nearest its syllables at 0.8 to 3.5 syllables a second, following
//!   on in the tune, skipping what does not fit and starting the tune again
//!   when the words outlast it;
//! - a section whose lines repeat an earlier one's is sung on its bars;
//! - up to eight bars before the first phrase stay as the intro, and the score
//!   ends one empty bar after the last line.
//!
//! Syllables are counted from the letters, not a dictionary. Ported from
//! YuE2-ComfyUI's `phrasing.py`.

use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

use regex::Regex;
use unicode_normalization_alignments::char::{canonical_combining_class, decompose_canonical};

use super::{abc, rebuild, sections, Q};

/// Quarter notes of rest that end a phrase: an eighth note.
const BREATH: Q = Q::HALF;
/// Quarter notes a note is held for that end the phrase it closes: a half note.
const HELD: i64 = 2;
/// Pieces of the tune in a row that one line may be sung on.
const MOST_PIECES: usize = 8;
/// Syllables a second above which a line is crowded.
const FASTEST: f64 = 3.5;
/// Syllables a second below which a line drags over its notes.
const SLOWEST: f64 = 0.8;
const PACE_WEIGHT: f64 = 2.0;
const JOIN: f64 = 0.15;
const MID_PHRASE: f64 = 0.2;
const JUMP: f64 = 0.5;
const SECTION_JUMP: f64 = 0.25;
const SKIP: f64 = 0.05;
const LOOP: f64 = 0.2;
/// Bars before the first phrase that stay as the intro.
const INTRO_BARS: usize = 8;
/// How much longer than its laid-out score a song may run before it is stopped, as a factor and in seconds.
const SLACK: f64 = 1.1;
const SLACK_SECONDS: f64 = 2.0;
/// Sections and bar ranges a notice names before it says how many more there are.
const LISTED: usize = 6;

const STRESSED_E: [char; 4] = ['\u{e9}', '\u{e8}', '\u{ea}', '\u{eb}'];
const CYRILLIC_VOWELS: [char; 13] = ['а', 'е', 'ё', 'и', 'о', 'у', 'ы', 'э', 'ю', 'я', 'і', 'ї', 'є'];
/// Words sung a syllable shorter than they are spelled: 'ev-ry', not 'ev-er-y'.
const SHORTENED: [&str; 21] = [
    "every", "everything", "everybody", "everywhere", "everyday", "evening", "evenings", "different", "difference", "differently", "several", "favorite", "favourite", "camera", "chocolate",
    "interest", "interesting", "business", "vegetable", "comfortable", "temperature",
];
const ALIASES: [(&str, &str); 5] = [("prechorus", "pre-chorus"), ("pre chorus", "pre-chorus"), ("post chorus", "post-chorus"), ("hook", "chorus"), ("refrain", "chorus")];

/// A score laid out for its words, what to say about how, and how long it runs.
#[derive(Clone, Debug)]
pub struct Laid {
    pub score: String,
    pub notices: Vec<(&'static str, String)>,
    pub seconds: f64,
    pub crowded: Option<Crowded>,
}

/// Notes and syllables parted by more than half again on average over the lines: `ratio` notes a
/// syllable on the middle line, the middle phrase's notes and the middle line's syllables.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Crowded {
    pub ratio: f64,
    pub notes: usize,
    pub syllables: usize,
}

#[derive(Clone, Copy, Debug)]
struct Piece {
    start: Q,
    end: Q,
    notes: usize,
    breath: bool,
}

/// Whether a score names a section: a `% label` comment line anywhere in it.
pub fn named(score: &str) -> bool {
    score.split('\n').any(|line| line.starts_with("% "))
}

/// The score with its section comments taken out.
pub fn bare(score: &str) -> String {
    score.split('\n').filter(|line| !line.starts_with("% ")).collect::<Vec<_>>().join("\n")
}

/// The score naming at least one section: one that names none is one `label` from its first bar.
/// Text with no `V: Vocal` line of music is given back as it is.
pub fn labelled(score: &str, label: &str) -> String {
    if named(score) {
        return score.to_string();
    }
    let lines: Vec<&str> = score.split('\n').collect();
    let Some(at) = lines.iter().position(|line| *line == "V: Vocal") else {
        return score.to_string();
    };
    let comment = format!("% {label}");
    lines[..at].iter().copied().chain([comment.as_str()]).chain(lines[at..].iter().copied()).collect::<Vec<_>>().join("\n")
}

/// The most seconds a song laid out on a tune this long is let sing: the tune, and a little over.
pub fn ceiling(seconds: f64) -> f64 {
    ((seconds * SLACK + SLACK_SECONDS) * 10.0).round() / 10.0
}

fn pattern(cell: &'static OnceLock<Regex>, text: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(text).expect("a valid pattern"))
}

fn word_pattern() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    pattern(&CELL, r"[^\W\d_]+(?:['\u{2019}][^\W\d_]+)*")
}

fn wide(character: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&character) || ('\u{3400}'..='\u{4dbf}').contains(&character) || ('\u{3040}'..='\u{30ff}').contains(&character) || ('\u{ac00}'..='\u{d7af}').contains(&character)
}

fn vowel_groups(text: &str, vowels: &[char]) -> usize {
    let mut count = 0;
    let mut inside = false;
    for character in text.chars() {
        let vowel = vowels.contains(&character);
        if vowel && !inside {
            count += 1;
        }
        inside = vowel;
    }
    count
}

const LATIN_VOWELS: [char; 9] = ['a', 'e', 'i', 'o', 'u', 'y', '\u{e6}', '\u{f8}', '\u{153}'];
const GREEK_VOWELS: [char; 7] = ['\u{3b1}', '\u{3b5}', '\u{3b7}', '\u{3b9}', '\u{3bf}', '\u{3c5}', '\u{3c9}'];

/// An 'i' sung as a syllable of its own before another vowel: pi-a-no, ra-di-o; not in -tion, -cial or mil-lion.
fn sounded_i(text: &[char]) -> usize {
    (1..text.len())
        .filter(|at| {
            let before = text[at - 1];
            text[*at] == 'i'
                && before.is_ascii_lowercase()
                && !"aeiouytscxgn".contains(before)
                && !(at >= &2 && text[at - 2] == 'l' && before == 'l')
                && text.get(at + 1).is_some_and(|next| "aou".contains(*next))
        })
        .count()
}

fn word_syllables(word: &str) -> usize {
    let lowered = word.to_lowercase();
    if lowered.chars().any(|character| ('\u{400}'..='\u{4ff}').contains(&character)) {
        return lowered.chars().filter(|character| CYRILLIC_VOWELS.contains(character)).count().max(1);
    }
    let wide_count = lowered.chars().filter(|character| wide(*character)).count();
    if wide_count > 0 {
        return wide_count;
    }
    let mut parted = String::new();
    let mut previous: Option<char> = None;
    for character in lowered.chars() {
        if "\u{ef}\u{eb}\u{fc}\u{ff}".contains(character) && previous.is_some_and(|before| "aeiouy".contains(before)) {
            parted.push('|');
        }
        parted.push(character);
        previous = Some(character);
    }
    let mut plain = String::new();
    for character in parted.chars() {
        decompose_canonical(character, |part| {
            if canonical_combining_class(part) == 0 {
                plain.push(part);
            }
        });
    }
    if plain.chars().any(|character| ('\u{370}'..='\u{3ff}').contains(&character)) {
        return vowel_groups(&plain, &GREEK_VOWELS).max(1);
    }
    let letters: Vec<char> = plain.chars().collect();
    let length = letters.len();
    let mut count = vowel_groups(&plain, &LATIN_VOWELS) as i64;
    if plain.ends_with("ing") && length > 3 && "aeiouy".contains(letters[length - 4]) {
        count += 1;
    }
    count += sounded_i(&letters) as i64;
    if SHORTENED.contains(&plain.as_str()) {
        count -= 1;
    }
    if count > 1 && length > 2 {
        if plain.ends_with('e')
            && !lowered.ends_with(STRESSED_E)
            && !(plain.ends_with("ee") || plain.ends_with("ie") || plain.ends_with("ye"))
            && !(plain.ends_with("le") && !"aeiouy".contains(letters[length - 3]))
        {
            count -= 1;
        } else if plain.ends_with("es") && length > 3 && !"aeiouysxzcgh".contains(letters[length - 3]) {
            count -= 1;
        } else if plain.ends_with("ed") && length > 3 && !"aeiouytd".contains(letters[length - 3]) {
            count -= 1;
        }
    }
    count.max(1) as usize
}

/// About how many syllables a line of words has: 0 for a line without a letter.
pub fn syllables(line: &str) -> usize {
    word_pattern().find_iter(line).map(|word| word_syllables(word.as_str())).sum()
}

/// The section comment a lyrics tag becomes: '[Verse 2]' a 'verse', '[Hook]' a 'chorus', an unknown tag a 'verse'.
pub fn label_of(tag: &str) -> String {
    static TRAILING: OnceLock<Regex> = OnceLock::new();
    let clean = tag.to_lowercase().replace('_', " ").split_whitespace().collect::<Vec<_>>().join(" ");
    let clean = pattern(&TRAILING, r"[\s\d:.#-]+$").replace(&clean, "").into_owned();
    let clean = ALIASES.iter().find(|(alias, _)| *alias == clean).map(|(_, label)| label.to_string()).unwrap_or(clean);
    if sections::TAGS.iter().any(|(label, _)| *label == clean) { clean } else { "verse".to_string() }
}

struct LyricSection {
    tag: String,
    label: String,
    lines: Vec<String>,
}

/// Text split at every line boundary Python's `str.splitlines` knows.
fn split_lines(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut start = 0;
    let mut characters = text.char_indices().peekable();
    while let Some((at, character)) = characters.next() {
        if matches!(character, '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{1c}' | '\u{1d}' | '\u{1e}' | '\u{85}' | '\u{2028}' | '\u{2029}') {
            found.push(&text[start..at]);
            let mut next = at + character.len_utf8();
            if character == '\r' && characters.peek().is_some_and(|(_, following)| *following == '\n') {
                characters.next();
                next += 1;
            }
            start = next;
        }
    }
    if start < text.len() {
        found.push(&text[start..]);
    }
    found
}

/// The lyrics as sections in order; lines before any tag are a verse; a line with no letter is not sung.
fn lyric_sections(lyrics: &str) -> Vec<LyricSection> {
    static TAG: OnceLock<Regex> = OnceLock::new();
    let tag_pattern = pattern(&TAG, r"^\[\s*([^\]]*?)\s*\]$");
    let mut found: Vec<LyricSection> = Vec::new();
    for raw in split_lines(lyrics) {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(tag) = tag_pattern.captures(line) {
            let tag = tag[1].to_string();
            found.push(LyricSection { label: label_of(&tag), tag, lines: Vec::new() });
            continue;
        }
        if syllables(line) == 0 {
            continue;
        }
        if found.is_empty() {
            found.push(LyricSection { tag: "Verse".into(), label: "verse".into(), lines: Vec::new() });
        }
        found.last_mut().expect("a section to hold the line").lines.push(line.to_string());
    }
    found
}

/// The section comments a score of these lyrics names, in order: each tag's label, words before any tag a verse.
pub fn section_labels(lyrics: &str) -> Vec<String> {
    lyric_sections(lyrics).into_iter().map(|section| section.label).collect()
}

fn phrases(notes: &[abc::Note]) -> Vec<Vec<abc::Note>> {
    let mut ordered = notes.to_vec();
    ordered.sort_by(|a, b| a.start.cmp(&b.start).then(a.pitch.cmp(&b.pitch)).then(a.duration.cmp(&b.duration)));
    let mut groups: Vec<Vec<abc::Note>> = Vec::new();
    for note in ordered {
        if let Some(group) = groups.last_mut() {
            let last = group.last().expect("a phrase holds a note");
            if note.start - (last.start + last.duration) < BREATH && last.duration < Q::int(HELD) {
                group.push(note);
                continue;
            }
        }
        groups.push(vec![note]);
    }
    groups
}

fn bar_index(bar_starts: &[Q], time: Q) -> usize {
    bar_starts.partition_point(|start| *start <= time).saturating_sub(1)
}

fn last_bar(bar_starts: &[Q], end: Q) -> usize {
    bar_starts.partition_point(|start| *start < end).saturating_sub(1)
}

/// The voice's notes as the pieces lines are laid on: phrases cut at their bar lines, the first piece of each a breath.
fn pieces(notes: &[abc::Note], bar_starts: &[Q]) -> Vec<Piece> {
    let mut found = Vec::new();
    for group in phrases(notes) {
        let mut chunks: Vec<(usize, Vec<abc::Note>)> = Vec::new();
        for note in group {
            let bar = bar_starts.partition_point(|start| *start <= note.start) as i64 - 1;
            match chunks.last_mut() {
                Some((at, chunk)) if *at as i64 == bar => chunk.push(note),
                _ => chunks.push((bar.max(0) as usize, vec![note])),
            }
        }
        for (position, (_, chunk)) in chunks.iter().enumerate() {
            let last = chunk.last().expect("a chunk holds a note");
            found.push(Piece { start: chunk[0].start, end: last.start + last.duration, notes: chunk.len(), breath: position == 0 });
        }
    }
    found
}

/// `(k, m)` for each line of one section: it is sung on `m` pieces from piece `k`, the cheapest way through.
fn path(found: &[Piece], counts: &[usize], start: usize, first: bool, bpm: u32) -> Vec<(usize, usize)> {
    let size = found.len();
    let mut total = vec![0usize];
    for piece in found {
        total.push(total[total.len() - 1] + piece.notes);
    }
    let per_quarter = 60.0 / bpm as f64;
    let cost = |k: usize, m: usize, count: usize| -> f64 {
        let notes = (total[k + m] - total[k]) as f64;
        let seconds = (found[k + m - 1].end - found[k].start).to_f64() * per_quarter;
        let pace = if seconds > 0.0 { count as f64 / seconds } else { FASTEST * 8.0 };
        let mut value = (notes / count as f64).ln().abs();
        value += PACE_WEIGHT * ((pace / FASTEST).ln().max(0.0) + (SLOWEST / pace).ln().max(0.0));
        value += JOIN * found[k + 1..k + m].iter().filter(|piece| piece.breath).count() as f64;
        if k + m < size && !found[k + m].breath {
            value += MID_PHRASE;
        }
        value
    };
    let mut best = vec![f64::INFINITY; size + 1];
    best[start.min(size)] = 0.0;
    let mut steps: Vec<Vec<Option<(usize, usize, usize)>>> = Vec::new();
    for (index, count) in counts.iter().enumerate() {
        let jump = if index == 0 && !first { SECTION_JUMP } else { JUMP };
        let mut ahead = vec![(f64::INFINITY, None); size + 1];
        let mut low = (f64::INFINITY, None);
        for k in 0..=size {
            ahead[k] = low;
            if best[k] - SKIP * (k as f64) < low.0 {
                low = (best[k] - SKIP * (k as f64), Some(k));
            }
        }
        let mut behind = vec![(f64::INFINITY, None); size + 1];
        let mut low = (f64::INFINITY, None);
        for k in (0..=size).rev() {
            behind[k] = low;
            if best[k] < low.0 {
                low = (best[k], Some(k));
            }
        }
        let mut following = vec![f64::INFINITY; size + 1];
        let mut chosen: Vec<Option<(usize, usize, usize)>> = vec![None; size + 1];
        for k in 0..size {
            let (mut came, mut origin) = (best[k], k);
            if let (value, Some(from)) = ahead[k] {
                if value + jump + SKIP * (k as f64) < came {
                    (came, origin) = (value + jump + SKIP * (k as f64), from);
                }
            }
            if let (value, Some(from)) = behind[k] {
                if value + jump + LOOP < came {
                    (came, origin) = (value + jump + LOOP, from);
                }
            }
            if came == f64::INFINITY {
                continue;
            }
            for m in 1..=MOST_PIECES.min(size - k) {
                let value = came + cost(k, m, *count);
                if value < following[k + m] {
                    following[k + m] = value;
                    chosen[k + m] = Some((k, m, origin));
                }
            }
        }
        steps.push(chosen);
        best = following;
    }
    let mut end = (0..=size).min_by(|a, b| best[*a].total_cmp(&best[*b]).then(a.cmp(b))).expect("a tune has pieces");
    let mut spans = Vec::new();
    for chosen in steps.iter().rev() {
        let (k, m, origin) = chosen[end].expect("every line is sung somewhere");
        spans.push((k, m));
        end = origin;
    }
    spans.reverse();
    spans
}

fn line_key(lines: &[String]) -> Vec<String> {
    static CELL: OnceLock<Regex> = OnceLock::new();
    let pattern = pattern(&CELL, r"[^\w\s']");
    lines.iter().map(|line| pattern.replace_all(&line.to_lowercase(), " ").split_whitespace().collect::<Vec<_>>().join(" ")).collect()
}

/// Bar numbers as a person reads them: 'bar 5', 'bars 5-9', 'bars 1-4 and 9'.
fn bar_words(numbers: &[usize]) -> String {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for &number in numbers {
        match runs.last_mut() {
            Some(run) if number <= run.1 + 1 => run.1 = run.1.max(number),
            _ => runs.push((number, number)),
        }
    }
    let mut text: Vec<String> = runs.iter().map(|(low, high)| if low == high { low.to_string() } else { format!("{low}-{high}") }).collect();
    if text.len() > LISTED {
        let more = text.len() - LISTED;
        text.truncate(LISTED);
        text.push(format!("{more} more"));
    }
    let joined = if text.len() > 1 { format!("{} and {}", text[..text.len() - 1].join(", "), text[text.len() - 1]) } else { text[0].clone() };
    format!("{}{joined}", if runs.len() == 1 && runs[0].0 == runs[0].1 { "bar " } else { "bars " })
}

/// An ABC chord symbol such as `Bbm7/F` as the label the score writer takes, `Bb:min7/F`.
fn chord_label(symbol: &str) -> Result<String, String> {
    static CELL: OnceLock<Regex> = OnceLock::new();
    let found = pattern(&CELL, r"^(?P<root>[A-G](?:bb|##|b|#)?)(?P<quality>.*?)(?:/(?P<bass>[A-G](?:bb|##|b|#)?))?$").captures(symbol);
    let found = found.ok_or_else(|| format!("unsupported chord symbol {}", abc::repr(symbol)))?;
    let quality = rebuild::QUALITY_TEXT.iter().find(|(_, text)| *text == &found["quality"]).map(|(label, _)| *label);
    let quality = quality.ok_or_else(|| format!("unsupported chord symbol {}", abc::repr(symbol)))?;
    Ok(format!("{}:{quality}{}", &found["root"], found.name("bass").map(|bass| format!("/{}", bass.as_str())).unwrap_or_default()))
}

fn intervals<T: Clone>(starts: &[(Q, T)], end: Q) -> Vec<(Q, Q, T)> {
    let mut rows = Vec::new();
    for (index, (time, value)) in starts.iter().enumerate() {
        let following = starts.get(index + 1).map(|next| next.0).unwrap_or(end);
        if *time < following {
            rows.push((*time, following, value.clone()));
        }
    }
    rows
}

struct Run {
    first: usize,
    last: usize,
    windows: Vec<(Q, Q)>,
    labels: Vec<(Q, String)>,
}

fn write(parsed: &abc::Score, runs: &[Run]) -> Result<String, String> {
    let (vocal, instrument) = (&parsed.voices[abc::VOCAL], &parsed.voices[abc::INS]);
    let per_quarter = 60.0 / parsed.bpm as f64;
    let seconds = |time: Q| time.to_f64() * per_quarter;
    let chords = intervals(&vocal.chords, vocal.time);
    let mut rows = rebuild::Rows::default();
    let (mut keys, mut labels): (Vec<(Q, String)>, Vec<(Q, String)>) = (Vec::new(), Vec::new());
    let mut now = Q::ZERO;
    let mut meter = vocal.bars[0].meter;
    for run in runs {
        let begin = vocal.bars[run.first].start;
        let finish = vocal.bars[run.last].start + vocal.bars[run.last].length;
        let shift = now - begin;
        for bar in &vocal.bars[run.first..=run.last] {
            meter = bar.meter;
            let (numerator, denominator) = (bar.meter.0 as i64, bar.meter.1 as i64);
            for beat in 0..numerator {
                rows.beats.push((seconds(bar.start + shift + Q::int(beat) * Q::new(4, denominator)), beat + 1, numerator, denominator));
            }
        }
        for (track, voice) in [vocal, instrument].into_iter().enumerate() {
            for note in &voice.notes {
                if !(begin <= note.start && note.start < finish) {
                    continue;
                }
                if track == 0 && !run.windows.iter().any(|(low, high)| *low <= note.start && note.start < *high) {
                    continue;
                }
                rows.notes.push((seconds(note.start + shift), seconds((note.start + note.duration).min(finish) + shift), note.pitch, track));
            }
        }
        for (low, high, symbol) in &chords {
            let (low, high) = ((*low).max(begin), (*high).min(finish));
            if low < high {
                rows.chords.push((seconds(low + shift), seconds(high + shift), chord_label(symbol)?));
            }
        }
        let keyed = vocal.keys.iter().filter(|(time, _)| *time <= begin).last().map(|(_, key)| key.clone());
        keys.push((now, keyed.unwrap_or_else(|| vocal.keys[0].1.clone())));
        keys.extend(vocal.keys.iter().filter(|(time, _)| begin < *time && *time < finish).map(|(time, key)| (*time + shift, key.clone())));
        labels.extend(run.labels.iter().map(|(time, label)| (*time + shift, label.clone())));
        now += finish - begin;
    }
    let (numerator, denominator) = (meter.0 as i64, meter.1 as i64);
    for beat in 0..=numerator {
        rows.beats.push((seconds(now + Q::int(beat) * Q::new(4, denominator)), beat % numerator + 1, numerator, denominator));
    }
    now += Q::new(4 * numerator, denominator);
    let mut merged: Vec<(Q, String)> = Vec::new();
    for (time, key) in keys {
        match merged.last_mut() {
            Some(last) if last.1 == key => {}
            Some(last) if last.0 == time => *last = (time, key),
            _ => merged.push((time, key)),
        }
    }
    rows.keys = intervals(&merged, now).into_iter().map(|(low, high, key)| (seconds(low), seconds(high), key)).collect();
    rows.structures = intervals(&labels, now).into_iter().map(|(low, high, label)| (seconds(low), seconds(high), label)).collect();
    rows.notes.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)).then(a.3.cmp(&b.3)));
    let smallest = runs.iter().flat_map(|run| vocal.bars[run.first..=run.last].iter().map(|bar| bar.meter.1 as i64)).min().expect("a run holds a bar");
    let subbeats = (parsed.unit.den() / smallest).max(1) as usize;
    rebuild::build(&rows, vocal.chords.is_empty(), subbeats)
}

/// The score laid out for these lyrics, or None when it is sung as it came: it names a section, it is
/// not in YuE2's dialect, its voice has no notes, or the lyrics have no sung line.
pub fn lay(score: &str, lyrics: &str) -> Option<Laid> {
    if named(score) {
        return None;
    }
    let parsed = abc::parse(score).ok()?;
    let vocal = &parsed.voices[abc::VOCAL];
    let words = lyric_sections(lyrics);
    if vocal.notes.is_empty() || words.iter().all(|section| section.lines.is_empty()) {
        return None;
    }
    let bar_starts: Vec<Q> = vocal.bars.iter().map(|bar| bar.start).collect();
    let found = pieces(&vocal.notes, &bar_starts);
    let mut sung: Vec<(usize, Vec<usize>, Vec<(usize, usize)>)> = Vec::new();
    let mut done: HashMap<Vec<String>, Vec<(usize, usize)>> = HashMap::new();
    let mut position = 0;
    for (index, section) in words.iter().enumerate() {
        if section.lines.is_empty() {
            continue;
        }
        let counts: Vec<usize> = section.lines.iter().map(|line| syllables(line).max(1)).collect();
        let key = line_key(&section.lines);
        let spans = match done.get(&key) {
            Some(spans) => spans.clone(),
            None => {
                let spans = path(&found, &counts, position, sung.is_empty(), parsed.bpm);
                done.insert(key, spans.clone());
                spans
            }
        };
        let last = spans[spans.len() - 1];
        position = last.0 + last.1;
        sung.push((index, counts, spans));
    }

    let mut runs: Vec<Run> = Vec::new();
    let mut previous: Option<usize> = None;
    for (index, _, spans) in &sung {
        for (line, &(k, m)) in spans.iter().enumerate() {
            let (begin, end) = (found[k].start, found[k + m - 1].end);
            let (first, last) = (bar_index(&bar_starts, begin), last_bar(&bar_starts, end));
            match (runs.last_mut(), previous) {
                (Some(run), Some(previous)) if k == previous => {
                    run.last = run.last.max(last);
                    run.windows.last_mut().expect("a run has a window").1 = end;
                }
                (Some(run), Some(previous)) if k > previous && first <= run.last => {
                    run.last = run.last.max(last);
                    run.windows.push((begin, end));
                }
                _ => runs.push(Run { first, last, windows: vec![(begin, end)], labels: Vec::new() }),
            }
            if line == 0 {
                runs.last_mut().expect("a run was just made").labels.push((begin, words[*index].label.clone()));
            }
            previous = Some(k + m);
        }
    }
    let mut intro: Vec<usize> = Vec::new();
    if let Some((first_index, _, spans)) = sung.first() {
        if spans[0].0 == 0 && runs[0].first > 0 {
            let leading: Vec<&LyricSection> = words[..*first_index].iter().filter(|section| section.lines.is_empty()).collect();
            let old = runs[0].first;
            runs[0].first = old.saturating_sub(INTRO_BARS);
            intro = (runs[0].first..old).collect();
            let label = leading.last().map(|section| section.label.clone()).unwrap_or_else(|| "intro".to_string());
            let at = vocal.bars[runs[0].first].start;
            runs[0].labels.insert(0, (at, label));
        }
    }
    let written = write(&parsed, &runs).ok()?;
    let laid = abc::parse(&written).ok()?;
    let seconds = laid.voices[abc::VOCAL].time.to_f64() * 60.0 / laid.bpm as f64;

    let mut places: Vec<String> = Vec::new();
    if !intro.is_empty() {
        places.push(format!("{} as the intro", bar_words(&intro.iter().map(|bar| bar + 1).collect::<Vec<_>>())));
    }
    for (index, _, spans) in &sung {
        let bars: BTreeSet<usize> = spans.iter().flat_map(|&(k, m)| (bar_index(&bar_starts, found[k].start)..=last_bar(&bar_starts, found[k + m - 1].end)).map(|bar| bar + 1)).collect();
        places.push(format!("[{}] on {}", words[*index].tag, bar_words(&bars.into_iter().collect::<Vec<_>>())));
    }
    if places.len() > LISTED {
        let more = places.len() - LISTED;
        places.truncate(LISTED);
        places.push(format!("{more} more sections"));
    }
    let windows: Vec<(Q, Q)> = runs.iter().flat_map(|run| run.windows.iter().copied()).collect();
    let voiced: BTreeSet<usize> = vocal.notes.iter().map(|note| bar_index(&bar_starts, note.start)).collect();
    let heard: BTreeSet<usize> = vocal.notes.iter().filter(|note| windows.iter().any(|(low, high)| *low <= note.start && note.start < *high)).map(|note| bar_index(&bar_starts, note.start)).collect();
    let unsung: Vec<usize> = voiced.difference(&heard).map(|bar| bar + 1).collect();
    let mut notices = vec![(
        "notice",
        format!(
            "The score came without sections, so the lyrics were laid along its tune before it was sung: {}. {}The song ends one empty bar after the last line, {:.0} seconds in; with an automatic length the singing is stopped at {:.0} seconds should it run on.",
            places.join("; "),
            if unsung.is_empty() { String::new() } else { format!("Not sung: {} of the tune. ", bar_words(&unsung)) },
            seconds,
            ceiling(seconds)
        ),
    )];
    let mut ratios: Vec<f64> = Vec::new();
    for (_, counts, spans) in &sung {
        for (count, &(k, m)) in counts.iter().zip(spans) {
            ratios.push(found[k..k + m].iter().map(|piece| piece.notes).sum::<usize>() as f64 / *count as f64);
        }
    }
    let mut crowded = None;
    if ratios.iter().map(|ratio| ratio.ln().abs()).sum::<f64>() / ratios.len() as f64 > 1.5f64.ln() {
        let mut sorted = ratios.clone();
        sorted.sort_by(f64::total_cmp);
        let middle = sorted[sorted.len() / 2];
        let mut phrase_notes: Vec<usize> = phrases(&vocal.notes).iter().map(Vec::len).collect();
        phrase_notes.sort_unstable();
        let mut line_syllables: Vec<usize> = sung.iter().flat_map(|(_, counts, _)| counts.iter().copied()).collect();
        line_syllables.sort_unstable();
        let (notes, syllables) = (phrase_notes[phrase_notes.len() / 2], line_syllables[line_syllables.len() / 2]);
        crowded = Some(Crowded { ratio: middle, notes, syllables });
        notices.push((
            "warn",
            if middle > 1.0 {
                format!("The tune has about {middle:.1} notes for each syllable of these lyrics -- its phrases hold about {notes} notes, the lines about {syllables} syllables -- so YuE2 is likely to sing sounds or words of its own on the notes left over.")
            } else {
                format!("These lyrics have more syllables than the tune has notes for them -- its phrases hold about {notes} notes, the lines about {syllables} syllables -- so YuE2 is likely to crowd or drop words.")
            },
        ));
    }
    Some(Laid { score: written, notices, seconds, crowded })
}

#[cfg(test)]
mod tests;
