# DLA Launcher's machine translation for RPG Maker VX Ace.
#
# The launcher puts this script first in the game's scripts and the
# translations finished so far in dla-translation/translations.rvdata2;
# lines not translated yet stay in Japanese. It hooks in when the game
# starts its main loop, once every script of the game has loaded, and reads
# the translations again on every map change while the launcher prepares
# the rest. It writes the map the player is on, so the launcher translates
# that map next.
module DLA_Translate
  FOLDER = "dla-translation"
  TRANSLATIONS = FOLDER + "/translations.rvdata2"
  LAUNCHER = FOLDER + "/launcher.json"
  PLACE = FOLDER + "/place.txt"
  LOG = FOLDER + "/log.txt"
  # Hiragana, katakana, CJK ideographs and half-width katakana.
  JAPANESE = Regexp.new("[" +
    [0x3040].pack("U") + "-" + [0x30ff].pack("U") +
    [0x3400].pack("U") + "-" + [0x9fff].pack("U") +
    [0xff66].pack("U") + "-" + [0xff9f].pack("U") + "]")
  FORMAT = /%(?:\d+\$)?[sd]/
  # Half-width katakana (U+FF66 to U+FF9D) as full-width: games write names
  # both ways (a skill's note half-width, the database full-width).
  FULL_WIDTH = [
    0x30F2, 0x30A1, 0x30A3, 0x30A5, 0x30A7, 0x30A9, 0x30E3, 0x30E5, 0x30E7, 0x30C3, 0x30FC, 0x30A2, 0x30A4, 0x30A6,
    0x30A8, 0x30AA, 0x30AB, 0x30AD, 0x30AF, 0x30B1, 0x30B3, 0x30B5, 0x30B7, 0x30B9, 0x30BB, 0x30BD, 0x30BF, 0x30C1,
    0x30C4, 0x30C6, 0x30C8, 0x30CA, 0x30CB, 0x30CC, 0x30CD, 0x30CE, 0x30CF, 0x30D2, 0x30D5, 0x30D8, 0x30DB, 0x30DE,
    0x30DF, 0x30E0, 0x30E1, 0x30E2, 0x30E4, 0x30E6, 0x30E8, 0x30E9, 0x30EA, 0x30EB, 0x30EC, 0x30ED, 0x30EF, 0x30F3
  ]
  HALF_WIDTH = Regexp.new("[" + [0xff66].pack("U") + "-" + [0xff9f].pack("U") + "]")
  # The full-width colon ending a Japanese label.
  COLON = [0xff1a].pack("U")
  # A line's leading codes, then the spaces that indent it.
  INDENT = Regexp.new("\\A((?:\\\\(?:[A-Za-z]+|[{}])(?:\\[[^\\]]*\\])?)*)([ " + [0x3000].pack("U") + "]*)")
  # Database messages VX Ace puts right after a name ("Eric" + message1).
  SUFFIXES = [:message1, :message2, :message3, :message4]
  ICON_WIDTH = 24

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

  class << self
    def log(text)
      Dir.mkdir(FOLDER) unless File.directory?(FOLDER)
      File.open(LOG, "a") { |file| file.write(Time.now.strftime("%Y-%m-%d %H:%M:%S ") + text + "\n") }
    rescue StandardError
      nil
    end

    # Reads the translations again when the launcher wrote new ones.
    def refresh
      launcher = File.exist?(LAUNCHER) ? File.read(LAUNCHER) : ""
      @shown = launcher !~ /"shown"\s*:\s*false/
      return false unless File.exist?(TRANSLATIONS)
      stamp = File.mtime(TRANSLATIONS)
      return false if stamp == @stamp
      data = File.open(TRANSLATIONS, "rb") { |file| Marshal.load(file) }
      @texts = data["texts"] || {}
      @names = data["names"] || {}
      @kept = {}
      (data["kept"] || []).each { |text| @kept[text] = true }
      @ours = {}
      @texts.each_value { |english| @ours[english] = true }
      @names.each_value { |english| @ours[english] = true }
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
      result = ""
      at = 0
      changed = false
      while at < text.length
        piece = (@pieces[text[at]] || []).find { |japanese| text[at, japanese.length] == japanese }
        if piece
          english = @texts[piece] || @names[piece]
          result << " " if result =~ /[A-Za-z0-9:;,.!?)]\z/ && english =~ /\A[A-Za-z0-9]/
          result << english
          at += piece.length
          result << " " if english =~ /[A-Za-z0-9:]\z/ && text[at].to_s =~ /\A[A-Za-z0-9]/
          changed = true
        else
          result << text[at]
          at += 1
        end
      end
      @swapped[text] = changed ? result : nil
    rescue StandardError
      nil
    end

    # The short texts that can be pieces of a longer string, by their
    # first character.
    def index_pieces
      @swapped = {}
      @pieces = {}
      [@texts, @names].each do |table|
        table.each_key do |japanese|
          next if japanese.length < 2 || japanese.length > 30 || japanese.include?("\n")
          (@pieces[japanese[0]] ||= []) << japanese
        end
      end
      @pieces.each_value { |list| list.sort! { |a, b| b.length <=> a.length } }
    end

    # A label ending in a full-width colon, drawn as English ending in ":".
    def colon_label?(japanese, english)
      japanese.is_a?(String) && japanese.rstrip.end_with?(COLON) && english.end_with?(":")
    end

    # What to draw for a text: its English, or its known pieces in English,
    # also when it writes katakana half-width.
    def drawn(text)
      english = shown(text)
      return english if english
      return pieces_in(text) unless text.is_a?(String) && text =~ HALF_WIDTH
      full = full_width(text)
      shown(full) || [pieces_in(text), pieces_in(full)].compact.min_by { |swapped| swapped.scan(JAPANESE).size }
    end

    # A picture's lines (CACAO's picture extension) in English, indented as
    # the Japanese were, all at one size so the columns of a table stay
    # aligned: the largest (\S[n], the picture script's text size) at which
    # every line fits the picture.
    def picture_lines(lines, width)
      english = lines.map { |line| picture_text(line) }
      return if english == lines
      size = english.reject(&:empty?).map { |text| picture_size(text, width) }.min || Font.default_size
      if size < Font.default_size
        # The script sizes a line by the largest text size its codes reach:
        # ending with the Japanese line's keeps the rows where the game's
        # cursor expects them.
        english = english.each_with_index.map do |text, at|
          text.empty? ? text : "\\S[#{size}]" + text + "\\S[#{tallest(lines[at])}]"
        end
      end
      lines.replace(english)
    rescue StandardError => error
      log("picture lines: #{error.message}")
    end

    # The largest text size a picture line's codes reach (\{, \}, \S[n]).
    def tallest(line)
      now = Font.default_size
      top = now
      line.to_s.scan(/\\(\{|\}|S\[\d+\])/i) do |found|
        code = found[0]
        now = case code
              when "{" then now + 8
              when "}" then now - 8
              else code[/\d+/].to_i.zero? ? Font.default_size : code[/\d+/].to_i
              end
        top = now if now > top
      end
      top
    end

    def picture_text(line)
      english = drawn(line)
      return line unless english
      japanese = INDENT.match(line)
      ours = INDENT.match(english)
      ours[2].empty? ? ours[1] + japanese[2] + english[ours[0].length..-1] : english
    end

    def picture_size(text, width)
      room = width.to_i - 8
      size = Font.default_size
      size -= 2 while room > 0 && size > 12 && measure(text, size)[0] > room
      size
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

    # The database's own field, not its reader: games replace readers (an
    # equipment name the party renamed) with ones that fail while loading.
    def field(owner, key)
      variable = "@#{key}"
      owner.instance_variable_defined?(variable) ? owner.instance_variable_get(variable) : owner.send(key)
    end

    def set_field(owner, key, text)
      variable = "@#{key}"
      if owner.instance_variable_defined?(variable)
        owner.instance_variable_set(variable, text)
      else
        owner.send("#{key}=", text)
      end
    end

    def remember_database
      @spots = []
      system = $data_system
      terms = system.terms
      [terms.basic, terms.params, terms.etypes, terms.commands,
       system.elements, system.skill_types, system.weapon_types, system.armor_types].each do |list|
        list.each_index { |index| remember(list, index) } if list
      end
      remember(system, :currency_unit)
      remember(system, :game_title)
      [[$data_actors, [:name, :nickname, :description]],
       [$data_classes, [:name]],
       [$data_skills, [:name, :description, :message1, :message2]],
       [$data_items, [:name, :description]],
       [$data_weapons, [:name, :description]],
       [$data_armors, [:name, :description]],
       [$data_enemies, [:name]],
       [$data_states, [:name, :message1, :message2, :message3, :message4]]].each do |table, fields|
        (table || []).each do |row|
          fields.each { |field| remember(row, field) } if row
        end
      end
      # Once: after an F12 reset the constants already hold the English.
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
        text = " " + text if text != japanese && SUFFIXES.include?(key) && text =~ /\A[[:alnum:]]/
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

    def started
      refresh
      remember_database
    end

    def arrive(map_id)
      File.open(PLACE, "w") { |file| file.write("map:#{map_id}") }
      apply if refresh
    rescue StandardError => error
      log("map #{map_id}: #{error.message}")
    end

    # The next message or scrolling text, from its first line on.
    def expect(list, index, code)
      lines = []
      lines << list[index + lines.size].parameters[0] while list[index + lines.size] && list[index + lines.size].code == code
      $game_message.dla_expect(english(lines.join("\n")), lines.size)
    rescue StandardError => error
      log("message: #{error.message}")
    end

    # English is longer than the Japanese it replaces: wrapped to `width`
    # before the game sees it, so message scripts that size their window
    # from the lines (speech balloons) fit the English too.
    def wrap(text, width, size = Font.default_size)
      lines = []
      line = ""
      line_size = size
      text.gsub("\n", " ").split(" ").each do |word|
        # Codes that draw nothing (a balloon's \h[3], a colour) take no
        # space after them: the model writes "{1} Lately" as often as
        # "{1}Lately", and the space would indent the line.
        invisible = measure(line, line_size)[0] == 0
        candidate = invisible ? line + word : line + " " + word
        if !invisible && measure(candidate, line_size)[0] > width
          lines << line
          line_size = measure(line, line_size)[1]
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

    # The largest text size at which `text` fits `height`: the wrapped
    # text, the size and the line height. VX Ace's lines are 24 pixels tall
    # at least, so a smaller size alone fits no more lines.
    def fit(text, width, height)
      size = Font.default_size
      loop do
        wrapped = wrap(text, width, size)
        lines = wrapped.count("\n") + 1
        line_height = [height / lines, [size, 24].max].min
        return [wrapped, size, line_height] if line_height >= size || size <= 12
        size -= 2
      end
    end

    # A message wrapped as evenly as its lines allow: the narrowest width
    # that needs no more lines than the full width does, so no line ends far
    # short of the others, and a balloon is no wider than it must be.
    def balanced(text, width, size = Font.default_size)
      wrapped = wrap(text, width, size)
      lines = wrapped.count("\n") + 1
      return wrapped if lines < 2
      low = width / 2
      high = width
      while high - low > 4
        middle = (low + high) / 2
        if wrap(text, middle, size).count("\n") + 1 <= lines
          high = middle
        else
          low = middle
        end
      end
      wrap(text, high, size)
    rescue StandardError => error
      log("balanced: #{error.message}")
      wrapped || text
    end

    # The widest of a message's lines as drawn, its text size carried from
    # line to line.
    def widest(text, size = Font.default_size)
      text.to_s.split("\n").map do |line|
        width, size = measure(line, size)
        width
      end.max.to_i
    end

    # The room any message window gives a line: the screen less the
    # window's edges, and the face beside the text.
    def message_room
      Graphics.width - 48 - ($game_message.face_name.empty? ? 0 : 112)
    end

    # How wide the game draws a line, character by character as
    # Window_Base does, with \{ \} text sizes, icons, names and variables.
    # Returns the width and the text size the line ends with.
    def measure(text, size)
      width = 0
      rest = text.dup
      until rest.empty?
        if rest.sub!(/\A\\\{/, "")
          size += 8 if size <= 64
        elsif rest.sub!(/\A\\\}/, "")
          size -= 8 if size >= 16
        elsif rest.sub!(/\A\\I\[\d+\]/i, "")
          width += ICON_WIDTH
        elsif rest.sub!(/\A\\([NPV])\[(\d+)\]/i, "")
          width += plain_width(code_value($1.upcase, $2.to_i), size)
        elsif rest.sub!(/\A\\G/i, "")
          width += plain_width(Vocab.currency_unit, size)
        elsif rest.sub!(/\A\\[A-Za-z_]+(\[[^\]]*\])?/, "") || rest.sub!(/\A\\./, "")
          # A code the game shows nothing for: colour, face, wait, name box.
        else
          width += plain_width(rest.slice!(0, 1), size)
        end
      end
      [width, size]
    end

    def code_value(code, number)
      case code
      when "N" then ($game_actors[number] ? $game_actors[number].name : "")
      when "P" then ($game_party.members[number - 1] ? $game_party.members[number - 1].name : "")
      else $game_variables[number].to_s
      end
    rescue StandardError
      ""
    end

    def plain_width(text, size)
      @ruler ||= Bitmap.new(1, 1)
      @widths ||= {}
      width = 0
      text.to_s.each_char do |character|
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
      HOOKS.each do |name, code|
        begin
          eval(code, TOPLEVEL_BINDING, "DLA Translation (#{name})")
        rescue StandardError, SyntaxError, NameError => error
          log("left out #{name}: #{error.message}")
        end
      end
      log("started")
    end
  end

  HOOKS = [
    ["data", <<-'RUBY'],
      class << DataManager
        alias_method :dla_translate_load_database, :load_database
        def load_database
          dla_translate_load_database
          DLA_Translate.started
        end
      end
    RUBY
    ["messages", <<-'RUBY'],
      class Game_Message
        attr_reader :dla_translated
        alias_method :dla_translate_clear, :clear
        def clear
          dla_translate_clear
          @dla_english = nil
          @dla_lines = 0
          @dla_translated = false
        end
        def dla_expect(english, lines)
          @dla_english = english
          @dla_lines = lines
        end
        alias_method :dla_translate_add, :add
        def add(text)
          if @dla_lines.to_i > 0
            @dla_lines -= 1
            if @dla_english
              unless @dla_translated
                english = DLA_Translate.balanced(@dla_english, DLA_Translate.message_room)
                english.split("\n").each { |line| dla_translate_add(line) }
              end
              @dla_translated = true
              return
            end
          end
          # A message from the game's scripts, or a line of one not
          # translated as a whole.
          english = DLA_Translate.shown(text)
          if english && english != text
            DLA_Translate.balanced(english, DLA_Translate.message_room).split("\n").each { |line| dla_translate_add(line) }
            return
          end
          dla_translate_add(text)
        end
      end
      class Game_Interpreter
        alias_method :dla_translate_command_101, :command_101
        def command_101
          wait_for_message
          DLA_Translate.expect(@list, @index + 1, 401)
          dla_translate_command_101
          # Not left for a later message when a script took the lines.
          $game_message.dla_expect(nil, 0)
        end
        alias_method :dla_translate_command_105, :command_105
        def command_105
          Fiber.yield while $game_message.visible
          DLA_Translate.expect(@list, @index + 1, 405)
          dla_translate_command_105
          $game_message.dla_expect(nil, 0)
        end
        alias_method :dla_translate_setup_choices, :setup_choices
        def setup_choices(params)
          choices = params[0].map { |choice| DLA_Translate.english(choice) || choice }
          dla_translate_setup_choices([choices] + params[1..-1])
        end
      end
    RUBY
    # A help window holds a fixed number of lines: English that needs more is
    # drawn smaller, with the lines closer together.
    # Scrolling text in English (an intro over the map) is centred as a
    # block, its lines left-aligned within it.
    ["scrolling text", <<-'RUBY'],
      class Window_ScrollText
        alias_method :dla_translate_scroll_draw_text_ex, :draw_text_ex
        def draw_text_ex(x, y, text)
          if $game_message.dla_translated
            x = [(contents_width - DLA_Translate.widest(text)) / 2, x].max
          end
          dla_translate_scroll_draw_text_ex(x, y, text)
        end
      end
    RUBY
    ["help window", <<-'RUBY'],
      class Window_Help
        alias_method :dla_translate_refresh, :refresh
        def refresh
          @dla_fit = DLA_Translate.ours?(@text) ? DLA_Translate.fit(@text, contents_width - 8, contents.height) : nil
          dla_translate_refresh
        ensure
          @dla_fit = nil
        end
        alias_method :dla_translate_convert_escape_characters, :convert_escape_characters
        def convert_escape_characters(text)
          dla_translate_convert_escape_characters(@dla_fit ? @dla_fit[0] : text)
        end
        alias_method :dla_translate_reset_font_settings, :reset_font_settings
        def reset_font_settings
          dla_translate_reset_font_settings
          contents.font.size = @dla_fit[1] if @dla_fit
        end
        alias_method :dla_translate_set_text, :set_text
        def set_text(text)
          dla_translate_set_text(DLA_Translate.drawn(text) || text)
        end
        alias_method :dla_translate_calc_line_height, :calc_line_height
        def calc_line_height(text, restore_font_size = true)
          @dla_fit ? @dla_fit[2] : dla_translate_calc_line_height(text, restore_font_size)
        end
      end
    RUBY
    # Strings the game's scripts draw whole: a label, a menu command, a name
    # kept Japanese in the data. Drawn one character at a time, a message
    # is left to the message hooks.
    ["drawn text", <<-'RUBY'],
      class Bitmap
        alias_method :dla_translate_draw_text, :draw_text
        # English wider than the box the Japanese had is drawn smaller.
        def draw_text(*args)
          at = args[0].is_a?(Rect) ? 1 : 4
          japanese = args[at]
          english = DLA_Translate.drawn(japanese)
          english ||= japanese if DLA_Translate.ours?(japanese)
          return dla_translate_draw_text(*args) unless english
          args = args.dup
          args[at] = english
          x, width = at == 1 ? [args[0].x, args[0].width] : [args[0], args[2]]
          width = [width, self.width - x].min if width.is_a?(Numeric) && x.is_a?(Numeric)
          # The full-width colon's right half is a gap ":" lacks, and what
          # the game draws next starts where the Japanese ended.
          if width.is_a?(Numeric) && DLA_Translate.colon_label?(japanese, english)
            width -= text_size(DLA_Translate::COLON).width / 2
          end
          size = font.size
          font.size -= 2 while width.is_a?(Numeric) && font.size > 14 && text_size(english).width > width
          dla_translate_draw_text(*args)
        ensure
          font.size = size if size
        end
      end
      class Window_Base
        alias_method :dla_translate_draw_text_ex, :draw_text_ex
        # English drawn in a window keeps the Japanese's number of lines: a
        # row stays one row, a story page its lines, smaller if it must.
        def draw_text_ex(x, y, text)
          english = DLA_Translate.drawn(text)
          return dla_translate_draw_text_ex(x, y, text) unless english
          lines = text.to_s.count("\n") + 1
          room = [lines * line_height, contents.height - y].min
          @dla_fit_ex = DLA_Translate.fit(english, contents_width - x - 4, room)
          dla_translate_draw_text_ex(x, y, @dla_fit_ex[0])
        ensure
          @dla_fit_ex = nil
        end
        alias_method :dla_translate_ex_reset_font_settings, :reset_font_settings
        def reset_font_settings
          dla_translate_ex_reset_font_settings
          contents.font.size = @dla_fit_ex[1] if @dla_fit_ex
        end
        alias_method :dla_translate_ex_calc_line_height, :calc_line_height
        def calc_line_height(text, restore_font_size = true)
          @dla_fit_ex ? @dla_fit_ex[2] : dla_translate_ex_calc_line_height(text, restore_font_size)
        end
      end
    RUBY
    # CACAO's picture extension writes scrolling text into a picture, a
    # line an item (an option menu): translated line by line.
    ["picture text", <<-'RUBY'],
      class Game_Interpreter
        alias_method :dla_translate_picture_command_105, :command_105
        def command_105
          dla_translate_picture_command_105
          id = $game_temp.picture_writable_id.to_i
          picture = id > 0 ? screen.pictures[id] : nil
          lines = picture ? picture.texts : nil
          width = picture.respond_to?(:width) ? picture.width : 0
          DLA_Translate.picture_lines(lines, width) if lines.is_a?(Array)
        end
      end
    RUBY
    # A list draws a name in 172 pixels, a price or count at the row's end:
    # an English name too wide for the room before them is drawn smaller.
    # English capitals start at their edge, where Japanese has room in the
    # glyph: an English name keeps 2 pixels from its icon.
    ["item names", <<-'RUBY'],
      class Window_Base
        alias_method :dla_translate_draw_item_name, :draw_item_name
        def draw_item_name(item, x, y, enabled = true, width = 172)
          name = item && (DLA_Translate.shown(item.name) || item.name)
          return dla_translate_draw_item_name(item, x, y, enabled, width) unless DLA_Translate.ours?(name)
          room = (@dla_name_room ? [@dla_name_room, width].max : width) - 2
          size = contents.font.size
          contents.font.size -= 2 while contents.font.size > 14 && text_size(name).width > room
          @dla_name_size = contents.font.size
          @dla_text_shift = 2
          dla_translate_draw_item_name(item, x, y, enabled, room)
        ensure
          @dla_text_shift = nil
          contents.font.size = size if size
        end
        alias_method :dla_translate_window_draw_text, :draw_text
        def draw_text(*args)
          if @dla_text_shift
            args = args.dup
            if args[0].is_a?(Rect)
              args[0] = Rect.new(args[0].x + @dla_text_shift, args[0].y, args[0].width, args[0].height)
            else
              args[0] += @dla_text_shift
            end
          end
          dla_translate_window_draw_text(*args)
        end
        def dla_name_room(rect, right)
          @dla_name_room = rect.width - text_size(right).width - 8 - 24
        end
      end
      class Window_ShopBuy
        alias_method :dla_translate_draw_item, :draw_item
        def draw_item(index)
          dla_name_room(item_rect(index), price(@data[index]).to_s) if @data && @data[index]
          dla_translate_draw_item(index)
        ensure
          @dla_name_room = nil
        end
      end
      class Window_ItemList
        alias_method :dla_translate_draw_item, :draw_item
        def draw_item(index)
          dla_name_room(item_rect(index), ":00")
          dla_translate_draw_item(index)
        ensure
          @dla_name_room = nil
        end
      end
      class Window_SkillList
        alias_method :dla_translate_draw_item, :draw_item
        def draw_item(index)
          dla_name_room(item_rect(index), "000")
          dla_translate_draw_item(index)
        ensure
          @dla_name_room = nil
        end
      end
    RUBY
    ["maps", <<-'RUBY'],
      class Game_Map
        alias_method :dla_translate_setup, :setup
        def setup(map_id)
          dla_translate_setup(map_id)
          DLA_Translate.arrive(map_id)
        end
        alias_method :dla_translate_display_name, :display_name
        def display_name
          name = dla_translate_display_name
          DLA_Translate.english(name) || name
        end
      end
    RUBY
    # Actors copy their names into the save at New Game; read through the
    # translation, so saves made before still show English.
    ["actors", <<-'RUBY'],
      class Game_Actor
        [:name, :nickname].each do |reader|
          original = "dla_translate_#{reader}"
          alias_method original, reader
          define_method(reader) do
            text = send(original)
            DLA_Translate.english(text) || text
          end
        end
      end
    RUBY
  ]
end

class Object
  alias_method :dla_translate_rgss_main, :rgss_main
  def rgss_main(*args, &block)
    DLA_Translate.hook
    dla_translate_rgss_main(*args, &block)
  end
  private :rgss_main, :dla_translate_rgss_main
end
