//! The texts an RPG Maker VX Ace game shows, read from its archive or its
//! `Data` folder in the form the launcher's script looks them up while the
//! game runs (`rgss_texts`): the system's term lists, the database's names
//! and descriptions, the `Vocab` constants, map names and every event's
//! messages and choices.

use std::{io, path::Path};

use dla_application::game_translation::GameTexts;
use dla_domain::translation::TextKind;

use crate::{
    rgss::{GameData, Generation, script_sections, strings_and_files},
    rgss_texts::{Texts, vocab_strings},
    ruby_marshal::Value,
};

const GENERATION: Generation = Generation::VxAce;

pub fn read(game: &Path) -> io::Result<GameTexts> {
    let data = GameData::find(game, GENERATION)?;
    let scripts = script_sections(&data.load(&GENERATION.scripts())?);
    let found = strings_and_files(game, &scripts, GENERATION)?;
    let mut texts = Texts::new(found.strings, false);

    let system = data.load(&GENERATION.file("System"))?;
    if system == Value::Nil {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "the game has no Data/System.rvdata2",
        ));
    }
    for field in ["@basic", "@params", "@etypes", "@commands"] {
        texts.each(system.field("@terms").field(field), TextKind::Menu);
    }
    for list in ["@elements", "@skill_types", "@weapon_types", "@armor_types"] {
        texts.each(system.field(list), TextKind::Menu);
    }
    texts.text(system.field("@currency_unit"), TextKind::Menu);
    texts.text(system.field("@game_title"), TextKind::Menu);
    for (_, source) in &scripts {
        for text in vocab_strings(source) {
            texts.add(text, TextKind::Menu);
        }
    }

    let tables: [(&str, &[&str]); 8] = [
        ("Actors", &["@nickname", "@description"]),
        ("Classes", &[]),
        ("Skills", &["@description", "@message1", "@message2"]),
        ("Items", &["@description"]),
        ("Weapons", &["@description"]),
        ("Armors", &["@description"]),
        ("Enemies", &[]),
        (
            "States",
            &["@message1", "@message2", "@message3", "@message4"],
        ),
    ];
    for (table, fields) in tables {
        for row in data.load(&GENERATION.file(table))?.items() {
            // Actors' names join the name list, so dialogue spells them alike.
            if table == "Actors" {
                texts.name(row.field("@name"));
            } else {
                texts.database_name(row.field("@name"));
            }
            for field in fields {
                texts.text(row.field(field), TextKind::Database);
            }
        }
    }
    texts.drawn_script_strings();

    let maps = data
        .maps()?
        .into_iter()
        .map(|(id, name)| data.load(&name).map(|map| (id, map)))
        .collect::<io::Result<Vec<_>>>()?;
    texts.events(
        &data.load(&GENERATION.file("CommonEvents"))?,
        &data.load(&GENERATION.file("Troops"))?,
        maps,
        |texts, map| texts.text(map.field("@display_name"), TextKind::Menu),
    );
    for text in found.files.texts().chain(found.shown.texts()) {
        texts.add(text.text, text.kind);
    }
    Ok(texts.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        rgss::scripts_file,
        rgss_texts::command,
        rgss3a,
        ruby_marshal::{Strings, Writer},
    };
    use std::fs;

    fn file(build: impl FnOnce(&mut Writer)) -> Vec<u8> {
        let mut writer = Writer::new();
        build(&mut writer);
        writer.finish()
    }

    fn scripts() -> Vec<u8> {
        scripts_file(
            &[
                (
                    1,
                    "Vocab",
                    "module Vocab\n  Attack = \"%sの攻撃！\"\n  ObtainGold = \"お金を %s\\\\G 手に入れた！\"\nend\n",
                ),
                (
                    2,
                    "Shop add-on",
                    "module Vocab\n  Sell = \"売却\"\nend\nclass Game_Party\n  def key?\n    has_item_named?(\"古い鍵\")\n  end\nend\n",
                ),
            ],
            Strings::Tagged,
        )
    }

    fn game(directory: &Path) {
        let system = file(|writer| {
            writer.object("RPG::System", 3);
            writer.symbol("@game_title");
            writer.text("冒険の旅");
            writer.symbol("@currency_unit");
            writer.text("G");
            writer.symbol("@terms");
            writer.object("RPG::System::Terms", 1);
            writer.symbol("@commands");
            writer.array(2);
            writer.text("戦う");
            writer.text("逃げる");
        });
        let actors = file(|writer| {
            writer.array(2);
            writer.text("");
            writer.object("RPG::Actor", 2);
            writer.symbol("@name");
            writer.text("ルーク");
            writer.symbol("@nickname");
            writer.text("見習い騎士");
        });
        let items = file(|writer| {
            writer.array(3);
            writer.text("");
            for name in ["古い鍵", "薬草"] {
                writer.object("RPG::Item", 2);
                writer.symbol("@name");
                writer.text(name);
                writer.symbol("@description");
                writer.text("どこかの扉を開ける。");
            }
        });
        let common_events = file(|writer| {
            writer.array(2);
            writer.text("");
            writer.object("RPG::CommonEvent", 2);
            writer.symbol("@id");
            writer.integer(1);
            writer.symbol("@list");
            writer.array(11);
            command(writer, 101, &["", "0", "0", "2"]);
            command(writer, 401, &["宿屋へようこそ。"]);
            // A popup a script draws, its heading in the call; a sound's
            // file name and the comment are not drawn.
            command(writer, 355, &["type = 0 # 種類"]);
            command(writer, 655, &["text = \"アイテム獲得！\""]);
            command(writer, 655, &["Audio.se_play(\"Audio/SE/決定\")"]);
            command(writer, 655, &["e.push(Window_Getinfo.new(1, type, text))"]);
            // An option menu written into a picture, a line an entry.
            command(writer, 355, &["Pictures[12].permitw"]);
            command(writer, 105, &["2", "0"]);
            command(writer, 405, &["\\{断面図の表示"]);
            command(writer, 405, &[""]);
            command(writer, 405, &["ボイス"]);
        });
        let map = file(|writer| {
            writer.object("RPG::Map", 2);
            writer.symbol("@display_name");
            writer.text("始まりの村");
            writer.symbol("@events");
            writer.hash(1);
            writer.integer(1);
            writer.object("RPG::Event", 1);
            writer.symbol("@pages");
            writer.array(1);
            writer.object("RPG::Event::Page", 1);
            writer.symbol("@list");
            writer.array(7);
            command(writer, 101, &["Actor1", "0", "0", "2"]);
            command(writer, 401, &["\\N[1]、起きて！"]);
            command(writer, 401, &["朝だよ。"]);
            // Show Choices: the choices, then the cancel choice.
            writer.object("RPG::EventCommand", 2);
            writer.symbol("@code");
            writer.integer(102);
            writer.symbol("@parameters");
            writer.array(2);
            writer.array(2);
            writer.text("はい");
            writer.text("いいえ");
            writer.integer(2);
            command(writer, 117, &["1"]);
            command(writer, 105, &["2", "0"]);
            command(writer, 405, &["遠い昔のこと……"]);
        });
        let archive = rgss3a::build(&[
            ("Data\\System.rvdata2", &system),
            ("Data\\Scripts.rvdata2", &scripts()),
            ("Data\\Actors.rvdata2", &actors),
            ("Data\\Items.rvdata2", &items),
            ("Data\\CommonEvents.rvdata2", &common_events),
            ("Data\\Map001.rvdata2", &map),
        ]);
        fs::write(directory.join("Game.rgss3a"), archive).unwrap();
    }

    #[test]
    fn reads_what_the_script_looks_up_with_where_it_appears() {
        let directory = tempfile::tempdir().unwrap();
        game(directory.path());
        let texts = read(directory.path()).unwrap();
        let listed = texts
            .texts
            .iter()
            .map(|text| (text.text.as_str(), text.kind))
            .collect::<Vec<_>>();
        assert_eq!(
            listed,
            [
                ("戦う", TextKind::Menu),
                ("逃げる", TextKind::Menu),
                ("冒険の旅", TextKind::Menu),
                ("%sの攻撃！", TextKind::Menu),
                ("お金を %s\\G 手に入れた！", TextKind::Menu),
                ("売却", TextKind::Menu),
                ("見習い騎士", TextKind::Database),
                // The add-on compares 古い鍵 by name: translated, it stays
                // Japanese in the data and English only where it is drawn.
                ("古い鍵", TextKind::Database),
                ("どこかの扉を開ける。", TextKind::Database),
                ("薬草", TextKind::Database),
                ("宿屋へようこそ。", TextKind::Dialogue),
                ("アイテム獲得！", TextKind::Addon),
                ("\\{断面図の表示", TextKind::Menu),
                ("ボイス", TextKind::Menu),
                ("始まりの村", TextKind::Menu),
                ("\\N[1]、起きて！\n朝だよ。", TextKind::Dialogue),
                ("はい", TextKind::Choice),
                ("いいえ", TextKind::Choice),
                ("遠い昔のこと……", TextKind::Dialogue),
            ]
        );
        assert_eq!(texts.names, ["ルーク"]);
        let places = |text: &str| texts.places.get(text).cloned().unwrap_or_default();
        assert_eq!(places("\\N[1]、起きて！\n朝だよ。"), ["map:1"]);
        // A shared event is listed under the map that calls it.
        assert_eq!(places("宿屋へようこそ。"), ["common:1", "map:1"]);
        assert_eq!(places("アイテム獲得！"), ["common:1", "map:1"]);
        assert!(places("薬草").is_empty());
        let mut in_scripts = crate::rgss::strings_in_scripts(directory.path(), GENERATION)
            .map(|scripts| scripts.strings)
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>();
        in_scripts.sort();
        assert_eq!(in_scripts, ["古い鍵", "売却"]);
    }
}
