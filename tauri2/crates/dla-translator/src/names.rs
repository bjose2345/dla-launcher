//! Japanese person names, behind the application's `NameReadings` port.
//!
//! Two dictionaries work together. IPADIC, the one search uses, tells a
//! person's name from a word (先生 is "teacher", 小野塚 a surname) and gives a
//! name its usual reading (涼子 is Ryouko). JMnedict knows many more names,
//! katakana ones from other languages among them, but not which reading is
//! usual, and lists many words as names too (天使, "angel", as Yukari), so its
//! readings only go to the translator as hints.
//!
//! `data/names.tsv.gz` is a trimmed copy of JMnedict, the property of the
//! Electronic Dictionary Research and Development Group, used under its
//! licence (CC BY-SA 4.0, <https://www.edrdg.org/edrdg/licence.html>).
//! `scripts/build-name-list.py` makes it; each line is
//! `name<TAB>kinds<TAB>spelling|spelling…`, sorted by name.

use std::{borrow::Cow, io::Read, ops::Range, sync::OnceLock};

use dla_application::translation::{NameReading, NameReadings};
use flate2::read::GzDecoder;
use lindera::{
    dictionary::{DictionaryKind, load_embedded_dictionary},
    mode::Mode,
    segmenter::Segmenter,
};

static LIST: &[u8] = include_bytes!("../data/names.tsv.gz");

// Words after a surname that are forms of address or titles, not given
// names, though JMnedict lists some of them as names.
const NOT_GIVEN_NAMES: [&str; 30] = [
    "さん",
    "くん",
    "ちゃん",
    "さま",
    "様",
    "君",
    "殿",
    "氏",
    "たち",
    "達",
    "先生",
    "先輩",
    "後輩",
    "教授",
    "博士",
    "部長",
    "課長",
    "社長",
    "店長",
    "会長",
    "委員長",
    "隊長",
    "団長",
    "院長",
    "校長",
    "署長",
    "局長",
    "王子",
    "殿下",
    "陛下",
];
// A full name's parts, at most, when finding names in a line.
const LONGEST_PART: usize = 4;
// Spellings of a full name from JMnedict: the two likeliest of each part.
const FULL_NAME_SPELLINGS: usize = 2;

/// Both dictionaries, each read on first use and kept: IPADIC is built in
/// (about 7 MB), the name list about 12 MB.
#[derive(Default)]
pub struct NameList {
    lines: OnceLock<Lines>,
    words: OnceLock<Option<Segmenter>>,
}

struct Lines {
    text: String,
    starts: Vec<u32>,
}

/// One name's line in the name list.
struct Entry<'a> {
    kinds: &'a str,
    spellings: Vec<&'a str>,
}

