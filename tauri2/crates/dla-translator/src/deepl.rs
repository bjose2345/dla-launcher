use std::collections::HashMap;

use dla_application::translation::{NameToSpell, Translator, TranslatorBatch, TranslatorError};
use serde_json::{Value, json};
use ureq::Agent;

const FREE_ENDPOINT: &str = "https://api-free.deepl.com/v2/translate";
const PRO_ENDPOINT: &str = "https://api.deepl.com/v2/translate";

/// DeepL with the player's own API key. Placeholders travel as XML tags,
/// which DeepL leaves untouched.
pub struct DeepLTranslator {
    agent: Agent,
    endpoint: &'static str,
    authorization: String,
}

impl DeepLTranslator {
    pub fn new(key: &str, free_plan: bool) -> Self {
        Self {
            agent: crate::agent(),
            endpoint: if free_plan {
                FREE_ENDPOINT
            } else {
                PRO_ENDPOINT
            },
            authorization: format!("DeepL-Auth-Key {}", key.trim()),
        }
    }

    fn translate_texts(&self, texts: &[String]) -> Result<Vec<Option<String>>, TranslatorError> {
        let reply = crate::post_json(
            &self.agent,
            self.endpoint,
            Some(&self.authorization),
            &request(texts),
        )?;
        Ok(translations(&reply, texts.len()))
    }
}

impl Translator for DeepLTranslator {
    fn name(&self) -> String {
        "DeepL".to_owned()
    }

    fn translate(
        &self,
        batch: &TranslatorBatch<'_>,
    ) -> Result<Vec<Option<String>>, TranslatorError> {
        self.translate_texts(batch.lines)
    }

    // DeepL cannot weigh a name dictionary's readings, so a name that has
    // them takes the likeliest.
    fn spell_names(
        &self,
        names: &[NameToSpell],
    ) -> Result<HashMap<String, String>, TranslatorError> {
        let mut spelled = names
            .iter()
            .filter_map(|name| {
                name.readings
                    .first()
                    .map(|reading| (name.japanese.clone(), reading.clone()))
            })
            .collect::<HashMap<_, _>>();
        let rest = names
            .iter()
            .filter(|name| name.readings.is_empty())
            .map(|name| name.japanese.clone())
            .collect::<Vec<_>>();
        if !rest.is_empty() {
            let translated = self.translate_texts(&rest)?;
            spelled.extend(
                rest.into_iter()
                    .zip(translated)
                    .filter_map(|(name, english)| english.map(|english| (name, english))),
            );
        }
        Ok(spelled)
    }
}

fn request(texts: &[String]) -> Value {
    json!({
        "text": texts.iter().map(|text| to_xml(text)).collect::<Vec<_>>(),
        "source_lang": "JA",
        "target_lang": "EN-US",
        "tag_handling": "xml",
        "ignore_tags": ["x"],
    })
}

fn translations(reply: &Value, count: usize) -> Vec<Option<String>> {
    let mut found = reply["translations"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    item["text"]
                        .as_str()
                        .map(from_xml)
                        .filter(|text| !text.trim().is_empty())
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    found.resize(count, None);
    found
}

fn to_xml(text: &str) -> String {
    let escaped = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let mut xml = String::with_capacity(escaped.len());
    let mut rest = escaped.as_str();
    while let Some(start) = rest.find('{') {
        xml.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && after.as_bytes().get(digits) == Some(&b'}') {
            xml.push_str(&format!("<x id=\"{}\"/>", &after[..digits]));
            rest = &after[digits + 1..];
        } else {
            xml.push('{');
            rest = after;
        }
    }
    xml.push_str(rest);
    xml
}

fn from_xml(xml: &str) -> String {
    let mut text = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(start) = rest.find("<x id=\"") {
        text.push_str(&rest[..start]);
        let after = &rest[start + 7..];
        match after.find("\"/>") {
            Some(end) if after[..end].bytes().all(|byte| byte.is_ascii_digit()) => {
                text.push_str(&format!("{{{}}}", &after[..end]));
                rest = &after[end + 3..];
            }
            _ => {
                text.push_str("<x id=\"");
                rest = after;
            }
        }
    }
    text.push_str(rest);
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sends_placeholders_as_tags_deepl_leaves_alone() {
        let body = request(&["特に{1}妖魔{2}を狩る <危険> & {x}".to_owned()]);
        assert_eq!(
            body["text"][0],
            "特に<x id=\"1\"/>妖魔<x id=\"2\"/>を狩る &lt;危険&gt; &amp; {x}"
        );
        assert_eq!(body["tag_handling"], "xml");
        assert_eq!(body["target_lang"], "EN-US");
    }

    #[test]
    fn reads_translations_back_with_placeholders_and_characters_restored() {
        let reply = json!({ "translations": [
            { "detected_source_language": "JA", "text": "I hunt <x id=\"1\"/>demons<x id=\"2\"/> &lt;danger&gt; &amp; more" },
            { "text": "" }
        ]});
        assert_eq!(
            translations(&reply, 3),
            vec![
                Some("I hunt {1}demons{2} <danger> & more".to_owned()),
                None,
                None
            ]
        );
    }

    #[test]
    fn uses_the_endpoint_of_the_players_plan() {
        assert_eq!(DeepLTranslator::new("k:fx", true).endpoint, FREE_ENDPOINT);
        assert_eq!(DeepLTranslator::new("k", false).endpoint, PRO_ENDPOINT);
        assert_eq!(
            DeepLTranslator::new(" key ", false).authorization,
            "DeepL-Auth-Key key"
        );
    }
}
