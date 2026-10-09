//! An edited score belongs to the words it was made for. On Apply the editor
//! marks the score with the style, lyrics and `cot` in a comment line that is
//! taken off before anything is sung; other words leave the edit unsung
//! unless it was kept for new words. The mark is the one the ComfyUI node
//! writes, so a score carried between the two keeps its words.

use sha2::{Digest, Sha256};

pub const MARK_PREFIX: &str = "%yue2-words ";
const KEEP: &str = " keep";

/// What a score field holds: the score trimmed and without its mark, the mark, and whether it is kept for new words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub score: String,
    pub words: Option<String>,
    pub keep: bool,
}

/// A string as Python's `json.dumps` writes it: ASCII only, `\uXXXX` in lower case.
fn python_json(text: &str) -> String {
    let mut out = String::from("\"");
    for unit in text.encode_utf16() {
        match unit {
            0x22 => out.push_str("\\\""),
            0x5C => out.push_str("\\\\"),
            0x0A => out.push_str("\\n"),
            0x0D => out.push_str("\\r"),
            0x09 => out.push_str("\\t"),
            0x08 => out.push_str("\\b"),
            0x0C => out.push_str("\\f"),
            0x20..=0x7E => out.push(unit as u8 as char),
            _ => out.push_str(&format!("\\u{unit:04x}")),
        }
    }
    out.push('"');
    out
}

/// Sixteen hex digits naming the words a score was written for; the style and lyrics trimmed first.
pub fn mark(style: &str, lyrics: &str, cot: &str) -> String {
    let payload = format!("[{}, {}, {}]", python_json(style.trim()), python_json(lyrics.trim()), python_json(cot));
    let digest = Sha256::digest(payload.as_bytes());
    digest.iter().take(8).map(|byte| format!("{byte:02x}")).collect()
}

fn mark_line(line: &str) -> Option<(String, bool)> {
    let rest = line.strip_prefix(MARK_PREFIX)?;
    let words = rest.get(..16)?;
    if !words.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
        return None;
    }
    let rest = &rest[16..];
    let (keep, tail) = match rest.strip_prefix(KEEP) {
        Some(tail) => (true, tail),
        None => (false, rest),
    };
    tail.bytes().all(|byte| matches!(byte, b' ' | b'\t' | b'\r')).then(|| (words.to_string(), keep))
}

/// The score and its mark out of a score field: every mark line is taken out, the last one wins.
pub fn read(text: &str) -> Edit {
    let mut words = None;
    let mut keep = false;
    let mut kept = Vec::new();
    for line in text.split('\n') {
        match mark_line(line) {
            Some((found, kept_for_new)) => {
                words = Some(found);
                keep = kept_for_new;
            }
            None => kept.push(line),
        }
    }
    Edit { score: kept.join("\n").trim().to_string(), words, keep }
}

/// The score with the mark as its last line, or the bare score when there is no mark.
pub fn attach(score: &str, words: Option<&str>, keep: bool) -> String {
    let clean = score.trim_end();
    match words {
        Some(words) => format!("{clean}\n{MARK_PREFIX}{words}{}", if keep { KEEP } else { "" }),
        None => clean.to_string(),
    }
}

/// Why an edit is not sung with these words, or `None` when nothing stops it.
pub fn mismatch(edit: &Edit, style: &str, lyrics: &str, cot: &str) -> Option<&'static str> {
    if edit.score.is_empty() {
        return None;
    }
    if cot == "off" {
        return Some("cot_off");
    }
    match &edit.words {
        Some(words) if !edit.keep && *words != mark(style, lyrics, cot) => Some("other_words"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mark_is_the_nodes_own() {
        // hashlib.sha256(json.dumps(["warm pop", "[Verse]\nПривет", "full"]).encode()).hexdigest()[:16]
        let payload = format!("[{}, {}, {}]", python_json("warm pop"), python_json("[Verse]\nПривет"), python_json("full"));
        assert_eq!(payload, "[\"warm pop\", \"[Verse]\\n\\u041f\\u0440\\u0438\\u0432\\u0435\\u0442\", \"full\"]");
        assert_eq!(mark(" warm pop\n", "[Verse]\nПривет\n", "full"), mark("warm pop", "[Verse]\nПривет", "full"));
        assert_eq!(mark("a", "b", "full").len(), 16);
    }

    #[test]
    fn a_mark_comes_off_and_goes_back_on() {
        let words = mark("s", "l", "full");
        let text = attach("X:1\nK:C\n", Some(&words), true);
        assert!(text.ends_with(" keep"));
        let edit = read(&text);
        assert_eq!(edit, Edit { score: "X:1\nK:C".into(), words: Some(words.clone()), keep: true });
        assert_eq!(mismatch(&edit, "s", "l", "full"), None);
        let strict = Edit { keep: false, ..edit };
        assert_eq!(mismatch(&strict, "s", "other", "full"), Some("other_words"));
        assert_eq!(mismatch(&strict, "s", "l", "off"), Some("cot_off"));
    }
}
