//! The opening of a score the model writes the rest of a song from: a hummed,
//! played or written seed. Its silent tail goes, so the plan picks up from the
//! last note sung instead of from bars of rest.

/// Whether a line of notes holds nothing but rests and bar lines.
fn only_rests(line: &str) -> bool {
    let mut quoted = false;
    line.chars().all(|character| {
        if character == '"' {
            quoted = !quoted;
            return true;
        }
        quoted || matches!(character, 'z' | 'Z' | 'x' | '|' | ' ' | ':' | '[' | ']' | '/') || character.is_ascii_digit()
    })
}

/// The opening without its trailing blocks in which the Vocal voice only rests.
pub fn trimmed(abc: &str) -> String {
    let lines: Vec<&str> = abc.lines().collect();
    let mut end = lines.len();
    loop {
        let Some(vocal) = lines[..end].iter().rposition(|line| line.trim_start().starts_with("V:") && line.contains("Vocal") && !line.contains("clef")) else {
            break;
        };
        let block = &lines[vocal + 1..end];
        let notes_of_vocal: Vec<&&str> = block.iter().take_while(|line| !line.trim_start().starts_with("V:")).collect();
        if notes_of_vocal.is_empty() || !notes_of_vocal.iter().all(|line| only_rests(line)) {
            break;
        }
        end = vocal;
        while end > 0 && lines[end - 1].trim_start().starts_with('%') && !lines[end - 1].starts_with("%%") {
            end -= 1;
        }
    }
    let mut out = lines[..end].join("\n");
    out.push('\n');
    out
}

/// The opening to continue from, refused when trimming leaves no sung block (a chord bed or a score of rests).
pub fn sung(abc: &str) -> Result<String, String> {
    let opening = trimmed(abc);
    if opening.lines().any(|line| line.trim_start().starts_with("V:") && line.contains("Vocal") && !line.contains("clef")) {
        Ok(opening)
    } else {
        Err("the opening has no sung notes to continue from: hum, play or write a melody first".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "X:1\nM:4/4\nL:1/16\nV: Vocal clef=treble name=\"Vocal Melody\" snm=\"Vocal\"\nV: Ins clef=treble name=\"Ins Melody\" snm=\"Inst.\"\nK:Am\n";

    #[test]
    fn trailing_blocks_of_rest_go() {
        let sung = format!("{HEAD}% verse\nV: Vocal\ne4e2d2c4A4|B4c4d8|\nV: Ins\nZ2|\n");
        let tail = "V: Vocal\nz16|\"Am\"z16|\nV: Ins\nZ2|\nV: Vocal\nZ2|\nV: Ins\nZ2|\n";
        assert_eq!(trimmed(&format!("{sung}{tail}")), sung);
        assert_eq!(trimmed(&format!("{sung}% outro\nV: Vocal\nZ4|\nV: Ins\nZ4|\n")), sung, "a section comment over the silence goes with it");
    }

    #[test]
    fn a_score_ending_on_notes_stays_as_it_is() {
        let sung = format!("{HEAD}% verse\nV: Vocal\nz8e4e4|A16|\nV: Ins\nZ2|\n");
        assert_eq!(trimmed(&sung), sung);
        assert_eq!(trimmed(HEAD), HEAD, "nothing to trim without a block");
    }

    #[test]
    fn an_opening_without_sung_notes_is_refused() {
        let sung_opening = format!("{HEAD}% verse\nV: Vocal\ne4e2d2c4A4|B4c4d8|\nV: Ins\nZ2|\n");
        assert_eq!(sung(&sung_opening).unwrap(), sung_opening);
        let bed = format!("{HEAD}% verse\nV: Vocal\n\"Am\"z16|\"F\"z16|\nV: Ins\nZ2|\n");
        assert!(sung(&bed).unwrap_err().contains("no sung notes"));
    }
}
