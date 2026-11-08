//! Pairs a game's Japanese texts with its official English release, as
//! training data for the translator: the two copies of the game are read as
//! the launcher reads them, and each text is paired with the one in the same
//! place in the other copy.
//!
//! ```text
//! pair_translations [--work <code>] <japanese-game-folder> <english-game-folder> <pairs.jsonl>
//! ```
//!
//! Writes one `{"work", "place", "kind", "ja", "en", "codes_match", "face"}` a line
//! and prints how many texts were paired, left without a partner (a message
//! the translator split, merged or moved) and left out (still Japanese, a
//! name or number missing). Both copies must be the same release; the model
//! trained on the pairs stays private.

use std::{collections::BTreeMap, io::Write, path::Path};

use dla_game_translation::pairing::pair_games;
use serde_json::json;

fn main() {
    let mut arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let work = match arguments.iter().position(|argument| argument == "--work") {
        Some(at) if at + 1 < arguments.len() => {
            arguments.remove(at);
            Some(arguments.remove(at))
        }
        _ => None,
    };
    let [japanese, english, out] = arguments.as_slice() else {
        eprintln!("usage: see the top of examples/pair_translations.rs");
        std::process::exit(2);
    };
    if let Err(error) = run(
        work.as_deref(),
        Path::new(japanese),
        Path::new(english),
        Path::new(out),
    ) {
        eprintln!("pair_translations: {error}");
        std::process::exit(1);
    }
}

fn run(work: Option<&str>, japanese: &Path, english: &Path, out: &Path) -> Result<(), String> {
    let pairing = pair_games(japanese, english).map_err(|error| error.to_string())?;
    let mut file =
        std::io::BufWriter::new(std::fs::File::create(out).map_err(|error| error.to_string())?);
    let mut by_kind = BTreeMap::<String, usize>::new();
    for pair in &pairing.pairs {
        let kind = format!("{:?}", pair.kind).to_lowercase();
        *by_kind.entry(kind.clone()).or_default() += 1;
        let line = json!({
            "work": work,
            "place": pair.place,
            "kind": kind,
            "ja": pair.japanese,
            "en": pair.english,
            "codes_match": pair.codes_match,
            "face": pair.face,
        });
        writeln!(file, "{line}").map_err(|error| error.to_string())?;
    }
    file.flush().map_err(|error| error.to_string())?;
    let codes = pairing.pairs.iter().filter(|pair| pair.codes_match).count();
    println!(
        "{} pairs {by_kind:?}, {codes} with the same codes; {} without a partner; left out {:?}",
        pairing.pairs.len(),
        pairing.unpaired,
        pairing.dropped
    );
    println!("wrote {}", out.display());
    Ok(())
}