impl Entry<'_> {
    /// Written in katakana, usually from another language.
    fn foreign(&self) -> bool {
        self.kinds.contains('f')
    }

    fn surname(&self) -> bool {
        !self.foreign() && (self.kinds.contains('s') || self.kinds.contains('u'))
    }

    fn given(&self) -> bool {
        !self.foreign() && (self.kinds.contains('g') || self.kinds.contains('u'))
    }

    fn hint(&self) -> NameReading {
        NameReading {
            spellings: self
                .spellings
                .iter()
                .map(|&spelling| spelling.to_owned())
                .collect(),
            certain: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WordKind {
    Surname,
    GivenName,
    /// A person's name, neither surname nor given name.
    Person,
    /// A word the language has: a noun such as 先生, a particle, a verb.
    Word,
    /// Not in IPADIC.
    Unknown,
}

/// One word of a text as IPADIC reads it.
struct Word {
    at: Range<usize>,
    kind: WordKind,
    /// In katakana.
    reading: Option<String>,
}

impl Word {
    fn person(&self) -> bool {
        matches!(
            self.kind,
            WordKind::Surname | WordKind::GivenName | WordKind::Person
        )
    }
}

impl NameList {
    pub fn new() -> Self {
        Self::default()
    }

    fn lines(&self) -> &Lines {
        self.lines.get_or_init(|| {
            let mut text = String::new();
            // The list is part of the launcher; it always unpacks.
            GzDecoder::new(LIST)
                .read_to_string(&mut text)
                .expect("the built-in name list unpacks");
            let mut starts = vec![0];
            starts.extend(
                text.match_indices('\n')
                    .map(|(at, _)| u32::try_from(at + 1).expect("the name list is small"))
                    .filter(|&at| (at as usize) < text.len()),
            );
            Lines { text, starts }
        })
    }

    fn entry(&self, name: &str) -> Option<Entry<'_>> {
        let lines = self.lines();
        let index = lines
            .starts
            .binary_search_by(|&start| {
                let rest = &lines.text[start as usize..];
                rest[..rest.find('\t').unwrap_or(rest.len())].cmp(name)
            })
            .ok()?;
        let start = lines.starts[index] as usize;
        let rest = &lines.text[start..];
        let line = &rest[..rest.find('\n').unwrap_or(rest.len())];
        let mut fields = line.split('\t').skip(1);
        let kinds = fields.next()?;
        let spellings = fields
            .next()?
            .split('|')
            .filter(|spelling| !spelling.is_empty())
            .collect::<Vec<_>>();
        (!spellings.is_empty()).then_some(Entry { kinds, spellings })
    }

    /// The text's words as IPADIC reads them; none if it cannot be loaded.
    /// Whether IPADIC reads a text alone as one name: 涼子 and 佐藤 are,
    /// 姫 ("princess") and 文章 ("text") are words.
    fn name_alone(&self, text: &str) -> bool {
        let words = self.words(text);
        words.len() == 1 && words[0].person()
    }

    fn words(&self, text: &str) -> Vec<Word> {
        let segmenter = self.words.get_or_init(|| {
            load_embedded_dictionary(DictionaryKind::IPADIC)
                .ok()
                .map(|dictionary| Segmenter::new(Mode::Normal, dictionary, None))
        });
        let Some(segmenter) = segmenter else {
            return Vec::new();
        };
        let Ok(mut tokens) = segmenter.segment(Cow::Borrowed(text)) else {
            return Vec::new();
        };
        tokens
            .iter_mut()
            .map(|token| {
                let at = token.byte_start..token.byte_end;
                let details = token.details();
                let detail =
                    |index: usize| details.get(index).copied().filter(|&value| value != "*");
                let kind = match (detail(1), detail(2), detail(3)) {
                    (Some("固有名詞"), Some("人名"), Some("姓")) => WordKind::Surname,
                    (Some("固有名詞"), Some("人名"), Some("名")) => WordKind::GivenName,
                    (Some("固有名詞"), Some("人名"), _) => WordKind::Person,
                    _ if detail(7).is_some() => WordKind::Word,
                    _ => WordKind::Unknown,
                };
                Word {
                    at,
                    kind,
                    reading: detail(7).map(str::to_owned),
                }
            })
            .collect()
    }

    /// A surname followed by a given name, from the name list.
    fn full_name(&self, name: &str) -> Option<NameReading> {
        let cuts = name
            .char_indices()
            .map(|(at, _)| at)
            .skip(1)
            .collect::<Vec<_>>();
        cuts.into_iter().rev().find_map(|cut| {
            let (surname, given) = name.split_at(cut);
            let surname_entry = self.entry(surname).filter(|entry| entry.surname())?;
            let given_entry = self.entry(given).filter(|entry| entry.given())?;
            (surname.chars().any(is_kanji) && given_name_shape(given))
                .then(|| full_name_hint(&surname_entry, &given_entry))
        })
    }

    /// The longest name of the wanted kind that starts the text.
    fn longest<'a>(
        &self,
        text: &'a str,
        wanted: impl Fn(&Entry<'_>) -> bool,
    ) -> Option<(&'a str, Entry<'_>)> {
        let ends = text
            .char_indices()
            .map(|(at, c)| at + c.len_utf8())
            .take(LONGEST_PART)
            .collect::<Vec<_>>();
        ends.into_iter().rev().find_map(|end| {
            let part = &text[..end];
            self.entry(part)
                .filter(|entry| wanted(entry))
                .map(|entry| (part, entry))
        })
    }
}

impl NameReadings for NameList {
    // A katakana name is never spelled from its reading: IPADIC knows some
    // from other languages as names too, and アルベール is Albert, not
    // Arubeeru.
    fn look_up(&self, name: &str) -> Option<NameReading> {
        let words = self.words(name);
        if !is_katakana(name)
            && !words.is_empty()
            && words.iter().all(Word::person)
            && let Some(spelling) = person_spelling(&words)
        {
            return Some(NameReading {
                spellings: vec![spelling],
                certain: true,
            });
        }
        // A word such as 先生 or 天使 is a description for the translator,
        // though the name list has many of them as names too.
        if let [word] = words.as_slice()
            && word.kind == WordKind::Word
        {
            return None;
        }
        match self.entry(name) {
            Some(entry) => Some(entry.hint()),
            None => self.full_name(name),
        }
    }

    // The names IPADIC tags as such, with its reading; then, where it saw no
    // name, a full name from the name list (徳島黒子 is a place and a word to
    // IPADIC) or a surname of three or more kanji. Shorter surnames from the
    // list alone are too often words (東雲 is "daybreak"), and katakana names
    // are left out: the list spells ケイシー "Cayce", and a model given that
    // writes it.
    fn find_in(&self, line: &str) -> Vec<(String, NameReading)> {
        let mut found: Vec<(String, NameReading)> = Vec::new();
        let mut add = |name: &str, reading: NameReading| {
            if !found.iter().any(|(seen, _)| seen == name) {
                found.push((name.to_owned(), reading));
            }
        };
        let words = self.words(line);
        let mut covered = Vec::new();
        let mut index = 0;
        while index < words.len() {
            if !words[index].person() {
                index += 1;
                continue;
            }
            let start = index;
            while index < words.len() && words[index].person() {
                index += 1;
            }
            let run = &words[start..index];
            let at = run[0].at.start..run[run.len() - 1].at.end;
            // Out of context IPADIC tags many words as names (聖 as
            // Kiyoshi, 五大 as Godai): one character, kana alone or a
            // surname alone is a name only when someone is addressed by it.
            // And each part must be a name on its own: 上部文章 ("upper
            // text") reads as a surname and a given name only together.
            let name = &line[at.clone()];
            let named = run.iter().any(|word| {
                word.kind != WordKind::Surname
                    && line[word.at.clone()].chars().count() >= 2
                    && line[word.at.clone()].chars().any(is_kanji)
            }) || run.len() >= 2 && name.chars().filter(|c| is_kanji(*c)).count() >= 3;
            if !is_katakana(name)
                && (named || addressed(&line[at.end..]))
                && run
                    .iter()
                    .all(|word| self.name_alone(&line[word.at.clone()]))
                && let Some(spelling) = person_spelling(run)
            {
                add(
                    &line[at.clone()],
                    NameReading {
                        spellings: vec![spelling],
                        certain: true,
                    },
                );
            }
            covered.push(at);
        }

        let mut at = 0;
        while at < line.len() {
            if let Some(name) = covered.iter().find(|name| name.contains(&at)) {
                at = name.end;
                continue;
            }
            let rest = &line[at..];
            let first = rest.chars().next().expect("not at the end");
            if is_kanji(first)
                && let Some((surname, surname_entry)) = self.longest(rest, |entry| entry.surname())
            {
                let after = &rest[surname.len()..];
                let given = self
                    .longest(after, |entry| entry.given())
                    .filter(|(given, _)| given_name_shape(given));
                // The name list has many words as names (是非 + とも, 我慢 +
                // でき): its names count only where someone is addressed.
                if surname.chars().count() >= 2
                    && let Some((given, given_entry)) = given
                    && addressed(&after[given.len()..])
                {
                    add(
                        &rest[..surname.len() + given.len()],
                        full_name_hint(&surname_entry, &given_entry),
                    );
                    at += surname.len() + given.len();
                    continue;
                }
                if surname.chars().count() >= 3 && addressed(after) {
                    add(surname, surname_entry.hint());
                    at += surname.len();
                    continue;
                }
            }
            at += first.len_utf8();
        }
        found
    }
}

/// A person's name from IPADIC's readings, given name first.
fn person_spelling(words: &[Word]) -> Option<String> {
    let mut given = Vec::new();
    let mut surnames = Vec::new();
    for word in words {
        let spelled = romaji(word.reading.as_deref()?)?;
        if word.kind == WordKind::Surname {
            surnames.push(spelled);
        } else {
            given.push(spelled);
        }
    }
    Some(
        given
            .into_iter()
            .chain(surnames)
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn full_name_hint(surname: &Entry<'_>, given: &Entry<'_>) -> NameReading {
    let mut spellings = Vec::new();
    for given_spelling in given.spellings.iter().take(FULL_NAME_SPELLINGS) {
        for surname_spelling in surname.spellings.iter().take(FULL_NAME_SPELLINGS) {
            spellings.push(format!("{given_spelling} {surname_spelling}"));
        }
    }
    NameReading {
        spellings,
        certain: false,
    }
}

/// Whether a name is followed by a form of address or a title (さん, 様,
/// 部長), as when someone is spoken to or of by name. 君 is as often "you"
/// (是非とも君には), and たち a plural (私達).
fn addressed(after: &str) -> bool {
    NOT_GIVEN_NAMES
        .iter()
        .filter(|title| !["君", "たち", "達"].contains(title))
        .any(|title| after.starts_with(title))
}

/// Two or more characters, and not a title: one character after a surname
/// is too often a word (大野原 is a field, not 大野 原).
fn given_name_shape(given: &str) -> bool {
    given.chars().count() >= 2 && !NOT_GIVEN_NAMES.contains(&given)
}

fn is_katakana(text: &str) -> bool {
    text.chars()
        .all(|c| matches!(c, '\u{30A1}'..='\u{30FA}' | 'ー' | '・'))
}

fn is_kanji(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '々')
}

/// Kana in Hepburn without macrons, as the name list writes names: long
/// vowels as written (Satou, Ookubo), っ doubling the next consonant.
fn romaji(kana: &str) -> Option<String> {
    let kana = kana
        .chars()
        .map(|c| match c {
            'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            _ => c,
        })
        .collect::<Vec<_>>();
    let mut out = String::new();
    let mut double = false;
    let mut index = 0;
    while index < kana.len() {
        let syllable = if let Some(syllable) = kana
            .get(index + 1)
            .and_then(|&next| digraph(kana[index], next))
        {
            index += 2;
            syllable.to_owned()
        } else if kana[index] == 'っ' {
            double = true;
            index += 1;
            continue;
        } else if kana[index] == 'ー' {
            index += 1;
            out.chars().last()?.to_string()
        } else {
            index += 1;
            monograph(kana[index - 1])?.to_owned()
        };
        if double {
            let first = if syllable.starts_with("ch") {
                't'
            } else {
                syllable.chars().next()?
            };
            out.push(first);
            double = false;
        }
        out.push_str(&syllable);
    }
    let mut chars = out.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().chain(chars).collect())
}

fn digraph(first: char, second: char) -> Option<&'static str> {
    let row = match first {
        'き' => ["kya", "kyu", "kyo"],
        'し' => ["sha", "shu", "sho"],
        'ち' => ["cha", "chu", "cho"],
        'に' => ["nya", "nyu", "nyo"],
        'ひ' => ["hya", "hyu", "hyo"],
        'み' => ["mya", "myu", "myo"],
        'り' => ["rya", "ryu", "ryo"],
        'ぎ' => ["gya", "gyu", "gyo"],
        'じ' | 'ぢ' => ["ja", "ju", "jo"],
        'び' => ["bya", "byu", "byo"],
        'ぴ' => ["pya", "pyu", "pyo"],
        _ => return None,
    };
    match second {
        'ゃ' => Some(row[0]),
        'ゅ' => Some(row[1]),
        'ょ' => Some(row[2]),
        _ => None,
    }
}

