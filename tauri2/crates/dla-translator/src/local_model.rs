use std::collections::HashMap;

use dla_application::translation::{
    BatchStyle, NameToSpell, Translator, TranslatorBatch, TranslatorError,
};
use serde_json::{Value, json};
use ureq::Agent;

use crate::reply::{numbered_lines, without_thinking};

const LINE_TOKENS: u32 = 160;
const NAME_TOKENS: u32 = 800;
/// The sampling every request is sent with; a translation job tells the PC
/// that runs it the same.
pub const TEMPERATURE: f64 = 0.2;
pub const TOP_P: f64 = 0.9;

/// A model behind the OpenAI-compatible chat API: Ollama, LM Studio,
/// llama.cpp's server, or any service the player points the launcher at.
pub struct LocalModelTranslator {
    agent: Agent,
    endpoint: String,
    model: String,
}

impl LocalModelTranslator {
    /// `address` is the API's base, for example `http://127.0.0.1:11434/v1`.
    pub fn new(address: &str, model: &str) -> Self {
        Self {
            agent: crate::agent(),
            endpoint: format!("{}/chat/completions", address.trim().trim_end_matches('/')),
            model: model.trim().to_owned(),
        }
    }

    fn chat(&self, messages: Value, max_tokens: u32) -> Result<String, TranslatorError> {
        let body = json!({
            "model": self.model,
            "messages": messages,
            "max_tokens": max_tokens,
            "temperature": TEMPERATURE,
            "top_p": TOP_P,
            "stream": false,
        });
        let reply = crate::post_json(&self.agent, &self.endpoint, None, &body)?;
        reply["choices"][0]["message"]["content"]
            .as_str()
            .map(|content| without_thinking(content).to_owned())
            .ok_or_else(|| TranslatorError::UnreadableReply("the reply had no message".to_owned()))
    }
}

impl Translator for LocalModelTranslator {
    fn name(&self) -> String {
        self.model.clone()
    }

    fn translate(
        &self,
        batch: &TranslatorBatch<'_>,
    ) -> Result<Vec<Option<String>>, TranslatorError> {
        let (messages, max_tokens) = batch_request(batch);
        let reply = self.chat(messages, max_tokens)?;
        Ok(read_batch_reply(&reply, batch))
    }

    // Numbered like the translations: small models asked for JSON drop the
    // keys when there is only one name (`{"Yukina"}`).
    fn spell_names(
        &self,
        names: &[NameToSpell],
    ) -> Result<HashMap<String, String>, TranslatorError> {
        let lines = names.iter().map(name_line).collect::<Vec<_>>();
        let reply = self.chat(
            json!([
                { "role": "system", "content": NAME_INSTRUCTIONS },
                { "role": "user", "content": numbered(&lines) },
            ]),
            NAME_TOKENS,
        )?;
        Ok(names
            .iter()
            .map(|name| name.japanese.clone())
            .zip(numbered_lines(&reply, names.len(), &[]))
            .filter_map(|(name, english)| english.map(|english| (name, english)))
            .collect())
    }
}

// Tested on Qwen3 4B with RJ01678657's 19 speakers: without the katakana
// and description rules it wrote ケイシー as "Kaisu", gave two girls the same
// hair colour and put "The" before every villager.
const NAME_INSTRUCTIONS: &str = "You write the English names of the characters of a Japanese game, \
     as an English release of the game would show them above their lines.\n\
     Rules:\n\
     - Japanese names: romaji, given name first (ユキナ = Yukina, トモヤ = Tomoya).\n\
     - Names written in katakana from other languages: their usual English spelling \
     (ケイシー = Casey, ミシェル = Michelle).\n\
     - A name followed by readings from a name dictionary: write its first reading exactly, \
     or another of them if the game shows it is that one; never one of your own.\n\
     - A katakana name followed by possible spellings: the one that fits best, or its usual \
     English spelling if none does.\n\
     - Descriptions instead of names: a short English label in title case, without \"The\" \
     (村人 = Villager, 村長 = Village Chief, 金髪の少年 = Blond Boy).\n\
     - Translate every word of a description exactly; colours, ages and occupations matter.\n\
     - No two characters get the same English name.\n\
     - Output only the names. No notes, no explanations.\n\
     - Reply with the same numbers, one name each, in order.";

/// A name as the model sees it, with the dictionary's readings after it. A
/// katakana name's are only suggestions: the dictionary spells ケイシー
/// "Cayce", where a game would write "Casey".
fn name_line(name: &NameToSpell) -> String {
    if name.readings.is_empty() {
        return name.japanese.clone();
    }
    let katakana = name
        .japanese
        .chars()
        .all(|c| matches!(c, '\u{30A1}'..='\u{30FA}' | 'ー' | '・'));
    let label = if katakana {
        "possible spellings"
    } else {
        "readings"
    };
    format!("{} ({label}: {})", name.japanese, name.readings.join(" / "))
}

