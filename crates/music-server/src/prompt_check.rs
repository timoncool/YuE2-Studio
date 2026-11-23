//! What the engine would do to a caption and lyrics before it is asked to.
//!
//! The model's input contract loses things without a word: the engine keeps
//! only the tags of a line that starts with a section tag, so words written
//! after `[verse]` on the same line are never sung; the model card's caption
//! format names the singer's gender in its vocal details, and a caption without
//! one leaves the voice to chance; and a prompt over 5000 tokens is refused
//! outright. These checks say so while the song is still being written.

use serde::Serialize;

/// The prompt budget of the official serving stack, which the engine enforces.
pub const MAX_PROMPT_TOKENS: u64 = 5000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Issue {
    /// `words_on_tag_line`, `vocal_gender_missing` or `prompt_too_long`.
    pub code: &'static str,
    /// `error` refuses the song, `warning` only says what will happen.
    pub severity: &'static str,
    /// The 1-based line of the lyrics the issue is on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    /// What is lost or found: the words after the tags, or the token count.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PromptCheck {
    pub tokens: u64,
    /// False when the engine was not there to count and `tokens` is an estimate.
    pub exact: bool,
    pub limit: u64,
    pub issues: Vec<Issue>,
}

/// The length the engine's tokenizer would give, when it cannot be asked:
/// about 3.5 characters a token, plus the framing tokens of the prompt.
pub fn estimate_tokens(caption: &str, lyrics: &str) -> u64 {
    let characters = caption.trim().chars().count() + lyrics.trim().chars().count();
    (characters as f64 / 3.5).ceil() as u64 + 24
}

/// The engine's own reading of a lyrics line: a leading run of `[tag]` groups
/// separated by spaces or tabs. Returns what follows that run, which the engine
/// drops, or None when the line does not start with a tag or holds nothing more.
fn words_after_tags(line: &str) -> Option<&str> {
    let start = line.trim_start_matches([' ', '\t']);
    let mut rest = start;
    let mut tagged = false;
    while let Some(after_open) = rest.strip_prefix('[') {
        let Some(close) = after_open.find(']') else { break };
        if close == 0 {
            break;
        }
        tagged = true;
        rest = after_open[close + 1..].trim_start_matches([' ', '\t']);
    }
    let words = rest.trim();
    (tagged && !words.is_empty()).then_some(words)
}

/// Whether the lyrics carry any sung words, not only section tags.
fn has_sung_words(lyrics: &str) -> bool {
    lyrics.lines().map(str::trim).any(|line| !line.is_empty() && !(line.starts_with('[') && line.ends_with(']')))
}

/// Whether the caption says who sings: a gender, or a voice type that implies one.
fn names_vocal_gender(caption: &str) -> bool {
    const WORDS: &[&str] = &[
        "male", "female", "man", "woman", "men", "women", "boy", "girl", "baritone", "tenor", "soprano", "alto",
        "mezzo", "contralto", "countertenor",
    ];
    const FRAGMENTS: &[&str] = &["мужск", "женск", "男", "女", "남성", "여성", "남자", "여자"];
    let lowered = caption.to_lowercase();
    lowered
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| WORDS.contains(&word))
        || FRAGMENTS.iter().any(|fragment| lowered.contains(fragment))
}

/// The issues of a caption and lyrics, with `tokens` already counted.
pub fn check(caption: &str, lyrics: &str, instrumental: bool, tokens: u64, exact: bool) -> PromptCheck {
    let mut issues = Vec::new();
    if !instrumental {
        for (index, line) in lyrics.lines().enumerate() {
            if let Some(words) = words_after_tags(line) {
                issues.push(Issue { code: "words_on_tag_line", severity: "warning", line: Some(index + 1), text: Some(words.to_owned()) });
            }
        }
        if has_sung_words(lyrics) && !names_vocal_gender(caption) {
            issues.push(Issue { code: "vocal_gender_missing", severity: "warning", line: None, text: None });
        }
    }
    if tokens > MAX_PROMPT_TOKENS {
        issues.push(Issue { code: "prompt_too_long", severity: "error", line: None, text: Some(tokens.to_string()) });
    }
    PromptCheck { tokens, exact, limit: MAX_PROMPT_TOKENS, issues }
}

/// The refusal of a prompt the engine would not take, in words a person can act on.
pub fn too_long_message(tokens: u64) -> crate::song_errors::SongError {
    crate::song_errors::SongError::PromptTooLong { tokens, limit: MAX_PROMPT_TOKENS }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_after_a_section_tag_are_found_where_the_engine_drops_them() {
        assert_eq!(words_after_tags("[verse] Morning light"), Some("Morning light"));
        assert_eq!(words_after_tags("  [chorus][male vocal]\tWe are"), Some("We are"));
        assert_eq!(words_after_tags("[verse]"), None);
        assert_eq!(words_after_tags("[verse]  "), None);
        assert_eq!(words_after_tags("Morning light [chorus]"), None);
        assert_eq!(words_after_tags("[] words"), None);
        assert_eq!(words_after_tags("[unclosed words"), None);
    }

    #[test]
    fn each_lost_line_is_named_by_its_number() {
        let found = check("Singer A (Female)", "[intro]\n[verse] Morning light\nEvery street\n[chorus]  We are", false, 40, true);
        let lines: Vec<_> = found.issues.iter().filter(|issue| issue.code == "words_on_tag_line").map(|issue| (issue.line, issue.text.clone())).collect();
        assert_eq!(lines, vec![(Some(2), Some("Morning light".into())), (Some(4), Some("We are".into()))]);
    }

    #[test]
    fn a_sung_song_without_a_singer_s_gender_is_warned() {
        let codes = |caption: &str, lyrics: &str, instrumental: bool| {
            check(caption, lyrics, instrumental, 10, true).issues.into_iter().map(|issue| issue.code).collect::<Vec<_>>()
        };
        assert_eq!(codes("Synth-pop, warm pads", "[verse]\nneon", false), vec!["vocal_gender_missing"]);
        assert!(codes("Singer A (Male), a baritone", "[verse]\nneon", false).is_empty());
        assert!(codes("female-led synthpop, male-fronted chorus", "[verse]\nneon", false).is_empty(), "a hyphenated gender names the singer");
        assert!(codes("a woman's breathy voice", "[verse]\nneon", false).is_empty());
        assert!(codes("Женский вокал", "[verse]\nneon", false).is_empty());
        assert!(codes("Synth-pop, romantic", "[verse]\n[chorus]", false).is_empty(), "no words, nothing to sing");
        assert!(codes("Synth-pop, romantic", "[verse]\nneon", true).is_empty(), "an instrumental has no singer");
        assert_eq!(codes("romantic manifesto", "[verse]\nneon", false), vec!["vocal_gender_missing"], "a word inside a word is not one");
    }

    #[test]
    fn a_prompt_over_the_limit_is_an_error_with_its_count() {
        let found = check("Singer A (Female)", "[verse]\nneon", false, 5001, true);
        assert_eq!(found.issues, vec![Issue { code: "prompt_too_long", severity: "error", line: None, text: Some("5001".into()) }]);
        assert!(check("Singer A (Female)", "[verse]\nneon", false, 5000, true).issues.is_empty());
        assert!(too_long_message(5120).to_string().contains("by 120 tokens"));
    }

    #[test]
    fn the_estimate_counts_the_framing_too() {
        assert_eq!(estimate_tokens("", ""), 24);
        assert_eq!(estimate_tokens("abcdefg", ""), 26);
    }
}
