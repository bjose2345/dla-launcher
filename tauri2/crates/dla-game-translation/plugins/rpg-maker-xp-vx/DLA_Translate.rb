# DLA Launcher's machine translation for RPG Maker XP and VX.
#
# The launcher puts this script first in the game's scripts and the
# translations finished so far in dla-translation/translations.marshal;
# lines not translated yet stay in Japanese. Both engines run Ruby 1.8, so
# strings are bytes here: characters come from scan(/./mu) and unpack("U*").
# The script hooks in when the game loads its System data, once every script
# of the game has loaded, tells XP from VX by the classes the game defines,
# and reads the translations again on every map change while the launcher
# prepares the rest. It writes the map the player is on, so the launcher
# translates that map next. Pressing F12 runs every script again, so
# anything added to the engine's own classes is added only once.
module DLA_Translate
  FOLDER = "dla-translation"
  TRANSLATIONS = FOLDER + "/translations.marshal"
  LAUNCHER = FOLDER + "/launcher.json"
  PLACE = FOLDER + "/place.txt"
  LOG = FOLDER + "/log.txt"
  SCRIPTS = FOLDER + "/scripts/"
  SECTIONS = FOLDER + "/sections/"
  # What was learned about the room texts have, kept from one play to the
  # next.
  LAYOUT = FOLDER + "/layout.marshal"
  LAYOUT_VERSION = 2
  # The gap left before a text that stays where it is, and between a text
  # and the one moved to start after it.
  ROOM_GAP = 4
  FLOW_GAP = 12
  ROOM_REMEMBERED = 300
  # How much narrower the engine may draw a text before it is drawn smaller,
  # and how much smaller a window's text may all be drawn.
  NARROWEST = 0.8
  SMALLEST_SCALE = 0.75
  # A text drawn on a bitmap: where it starts and ends, its row, the text
  # the game gave, what tells an English and its place (key), the room its
  # Japanese needed, its box, how it is set in it, whether it is English of
  # the launcher's, whether it was moved, the run of texts it ends (where
  # the run starts, its texts' widths at full size, the gaps), and its own
  # width at full size.
  Drawn = Struct.new(:start, :finish, :y, :height, :text, :key, :needed, :lo, :hi, :align, :ours, :moved, :run, :natural)
  # This script's own section among the game's.
  OUR_SECTION = 77_000_001
  # A game may draw plain letters as codes (a "w" in its battle text is a
  # wait). The launcher writes such a letter in an English word as the
  # character U+F800 above it, and it is drawn as the letter.
  STAND_IN = /[#{[0xf841].pack("U")}-#{[0xf87a].pack("U")}]/u
  STAND_IN_START = "\357\241"
  LETTERS = "A-Za-z0-9#{[0xf841].pack("U")}-#{[0xf87a].pack("U")}"
  WORD_START = /\A[#{LETTERS}]/u
  ENGLISH_WORD = /[A-Za-z#{[0xf841].pack("U")}-#{[0xf87a].pack("U")}]{3}/u
  # A code of a message window, with what it takes.
  CODE = /\\[A-Za-z]+(?:\[[^\]]*\])?/
  WORD_END = /[#{LETTERS}:;,.!?)]\z/u
  LABEL_END = /[#{LETTERS}:]\z/u
  # Hiragana, katakana, CJK ideographs and half-width katakana.
  JAPANESE = /[#{[0x3040].pack("U")}-#{[0x30ff].pack("U")}#{[0x3400].pack("U")}-#{[0x9fff].pack("U")}#{[0xff66].pack("U")}-#{[0xff9f].pack("U")}]/u
  FORMAT = /%(?:\d+\$)?[sd]/
  # Half-width katakana (U+FF66 to U+FF9D) as full-width: games write names
  # both ways (a skill's note half-width, the database full-width).
  FULL_WIDTH = [
    0x30F2, 0x30A1, 0x30A3, 0x30A5, 0x30A7, 0x30A9, 0x30E3, 0x30E5, 0x30E7, 0x30C3, 0x30FC, 0x30A2, 0x30A4, 0x30A6,
    0x30A8, 0x30AA, 0x30AB, 0x30AD, 0x30AF, 0x30B1, 0x30B3, 0x30B5, 0x30B7, 0x30B9, 0x30BB, 0x30BD, 0x30BF, 0x30C1,
    0x30C4, 0x30C6, 0x30C8, 0x30CA, 0x30CB, 0x30CC, 0x30CD, 0x30CE, 0x30CF, 0x30D2, 0x30D5, 0x30D8, 0x30DB, 0x30DE,
    0x30DF, 0x30E0, 0x30E1, 0x30E2, 0x30E4, 0x30E6, 0x30E8, 0x30E9, 0x30EA, 0x30EB, 0x30EC, 0x30ED, 0x30EF, 0x30F3
  ]
  HALF_WIDTH = /[#{[0xff66].pack("U")}-#{[0xff9f].pack("U")}]/u
  # The full-width colon ending a Japanese label.
  COLON = [0xff1a].pack("U")
  CHARACTER = /./mu
  # What a message line is measured by: a name or variable code, a code
  # that draws nothing, an escaped character, then any one character.
  TOKEN = /\\[NnVv]\[\d+\]|\\[A-Za-z]+(?:\[[^\]]*\])?|\\.|./mu
  # Database messages VX puts right after a name ("Eric" + message1).
  SUFFIXES = [:message1, :message2, :message3, :message4]
  SMALLEST = 14
  # XP's message window: a row of 32 pixels, text drawn 4 pixels in, 8
  # more for a choice; VX's: a face takes 112 pixels of the line.
  XP_ROW = 32
  XP_MARGIN = 12
  VX_FACE = 112
  # The XRXS message window: its text surface is 16 pixels narrower than
  # the window and text starts 4 pixels in; a face takes 120.
  XRXS_EDGE = 24
  XRXS_FACE = 120
  # A battle text window that draws a character at a time (the one of
  # Succubus Rhapsodia's scripts): text starts 4 pixels in; "w", "m" and
  # "y" are waits, "q" ends a row and "H" is a heart 16 pixels wide.
  BATTLE_TEXT_EDGE = 8
  BATTLE_TEXT_WAITS = "wmy"
  BATTLE_TEXT_ROW = "q"
  BATTLE_TEXT_HEART = 16
  BATTLE_TEXT_TOKEN = /\\[NnVv]\[\d+\]|\\[Cc]\[\d+\]|\001\[\d+\]|./mu

  @texts = {}
  @names = {}
  @kept = {}
  @ours = {}
  @pieces = {}
  @swapped = {}
  @stamp = nil
  @shown = true
  @spots = []
  @vocab = []
  @hooked = false
  @started = false
  @tables_missing = false
  @shown_read = false
  @scripts = 0
  @scripts_reported = 0
  @sections = 0
  @sections_reported = 0
  @layout = nil
  @layout_changed = false
  @layout_saved = Time.at(0)

  class << self
    # VX defines Game_Message and Game_Interpreter; XP has Game_Temp hold
    # the message and an Interpreter run events.
    def vx?
      defined?(Game_Message) ? true : false
    end

    def log(text)
      Dir.mkdir(FOLDER) unless File.directory?(FOLDER)
      File.open(LOG, "a") { |file| file.write(Time.now.strftime("%Y-%m-%d %H:%M:%S ") + text + "\n") }
    rescue StandardError
      nil
    end

    def characters(text)
      text.scan(CHARACTER)
    end

    def read_shown
      launcher = File.exist?(LAUNCHER) ? File.read(LAUNCHER) : ""
      @shown = launcher !~ /"shown"\s*:\s*false/
    end

    # The launcher's copy of a Ruby file the game loads from its own folder,
    # with the English in it, when there is one and translation is shown.
    # Games load such files while their scripts still run, before anything
    # here has read the launcher's settings.
    def script_for(name)
      unless @shown_read
        read_shown
        @shown_read = true
      end
      return nil unless @shown
      path = name.to_s.gsub("\\", "/").sub(/\A(?:\.\/)+/, "")
      return nil if path =~ /\A\// || path =~ /\A[A-Za-z]:/ || path =~ /(?:\A|\/)\.\.(?:\/|\z)/
      ours = SCRIPTS + path
      File.file?(ours) ? ours : nil
    rescue StandardError
      nil
    end

    def script_loaded
      @scripts += 1
    end

    def script_failed(name, error)
      log("#{name}: the copy with the English could not be loaded, the game's own was (#{error.class}: #{error.message.to_s[0, 200]})")
    end

    def report_scripts
      if @sections != @sections_reported
        @sections_reported = @sections
        log("#{@sections} of the game's script sections run with the English")
      end
      return if @scripts == @scripts_reported
      @scripts_reported = @scripts
      log("#{@scripts} of the game's script files loaded with the English")
    end

    # The launcher's copies of the game's script sections, with the English
    # in the sentences they put together. The engine has every section's
    # source in $RGSS_SCRIPTS before it runs the first, and runs what is
    # there when it gets to a section: this script runs first, so a copy put
    # in now is what runs. A copy goes in only for the very source it was
    # made from, and only when it is Ruby the engine reads; else the game's
    # own section runs.
    def swap_sections
      unless @shown_read
        read_shown
        @shown_read = true
      end
      list = SECTIONS + "list.txt"
      return unless @shown && $RGSS_SCRIPTS.is_a?(Array) && File.file?(list)
      made = {}
      File.read(list).split("\n").each do |line|
        at, sum, size = line.split(" ")
        made[at.to_i] = [sum.to_i, size.to_i] if size
      end
      at = -1
      $RGSS_SCRIPTS.each do |section|
        next if section.is_a?(Array) && section[0] == OUR_SECTION
        at += 1
        sum, size = made[at]
        next unless sum && section.is_a?(Array) && section[3].is_a?(String)
        next unless section[3].size == size && Zlib.crc32(section[3]) == sum
        code = File.open(SECTIONS + ("%04d.rb" % at), "rb") { |file| file.read }
        if reads_as_ruby?(code)
          section[3] = code
          @sections += 1
        else
          log("section #{at}: the copy with the English is not Ruby the engine reads, the game's own runs")
        end
      end
    rescue StandardError => error
      log("the script sections: #{error.message}")
    end

    # Whether the engine can read `code`: it is read whole before its first
    # line runs, and that line leaves at once.
    def reads_as_ruby?(code)
      catch(:dla_translate_read) do
        eval("throw :dla_translate_read, true\n" + code, TOPLEVEL_BINDING, "DLA Translation (a section)")
      end
    rescue ScriptError, StandardError
      false
    end

    # A text with the letters themselves where characters stand for them,
    # or nil when it has none.
    def letters_back(text)
      return nil unless text.is_a?(String) && text.index(STAND_IN_START)
      text.gsub(STAND_IN) { |character| (character.unpack("U")[0] - 0xf800).chr }
    rescue StandardError
      nil
    end

    # Reads the translations again when the launcher wrote new ones.
    def refresh
      read_shown
      return false unless File.exist?(TRANSLATIONS)
      stamp = File.mtime(TRANSLATIONS)
      return false if stamp == @stamp
      data = File.open(TRANSLATIONS, "rb") { |file| Marshal.load(file) }
      @texts = data["texts"] || {}
      @names = data["names"] || {}
      @kept = {}
      (data["kept"] || []).each { |text| @kept[text] = true }
      @ours = {}
      @texts.each { |japanese, english| @ours[english] ||= japanese }
      @names.each { |japanese, english| @ours[english] ||= japanese }
      index_pieces
      @stamp = stamp
      log("#{@texts.size} translations and #{@names.size} names" + (@shown ? "" : ", turned off"))
      true
    rescue StandardError => error
      log("could not read the translations: #{error.message}")
      false
    end

    def english(text)
      return nil unless @shown && text.is_a?(String) && text =~ JAPANESE
      @texts[text] || @names[text]
    rescue StandardError
      nil
    end

    # A text as it is drawn: any the launcher has English for, the strings
    # in the game's scripts and the player's own replacements included.
    def shown(text)
      return nil unless @shown && text.is_a?(String) && !text.empty?
      @texts[text] || @names[text]
    rescue StandardError
      nil
    end

    # A string the game puts together (a label and a number, a label and a
    # name): each known Japanese piece in it swapped for its English, the
    # longest first. Results are kept, as some windows draw every frame.
    def pieces_in(text)
      return nil unless @shown && text.is_a?(String) && text =~ JAPANESE
      return @swapped[text] if @swapped.include?(text)
      @swapped.clear if @swapped.size > 2000
      chars = characters(text)
      result = ""
      at = 0
      changed = false
      while at < chars.size
        piece = (@pieces[chars[at]] || []).find { |japanese, count| chars[at, count].join == japanese }
        if piece
          english = @texts[piece[0]] || @names[piece[0]]
          result << " " if result =~ WORD_END && english =~ WORD_START
          result << english
          at += piece[1]
          result << " " if english =~ LABEL_END && chars[at].to_s =~ WORD_START
          changed = true
        else
          result << chars[at]
          at += 1
        end
      end
      @swapped[text] = changed ? result : nil
    rescue StandardError
      nil
    end

    # The short texts that can be pieces of a longer string, by their
    # first character, each with its length in characters.
    def index_pieces
      @swapped = {}
      @pieces = {}
      [@texts, @names].each do |table|
        table.each_key do |japanese|
          chars = characters(japanese)
          next if chars.size < 2 || chars.size > 30 || japanese.include?("\n")
          (@pieces[chars[0]] ||= []) << [japanese, chars.size]
        end
      end
      @pieces.each_value { |list| list.sort! { |a, b| b[1] <=> a[1] } }
    end

    # A label ending in a full-width colon, drawn as English ending in ":".
    def colon_label?(japanese, english)
      japanese.is_a?(String) && japanese.rstrip[-COLON.size, COLON.size] == COLON && english[-1, 1] == ":"
    end

    # What to draw for a text: its English, or its known pieces in English,
    # also when it writes katakana half-width. A single character is left
    # alone: the message window draws one at a time.
    def drawn(text)
      return nil unless text.is_a?(String) && characters(text).size > 1
      english = shown(text)
      return english if english
      return pieces_in(text) unless text =~ HALF_WIDTH
      full = full_width(text)
      english = shown(full)
      return english if english
      [pieces_in(text), pieces_in(full)].compact.sort_by { |swapped| swapped.scan(JAPANESE).size }.first
    end

    def full_width(text)
      codes = []
      text.unpack("U*").each do |code|
        if code == 0xff9e && codes.last && voiced?(codes.last)
          codes[-1] = codes.last == 0x30a6 ? 0x30f4 : codes.last + 1
        elsif code == 0xff9f && codes.last && (0x30cf..0x30db).include?(codes.last) && (codes.last - 0x30cf) % 3 == 0
          codes[-1] = codes.last + 2
        elsif code >= 0xff66 && code <= 0xff9d
          codes << FULL_WIDTH[code - 0xff66]
        else
          codes << code
        end
      end
      codes.pack("U*")
    end

    # Kana a dakuten voices: ka to to, ha to ho, and u (vu); ha to ho also
    # take the handakuten (pa).
    def voiced?(code)
      code == 0x30a6 || (0x30ab..0x30c8).include?(code) && code != 0x30c3 ||
        (0x30cf..0x30db).include?(code) && (code - 0x30cf) % 3 == 0
    end

    def ours?(text)
      @shown && @ours.include?(text)
    end

    # The Japanese an English of the launcher's stands for.
    def source(english)
      @ours[english]
    end

    # Games lay a row out for the Japanese: a label, then a value or the
    # next choice at a fixed place after it, in boxes often wider than the
    # room there is. English is wider, so a text is placed by what is on
    # its row. A text that an English before it would run into is moved to
    # start after that one. A row that runs out of the window that way, or
    # an English that reaches a text which cannot move (a number set to the
    # right), tells how much smaller the window's text has to be: that is
    # remembered, from one play to the next, and from the next time on all
    # of the window's text is drawn that much smaller, so it keeps one look.
    # Returns where the text starts and ends and the text to draw, and sets
    # the size to draw it at.
    def place(bitmap, japanese, english, x, y, width, height, align)
      shown = english || japanese
      plain = letters_back(shown) || shown
      natural = bitmap.dla_translate_text_size(plain).width
      context = [bitmap.width, bitmap.height]
      scale = layout["scales"][context]
      bitmap.font.size = [(bitmap.font.size * scale).round, SMALLEST].max if scale
      drawn = bitmap.instance_variable_get(:@dla_translate_drawn)
      unless drawn
        drawn = []
        bitmap.instance_variable_set(:@dla_translate_drawn, drawn)
      end
      # A bitmap with no English on it yet (a message typed a character at
      # a time): the text is only noted, for an English that may follow.
      unless english || bitmap.instance_variable_get(:@dla_translate_english)
        hi = x + width
        across = [bitmap.dla_translate_text_size(plain).width, width].min
        start = align == 2 ? hi - across : (align == 1 ? x + (width - across) / 2 : x)
        drawn << Drawn.new(start, start + across, y, height, japanese, nil, 0, x, hi, align, false, false, nil, natural)
        drawn.shift if drawn.size > ROOM_REMEMBERED
        return [x, hi, plain]
      end
      bitmap.instance_variable_set(:@dla_translate_english, true) if english
      row = drawn.select { |entry| entry.text != japanese && same_row?(y, height, entry.y, entry.height) }
      lo = x
      run = [x, natural, 0]
      moved = false
      # A single character is a message being typed: it stays where the
      # game puts it.
      if align == 0 && characters(plain).size > 1
        before = nil
        row.each do |entry|
          next unless (entry.ours || entry.moved) && entry.start <= x && entry.finish + FLOW_GAP > x
          before = entry if before.nil? || entry.finish > before.finish
        end
        if before
          run = [before.run[0], before.run[1] + natural, before.run[2] + FLOW_GAP]
          if before.finish + FLOW_GAP + SMALLEST < bitmap.width
            lo = before.finish + FLOW_GAP
            moved = true
          end
          across = bitmap.dla_translate_text_size(plain).width
          if before.finish + FLOW_GAP + across > bitmap.width
            learn_scale(context, (bitmap.width - run[0] - run[2]) / run[1].to_f)
          end
        end
      end
      hi = [lo + width, bitmap.width].min
      needed = 0
      key = nil
      if english
        key = [japanese, x, width, bitmap.width]
        # The full-width colon's right half is a gap ":" lacks, and what
        # the game draws next starts where the Japanese ended.
        hi -= bitmap.dla_translate_text_size(COLON).width / 2 if align == 0 && colon_label?(japanese, english)
        needed = bitmap.dla_translate_text_size(japanese).width * 9 / 10
        limit = layout["limits"][key]
        limit = nil if limit && limit < needed
        if align == 2
          row.each do |entry|
            from = entry.finish + ROOM_GAP
            lo = from if entry.finish < hi && from > lo && hi - from >= needed
          end
          lo = hi - limit if limit && hi - limit > lo
        elsif align == 1
          centre = (lo + hi) / 2.0
          half = [centre - lo, hi - centre].min
          row.each do |entry|
            reach = entry.finish <= centre ? centre - entry.finish - ROOM_GAP : (entry.start >= centre ? entry.start - ROOM_GAP - centre : nil)
            half = reach if reach && reach < half && reach * 2 >= needed
          end
          half = limit / 2.0 if limit && limit / 2.0 < half
          lo = (centre - half).round
          hi = (centre + half).round
        else
          row.each do |entry|
            to = entry.start - ROOM_GAP
            hi = to if entry.start > lo && to < hi && to - lo >= needed
          end
          hi = lo + limit if limit && lo + limit < hi
        end
        # The engine draws a text a little narrower by itself; smaller
        # than the rest of the window only when that is not enough.
        while bitmap.font.size > SMALLEST && bitmap.dla_translate_text_size(plain).width * NARROWEST > hi - lo
          bitmap.font.size -= 1
        end
      end
      across = [bitmap.dla_translate_text_size(plain).width, hi - lo].min
      start = align == 2 ? hi - across : (align == 1 ? lo + (hi - lo - across) / 2 : lo)
      finish = start + across
      # An English drawn before on the row that reaches into a text which
      # stays where it is had less room than it took.
      unless moved
        row.each do |entry|
          next unless entry.key
          if entry.align == 2
            cramped(entry, entry.hi - finish - ROOM_GAP, context) if entry.finish > finish && entry.start < finish - 1
          elsif entry.align == 1
            centre = (entry.lo + entry.hi) / 2.0
            cramped(entry, ((start - ROOM_GAP - centre) * 2).floor, context) if start >= centre && entry.finish > start + 1
            cramped(entry, ((centre - finish - ROOM_GAP) * 2).floor, context) if finish <= centre && entry.start < finish - 1
          elsif entry.start < start && entry.finish > start + 1
            cramped(entry, start - ROOM_GAP - entry.lo, context)
          end
        end
      end
      drawn << Drawn.new(start, finish, y, height, japanese, key, needed, lo, hi, align, !english.nil?, moved, run, natural)
      drawn.shift if drawn.size > ROOM_REMEMBERED
      save_layout if @layout_changed
      [lo, hi, plain]
    rescue StandardError => error
      log("layout: #{error.message}")
      [x, x + width, letters_back(english || japanese) || english || japanese]
    end

    def same_row?(y, height, top, tall)
      ([y + height, top + tall].min - [y, top].max) * 2 > [height, tall].min
    end

    # An English had `limit` pixels before a text that stays where it is.
    def cramped(entry, limit, context)
      return if limit < entry.needed || limit <= 0
      known = layout["limits"][entry.key]
      if known.nil? || known > limit
        layout["limits"][entry.key] = limit
        @layout_changed = true
      end
      learn_scale(context, limit / entry.natural.to_f) if entry.natural > 0
    end

    # A window's text is drawn `scale` times its size from now on: never
    # less than SMALLEST_SCALE, below which a text is made narrower instead.
    def learn_scale(context, scale)
      scale = [(scale * 100).floor / 100.0, SMALLEST_SCALE].max
      return if scale > 0.97
      known = layout["scales"][context]
      return if known && known <= scale
      layout["scales"][context] = scale
      @layout_changed = true
    end

    # What an earlier version of this script learned is not kept: it moved
    # the letters of typed messages too.
    def layout
      @layout ||= begin
        found = File.exist?(LAYOUT) ? File.open(LAYOUT, "rb") { |file| Marshal.load(file) } : nil
        found = nil unless found.is_a?(Hash) && found["version"] == LAYOUT_VERSION
        found || { "version" => LAYOUT_VERSION, "limits" => {}, "scales" => {} }
      rescue StandardError
        { "version" => LAYOUT_VERSION, "limits" => {}, "scales" => {} }
      end
    end

    def save_layout
      return if Time.now - @layout_saved < 3
      @layout_saved = Time.now
      @layout_changed = false
      File.open(LAYOUT, "wb") { |file| Marshal.dump(@layout, file) }
    rescue StandardError => error
      log("layout: #{error.message}")
    end

    # Menu words and the database: remembered where they are, so lines
    # finished later are put in place on the next map change, and the
    # Japanese goes back when the player turns the translation off. A text
    # the game's scripts use stays Japanese there, for them to compare; its
    # English is drawn instead.
    def remember(owner, key)
      text = key.is_a?(Integer) ? owner[key] : field(owner, key)
      @spots << [owner, key, text] if text.is_a?(String) && text =~ JAPANESE && !@kept.include?(text)
    rescue StandardError
      nil
    end

    def has_field?(owner, key)
      owner.instance_variables.any? { |variable| variable.to_s == "@#{key}" }
    end

    # The database's own field, not its reader: games replace readers with
    # ones that need the party, which does not exist yet while loading.
    def field(owner, key)
      return owner.instance_variable_get("@#{key}") if has_field?(owner, key)
      owner.respond_to?(key) ? owner.send(key) : nil
    end

    def set_field(owner, key, text)
      if has_field?(owner, key)
        owner.instance_variable_set("@#{key}", text)
      else
        owner.send("#{key}=", text)
      end
    end

    # XP keeps its menu words in System#words, VX in System#terms: one text
    # field each. A table not loaded yet is looked at again on the first map.
    def remember_database(system = $data_system)
      @spots = []
      return unless system
      words = system.respond_to?(:words) ? system.words : (system.respond_to?(:terms) ? system.terms : nil)
      if words
        words.instance_variables.each { |variable| remember(words, variable.to_s[1..-1].intern) }
      end
      if system.respond_to?(:elements) && system.elements
        system.elements.each_index { |index| remember(system.elements, index) }
      end
      remember(system, :game_title) if system.respond_to?(:game_title)
      tables = [[$data_actors, [:name]],
       [$data_classes, [:name]],
       [$data_skills, [:name, :description, :message1, :message2]],
       [$data_items, [:name, :description]],
       [$data_weapons, [:name, :description]],
       [$data_armors, [:name, :description]],
       [$data_enemies, [:name]],
       [$data_states, [:name, :message1, :message2, :message3, :message4]]]
      @tables_missing = tables.any? { |table, _| table.nil? }
      tables.each do |table, fields|
        (table || []).each do |row|
          fields.each { |key| remember(row, key) if has_field?(row, key) } if row
        end
      end
      # Once: the constants hold the English after the first apply.
      if defined?(Vocab) && @vocab.empty?
        Vocab.constants.each do |name|
          text = Vocab.const_get(name)
          @vocab << [name, text] if text.is_a?(String) && text =~ JAPANESE
        end
      end
      apply
      log("#{@spots.size} menu and database texts, #{@vocab.size} battle messages")
    end

    def apply
      @spots.each do |owner, key, japanese|
        text = english(japanese) || japanese
        # "Eric" + "casts Fire!" needs the space Japanese does without.
        text = " " + text if text != japanese && SUFFIXES.include?(key) && text =~ WORD_START
        if key.is_a?(Integer)
          owner[key] = text
        else
          set_field(owner, key, text)
        end
      end
      @vocab.each do |name, japanese|
        text = english(japanese)
        # Battle messages are formats: a translation missing a value would
        # stop the game with an error.
        text = japanese unless text && text.scan(FORMAT).size == japanese.scan(FORMAT).size
        Vocab.send(:remove_const, name)
        Vocab.const_set(name, text)
      end
    rescue StandardError => error
      log("could not put the translations in place: #{error.message}")
    end

    # The game loaded a data file. Its first scene loads the System file,
    # once every script has run, so the engine's classes can be added to
    # then; the System object is passed, as $data_system is assigned only
    # afterwards. A script may load that file earlier, while the scripts
    # are still being run, to read a setting: there is no scene yet then,
    # and the classes after that script are not there.
    def loaded(filename, data)
      return unless filename.to_s =~ /System\.r[xv]data\z/i && $scene
      hook
      started(data)
    rescue StandardError => error
      log("#{filename}: #{error.message}")
    end

    def started(system = $data_system)
      @started = true
      refresh
      remember_database(system)
    end

    def arrive(map_id)
      File.open(PLACE, "w") { |file| file.write("map:#{map_id}") }
      report_scripts
      remember_database if @tables_missing
      apply if refresh
    rescue StandardError => error
      log("map #{map_id}: #{error.message}")
    end

    # The English of the message starting at list[index], and how many
    # lines the Japanese has. XP keeps the first line in the Show Text
    # command itself; VX keeps every line in the commands after it.
    def message(list, index)
      lines = []
      lines << list[index].parameters[0].to_s unless vx?
      at = index + 1
      while list[at] && list[at].code == 401
        lines << list[at].parameters[0].to_s
        at += 1
      end
      [english(lines.join("\n")), lines.size]
    rescue StandardError => error
      log("message: #{error.message}")
      [nil, 0]
    end

    # XP: the message window draws $game_temp.message_text, the message's
    # lines then the choices', a row each in four rows. The English takes the
    # message's rows, smaller when it needs more than there are; returns the
    # text size to draw it at.
    def replace_xp_message(window)
      pending = $game_temp.dla_pending
      unless pending && $game_temp.message_text
        if @shown && $game_temp.message_text.is_a?(String)
          told = with_variable_texts($game_temp.message_text)
          $game_temp.message_text = told unless told.equal?($game_temp.message_text)
        end
        return fit_xp_message(window)
      end
      $game_temp.dla_pending = nil
      english, count = pending
      english = with_variable_texts(english)
      lines = $game_temp.message_text.split("\n", -1)
      lines.pop if lines.last == ""
      return nil if lines.size < count
      rest = lines[count..-1]
      width, size = own_surface(window, english)
      if width
        wrapped, size = wrap(english, width, size), nil
      else
        rows = window.contents.height / XP_ROW
        available = rows - rest.size
        available = 1 if available < 1
        wrapped, size = fit_lines(english, window.contents.width - XP_MARGIN, available)
      end
      ours = wrapped.split("\n")
      $game_temp.message_text = (ours + rest).join("\n") + "\n"
      shift = ours.size - count
      $game_temp.choice_start += shift if $game_temp.choice_max > 0
      $game_temp.num_input_start += shift if $game_temp.num_input_variable_id > 0
      size
    rescue StandardError => error
      log("message: #{error.message}")
      nil
    end

    # A message may show a text the game keeps in a variable (`\v[5]`, a
    # sentence or a speaker's name an event's script put there): the window
    # would put the Japanese in. Its English goes in where the code is.
    def with_variable_texts(text)
      return text unless text =~ /\\[Vv]\[\d+\]/ && $game_variables
      text.gsub(/\\[Vv]\[(\d+)\]/) do |code|
        value = $game_variables[$1.to_i]
        value.is_a?(String) ? (english(value) || code) : code
      end
    rescue StandardError
      text
    end

    # A message the game's scripts put together. A line of it may hold
    # English and a word the scripts keep in Japanese (`Status:正常`): the
    # words the launcher has English for are put in. And each line too wide
    # for the window, as English in a script's sentence is, is broken at its
    # spaces. The choices' lines stay as they are.
    def fit_xp_message(window)
      text = $game_temp.message_text
      return nil unless @shown && text.is_a?(String)
      width, size = own_surface(window, text)
      width ||= window.contents.width - XP_MARGIN
      size ||= window.contents.font.size
      lines = text.split("\n", -1)
      lines.pop if lines.last == ""
      choices = $game_temp.choice_max > 0 ? $game_temp.choice_start : lines.size
      fitted = []
      added = 0
      changed = false
      lines.each_with_index do |line, at|
        if at < choices
          known = with_known_words(line)
          changed = true if known != line
          line = known
        end
        parts = at < choices && line.index(" ") && measure(line, size) > width ? wrap(line, width, size).split("\n") : [line]
        added += parts.size - 1
        fitted.concat(parts)
      end
      return nil if added == 0 && !changed
      $game_temp.message_text = fitted.join("\n") + "\n"
      $game_temp.choice_start += added if $game_temp.choice_max > 0
      $game_temp.num_input_start += added if $game_temp.num_input_variable_id > 0
      nil
    rescue StandardError => error
      log("message: #{error.message}")
      nil
    end

    # A line with English in it and Japanese too: the Japanese words the
    # launcher has English for, in English. A line all in Japanese is a
    # sentence not translated yet, and stays whole.
    def with_known_words(line)
      return line unless line =~ JAPANESE && line.gsub(CODE, "") =~ ENGLISH_WORD
      pieces_in(line) || line
    end

    # Battle text a game's own window draws a character at a time, with no
    # line breaking of its own: a row is as wide as the Japanese was written
    # to be. English is broken at its spaces where a row would run past the
    # window; `x` is where the row being drawn has got to.
    def fit_battle_text(window, x)
      text = $game_temp.battle_log_text
      return unless @shown && text.is_a?(String) && (text.index(" ") || text =~ JAPANESE)
      width = window.contents.width - BATTLE_TEXT_EDGE
      size = window.contents.font.size
      fitted = ""
      text.split(/(\n|#{BATTLE_TEXT_ROW})/).each do |row|
        if row == "\n" || row == BATTLE_TEXT_ROW
          fitted << row
          x = 0
          next
        end
        row = with_known_words(row)
        gap = nil
        row.split(/( +)/).each do |word|
          next if word.empty?
          if word[0, 1] == " "
            gap = word
            next
          end
          across = battle_text_width(word, size)
          between = gap && x > 0 ? battle_text_width(gap, size) : 0
          if x > 0 && x + between + across > width
            fitted << "\n"
            x = 0
          elsif between > 0
            fitted << gap
            x += between
          end
          fitted << word
          x += across
          gap = nil
        end
        # What the game adds next may be the row's next word.
        fitted << gap if gap
      end
      text.replace(fitted) if fitted != text
    rescue StandardError => error
      log("battle text: #{error.message}")
    end

    # A line a game's own window draws a character at a time in a corner
    # of the screen (a place's name, a hint; the one of Succubus
    # Rhapsodia's scripts): its English when the whole line has one, and
    # broken at its spaces when a row of it is wider than the window.
    def fit_announcement(window, box)
      return unless @shown && box.is_a?(Array) && box[0].is_a?(String)
      text = @texts[box[0]] || @names[box[0]] || box[0]
      width = window.contents.width - ROOM_GAP
      size = window.contents.font.size
      if text.split("\n").any? { |row| measure(row, size) > width }
        text = wrap(text, width, size)
      end
      box[0] = text.dup if text != box[0]
    rescue StandardError => error
      log("announcement: #{error.message}")
    end

    def battle_text_width(text, size)
      width = 0
      text.scan(BATTLE_TEXT_TOKEN) do |token|
        if token =~ /\A\\([NnVv])\[(\d+)\]\z/
          width += plain_width(code_value($1.upcase, $2.to_i), size)
        elsif token.size > 1 && (token[0, 1] == "\\" || token[0, 1] == "\001")
          # A colour.
        elsif token == "H"
          width += BATTLE_TEXT_HEART
        elsif !BATTLE_TEXT_WAITS.include?(token)
          width += plain_width(token == "\000" ? "\\" : token, size)
        end
      end
      width
    end

    # The message script many XP games use (XRXS) draws on a surface of its
    # own, at one text size, in a window that grows with its lines: a
    # smaller size on the window's contents changes nothing there. Returns
    # the width a line has and that text size, or nil for another window.
    def own_surface(window, text)
      return nil unless window.instance_variables.any? { |name| name.to_s == "@text_sprite" }
      return nil unless Window_Message.const_defined?(:DEFAULT_RECT)
      rect = window.instance_variable_get(:@default_rect) || Window_Message.const_get(:DEFAULT_RECT)
      size = Font.default_size
      size = Window_Message.const_get(:DEFAULT_FONT_SIZE) if Window_Message.const_defined?(:DEFAULT_FONT_SIZE)
      face = text =~ /\\[Ff]\[.+?,\d+\]/ ? XRXS_FACE : 0
      [rect.width - XRXS_EDGE - face, size]
    rescue StandardError => error
      log("message window: #{error.message}")
      nil
    end

    # VX: the message window takes $game_message.texts, the message's lines
    # then the choices', and turns a page when they run past four.
    def replace_vx_message(window)
      pending = $game_message.dla_pending
      return unless pending
      $game_message.dla_pending = nil
      english, count = pending
      texts = $game_message.texts
      return if texts.size < count
      room = window.contents.width - ($game_message.face_name.empty? ? 0 : VX_FACE) - 4
      ours = wrap(english, room).split("\n")
      texts[0, count] = ours
      shift = ours.size - count
      $game_message.choice_start += shift if $game_message.choice_max > 0
      $game_message.num_input_start += shift if $game_message.num_input_variable_id > 0
    rescue StandardError => error
      log("message: #{error.message}")
    end

    # English is longer than the Japanese it replaces: wrapped to `width`
    # as the window will draw it.
    def wrap(text, width, size = Font.default_size)
      lines = []
      line = ""
      text.gsub("\n", " ").split(" ").each do |word|
        # Codes that draw nothing (a colour) take no space after them.
        invisible = measure(line, size) == 0
        candidate = invisible ? line + word : line + " " + word
        if !invisible && measure(candidate, size) > width
          lines << line
          line = word
        else
          line = candidate
        end
      end
      lines << line
      lines.join("\n")
    rescue StandardError => error
      log("wrap: #{error.message}")
      text
    end

    # The text wrapped into at most `rows` lines: at the default text size
    # when it fits, else at the largest smaller size that does.
    def fit_lines(text, width, rows)
      size = Font.default_size
      loop do
        wrapped = wrap(text, width, size)
        return [wrapped, size] if wrapped.count("\n") + 1 <= rows || size <= SMALLEST
        size -= 2
      end
    end

    # How wide the game draws a line, character by character as the message
    # window does, with names and variables put in and codes left out.
    def measure(text, size)
      width = 0
      text.scan(TOKEN) do |token|
        if token =~ /\A\\([NnVv])\[(\d+)\]\z/
          width += plain_width(code_value($1.upcase, $2.to_i), size)
        elsif token == "\\\\"
          width += plain_width("\\", size)
        elsif token[0, 1] == "\\" && token.size > 1
          # A code the game shows nothing for: colour, gold, wait.
        else
          width += plain_width(token, size)
        end
      end
      width
    end

    def code_value(code, number)
      code == "N" ? ($game_actors[number] ? $game_actors[number].name : "") : $game_variables[number].to_s
    rescue StandardError
      ""
    end

    def plain_width(text, size)
      @ruler ||= Bitmap.new(1, 1)
      @widths ||= {}
      width = 0
      characters(text.to_s).each do |character|
        width += (@widths[[size, character]] ||= begin
          @ruler.font.size = size
          @ruler.text_size(character).width
        end)
      end
      width
    end

    def hook
      return if @hooked
      @hooked = true
      (HOOKS + (vx? ? VX_HOOKS : XP_HOOKS)).each do |name, code|
        begin
          eval(code, TOPLEVEL_BINDING, "DLA Translation (#{name})")
        rescue StandardError, SyntaxError, NameError => error
          log("left out #{name}: #{error.message}")
        end
      end
      log("started (#{vx? ? "VX" : "XP"})")
    end
  end

  # Added to the game's own classes, which every script run defines anew.
  HOOKS = [
    ["maps", <<-'RUBY'],
      class Game_Map
        alias_method :dla_translate_setup, :setup
        def setup(map_id)
          dla_translate_setup(map_id)
          DLA_Translate.arrive(map_id)
        end
      end
    RUBY
    # Actors copy their names into the save at New Game; read through the
    # translation, so saves made before still show English.
    ["actors", <<-'RUBY'],
      class Game_Actor
        alias_method :dla_translate_name, :name
        def name
          text = dla_translate_name
          DLA_Translate.english(text) || text
        end
      end
    RUBY
    # Strings the game's scripts draw whole: a label, a menu command, a help
    # line, a name kept Japanese in the data. Bitmap is the engine's own
    # class, so this is added once.
    ["drawn text", <<-'RUBY'],
      class Bitmap
        unless method_defined?(:dla_translate_draw_text)
          alias_method :dla_translate_draw_text, :draw_text
          # Every text is placed by what is on its row (DLA_Translate.place).
          def draw_text(*args)
            at = args[0].is_a?(Rect) ? 1 : 4
            japanese = args[at]
            x, y, width, height = at == 1 ? [args[0].x, args[0].y, args[0].width, args[0].height] : args[0, 4]
            unless japanese.is_a?(String) && [x, y, width, height].all? { |number| number.is_a?(Numeric) }
              return dla_translate_draw_text(*args)
            end
            english = DLA_Translate.drawn(japanese)
            # A text the data already holds in English is told by its
            # Japanese. A single character is a message being typed, never
            # such a text, even when a translation is that one letter.
            if !english && DLA_Translate.ours?(japanese) && DLA_Translate.characters(japanese).size > 1
              english, japanese = japanese, DLA_Translate.source(japanese)
            end
            size = font.size
            lo, hi, plain = DLA_Translate.place(self, japanese, english, x, y, width, height, args[at + 1].to_i)
            if at == 1
              dla_translate_draw_text(Rect.new(lo, y, hi - lo, height), plain, *args[2..-1])
            else
              dla_translate_draw_text(lo, y, hi - lo, height, plain, *args[5..-1])
            end
          ensure
            font.size = size if size
          end
          alias_method :dla_translate_clear, :clear
          def clear
            @dla_translate_drawn = nil
            @dla_translate_english = nil
            dla_translate_clear
          end
          if method_defined?(:clear_rect)
            alias_method :dla_translate_clear_rect, :clear_rect
            def clear_rect(*args)
              @dla_translate_drawn = nil
              @dla_translate_english = nil
              dla_translate_clear_rect(*args)
            end
          end
          alias_method :dla_translate_text_size, :text_size
          def text_size(text)
            dla_translate_text_size(DLA_Translate.letters_back(text) || text)
          end
        end
      end
    RUBY
  ]

  XP_HOOKS = [
    ["messages", <<-'RUBY'],
      class Game_Temp
        attr_accessor :dla_pending
      end
      class Interpreter
        alias_method :dla_translate_command_101, :command_101
        def command_101
          pending = $game_temp.message_text.nil? ? DLA_Translate.message(@list, @index) : nil
          result = dla_translate_command_101
          $game_temp.dla_pending = pending if result && pending && pending[0]
          result
        end
        alias_method :dla_translate_setup_choices, :setup_choices
        def setup_choices(parameters)
          choices = parameters[0].map { |choice| DLA_Translate.english(choice) || choice }
          dla_translate_setup_choices([choices] + parameters[1..-1])
        end
      end
      class Window_Message
        alias_method :dla_translate_refresh, :refresh
        def refresh
          size = DLA_Translate.replace_xp_message(self)
          return dla_translate_refresh unless size && size < self.contents.font.size
          original = self.contents.font.size
          self.contents.font.size = size
          begin
            dla_translate_refresh
          ensure
            self.contents.font.size = original
          end
        end
      end
    RUBY
    ["announcements", <<-'RUBY'],
      if defined?(Window_Announce) && (Window_Announce.instance_methods + Window_Announce.private_instance_methods).any? { |name| name.to_s == "announce_draw" }
        class Window_Announce
          alias_method :dla_translate_announce_draw, :announce_draw
          def announce_draw
            DLA_Translate.fit_announcement(self, @text_box)
            dla_translate_announce_draw
          end
        end
      end
    RUBY
    ["battle text", <<-'RUBY'],
      if defined?(Window_BattleLog) && Window_BattleLog.method_defined?(:refresh) && Game_Temp.method_defined?(:battle_log_text)
        class Window_BattleLog
          alias_method :dla_translate_refresh, :refresh
          def refresh
            DLA_Translate.fit_battle_text(self, @keep_flag == true ? @last_x.to_i : 0)
            dla_translate_refresh
          end
        end
      end
    RUBY
  ]

  VX_HOOKS = [
    ["messages", <<-'RUBY'],
      class Game_Message
        attr_accessor :dla_pending
      end
      class Game_Interpreter
        alias_method :dla_translate_command_101, :command_101
        def command_101
          pending = $game_message.busy ? nil : DLA_Translate.message(@list, @index)
          before = $game_message.texts.size
          result = dla_translate_command_101
          $game_message.dla_pending = pending if pending && pending[0] && $game_message.texts.size > before
          result
        end
        alias_method :dla_translate_setup_choices, :setup_choices
        def setup_choices(params)
          choices = params[0].map { |choice| DLA_Translate.english(choice) || choice }
          dla_translate_setup_choices([choices] + params[1..-1])
        end
      end
      class Window_Message
        alias_method :dla_translate_start_message, :start_message
        def start_message
          DLA_Translate.replace_vx_message(self)
          dla_translate_start_message
        end
      end
    RUBY
  ]
end

# load_data and Graphics are the engine's own, kept across an F12 reset, so
# they are wrapped once. Graphics.transition is the fallback for a game that
# loads its System data some other way: by then the data is loaded.
class Object
  unless private_instance_methods.any? { |name| name.to_s == "dla_translate_load_data" }
    alias_method :dla_translate_load_data, :load_data
    def load_data(filename)
      data = dla_translate_load_data(filename)
      DLA_Translate.loaded(filename, data)
      data
    end
    private :load_data, :dla_translate_load_data
  end
end

class << Graphics
  unless method_defined?(:dla_translate_transition)
    alias_method :dla_translate_transition, :transition
    def transition(*args)
      DLA_Translate.hook
      DLA_Translate.started if !DLA_Translate.instance_variable_get(:@started) && $data_system
      DLA_Translate.report_scripts
      dla_translate_transition(*args)
    end
  end
end

DLA_Translate.swap_sections

# Ruby files the game loads from its own folder: the launcher's copy with the
# English is loaded in place of each, and the file itself when the copy
# fails. Kernel is the engine's own, kept across an F12 reset.
module Kernel
  unless private_instance_methods.any? { |name| name.to_s == "dla_translate_load" }
    alias_method :dla_translate_load, :load
    def load(name, *rest)
      ours = DLA_Translate.script_for(name)
      if ours
        begin
          result = dla_translate_load(ours, *rest)
          DLA_Translate.script_loaded
          return result
        rescue ScriptError, StandardError => error
          DLA_Translate.script_failed(name, error)
        end
      end
      dla_translate_load(name, *rest)
    end
    private :load, :dla_translate_load
  end
end
