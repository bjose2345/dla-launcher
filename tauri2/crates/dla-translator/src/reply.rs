/// Reads a reply to numbered lines (`1. …`, `2) …`) back into one entry per
/// line. A single line often comes back without its number.
pub fn numbered_lines(reply: &str, count: usize, speakers: &[&str]) -> Vec<Option<String>> {
    let mut lines = vec![None; count];
    for line in reply.lines() {
        if let Some((number, text)) = numbered(line)
            && let Some(slot) = number.checked_sub(1).and_then(|at| lines.get_mut(at))
            && slot.is_none()
        {
            *slot = clean(text, speakers);
        }
    }
    if count == 1 && lines[0].is_none() {
        lines[0] = reply
            .lines()
            .find(|line| !line.trim().is_empty())
            .and_then(|line| clean(line, speakers));
    }
    lines
}

/// Some local models (Qwen3, DeepSeek) put their reasoning in the reply.
pub fn without_thinking(reply: &str) -> &str {
    reply
        .rsplit_once("</think>")
        .map_or(reply, |(_, answer)| answer)
        .trim()
}

fn numbered(line: &str) -> Option<(usize, &str)> {
    let line = line.trim_start();
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    let rest = line[digits..].strip_prefix(['.', ')'])?;
    if digits == 0 || !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some((line[..digits].parse().ok()?, rest))
}

// A model sometimes repeats who speaks ("[Yukina] …", "Yukina: …"). Only a
// known character name is taken off, so a line like "Note: …" stays whole.
fn clean(text: &str, speakers: &[&str]) -> Option<String> {
    let mut text = text.trim();
    if text.starts_with('[')
        && let Some(close) = text.find(']')
    {
        text = text[close + 1..].trim_start();
    }
    for speaker in speakers {
        if let Some(rest) = text
            .strip_prefix(speaker)
            .and_then(|rest| rest.strip_prefix(':'))
        {
            text = rest.trim_start();
            break;
        }
    }
    (!text.is_empty()).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_numbered_lines_in_any_order_and_ignores_extra_text() {
        let reply = "Here you go:\n2) Second line\n1. First line\n3. \n7. Out of range";
        assert_eq!(
            numbered_lines(reply, 3, &[]),
            vec![
                Some("First line".to_owned()),
                Some("Second line".to_owned()),
                None
            ]
        );
    }

    #[test]
    fn takes_a_lone_reply_without_a_number() {
        assert_eq!(
            numbered_lines("\n\"Skip the opening?\"\n", 1, &[]),
            vec![Some("\"Skip the opening?\"".to_owned())]
        );
    }

    #[test]
    fn removes_only_known_speakers_and_bracket_labels() {
        let reply = "1. Yukina: I gotta go.\n2. Note: shops close at 9.\n3. [Yukina] Hmm...";
        assert_eq!(
            numbered_lines(reply, 3, &["Yukina"]),
            vec![
                Some("I gotta go.".to_owned()),
                Some("Note: shops close at 9.".to_owned()),
                Some("Hmm...".to_owned())
            ]
        );
    }

    #[test]
    fn does_not_mistake_numbers_inside_a_line_for_line_numbers() {
        assert_eq!(
            numbered_lines("1. 100.5 gold", 1, &[]),
            vec![Some("100.5 gold".to_owned())]
        );
        assert_eq!(
            numbered_lines("3.14 is pi", 1, &[]),
            vec![Some("3.14 is pi".to_owned())]
        );
    }

    #[test]
    fn drops_reasoning_before_the_answer() {
        assert_eq!(
            without_thinking("<think>The user wants…</think>\n\n1. Yes"),
            "1. Yes"
        );
        assert_eq!(without_thinking("1. Yes"), "1. Yes");
    }
}
