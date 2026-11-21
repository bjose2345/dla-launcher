//! The texts an RPG Maker XP or VX game shows, read from its archive or its
//! `Data` folder in the form the launcher's script looks them up while the
//! game runs (`rgss_texts`). The two generations lay their data out alike,
//! with these differences: XP keeps its menu words in `System#words` and VX
//! in `System#terms` (an object with one text field each, where VX Ace has
//! lists); VX adds the `Vocab` constants, a game title and the skill and
//! state messages; XP keeps a message's first line in the Show Text command
//! itself. Map names come from `MapInfos`, which the games' own scripts
//! draw.

use std::{io, path::Path};

use dla_application::game_translation::GameTexts;
use dla_domain::translation::TextKind;

use crate::{
    rgss::{GameData, Generation, script_sections, strings_and_files},
    rgss_texts::{Texts, vocab_strings},
    ruby_marshal::Value,
};

pub fn read(game: &Path, generation: Generation) -> io::Result<GameTexts> {
    let data = GameData::find(game, generation)?;
    let scripts = script_sections(&data.load(&generation.scripts())?);
    let found = strings_and_files(game, &scripts, generation)?;
    let mut texts = Texts::new(found.strings, generation == Generation::Xp);

    let system = data.load(&generation.file("System"))?;
    if system == Value::Nil {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("the game has no {}", generation.file("System")),
        ));
    }
    for words in ["@words", "@terms"] {
        texts.fields(system.field(words), TextKind::Menu);
    }
    texts.each(system.field("@elements"), TextKind::Menu);
    texts.text(system.field("@game_title"), TextKind::Menu);
    for (_, source) in &scripts {
        for text in vocab_strings(source) {
            texts.add(text, TextKind::Menu);
        }
    }

    // A field a generation does not have reads as nil and is skipped.
    let tables: [(&str, &[&str]); 8] = [
        ("Actors", &[]),
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
        for row in data.load(&generation.file(table))?.items() {
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
    let mut map_infos = match data.load(&generation.file("MapInfos"))? {
        Value::Hash(pairs) => pairs
            .into_iter()
            .filter_map(|(id, info)| Some((id.as_integer()?, info)))
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    map_infos.sort_by_key(|(id, _)| *id);
    for (_, info) in &map_infos {
        texts.text(info.field("@name"), TextKind::Menu);
    }
    texts.drawn_script_strings();

    let maps = data
        .maps()?
        .into_iter()
        .map(|(id, name)| data.load(&name).map(|map| (id, map)))
        .collect::<io::Result<Vec<_>>>()?;
    texts.events(
        &data.load(&generation.file("CommonEvents"))?,
        &data.load(&generation.file("Troops"))?,
        maps,
        |_, _| {},
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
        rgssad,
        ruby_marshal::{Strings, Writer},
    };
    use std::fs;

    fn file(build: impl FnOnce(&mut Writer)) -> Vec<u8> {
        let mut writer = Writer::new();
        build(&mut writer);
        writer.finish()
    }

    fn table(class: &str, rows: &[(&str, Option<&str>)], description: &str) -> Vec<u8> {
        file(|writer| {
            writer.array(rows.len() + 1);
            writer.binary(b"");
            for (name, message) in rows {
                writer.object(class, if message.is_some() { 3 } else { 2 });
                writer.symbol("@name");
                writer.binary(name.as_bytes());
                writer.symbol(description);
                writer.binary("使い方の説明。".as_bytes());
                if let Some(message) = message {
                    writer.symbol("@message1");
                    writer.binary(message.as_bytes());
                }
            }
        })
    }

    fn common_events(first_line_in_command: bool) -> Vec<u8> {
        file(|writer| {
            writer.array(2);
            writer.binary(b"");
            writer.object("RPG::CommonEvent", 2);
            writer.symbol("@id");
            writer.integer(1);
            writer.symbol("@list");
            writer.array(if first_line_in_command { 2 } else { 3 });
            if first_line_in_command {
                command(writer, 101, &["宿屋へようこそ。"]);
            } else {
                command(writer, 101, &["", "0", "0", "2"]);
                command(writer, 401, &["宿屋へようこそ。"]);
            }
            command(writer, 401, &["お泊まりですか？"]);
        })
    }

    fn map_infos() -> Vec<u8> {
        file(|writer| {
            writer.hash(2);
            writer.integer(2);
            writer.object("RPG::MapInfo", 1);
            writer.symbol("@name");
            writer.binary("MAP002".as_bytes());
            writer.integer(1);
            writer.object("RPG::MapInfo", 1);
            writer.symbol("@name");
            writer.binary("始まりの村".as_bytes());
        })
    }

    fn map(first_line_in_command: bool) -> Vec<u8> {
        file(|writer| {
            writer.object("RPG::Map", 1);
            writer.symbol("@events");
            writer.hash(1);
            writer.integer(1);
            writer.object("RPG::Event", 1);
            writer.symbol("@pages");
            writer.array(1);
            writer.object("RPG::Event::Page", 1);
            writer.symbol("@list");
            writer.array(6);
            if first_line_in_command {
                command(writer, 101, &["\\N[1]、起きて！"]);
            } else {
                command(writer, 101, &["Actor1", "0", "0", "2"]);
                command(writer, 401, &["\\N[1]、起きて！"]);
            }
            command(writer, 401, &["朝だよ。"]);
            writer.object("RPG::EventCommand", 2);
            writer.symbol("@code");
            writer.integer(102);
            writer.symbol("@parameters");
            writer.array(2);
            writer.array(2);
            writer.binary("はい".as_bytes());
            writer.binary("いいえ".as_bytes());
            writer.integer(2);
            command(writer, 117, &["1"]);
            command(writer, 320, &["1", "ルーク"]);
            if first_line_in_command {
                command(writer, 108, &["コメント"]);
            }
        })
    }

    /// An XP game in an archive: menu words in `words`, the message's
    /// first line in the Show Text command.
    fn xp_game(directory: &Path) {
        let system = file(|writer| {
            writer.object("RPG::System", 2);
            writer.symbol("@words");
            writer.object("RPG::System::Words", 3);
            writer.symbol("@gold");
            writer.binary("ゴールド".as_bytes());
            writer.symbol("@attack");
            writer.binary("攻撃".as_bytes());
            writer.symbol("@weapon");
            writer.binary("武器".as_bytes());
            writer.symbol("@elements");
            writer.array(2);
            writer.binary(b"");
            writer.binary("炎".as_bytes());
        });
        let scripts = scripts_file(
            &[
                (
                    1,
                    "Window_Help",
                    "class Window_Help\n  def set_text(text, align = 0)\n    draw_text(4, 0, 100, 32, text, align)\n  end\nend\n",
                ),
                (
                    2,
                    "Scene_Title",
                    "s1 = \"ニューゲーム\"\nhas_item_named?(\"古い鍵\")\n",
                ),
            ],
            Strings::Plain,
        );
        let archive = rgssad::build(&[
            ("Data\\System.rxdata", &system),
            ("Data\\Scripts.rxdata", &scripts),
            (
                "Data\\Actors.rxdata",
                &table("RPG::Actor", &[("ルーク", None)], "@class_id"),
            ),
            (
                "Data\\Items.rxdata",
                &table(
                    "RPG::Item",
                    &[("古い鍵", None), ("薬草", None)],
                    "@description",
                ),
            ),
            ("Data\\CommonEvents.rxdata", &common_events(true)),
            ("Data\\MapInfos.rxdata", &map_infos()),
            ("Data\\Map001.rxdata", &map(true)),
        ]);
        fs::write(directory.join("Game.rgssad"), archive).unwrap();
    }

    /// A VX game without an archive: menu words in `terms`, `Vocab`
    /// constants in the scripts, messages' lines after the Show Text command.
    fn vx_game(directory: &Path) {
        let data = directory.join("Data");
        fs::create_dir_all(&data).unwrap();
        let system = file(|writer| {
            writer.object("RPG::System", 3);
            writer.symbol("@game_title");
            writer.binary("冒険の旅".as_bytes());
            writer.symbol("@terms");
            writer.object("RPG::System::Terms", 2);
            writer.symbol("@new_game");
            writer.binary("ニューゲーム".as_bytes());
            writer.symbol("@level_a");
            writer.binary("Lv".as_bytes());
            writer.symbol("@elements");
            writer.array(2);
            writer.binary(b"");
            writer.binary("炎".as_bytes());
        });
        let scripts = scripts_file(
            &[(
                1,
                "Vocab",
                "module Vocab\n  Emerge = \"%sが出現！\"\n  def self.gold\n    $data_system.terms.gold\n  end\nend\n",
            )],
            Strings::Plain,
        );
        fs::write(data.join("System.rvdata"), system).unwrap();
        fs::write(data.join("Scripts.rvdata"), scripts).unwrap();
        fs::write(
            data.join("Skills.rvdata"),
            table(
                "RPG::Skill",
                &[("ファイア", Some("は呪文を唱えた"))],
                "@description",
            ),
        )
        .unwrap();
        fs::write(data.join("CommonEvents.rvdata"), common_events(false)).unwrap();
        fs::write(data.join("Map001.rvdata"), map(false)).unwrap();
    }

    #[test]
    fn reads_an_xp_game_with_the_first_line_in_the_show_text_command() {
        let directory = tempfile::tempdir().unwrap();
        xp_game(directory.path());
        let texts = read(directory.path(), Generation::Xp).unwrap();
        let listed = texts
            .texts
            .iter()
            .map(|text| (text.text.as_str(), text.kind))
            .collect::<Vec<_>>();
        assert_eq!(
            listed,
            [
                ("ゴールド", TextKind::Menu),
                ("攻撃", TextKind::Menu),
                ("武器", TextKind::Menu),
                ("炎", TextKind::Menu),
                ("古い鍵", TextKind::Database),
                ("使い方の説明。", TextKind::Database),
                ("薬草", TextKind::Database),
                ("始まりの村", TextKind::Menu),
                ("ニューゲーム", TextKind::Addon),
                ("宿屋へようこそ。\nお泊まりですか？", TextKind::Dialogue),
                ("\\N[1]、起きて！\n朝だよ。", TextKind::Dialogue),
                ("はい", TextKind::Choice),
                ("いいえ", TextKind::Choice),
            ]
        );
        assert_eq!(texts.names, ["ルーク"]);
        let places = |text: &str| texts.places.get(text).cloned().unwrap_or_default();
        assert_eq!(places("\\N[1]、起きて！\n朝だよ。"), ["map:1"]);
        assert_eq!(
            places("宿屋へようこそ。\nお泊まりですか？"),
            ["common:1", "map:1"]
        );
        let mut in_scripts = crate::rgss::strings_in_scripts(directory.path(), Generation::Xp)
            .map(|scripts| scripts.strings)
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>();
        in_scripts.sort();
        assert_eq!(in_scripts, ["ニューゲーム", "古い鍵"]);
    }

    #[test]
    fn reads_a_vx_game_from_its_data_folder() {
        let directory = tempfile::tempdir().unwrap();
        vx_game(directory.path());
        let texts = read(directory.path(), Generation::Vx).unwrap();
        let listed = texts
            .texts
            .iter()
            .map(|text| (text.text.as_str(), text.kind))
            .collect::<Vec<_>>();
        assert_eq!(
            listed,
            [
                ("ニューゲーム", TextKind::Menu),
                ("炎", TextKind::Menu),
                ("冒険の旅", TextKind::Menu),
                ("%sが出現！", TextKind::Menu),
                ("ファイア", TextKind::Database),
                ("使い方の説明。", TextKind::Database),
                ("は呪文を唱えた", TextKind::Database),
                ("宿屋へようこそ。\nお泊まりですか？", TextKind::Dialogue),
                ("\\N[1]、起きて！\n朝だよ。", TextKind::Dialogue),
                ("はい", TextKind::Choice),
                ("いいえ", TextKind::Choice),
            ]
        );
        assert_eq!(texts.names, ["ルーク"]);
        assert!(
            crate::rgss::strings_in_scripts(directory.path(), Generation::Vx)
                .map(|scripts| scripts.strings)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn needs_the_system_file() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("Data")).unwrap();
        let error = read(directory.path(), Generation::Xp).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }
}