/// The chat messages and token limit sent for a batch: the same request a
/// translation job hands another PC.
pub fn batch_request(batch: &TranslatorBatch<'_>) -> (Value, u32) {
    let tokens = LINE_TOKENS.saturating_mul(u32::try_from(batch.lines.len()).unwrap_or(u32::MAX));
    (
        json!([
            { "role": "system", "content": instructions(batch) },
            { "role": "user", "content": numbered(batch.lines) },
        ]),
        tokens.max(LINE_TOKENS),
    )
}

/// Each line of the batch from the model's reply, `None` for any it left out.
pub fn read_batch_reply(reply: &str, batch: &TranslatorBatch<'_>) -> Vec<Option<String>> {
    let speakers = batch
        .names
        .iter()
        .map(|(_, english)| english.as_str())
        .collect::<Vec<_>>();
    numbered_lines(without_thinking(reply), batch.lines.len(), &speakers)
}

fn numbered(lines: &[String]) -> String {
    lines
        .iter()
        .enumerate()
        .map(|(at, line)| format!("{}. {line}", at + 1))
        .collect::<Vec<_>>()
        .join("\n")
}

fn instructions(batch: &TranslatorBatch<'_>) -> String {
    let opening = match batch.style {
        BatchStyle::Dialogue => {
            "You translate dialogue from a Japanese adult game into natural, casual English for players."
        }
        BatchStyle::Terms => {
            "You translate the menus and database of a Japanese adult game into short, natural English, \
             the way English games word them: menu commands, stat names, item, skill and equipment names, \
             and their descriptions."
        }
    };
    let mut rules = vec![
        "- Output only the English text. No notes, no explanations, no speaker labels.".to_owned(),
        "- Keep placeholders such as {1} and {2} exactly as written, at the matching place."
            .to_owned(),
        "- Keep every number exactly as written.".to_owned(),
    ];
    if batch.style == BatchStyle::Dialogue {
        rules.push("- Write Japanese 「」 quotes as English double quotes.".to_owned());
        // Qwen3 4B wrote あの子 ("that kid", a boy) as "that girl", and with a
        // weaker rule put a character's name in instead.
        rules.push(
            "- Japanese rarely says whether someone is a man or a woman. When a line doesn't say, \
             don't guess and don't put a name in: あの子 is \"that kid\", あいつ is \"that guy\"."
                .to_owned(),
        );
        // Qwen3 4B turned thoughts into questions to "you": 5 of 6 test lines
        // without this rule, 1 with its example, while spoken lines kept "you".
        rules.push(
            "- A line wrapped in （） or () is the speaker thinking to themselves; nobody hears it. \
             Keep the parentheses and never write \"you\" in it: （待っててくれないかな） is \
             \"(I wonder if they'd wait for me)\", not \"(Would you wait for me?)\"."
                .to_owned(),
        );
    }
    if !batch.names.is_empty() {
        let spellings = batch
            .names
            .iter()
            .map(|(japanese, english)| format!("{japanese} = {english}"))
            .collect::<Vec<_>>()
            .join("; ");
        rules.push(format!("- Use these name spellings: {spellings}."));
    }
    // Without them Qwen3 4B read 小野塚 as "Otonashi" and 長谷川 as "Nagahara".
    if !batch.readings.is_empty() {
        let readings = batch
            .readings
            .iter()
            .map(|(japanese, spellings)| format!("{japanese} = {}", spellings.join(" / ")))
            .collect::<Vec<_>>()
            .join("; ");
        rules.push(format!(
            "- Where these are names in the lines, write each as one of the readings a name \
             dictionary gives, exactly: {readings}."
        ));
    }
    if !batch.terms.is_empty() {
        let terms = batch
            .terms
            .iter()
            .map(|(japanese, english)| format!("{japanese} = {english}"))
            .collect::<Vec<_>>()
            .join("; ");
        rules.push(format!("- Write these names and words this way: {terms}."));
    }
    rules.push(
        "- Translate each numbered line. Reply with the same numbers, one line each, in order."
            .to_owned(),
    );
    match batch.note {
        Some(note) => format!("{opening}\n{note}\nRules:\n{}", rules.join("\n")),
        None => format!("{opening}\nRules:\n{}", rules.join("\n")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_chat_endpoint_from_the_address_the_player_typed() {
        let translator =
            LocalModelTranslator::new(" http://127.0.0.1:11434/v1/ ", " qwen3:4b-instruct ");
        assert_eq!(
            translator.endpoint,
            "http://127.0.0.1:11434/v1/chat/completions"
        );
        assert_eq!(translator.name(), "qwen3:4b-instruct");
    }

    #[test]
    fn tells_dialogue_and_terms_apart_and_lists_name_spellings() {
        let names = [("ユキナ".to_owned(), "Yukina".to_owned())];
        let lines = ["はい".to_owned()];
        let readings = [(
            "小野塚ゆか".to_owned(),
            vec!["Yuka Onozuka".to_owned(), "Yuka Onotsuka".to_owned()],
        )];
        let dialogue = instructions(&TranslatorBatch {
            style: BatchStyle::Dialogue,
            lines: &lines,
            names: &names,
            readings: &readings,
            note: None,
            terms: &[],
        });
        assert!(dialogue.contains("dialogue"));
        assert!(dialogue.contains("「」"));
        assert!(dialogue.contains("あの子"));
        assert!(dialogue.contains("thinking to themselves"));
        assert!(dialogue.contains("ユキナ = Yukina"));
        assert!(dialogue.contains("小野塚ゆか = Yuka Onozuka / Yuka Onotsuka"));
        let terms = instructions(&TranslatorBatch {
            style: BatchStyle::Terms,
            lines: &lines,
            names: &[],
            readings: &[],
            note: None,
            terms: &[],
        });
        assert!(terms.contains("menus and database"));
        assert!(!terms.contains("「」"));
        assert!(!terms.contains("thinking to themselves"));
        assert!(!terms.contains("name spellings"));
        assert!(!terms.contains("name dictionary"));
        assert!(!terms.contains("this way"));
    }

    #[test]
    fn tells_the_model_about_the_game_and_its_glossary_terms() {
        let lines = ["ポリンカード".to_owned()];
        let glossary = [("ポリン".to_owned(), "Poring".to_owned())];
        let told = instructions(&TranslatorBatch {
            style: BatchStyle::Terms,
            lines: &lines,
            names: &[],
            readings: &[],
            note: Some("This game parodies Ragnarok Online."),
            terms: &glossary,
        });
        assert!(told.contains("of a Japanese adult game into short, natural English, the way"));
        assert!(told.contains("their descriptions.\nThis game parodies Ragnarok Online.\nRules:"));
        assert!(told.contains("ポリン = Poring."));
    }

    #[test]
    fn gives_a_name_its_readings() {
        let name = |readings: &[&str]| NameToSpell {
            japanese: "由香".to_owned(),
            readings: readings.iter().map(|&reading| reading.to_owned()).collect(),
        };
        assert_eq!(name_line(&name(&[])), "由香");
        assert_eq!(
            name_line(&name(&["Yuka", "Yukari"])),
            "由香 (readings: Yuka / Yukari)"
        );
        let casey = NameToSpell {
            japanese: "ケイシー".to_owned(),
            readings: vec!["Cayce".to_owned()],
        };
        assert_eq!(name_line(&casey), "ケイシー (possible spellings: Cayce)");
    }

    /// Runs against a real translator: `DLA_TRANSLATOR_URL` (for example
    /// `http://127.0.0.1:8099/v1`) and optionally `DLA_TRANSLATOR_MODEL`.
    #[test]
    #[ignore = "needs DLA_TRANSLATOR_URL, a running local model"]
    fn translates_a_batch_with_a_real_model() {
        let address = std::env::var("DLA_TRANSLATOR_URL").expect("DLA_TRANSLATOR_URL");
        let model = std::env::var("DLA_TRANSLATOR_MODEL")
            .unwrap_or_else(|_| "qwen3-4b-instruct".to_owned());
        let translator = LocalModelTranslator::new(&address, &model);
        let names = translator
            .spell_names(&[NameToSpell {
                japanese: "ユキナ".to_owned(),
                readings: Vec::new(),
            }])
            .expect("names");
        println!("names: {names:?}");
        let lines = [
            "オープニングを飛ばしますか？".to_owned(),
            "そういえば名乗ってなかったわね私の名前は「ユキナ」魔物、特に{1}妖魔{2}を狩る為に旅をしているわ".to_owned(),
            "スポーツの本（100）".to_owned(),
            "（……あの人、少しだけ手伝ってくれないかな）".to_owned(),
        ];
        let names = [(
            "ユキナ".to_owned(),
            names
                .get("ユキナ")
                .cloned()
                .unwrap_or_else(|| "Yukina".to_owned()),
        )];
        let replies = translator
            .translate(&TranslatorBatch {
                style: BatchStyle::Dialogue,
                lines: &lines,
                names: &names,
                readings: &[],
                note: None,
                terms: &[],
            })
            .expect("translation");
        for (line, reply) in lines.iter().zip(&replies) {
            println!("{line}\n  -> {reply:?}");
        }
        assert!(replies.iter().all(Option::is_some));
        let thought = replies[3].as_deref().unwrap_or_default();
        assert!(
            thought.starts_with(['(', '（']) && thought.ends_with([')', '）']),
            "{thought}"
        );
        assert!(!thought.to_lowercase().contains("you"), "{thought}");
    }
}
