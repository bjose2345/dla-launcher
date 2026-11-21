//! Breaking English into the lines a game draws as they are written. What a
//! line measures is the caller's: each engine has its own codes.

/// The largest of `values` once the largest hundredth is left out; `None`
/// when there are none.
pub fn all_but_the_largest(mut values: Vec<usize>) -> Option<usize> {
    values.sort_unstable();
    let at = (values.len() * 99).div_ceil(100).checked_sub(1)?;
    values.get(at).copied()
}

fn words(text: &str) -> Vec<&str> {
    text.split_whitespace().collect()
}

fn fill(words: &[&str], width: usize, columns: &dyn Fn(&str) -> usize) -> Vec<String> {
    let mut lines = Vec::<String>::new();
    for word in words {
        match lines.last_mut() {
            Some(line) if columns(line) + 1 + columns(word) <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push((*word).to_owned()),
        }
    }
    lines
}

/// Breaks a text at spaces into lines of `width` at most, then as evenly
/// as that number of lines allows; a word wider than the width has a line
/// to itself.
pub fn wrap(text: &str, width: usize, columns: &dyn Fn(&str) -> usize) -> Vec<String> {
    let words = words(text);
    let lines = fill(&words, width, columns).len();
    within(&words, lines, width, columns)
}

/// Breaks a text at `width` when that takes `lines` lines at most, and
/// else into `lines` lines as narrow as they can be: wider than the
/// Japanese was, never more lines than it had.
pub fn wrap_into(
    text: &str,
    lines: usize,
    width: usize,
    columns: &dyn Fn(&str) -> usize,
) -> Vec<String> {
    let words = words(text);
    let needed = fill(&words, width, columns).len();
    if needed <= lines.max(1) {
        within(&words, needed, width, columns)
    } else {
        within(&words, lines.max(1), columns(text), columns)
    }
}

/// The narrowest filling of at most `lines` lines no wider than `widest`.
fn within(
    words: &[&str],
    lines: usize,
    widest: usize,
    columns: &dyn Fn(&str) -> usize,
) -> Vec<String> {
    let longest = words.iter().map(|word| columns(word)).max().unwrap_or(0);
    let (mut low, mut high) = (longest.min(widest), widest.max(longest));
    while low < high {
        let middle = (low + high) / 2;
        if fill(words, middle, columns).len() <= lines {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    fill(words, low, columns)
}
