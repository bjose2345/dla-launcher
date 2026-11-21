//! RPG Maker VX Ace (RGSS3, Ruby 1.9): the launcher's script for it, and a
//! check on a copy of a real game. Putting the script into a game is
//! `rgss::install`.

pub const SCRIPT: &str = include_str!("../plugins/rpg-maker-vx-ace/DLA_Translate.rb");
pub const TRANSLATIONS: &str = "translations.rvdata2";

#[cfg(test)]
mod tests {
    use std::{fs, io::Write};

    use flate2::{Compression, write::ZlibEncoder};
    use sha2::{Digest, Sha256};

    use crate::{
        rgss::{Generation, OUR_SECTION, archive_in},
        rgss3a::{self, Archive},
        ruby_marshal::{self, Writer},
    };

    const SCRIPTS: &str = "Data/Scripts.rvdata2";

    /// Records facts only, never the game's text: whether the menus and the
    /// messages came out in English, how wide each message drew against the
    /// room it had, and the map the player arrived on. It skips through
    /// messages (the first choice each time), then shows made-up English in
    /// a box, a box with a face, a balloon over the player (for games whose
    /// message script has them) and scrolling text.
    const PROBE: &str = r##"
module DLA_Probe
  JAPANESE = DLA_Translate::JAPANESE
  SAMPLE = "This sentence is made up for testing, and it keeps going well past " +
    "the width of any message window the game draws, so the launcher has to " +
    "wrap it onto several lines that all stay inside the window."
  @tests = [:box, :face, :balloon, :scrolling]
  def self.log(text)
    File.open("dla-translation/probe.txt", "a") { |file| file.write(text + "\n") }
  end
  def self.next_test
    test = @tests.shift
    return unless test
    leader = $game_party.leader
    face = test == :face && leader
    $game_message.face_name = face ? leader.face_name : ""
    $game_message.face_index = face ? leader.face_index : 0
    $game_message.scroll_mode = test == :scrolling
    $game_message.scroll_speed = 8
    $game_message.dla_expect((test == :balloon ? "\\h[-1]" : "") + SAMPLE, 1)
    $game_message.add("probe")
    log("test #{test}" + (face && leader.face_name.empty? ? " (the leader has no face)" : ""))
  end
end
class << DLA_Translate
  alias_method :dla_probe_remember_database, :remember_database
  def remember_database
    [["weapons", $data_weapons], ["armors", $data_armors]].each do |label, table|
      rows = (table || []).compact
      DLA_Probe.log("remembering #{label}: #{table ? rows.size : "no table"}, " +
        "#{rows.count { |row| row.instance_variable_get(:@name) =~ DLA_Probe::JAPANESE }} Japanese names")
    end
    dla_probe_remember_database
  end
end
class Object
  alias_method :dla_probe_rgss_main, :rgss_main
  def rgss_main(*args, &block)
    eval(<<-'RUBY', TOPLEVEL_BINDING, "DLA probe")
      class Scene_Title
        alias_method :dla_probe_start, :start
        def start
          dla_probe_start
          japanese = $data_system.terms.commands.compact.count { |text| text =~ DLA_Probe::JAPANESE }
          vocab = Vocab.constants.count { |name| Vocab.const_get(name).to_s =~ DLA_Probe::JAPANESE }
          DLA_Probe.log("title: #{japanese} menu commands and #{vocab} battle messages still Japanese")
          # Each English image is what the game gets where it asks for its own.
          (DLA_Translate.instance_variable_get(:@images) || {}).each do |key, file|
            english = Bitmap.new(file)
            got = Bitmap.new("Graphics/" + key + ".png")
            DLA_Translate.instance_variable_set(:@shown, false)
            own = Bitmap.new("Graphics/" + key)
            DLA_Translate.instance_variable_set(:@shown, true)
            points = (0...48).to_a.product((0...64).to_a).map { |row, column| [got.width * column / 64, got.height * row / 48] }
            pixel = lambda { |bitmap, at| color = bitmap.get_pixel(*at); [color.red, color.green, color.blue, color.alpha] }
            DLA_Probe.log("image #{key}: #{got.width}x#{got.height}, the game's own #{own.width}x#{own.height}, " +
              "unlike the English file at #{points.count { |at| pixel.call(got, at) != pixel.call(english, at) }} points, " +
              "unlike the game's own at #{points.count { |at| pixel.call(got, at) != pixel.call(own, at) }} of #{points.size}")
            [english, got, own].each(&:dispose)
          end
          [["items", $data_items], ["weapons", $data_weapons], ["armors", $data_armors]].each do |label, table|
            rows = table.compact
            left = rows.select { |row| row.name =~ DLA_Probe::JAPANESE }
            known = left.count { |row| DLA_Translate.english(row.name) }
            stripped = left.count { |row| row.name != row.name.strip }
            spots = DLA_Translate.instance_variable_get(:@spots) || []
            remembered = left.count { |row| spots.any? { |owner, key, _| owner.equal?(row) && key == :name } }
            DLA_Probe.log("#{label}: #{left.size} of #{rows.size} names still Japanese, " +
              "#{known} of them with a translation, #{stripped} with spaces at an end, " +
              "#{remembered} remembered; #{spots.size} spots in all")
            kind = { "items" => RPG::Item, "weapons" => RPG::Weapon, "armors" => RPG::Armor }[label]
            owners = spots.select { |owner, key, _| owner.is_a?(kind) && key == :name }.map { |owner, _, _| owner }
            current = owners.count { |owner| rows.any? { |row| row.equal?(owner) } }
            DLA_Probe.log("#{label}: #{owners.size} name spots, #{current} still in the table")
          end
          # Every translated description against a two-line help window.
          help = Window_Help.new
          room = help.contents_width - 8
          sizes = Hash.new(0)
          longest = nil
          [$data_items, $data_weapons, $data_armors, $data_skills].each do |table|
            table.compact.each do |row|
              text = row.description
              next unless DLA_Translate.ours?(text)
              size = DLA_Translate.fit(text, room, help.contents.height)[1]
              sizes[size] += 1
              longest = text if longest.nil? || text.size > longest.size
            end
          end
          help.set_text(longest) if longest
          DLA_Probe.log("help window: descriptions by text size #{sizes.sort.reverse.inspect}, " +
            "the longest drawn at #{help.contents.font.size}")
          help.dispose
          # Strings of the game's scripts: kept Japanese in the data, drawn
          # in English.
          kept = DLA_Translate.instance_variable_get(:@kept) || {}
          label = [0x73fe, 0x5728, 0x5730, 0xff1a].pack("U*")
          scratch = Bitmap.new(200, 24)
          scratch.draw_text(0, 0, 200, 24, label)
          scratch.dispose
          DLA_Probe.log("script strings: #{kept.size} kept in the data, " +
            "location label has English=#{!!DLA_Translate.shown(label)}, drawn without an error")
          # The label as the game's window draws it: in the box of the label
          # and a space, the map name starting where the Japanese ended.
          texts = DLA_Translate.instance_variable_get(:@texts)
          made = texts[label]
          texts[label] = "Location:"
          window = Bitmap.new(200, 24)
          box = window.text_size(label + " ")
          start = window.text_size(label).width
          window.draw_text(box, label)
          ink = (0...window.width).to_a.reverse.find do |px|
            (0...window.height).any? { |py| window.get_pixel(px, py).alpha > 0 }
          end
          window.dispose
          made ? texts[label] = made : texts.delete(label)
          DLA_Probe.log("location label: Location: ends at #{ink.to_i}, " +
            "the map name starts at #{start}, clear of it=#{ink.to_i < start}")
          # Known pieces inside a string the game puts together.
          rate = [0x56f3, 0x9451, 0x5b8c, 0x6210, 0x5ea6].pack("U*") + ": 5 %"
          DLA_Probe.log("pieces: completion line swapped=#{!DLA_Translate.drawn(rate).nil?}, " +
            "still Japanese=#{!!(DLA_Translate.drawn(rate).to_s =~ DLA_Probe::JAPANESE)}")
          # An option menu written into a picture, as the game's events do:
          # a script call, then scrolling text the picture script takes.
          option = "\\{" + [0x3000, 0x65ad, 0x9762, 0x56f3, 0x306e, 0x8868, 0x793a].pack("U*")
          if $game_temp.respond_to?(:picture_writable_id=)
            $game_temp.picture_writable_id = 12
            $game_map.screen.pictures[12].create_contents(420, 280)
            texts = DLA_Translate.instance_variable_get(:@texts) || {}
            long = texts.keys.find { |key| texts[key].length > 70 && !key.include?("\n") && key.length < 50 } || ""
            interpreter = Game_Interpreter.new
            list = [RPG::EventCommand.new(105, 0, [2, false]), RPG::EventCommand.new(405, 0, [option]),
              RPG::EventCommand.new(405, 0, [long]), RPG::EventCommand.new(0, 0, [])]
            interpreter.instance_variable_set(:@list, list)
            interpreter.instance_variable_set(:@index, 0)
            interpreter.instance_variable_set(:@params, list[0].parameters)
            interpreter.command_105
            $game_temp.picture_writable_id = 0
            lines = $game_map.screen.pictures[12].texts
            indent = [0x3000].pack("U")
            DLA_Probe.log("picture text: #{lines.size} lines stored, option in English=#{!(lines[0].to_s =~ DLA_Probe::JAPANESE)}, " +
              "indent kept=#{lines[0].to_s.include?("\\{" + indent)}, long row smaller=#{lines[1].to_s.start_with?("\\S[")}, " +
              "one size for both=#{lines[0].to_s[/\A\\S\[\d+\]/] == lines[1].to_s[/\A\\S\[\d+\]/]}, " +
              "heights kept=#{lines[0].to_s.end_with?("\\S[32]") && lines[1].to_s.end_with?("\\S[24]")}")
          end
          # A skill note's prerequisite, its name half-width.
          note = [0x524d, 0x63d0, 0x30b9, 0x30ad, 0x30eb, 0xff1a, 0xff83, 0xff9e, 0xff68, 0xff8a, 0xff9e, 0xff72, 0xff9d,
            0xff8c, 0xff9f, 0xff9b, 0xff83, 0xff78, 0xff7c, 0xff6e, 0xff9d].pack("U*") + "LV1"
          DLA_Probe.log("half-width name: drawn in English=#{!(DLA_Translate.drawn(note).to_s =~ DLA_Probe::JAPANESE) && !DLA_Translate.drawn(note).nil?}")
          # A one-line status row stays one line (a game's own profile list).
          row = (KURE::ExStatus::PROFILE[1][0][6] rescue nil)
          if row && DLA_Translate.drawn(row)
            status = Window_Base.new(0, 0, 640, 300)
            fitted = DLA_Translate.fit(DLA_Translate.drawn(row), status.contents_width - 284, 24)
            status.draw_text_ex(280, 24, row)
            status.dispose
            DLA_Probe.log("status row: one line=#{fitted[0].count("\n") == 0}, text size #{fitted[1]}")
          end
          # A long English name in a shop list, and a short one.
          sizes = ["Soldier Skeleton Card", "Pupa Card"].map do |wanted|
            armor = $data_armors.compact.find { |row| (DLA_Translate.shown(row.name) || row.name) == wanted }
            next "#{wanted}: not found" unless armor
            shop = Window_ShopBuy.new(0, 0, 120, [[2, armor.id, 1, 5000]])
            size = shop.instance_variable_get(:@dla_name_size)
            shop.dispose
            "#{wanted} at #{size.inspect}"
          end
          DLA_Probe.log("shop list: " + sizes.join(", "))
          # A code that draws nothing, then a space the model put after it.
          joined = DLA_Translate.wrap("\\h[3] Lately, an aura", 592)
          named = DLA_Translate.wrap("\\N[1] said so", 592)
          DLA_Probe.log("wrap: space after an invisible code dropped=#{joined == "\\h[3]Lately, an aura"}, " +
            "after a name kept=#{named == "\\N[1] said so"}")
          sample = DLA_Probe::SAMPLE[0, 60]
          by_character = 0
          sample.each_char { |c| by_character += contents_probe.text_size(c).width }
          DLA_Probe.log("font #{Font.default_name.inspect} #{Font.default_size}: a line measures " +
            "#{contents_probe.text_size(sample).width} whole, #{by_character} character by character")
          DataManager.setup_new_game
          $game_map.autoplay
          SceneManager.goto(Scene_Map)
        end
        def contents_probe
          @dla_probe_bitmap ||= Bitmap.new(1, 1)
        end
      end
      class Scene_Map
        alias_method :dla_probe_update, :update
        def update
          dla_probe_update
          # Events may run on without a message: two seconds of none will do.
          @dla_probe_idle = $game_message.busy? ? 0 : @dla_probe_idle.to_i + 1
          DLA_Probe.next_test if @dla_probe_idle == 120
        end
      end
      class Window_Base
        alias_method :dla_probe_process_normal_character, :process_normal_character
        def process_normal_character(c, pos)
          @dla_probe_left = [@dla_probe_left || 99_999, pos[:x]].min
          dla_probe_process_normal_character(c, pos)
          @dla_probe_right = [@dla_probe_right.to_i, pos[:x]].max
        end
      end
      class Window_Message
        def wait_for_one_character
        end
        def input_pause
        end
        alias_method :dla_probe_process_all_text, :process_all_text
        def process_all_text
          text = $game_message.all_text
          translated = !!$game_message.dla_translated
          japanese = !!(text =~ DLA_Probe::JAPANESE)
          @dla_probe_right = 0
          @dla_probe_left = nil
          dla_probe_process_all_text
          DLA_Probe.log("message: translated=#{translated} japanese=#{japanese} " +
            "lines=#{text.count("\n") + 1} drew #{@dla_probe_left}..#{@dla_probe_right} of #{contents_width}, " +
            "window #{x}..#{x + width} of #{Graphics.width}")
        end
      end
      class Window_ScrollText
        def scroll_speed
          32
        end
        alias_method :dla_probe_refresh, :refresh
        def refresh
          @dla_probe_right = 0
          @dla_probe_left = nil
          dla_probe_refresh
          DLA_Probe.log("scrolling text: drew #{@dla_probe_left}..#{@dla_probe_right} of #{contents_width}, " +
            "window #{x}..#{x + width} of #{Graphics.width}")
        end
      end
      class Window_ChoiceList
        alias_method :dla_probe_start, :start
        def start
          dla_probe_start
          call_ok_handler
        end
      end
    RUBY
    dla_probe_rgss_main(*args, &block)
  end
  private :rgss_main, :dla_probe_rgss_main
end
"##;

    /// On a copy of a VX Ace game (`DLA_VXACE_GAME`): reads its texts,
    /// patches its archive twice, hands it made-up English, the English
    /// images of a folder (`DLA_VXACE_PICTURES`, laid out as the game's
    /// `Graphics`) and, with `DLA_VXACE_PROBE=1`, adds the probe after the
    /// launcher's script. Run the copy, then again with `DLA_VXACE_REMOVE=1`
    /// to take it all off.
    #[test]
    #[ignore = "needs a copy of an RPG Maker VX Ace game"]
    fn smoke_test_on_a_game_copy() {
        use dla_application::{
            game_translation::{GameFolder, GameSession},
            translation::WorkTranslations,
        };
        use dla_domain::{
            game_engine::{GameEngine, GameEngineDetection},
            installation::InstallationId,
            translation::GameTranslation,
        };

        let game = std::path::PathBuf::from(std::env::var("DLA_VXACE_GAME").unwrap());
        let root = game.to_str().unwrap();
        let folder = crate::DesktopGameFolder::new();
        let detection = GameEngineDetection {
            engine: GameEngine::RpgMakerVxAce,
            root: None,
        };
        let archive = archive_in(&game, Generation::VxAce).unwrap().unwrap();
        if std::env::var_os("DLA_VXACE_REMOVE").is_some() {
            let changes = folder.install(root, &detection, &[]).unwrap();
            folder.remove(root, &changes).unwrap();
            assert_eq!(rgss3a::find_patch(&archive).unwrap(), None);
            eprintln!(
                "removed; archive sha256 {}",
                hex::encode(Sha256::digest(fs::read(&archive).unwrap()))
            );
            return;
        }
        let texts = crate::vx_ace_data::read(&game).unwrap();
        let dialogue = texts
            .texts
            .iter()
            .filter(|text| text.kind.is_dialogue())
            .count();
        let placed = texts.places.len();
        eprintln!(
            "texts {} (dialogue {dialogue}), names {}, texts with a place {placed}",
            texts.texts.len(),
            texts.names.len()
        );
        let changes = folder.install(root, &detection, &[]).unwrap();
        assert_eq!(folder.install(root, &detection, &changes).unwrap(), changes);
        eprintln!("changes {changes:?}");

        let format = regex_lite_count;
        let fake = |at: usize, japanese: &str| {
            let mut english = format!("EN{at} text");
            for value in 1..=format(japanese) {
                english.push_str(&format!(" %{value}$s"));
            }
            english
        };
        let pictures = std::env::var_os("DLA_VXACE_PICTURES")
            .map(std::path::PathBuf::from)
            .map(|source| {
                walkdir::WalkDir::new(&source)
                    .sort_by_file_name()
                    .into_iter()
                    .filter_map(Result::ok)
                    .filter(|entry| entry.file_type().is_file())
                    .map(|entry| dla_application::translation::TranslatedPicture {
                        path: entry
                            .path()
                            .strip_prefix(&source)
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .strip_suffix(".png")
                            .unwrap()
                            .to_owned(),
                        png: fs::read(entry.path()).unwrap(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let translations = WorkTranslations {
            speakers: Vec::new(),
            pictures,
            texts: texts
                .texts
                .iter()
                .enumerate()
                .map(|(at, text)| (text.text.clone(), fake(at, &text.text)))
                .collect(),
            names: texts
                .names
                .iter()
                .enumerate()
                .map(|(at, name)| (name.clone(), format!("Name{at}")))
                .collect(),
        };
        let translation = GameTranslation {
            installation_id: InstallationId("smoke".to_owned()),
            engine: GameEngine::RpgMakerVxAce,
            game_folder: None,
            shown: true,
            plugin: folder.plugin(GameEngine::RpgMakerVxAce).unwrap(),
            changes,
            turned_on_at: String::new(),
            paused: false,
            prepared: None,
            pack: None,
        };
        for picture in &translations.pictures {
            assert!(
                folder.has_image(root, &translation, &picture.path).unwrap(),
                "the game has no image {}",
                picture.path
            );
        }
        eprintln!("images in English {}", translations.pictures.len());
        let session = GameSession {
            work: "smoke".to_owned(),
            shown: true,
            relay: None,
            translations,
        };
        folder.write_session(root, &translation, &session).unwrap();

        if std::env::var_os("DLA_VXACE_PROBE").is_some() {
            let archive_data = Archive::open(&archive).unwrap();
            let entry = archive_data.entry(SCRIPTS).unwrap();
            let ours = archive_data.read(&archive, entry).unwrap();
            let sections = ruby_marshal::read(&ours).unwrap();
            let mut probe_code = ZlibEncoder::new(Vec::new(), Compression::default());
            probe_code.write_all(PROBE.as_bytes()).unwrap();
            let probe_code = probe_code.finish().unwrap();
            let items = sections.items();
            let mut writer = Writer::new();
            writer.array(items.len() + 1);
            for (at, section) in items.iter().enumerate() {
                let parts = section.items();
                writer.array(3);
                writer.integer(parts[0].as_integer().unwrap());
                writer.text(parts[1].as_str().unwrap_or("?"));
                writer.binary(parts[2].as_bytes().unwrap());
                if at == 0 {
                    writer.array(3);
                    writer.integer(OUR_SECTION + 1);
                    writer.text("DLA probe");
                    writer.binary(&probe_code);
                }
            }
            rgss3a::remove_patch(&archive).unwrap();
            rgss3a::add_patch(&archive, &writer.finish(), [0; 32]).unwrap();
            eprintln!("probe added");
        }
    }

    fn regex_lite_count(text: &str) -> usize {
        text.matches("%s").count() + text.matches("%d").count()
    }
}