fn monograph(c: char) -> Option<&'static str> {
    Some(match c {
        'あ' | 'ぁ' => "a",
        'い' | 'ぃ' | 'ゐ' => "i",
        'う' | 'ぅ' => "u",
        'え' | 'ぇ' | 'ゑ' => "e",
        'お' | 'ぉ' | 'を' => "o",
        'か' => "ka",
        'き' => "ki",
        'く' => "ku",
        'け' => "ke",
        'こ' => "ko",
        'さ' => "sa",
        'し' => "shi",
        'す' => "su",
        'せ' => "se",
        'そ' => "so",
        'た' => "ta",
        'ち' => "chi",
        'つ' => "tsu",
        'て' => "te",
        'と' => "to",
        'な' => "na",
        'に' => "ni",
        'ぬ' => "nu",
        'ね' => "ne",
        'の' => "no",
        'は' => "ha",
        'ひ' => "hi",
        'ふ' => "fu",
        'へ' => "he",
        'ほ' => "ho",
        'ま' => "ma",
        'み' => "mi",
        'む' => "mu",
        'め' => "me",
        'も' => "mo",
        'や' | 'ゃ' => "ya",
        'ゆ' | 'ゅ' => "yu",
        'よ' | 'ょ' => "yo",
        'ら' => "ra",
        'り' => "ri",
        'る' => "ru",
        'れ' => "re",
        'ろ' => "ro",
        'わ' | 'ゎ' => "wa",
        'ん' => "n",
        'が' => "ga",
        'ぎ' => "gi",
        'ぐ' => "gu",
        'げ' => "ge",
        'ご' => "go",
        'ざ' => "za",
        'じ' | 'ぢ' => "ji",
        'ず' | 'づ' => "zu",
        'ぜ' => "ze",
        'ぞ' => "zo",
        'だ' => "da",
        'で' => "de",
        'ど' => "do",
        'ば' => "ba",
        'び' => "bi",
        'ぶ' => "bu",
        'べ' => "be",
        'ぼ' => "bo",
        'ぱ' => "pa",
        'ぴ' => "pi",
        'ぷ' => "pu",
        'ぺ' => "pe",
        'ぽ' => "po",
        'ゔ' => "vu",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look_up(name: &str) -> Option<(Vec<String>, bool)> {
        NameList::new()
            .look_up(name)
            .map(|reading| (reading.spellings, reading.certain))
    }

    fn certain(spelling: &str) -> Option<(Vec<String>, bool)> {
        Some((vec![spelling.to_owned()], true))
    }

    #[test]
    fn spells_a_name_ipadic_knows_with_its_usual_reading() {
        assert_eq!(look_up("長谷川"), certain("Hasegawa"));
        assert_eq!(look_up("涼子"), certain("Ryouko"));
        assert_eq!(look_up("小野塚ゆか"), certain("Yuka Onozuka"));
        assert_eq!(look_up("伊集院"), certain("Ijuuin"));
    }

    #[test]
    fn leaves_words_to_the_translator_though_the_name_list_has_them() {
        for word in ["先生", "天使", "男", "姉", "村人", "謎の男"] {
            assert_eq!(look_up(word), None, "{word}");
        }
    }

    #[test]
    fn hints_at_names_only_the_name_list_knows() {
        let (kuroko, certain) = look_up("徳島黒子").expect("a full name");
        assert_eq!(kuroko[0], "Kuroko Tokushima");
        assert!(!certain);
        let (marsha, certain) = look_up("マーシャ").expect("a katakana name");
        assert!(marsha.contains(&"Marsha".to_owned()));
        assert!(!certain);
        assert_eq!(look_up("テレビ"), None);
        // IPADIC has these as names, read Arubeeru and Mishieru.
        assert_eq!(
            look_up("アルベール"),
            Some((vec!["Albert".to_owned()], false))
        );
        assert!(!look_up("ミシェル").expect("a katakana name").1);
    }

    #[test]
    fn picks_the_names_out_of_a_line() {
        let found = |line: &str| {
            NameList::new()
                .find_in(line)
                .into_iter()
                .map(|(name, reading)| (name, reading.spellings[0].clone()))
                .collect::<Vec<_>>()
        };
        let pairs = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|&(name, spelling)| (name.to_owned(), spelling.to_owned()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            found("小野塚ゆかです。伊集院さんと佐藤さんが来た。東雲の空。徳島黒子さん"),
            pairs(&[
                ("小野塚ゆか", "Yuka Onozuka"),
                ("伊集院", "Ijuuin"),
                ("佐藤", "Satou"),
                ("徳島黒子", "Kuroko Tokushima"),
            ])
        );
        // Words a dictionary lists as names, in lines nobody is named in:
        // they gave a translator "Kiyoshi", "Gaman" and "Tomo Zehi".
        for line in [
            "乳○三○聖○",
            "我慢できないの？",
            "興味があるのなら是非とも協力して欲しい…",
            "手持ちのブルージェムストーンが1つ増加する。",
            "あの五大魔王の一角を滅ぼした",
            "徳島黒子",
            "是非とも君にはその材料を集めてきて欲しい",
            "上部文章",
            "姫様、こちらへ",
        ] {
            assert_eq!(found(line), pairs(&[]), "{line}");
        }
        assert_eq!(
            found("中村部長、おはようございます。"),
            pairs(&[("中村", "Nakamura")])
        );
    }

    #[test]
    fn writes_kana_in_romaji() {
        for (kana, romaji_text) in [
            ("オノヅカ", "Onozuka"),
            ("イジュウイン", "Ijuuin"),
            ("サトウ", "Satou"),
            ("キッチョウ", "Kitchou"),
            ("ハットリ", "Hattori"),
            ("シンイチ", "Shinichi"),
        ] {
            assert_eq!(romaji(kana).as_deref(), Some(romaji_text), "{kana}");
        }
        assert_eq!(romaji("A"), None);
    }
}
