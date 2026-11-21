/*:
 * @target MZ
 * @plugindesc Machine translation from DLA Launcher, with your own translator. Must be last.
 * @author DLA
 * @url https://github.com/bjose2345/dla-launcher
 *
 * @help
 * Added by DLA Launcher when you turn on translation for this game, and
 * removed again by "Remove translation" there. Works in RPG Maker MV and MZ.
 *
 * Shows English in place of the Japanese of messages, choices, speaker
 * names, menus and the database, and of other Japanese drawn on screen
 * (other plugins' menu commands and hints) from the second time it is drawn.
 * Translations are kept in dla-translation/translations.json beside the
 * game. While the launcher runs, lines missing there are translated by the
 * translator set up in the launcher: the current conversation first, then
 * menus, then the dialogue nearest to the player, with the database mixed
 * in. Without the launcher the game shows the translations it already has.
 *
 * dla-translation/log.txt records each request.
 */
(() => {
    "use strict";
    if (typeof require !== "function") {
        return;
    }
    const fs = require("fs");
    const path = require("path");
    const http = require("http");
    const { Buffer } = require("buffer");

    const gameFolder = () => {
        const main = process.mainModule && process.mainModule.filename;
        if (main) {
            return path.dirname(main);
        }
        const www = path.join(process.cwd(), "www");
        return fs.existsSync(path.join(www, "index.html")) ? www : process.cwd();
    };
    const folder = path.join(gameFolder(), "dla-translation");
    const read = file => {
        try {
            return JSON.parse(fs.readFileSync(path.join(folder, file), "utf8"));
        } catch (error) {
            return null;
        }
    };
    const launcher = read("launcher.json");
    if (!launcher || !launcher.shown) {
        return;
    }

    const IS_MZ = Utils.RPGMAKER_NAME === "MZ";
    const WAIT_MS = 60000;
    // The first lines a waiting player needs go alone, so the box opens
    // after a few seconds; the rest of the scene follows in bigger requests.
    const URGENT_FIRST = 2;
    const URGENT_BATCH = 6;
    // Background requests stay small: one in progress delays a player who
    // starts waiting until it is done.
    const DIALOGUE_BATCH = 5;
    const TERM_BATCH = 12;
    const TIER = { menu: 1, addon: 1, dialogue: 2, choice: 2, database: 3 };

    const logFile = path.join(folder, "log.txt");
    try {
        if (fs.statSync(logFile).size > 512 * 1024) {
            fs.renameSync(logFile, path.join(folder, "log.old.txt"));
        }
    } catch (error) {
        // No log yet.
    }
    const log = line => {
        try {
            fs.appendFileSync(logFile, `${new Date().toISOString()} ${line}\n`);
        } catch (error) {
            // A read-only game folder still plays.
        }
    };

    const saved = read("translations.json") || {};
    const store = { texts: saved.texts || {}, names: saved.names || {} };
    const speakerOverrides = Array.isArray(saved.speakers) ? saved.speakers : [];
    const textKey = text => require("crypto").createHash("sha256").update(text, "utf8").digest("hex").slice(0, 32);
    let saveTimer = null;
    const save = () => {
        clearTimeout(saveTimer);
        saveTimer = setTimeout(() => {
            const file = path.join(folder, "translations.json");
            try {
                fs.writeFileSync(`${file}.tmp`, JSON.stringify({ version: 1, texts: store.texts, names: store.names, speakers: speakerOverrides }));
                fs.renameSync(`${file}.tmp`, file);
            } catch (error) {
                log(`could not save translations: ${error.message}`);
            }
        }, 2000);
    };

    // A code of letters with nothing in brackets ends where the Japanese goes
    // on. English letters right after it would read as part of its name
    // (`\acObtained`), and the engine drops a code it does not know: the
    // names are taken from the Japanese, and a longer run of letters is cut
    // to the longest of them it starts with.
    const bareCodes = new Set();
    const learnCodes = japanese => {
        japanese.replace(/\\([A-Za-z]+)(?![A-Za-z[])/g, (code, letters) => {
            bareCodes.add(letters.toUpperCase());
            return code;
        });
    };
    Object.keys(store.texts).forEach(learnCodes);
    const codeLength = letters => {
        for (let length = letters.length; length > 0; length--) {
            if (bareCodes.has(letters.slice(0, length))) {
                return length;
            }
        }
        return letters.length;
    };

    const JAPANESE = /[぀-ヿ㐀-鿿ｦ-ﾟ]/;
    const needsTranslation = text => typeof text === "string" && JAPANESE.test(text);
    const english = text => (typeof text === "string" && (store.texts[text] || store.names[text])) || null;

    // The launcher's relay; without it the game shows the translations it has.
    const relay = launcher.relay && launcher.relay.address ? new URL(launcher.relay.address) : null;
    let offlineUntil = relay ? 0 : Infinity;
    log(`started (${IS_MZ ? "MZ" : "MV"}, ${Object.keys(store.texts).length} translations, ${relay ? "launcher running" : "no launcher"})`);

    const post = (route, body) =>
        new Promise((resolve, reject) => {
            const data = Buffer.from(JSON.stringify(body));
            const request = http.request(
                {
                    hostname: relay.hostname,
                    port: relay.port,
                    path: route,
                    method: "POST",
                    headers: {
                        Authorization: `Bearer ${launcher.relay.token}`,
                        "Content-Type": "application/json",
                        "Content-Length": data.length
                    }
                },
                response => {
                    const chunks = [];
                    response.on("data", chunk => chunks.push(chunk));
                    response.on("end", () => {
                        let reply = null;
                        try {
                            reply = JSON.parse(Buffer.concat(chunks).toString("utf8"));
                        } catch (error) {
                            // Answered below.
                        }
                        if (response.statusCode === 200 && reply) {
                            resolve(reply);
                        } else {
                            const failure = new Error((reply && reply.error) || `status ${response.statusCode}`);
                            failure.status = response.statusCode;
                            reject(failure);
                        }
                    });
                }
            );
            request.on("error", reject);
            request.setTimeout(300000, () => request.destroy(new Error("timeout")));
            request.end(data);
        });

    const wentOffline = error => {
        if (error.status === 401) {
            offlineUntil = Infinity;
            log("the launcher was restarted since this game started; showing saved translations only");
            return;
        }
        const pause = error.status === 503 ? 60000 : 30000;
        offlineUntil = Date.now() + pause;
        log(`translator unavailable (${error.message}); trying again in ${pause / 1000}s`);
        setTimeout(pump, pause + 500);
    };

    // Texts waiting for translation, and those being translated now.
    const pending = new Map();
    const inFlight = new Map();
    const failed = new Set();
    const names = new Set();
    let namesInFlight = false;
    let rank = 0;
    let backgroundTurn = 0;

    const settled = text =>
        !needsTranslation(text) || text in store.texts || text in store.names || failed.has(text);

    const want = (text, kind, { urgent = false, pos = null } = {}) => {
        if (settled(text)) {
            return;
        }
        let entry = pending.get(text) || inFlight.get(text);
        if (!entry) {
            entry = { kind, urgent: false, rank: 0, pos };
            pending.set(text, entry);
        }
        entry.pos = entry.pos || pos;
        // A text already on its way stays there; marking it keeps the mark if
        // the request fails and the text goes back to waiting.
        if (urgent && !entry.urgent) {
            entry.urgent = true;
            entry.rank = ++rank;
        }
        pump();
    };

    const wantName = name => {
        if (needsTranslation(name) && !(name in store.names) && !failed.has(name)) {
            names.add(name);
            pump();
        }
    };

    const messageAt = (list, index) => {
        const lines = [];
        for (let i = index + 1; list[i] && list[i].code === 401; i++) {
            lines.push(list[i].parameters[0]);
        }
        return lines.join("\n");
    };

    // The choices of the choice command at `at`, and of the ones right after
    // its branches when a plugin makes one list of them (MPP_ChoiceEX, which
    // takes each command through `addChoices`).
    const joinedChoices = (list, at) => {
        const choices = [...list[at].parameters[0]];
        if (!Game_Interpreter.prototype.addChoices) {
            return choices;
        }
        const indent = list[at].indent;
        let index = at;
        for (;;) {
            do {
                index++;
            } while (list[index] && !(list[index].indent === indent && list[index].code === 404));
            const next = list[index + 1];
            if (!next || next.code !== 102) {
                return choices;
            }
            index += 1;
            choices.push(...next.parameters[0]);
        }
    };

    const choicesAfter = (list, index) => {
        let i = index + 1;
        while (list[i] && list[i].code === 401) {
            i++;
        }
        return list[i] && list[i].code === 102 ? joinedChoices(list, i) : [];
    };

    // What another plugin shows from a plugin command of an event (MZ): the
    // plugins and arguments known to hold text the player reads. A page (a
    // text drawn as a picture, a quest's notes) is translated by its
    // paragraphs. A note is a page the argument holds as a JSON string; the
    // quest plugin keeps it in the save, so the command gets it as it is and
    // its English is put in where it is drawn.
    const COMMAND_TEXTS = {
        TextPicture: { set: { name: "text", kind: "dialogue", page: true } },
        TorigoyaMZ_NotifyMessage: { notify: { name: "message", kind: "dialogue" } },
        Mano_CurrencyUnit: {
            setWalletItem: { name: "unit", kind: "menu" },
            setWalletVariable: { name: "unit", kind: "menu" },
        },
        QuestSystem: { ChangeDetail: { name: "DetailNote", kind: "database", page: true, note: true } },
    };
    const noteText = value => {
        try {
            const text = JSON.parse(value);
            return typeof text === "string" ? text : null;
        } catch (error) {
            return null;
        }
    };
    const commandText = parameters => {
        const known = (COMMAND_TEXTS[parameters[0]] || {})[parameters[1]];
        const held = known && parameters[3] && parameters[3][known.name];
        const text = typeof held === "string" && known.note ? noteText(held) : held;
        return typeof text === "string" ? Object.assign({ text }, known) : null;
    };
    const commandTexts = found =>
        (found.page ? pageBlocks(found.text).map(block => block.text) : [found.text]).filter(needsTranslation);

    // The string a script is when it is one string and nothing else: what an
    // event sets a variable to, to show it.
    const stringLiteral = script => {
        const text = String(script).trim().replace(/;$/, "").trim();
        if (text.startsWith('"')) {
            try {
                const value = JSON.parse(text);
                return typeof value === "string" ? value : null;
            } catch (error) {
                return null;
            }
        }
        return /^'[^'\\]*'$/.test(text) ? text.slice(1, -1) : null;
    };
    const STATUS_SCRIPT = "CBR-エロステータス";
    const STATUS_TEXT = "テキスト-";

    const queueList = (list, from, options) => {
        for (let i = from; i < (list || []).length; i++) {
            const command = list[i];
            if (command.code === 122 && command.parameters[2] === 0 && command.parameters[3] === 4) {
                want(stringLiteral(command.parameters[4]), "database", options);
            } else if (command.code === 355 && command.parameters[0] === STATUS_SCRIPT) {
                for (let line = i + 1; list[line] && list[line].code === 655; line++) {
                    const text = String(list[line].parameters[0]);
                    if (text.startsWith(STATUS_TEXT)) {
                        want(text.slice(STATUS_TEXT.length), "menu", options);
                    }
                }
            } else if (command.code === 101) {
                wantName(command.parameters[4]);
                want(messageAt(list, i), "dialogue", options);
            } else if (command.code === 102) {
                command.parameters[0].forEach(choice => want(choice, "choice", options));
            } else if (command.code === 357) {
                const found = commandText(command.parameters);
                (found ? commandTexts(found) : []).forEach(text => want(text, found.kind, options));
            }
        }
    };

    const distance = entry => {
        if (!entry.pos || !$gameMap || entry.pos.mapId !== $gameMap.mapId() || !$gamePlayer) {
            return 1e6;
        }
        return Math.abs(entry.pos.x - $gamePlayer.x) + Math.abs(entry.pos.y - $gamePlayer.y);
    };

    let urgentRun = 0;
    const nextUrgent = () =>
        [...pending.entries()]
            .filter(([, entry]) => entry.urgent)
            .sort((a, b) => a[1].rank - b[1].rank)
            .slice(0, urgentRun++ === 0 ? URGENT_FIRST : URGENT_BATCH);

    // Menu words first, then the dialogue nearest to the player, with the
    // database mixed in.
    const nextBackground = () => {
        const entries = [...pending.entries()].filter(([, entry]) => !entry.urgent);
        const of = tier => entries.filter(([, entry]) => TIER[entry.kind] === tier);
        const menu = of(1);
        if (menu.length) {
            return menu.slice(0, TERM_BATCH);
        }
        const dialogue = of(2).sort((a, b) => distance(a[1]) - distance(b[1]));
        const database = of(3);
        backgroundTurn++;
        if (database.length && (!dialogue.length || backgroundTurn % 3 === 0)) {
            return database.slice(0, TERM_BATCH);
        }
        return dialogue.slice(0, DIALOGUE_BATCH);
    };

    const translateNames = async () => {
        const list = [...names];
        names.clear();
        namesInFlight = true;
        try {
            const reply = await post("/v1/names", { work: launcher.work, names: list, urgent: true });
            Object.assign(store.names, reply.names || {});
            list.forEach(name => {
                if (!(name in store.names)) {
                    failed.add(name);
                }
                applyDatabase(name);
            });
            save();
            log(`names ${list.length}`);
        } catch (error) {
            list.forEach(name => names.add(name));
            throw error;
        } finally {
            namesInFlight = false;
        }
    };

    const translateBatch = async (items, urgent) => {
        items.forEach(([text, entry]) => {
            pending.delete(text);
            inFlight.set(text, entry);
        });
        const started = Date.now();
        let reply;
        try {
            reply = await post("/v1/translate", {
                work: launcher.work,
                texts: items.map(([text, entry]) => ({ text, kind: entry.kind })),
                urgent
            });
        } catch (error) {
            items.forEach(([text, entry]) => {
                inFlight.delete(text);
                pending.set(text, entry);
            });
            throw error;
        }
        const translations = Array.isArray(reply.translations) ? reply.translations : [];
        let missing = 0;
        items.forEach(([text], n) => {
            inFlight.delete(text);
            if (typeof translations[n] === "string" && translations[n]) {
                store.texts[text] = translations[n];
                learnCodes(text);
                applyDatabase(text);
            } else {
                failed.add(text);
                missing++;
            }
        });
        save();
        swapLate();
        log(
            `${urgent ? "urgent" : items[0][1].kind} batch of ${items.length} in ` +
                `${((Date.now() - started) / 1000).toFixed(1)}s` +
                (missing ? `, ${missing} without a translation` : "")
        );
    };

    // Two lanes, one request each: a player waiting never queues behind
    // work done ahead of play. The launcher lets the waiting player go first.
    const lanes = { urgent: false, background: false };
    const runLane = async urgent => {
        const lane = urgent ? "urgent" : "background";
        if (!relay || lanes[lane] || Date.now() < offlineUntil) {
            return;
        }
        lanes[lane] = true;
        try {
            while (Date.now() >= offlineUntil) {
                if (urgent && names.size) {
                    await translateNames();
                    continue;
                }
                // Names first, so dialogue is translated with their spelling.
                if (!urgent && (names.size || namesInFlight)) {
                    break;
                }
                const batch = urgent ? nextUrgent() : nextBackground();
                if (!batch.length) {
                    break;
                }
                await translateBatch(batch, urgent);
            }
        } catch (error) {
            wentOffline(error);
        } finally {
            lanes[lane] = false;
            if (urgent) {
                urgentRun = 0;
                runLane(false);
            }
        }
    };
    // Starts on the next tick, so texts queued together (a whole map, the
    // database) go out in full batches, names first.
    let pumping = false;
    const pump = () => {
        if (!pumping) {
            pumping = true;
            setTimeout(() => {
                pumping = false;
                runLane(true);
                runLane(false);
            }, 0);
        }
    };

    const status = { waiting: false };
    const stopWaiting = interpreter => {
        if (interpreter._dlaWaitStart) {
            log(`player waited ${((Date.now() - interpreter._dlaWaitStart) / 1000).toFixed(1)}s`);
        }
        interpreter._dlaWaitStart = 0;
        status.waiting = false;
    };

    // Waits (up to WAIT_MS) until the texts a command shows are translated.
    const waitFor = (interpreter, texts) => {
        if (texts.every(settled) || Date.now() < offlineUntil) {
            stopWaiting(interpreter);
            return false;
        }
        if (!interpreter._dlaWaitStart) {
            interpreter._dlaWaitStart = Date.now();
            queueList(interpreter._list, interpreter._index, { urgent: true });
        }
        if (Date.now() - interpreter._dlaWaitStart < WAIT_MS) {
            status.waiting = true;
            return true;
        }
        log("gave up waiting; showing Japanese");
        stopWaiting(interpreter);
        return false;
    };

    // A line shown in Japanese because its English came after the wait:
    // swapped in and typed out again while it is still on screen. Not while
    // choices are open, as the window would not draw it again until they close.
    const swapLate = () => {
        const late = $gameMessage && $gameMessage._dlaLate;
        const translated = late && english(late.text);
        if (!translated) {
            return;
        }
        const window = SceneManager._scene && SceneManager._scene._messageWindow;
        if (!window || !window.isOpen() || window.isAnySubWindowActive()) {
            return;
        }
        $gameMessage._dlaLate = null;
        $gameMessage._texts = [translated];
        $gameMessage._dlaTranslated = true;
        const speaker = late.correctedSpeaker || (late.speaker && english(late.speaker));
        if (speaker) {
            $gameMessage.setSpeakerName(speaker);
        }
        window.pause = false;
        window.startMessage();
        log("swapped in English that came late");
    };

    const correctedSpeaker = (interpreter, text, speaker) => {
        if (!IS_MZ || !speaker || !speakerOverrides.length || !$dataMap) {
            return null;
        }
        const matches = speakerOverrides.filter(fix => fix.map === interpreter._mapId &&
            fix.event === interpreter._eventId && fix.command === interpreter._index);
        for (const fix of matches) {
            const event = $dataMap.events && $dataMap.events[fix.event];
            const page = event && event.pages && event.pages[fix.page];
            // Saved interpreters have deserialized lists rather than the database's array.
            if (page && (page.list === interpreter._list || JSON.stringify(page.list) === JSON.stringify(interpreter._list)) &&
                fix.key === textKey(text) && fix.expected_speaker === textKey(speaker)) {
                return fix.text;
            }
        }
        return null;
    };

    // MZ passes the command's parameters; MV keeps them in _params.
    const _command101 = Game_Interpreter.prototype.command101;
    Game_Interpreter.prototype.command101 = function(params) {
        if ($gameMessage.isBusy()) {
            return false;
        }
        const text = messageAt(this._list, this._index);
        if (waitFor(this, [text, ...choicesAfter(this._list, this._index)])) {
            return false;
        }
        const speaker = IS_MZ ? (params || this._params)[4] : "";
        const corrected = correctedSpeaker(this, text, speaker);
        const result = _command101.call(this, params);
        const translated = english(text);
        if (translated) {
            $gameMessage._texts = [translated];
            $gameMessage._dlaTranslated = true;
        } else {
            $gameMessage._dlaLate = { text, speaker, correctedSpeaker: corrected };
        }
        const shownSpeaker = corrected || (speaker && english(speaker));
        if (shownSpeaker) {
            $gameMessage.setSpeakerName(shownSpeaker);
        }
        return result;
    };

    const _command102 = Game_Interpreter.prototype.command102;
    Game_Interpreter.prototype.command102 = function(params) {
        const here = this._list && this._list[this._index];
        const choices = here && here.code === 102 ? joinedChoices(this._list, this._index) : (params || this._params)[0];
        if ($gameMessage.isBusy() || waitFor(this, choices)) {
            return false;
        }
        return _command102.call(this, params);
    };

    // A choice may end in a condition another plugin reads (MPP_ChoiceEX:
    // `if(s[1])`, `en(v[2]>0)`): the English ends in it as the game wrote it.
    const CHOICE_CONDITIONS = /\s?(?:if|en)\(.+?\)/g;
    const shownChoices = params => [
        params[0].map(choice => {
            const translated = english(choice);
            const conditions = (translated && choice.match(CHOICE_CONDITIONS)) || [];
            return conditions.length
                ? translated.replace(CHOICE_CONDITIONS, "") + conditions.join("")
                : translated || choice;
        }),
        ...params.slice(1),
    ];
    const _setupChoices = Game_Interpreter.prototype.setupChoices;
    Game_Interpreter.prototype.setupChoices = function(params) {
        _setupChoices.call(this, shownChoices(params));
    };
    const _addChoices = Game_Interpreter.prototype.addChoices;
    if (_addChoices) {
        Game_Interpreter.prototype.addChoices = function(params, ...rest) {
            return _addChoices.call(this, shownChoices(params), ...rest);
        };
    }

    const _clear = Game_Message.prototype.clear;
    Game_Message.prototype.clear = function() {
        _clear.call(this);
        this._dlaTranslated = false;
        this._dlaLate = null;
    };

    // English is longer than the Japanese it replaces: wrap it to the width.
    const visible = text =>
        text.replace(/\x1b(?:([A-Z]+)(\[[^\]]*\])?|[^A-Za-z])/gi, (code, letters, brackets) =>
            letters && !brackets ? letters.slice(codeLength(letters.toUpperCase())) : ""
        );
    // A line's width, each part at the size \{ and \} set as the window draws
    // it, and the size at its end.
    // A window that draws a letter at a time gives each its own width: the
    // letters of a word measured together are closer (kerning), by a dozen
    // pixels over a line, which put a line's last letter under the border.
    // MV draws every text so, MZ the message window as it types.
    const lettersWidth = (window, text) => {
        if (IS_MZ && !(window instanceof Window_Message)) {
            return window.textWidth(text);
        }
        let width = 0;
        for (const letter of text) {
            width += window.textWidth(letter);
        }
        return width;
    };
    const measure = (window, text, size) => {
        const contents = window.contents;
        const before = contents.fontSize;
        let width = 0;
        text.split(/(\x1b[{}])/).forEach(part => {
            if (part === "\x1b{") {
                size += size <= 96 ? 12 : 0;
            } else if (part === "\x1b}") {
                size -= size >= 24 ? 12 : 0;
            } else if (part) {
                contents.fontSize = size;
                width += lettersWidth(window, visible(part));
            }
        });
        contents.fontSize = before;
        return { width, size };
    };
    // A word's letters and codes, each a piece that is never cut.
    const WORD_PIECES = /\x1b(?:[A-Z]+(?:\[[^\]]*\])?|[^A-Za-z])|[\s\S]/giu;
    const wrap = (window, text, width) => {
        const lines = [];
        let line = "";
        let size = window.contents.fontSize;
        const end = () => {
            lines.push(line);
            size = measure(window, line, size).size;
            line = "";
        };
        window
            .convertEscapeCharacters(text)
            .split(" ")
            .forEach(word => {
                const candidate = line ? `${line} ${word}` : word;
                if (measure(window, candidate, size).width <= width) {
                    line = candidate;
                    return;
                }
                if (line) {
                    end();
                }
                if (measure(window, word, size).width <= width) {
                    line = word;
                    return;
                }
                // A word wider than the line (a long cry) goes on in the
                // next, a letter at a time: nothing runs past the window.
                (word.match(WORD_PIECES) || []).forEach(piece => {
                    if (line && measure(window, line + piece, size).width > width) {
                        end();
                    }
                    line += piece;
                });
            });
        lines.push(line);
        return lines.join("\n");
    };

    const _obtainEscapeCode = Window_Base.prototype.obtainEscapeCode;
    Window_Base.prototype.obtainEscapeCode = function(textState) {
        const start = textState.index;
        const code = _obtainEscapeCode.call(this, textState);
        if (code.length < 2 || !/^[A-Z]+$/.test(code) || textState.text[textState.index] === "[") {
            return code;
        }
        const length = codeLength(code);
        textState.index = start + length;
        return code.slice(0, length);
    };

    const _startMessage = Window_Message.prototype.startMessage;
    Window_Message.prototype.startMessage = function() {
        if ($gameMessage._dlaTranslated) {
            this.resetFontSettings();
            // The window takes its place for a message as the message
            // starts, and other plugins give it another width there (one
            // for a face, one without): the lines are for that width.
            try {
                this.updatePlacement();
            } catch (error) {
                // Placed as the message starts, then.
            }
            const width = this.contentsWidth() - this.newLineX({ rtl: false }) - 8;
            $gameMessage._texts = [wrap(this, $gameMessage._texts.join(" "), width)];
        }
        _startMessage.call(this);
    };

    // Text that a window draws at once (a glossary page, a quest's notes): a
    // line too wide goes on in a new one, under the same indent, when the
    // window has the room for the lines that adds. A single line is left
    // alone in a list, where the next row is another item's.
    const wrapBlock = (window, text, width) =>
        text
            .split("\n")
            .map(line => {
                const indent = /^[ \u3000]*/.exec(line)[0];
                return wrap(window, line.slice(indent.length), width - window.textWidth(indent))
                    .split("\n")
                    .map(part => indent + part)
                    .join("\n");
            })
            .join("\n");
    const _drawTextEx = Window_Base.prototype.drawTextEx;
    // A line wider than its room, where it cannot go on in a new line (one
    // row of a list, a window with no row to spare), is drawn narrower: each
    // run of letters squeezed and moved up, so the line ends where its room
    // does. Under six tenths letters are hard to read, so a line is squeezed
    // that far and no further.
    const SQUEEZE_MOST = 0.6;
    const lineWidth = (window, line) => {
        const converted = window.convertEscapeCharacters(line);
        const icons = (converted.match(/\x1bI\[\d+\]/gi) || []).length;
        const iconWidth = (IS_MZ ? ImageManager.iconWidth : Window_Base._iconWidth) + 4;
        return measure(window, converted, window.contents.fontSize).width + icons * iconWidth;
    };
    const during = (owner, name, replacement, run) => {
        const own = Object.prototype.hasOwnProperty.call(owner, name);
        const before = owner[name];
        owner[name] = replacement(before);
        try {
            return run();
        } finally {
            if (own) {
                owner[name] = before;
            } else {
                delete owner[name];
            }
        }
    };
    const drawFitted = (window, text, x, y, width) => {
        const room = width || window.contentsWidth() - x;
        const widest = Math.max(...text.split("\n").map(line => lineWidth(window, line)));
        const scale = Math.max(SQUEEZE_MOST, room / widest);
        if (!(room > 0) || !(scale < 1)) {
            return _drawTextEx.call(window, text, x, y, width);
        }
        const moved = at => x + (at - x) * scale;
        const runs = draw =>
            function(run, runX, runY, runWidth, height, align) {
                return draw.call(this, run, moved(runX), runY, this.measureTextWidth(String(run)) * scale, height, align);
            };
        const icons = draw =>
            function(icon, iconX, iconY) {
                return draw.call(this, icon, moved(iconX), iconY);
            };
        return during(window.contents, "drawText", runs, () =>
            during(window, "drawIcon", icons, () => _drawTextEx.call(window, text, x, y, width))
        );
    };
    Window_Base.prototype.drawTextEx = function(text, x, y, width) {
        const plain = typeof text === "string" && text && !JAPANESE.test(text) && !(this instanceof Window_Message);
        if (!plain) {
            return _drawTextEx.call(this, text, x, y, width);
        }
        this.resetFontSettings();
        if (!(text.includes("\n") || !(this instanceof Window_Selectable))) {
            return drawFitted(this, text, x, y, width);
        }
        const wrapped = wrapBlock(this, text, (width || this.contentsWidth() - x) - 8);
        const rows = wrapped.split("\n").length;
        const added = rows - text.split("\n").length;
        if (added > 0 && y + rows * this.lineHeight() > this.contentsHeight()) {
            return drawFitted(this, text, x, y, width);
        }
        // A window that breaks lines itself does it a character at a time,
        // as Japanese is broken, and counts its codes as letters: these
        // lines are made for its width, and are drawn as the engine draws.
        const breaksItself =
            Window_Base.prototype.processAllText &&
            this.processAllText !== Window_Base.prototype.processAllText &&
            !Object.prototype.hasOwnProperty.call(this, "processAllText");
        if (!breaksItself) {
            return _drawTextEx.call(this, added > 0 ? wrapped : text, x, y, width);
        }
        this.processAllText = Window_Base.prototype.processAllText;
        try {
            return _drawTextEx.call(this, added > 0 ? wrapped : text, x, y, width);
        } finally {
            delete this.processAllText;
        }
    };

    // A page drawn as a picture is as wide as its longest line: the English
    // keeps to the width the Japanese had, in more lines.
    let measuring = null;
    const measuringWindow = () => {
        measuring = measuring || (IS_MZ ? new Window_Base(new Rectangle(0, 0, 0, 0)) : new Window_Base(0, 0, 0, 0));
        measuring.resetFontSettings();
        return measuring;
    };
    const pageInWidthOf = (japanese, page) => {
        const measuring = measuringWindow();
        const size = measuring.contents.fontSize;
        const width = Math.max(
            ...japanese.split("\n").map(line => measure(measuring, measuring.convertEscapeCharacters(line), size).width)
        );
        return wrapBlock(measuring, page, width);
    };
    const _command357 = Game_Interpreter.prototype.command357;
    if (_command357) {
        Game_Interpreter.prototype.command357 = function(params) {
            const found = commandText(params);
            if (!found || found.note) {
                return _command357.call(this, params);
            }
            if (waitFor(this, commandTexts(found))) {
                return false;
            }
            const translated = found.page ? composePage(found.text) : english(found.text) || found.text;
            const shown = found.page && translated !== found.text ? pageInWidthOf(found.text, translated) : translated;
            const args = Object.assign({}, params[3], { [found.name]: shown });
            return _command357.call(this, [params[0], params[1], params[2], args]);
        };
    }

    // A variable an event set to a string holds a text to show (`\V[n]`):
    // its English goes in where a window turns the code into the value. The
    // variable keeps its Japanese, which scripts may compare.
    let showingValues = 0;
    const _variableValue = Game_Variables.prototype.value;
    Game_Variables.prototype.value = function(variableId) {
        const value = _variableValue.call(this, variableId);
        if (!showingValues || !needsTranslation(value)) {
            return value;
        }
        const translated = english(value);
        if (!translated) {
            want(value, "database");
        }
        return translated || value;
    };
    const _convertEscapeCharacters = Window_Base.prototype.convertEscapeCharacters;
    Window_Base.prototype.convertEscapeCharacters = function(text) {
        showingValues++;
        try {
            return _convertEscapeCharacters.call(this, text);
        } finally {
            showingValues--;
        }
    };

    // The status pages of the CBR_EroStatus plugin: texts an event's script
    // sets, each drawn at its own place, some from the right (the numbers of
    // a column). The plugin is given the English for the time it draws. A
    // text that would run into the one after it in its row moves that one's
    // whole column on; when the page has no room for that, every text of the
    // page gets one smaller size, so rows and columns stay even.
    const STATUS_GAP = 12;
    const STATUS_SMALLEST = 0.7;
    const statusBoxes = (window, texts) =>
        texts.map(text => {
            const size = Number(text["サイズ"]) || $gameSystem.mainFontSize();
            const shown = String(text.val)
                .replace(/\\V\[(\d+)\]/g, (code, id) => $gameVariables.value(Number(id)))
                .replace(/\\[CI]\[\d+\]|\\[{}]/g, "");
            window.contents.fontSize = size;
            const width = window.textWidth(shown);
            const x = Number(text.x) || 0;
            const side = text["左右"];
            const left = side === "右" ? x - width : side === "中" ? x - width / 2 : x;
            return { text, size, width, left, right: left + width, y: Number(text.y) || 0, fromLeft: !side || side === "左" };
        });
    const fitStatusPage = (window, texts) => {
        const before = window.contents.fontSize;
        const room = window.contentsWidth();
        const moves = new Map();
        let boxes = statusBoxes(window, texts);
        boxes.forEach(box => {
            const next = boxes
                .filter(other => other !== box && Math.abs(other.y - box.y) < box.size * 0.6 && other.left > box.left)
                .sort((a, b) => a.left - b.left)[0];
            const over = box.fromLeft && next ? box.right + STATUS_GAP - next.left : 0;
            if (over > 0) {
                moves.set(next.text.x, Math.max(moves.get(next.text.x) || 0, over));
            }
        });
        texts.forEach(text => {
            if (moves.has(text.x)) {
                text.x = String(Number(text.x) + Math.ceil(moves.get(text.x)));
            }
        });
        boxes = statusBoxes(window, texts);
        const widest = Math.max(room, ...boxes.map(box => box.right));
        const scale = Math.max(STATUS_SMALLEST, room / widest);
        if (scale < 1) {
            boxes.forEach(box => {
                box.text["サイズ"] = String(Math.floor(box.size * scale));
                box.text.x = String(Math.floor(Number(box.text.x) * scale));
            });
        }
        window.contents.fontSize = before;
    };
    if (typeof Window_EroStatus === "function" && typeof CBR === "object" && CBR && CBR.eroStatus) {
        const _refreshStatus = Window_EroStatus.prototype.refresh;
        Window_EroStatus.prototype.refresh = function() {
            const page = CBR.eroStatus.data[CBR.eroStatus.pageNow];
            const texts = (page && page["テキスト"]) || [];
            const japanese = texts.map(text => ({ text, val: text.val, x: text.x, size: text["サイズ"] }));
            showingValues++;
            try {
                texts.forEach(text => {
                    const translated = english(text.val);
                    if (!translated) {
                        want(text.val, "menu");
                    }
                    text.val = String(translated || text.val).replace(/\\N\[(\d+)\]/g, (code, id) => {
                        const actor = $gameActors.actor(Number(id));
                        return actor ? actor.name() : code;
                    });
                });
                fitStatusPage(this, texts);
                return _refreshStatus.call(this);
            } finally {
                showingValues--;
                japanese.forEach(({ text, val, x, size }) => {
                    text.val = val;
                    text.x = x;
                    if (size === undefined) {
                        delete text["サイズ"];
                    } else {
                        text["サイズ"] = size;
                    }
                });
            }
        };
    }

    // QuestSystem splits a quest's notes into pages by their lines, then
    // draws a page with its lines broken at any character. The notes are
    // given in English, in lines of the window's width, for the time the
    // pages are made: the quest, and the save, keep the Japanese.
    const questWindow = typeof QuestSystem === "object" && QuestSystem && QuestSystem.Window_QuestDetail;
    const questDrawer = questWindow && QuestSystem.TextDrawer;
    if (questWindow && questDrawer && Window_Base.prototype.createTextState) {
        const questLines = (window, text, width) => wrapBlock(window, text, width - 8);
        const _prepareDrawDetail = questWindow.prototype.prepareDrawDetail;
        questWindow.prototype.prepareDrawDetail = function() {
            const quest = this._questData;
            const detail = quest && quest._detail;
            if (this._drawDetailPrepared || !needsTranslation(detail)) {
                return _prepareDrawDetail.call(this);
            }
            pageBlocks(detail).forEach(({ text }) => want(text, "database", { urgent: true }));
            const page = composePage(detail);
            quest._detail = JAPANESE.test(page) ? page : questLines(this, page, this.width - this.padding * 2 - 24);
            try {
                return _prepareDrawDetail.call(this);
            } finally {
                quest._detail = detail;
            }
        };
        const _drawTextExWrap = questDrawer.prototype.drawTextExWrap;
        questDrawer.prototype.drawTextExWrap = function(text, x, y, width) {
            if (typeof text !== "string" || !text || JAPANESE.test(text)) {
                return _drawTextExWrap.call(this, text, x, y, width);
            }
            this._window.resetFontSettings();
            const lines = questLines(this._window, text, width);
            this._window.processAllText(this._window.createTextState(lines, x, y, width));
            return lines.split("\n").length;
        };
    }

    // The skill tree (SkillTree, set up in the SkillTreeConfig script) names
    // each tree and says what it is for, and a skill of a tree may have a
    // note. The script's other names are keys its lists and events' scripts
    // link trees and skills by, so the configuration stays as it is: the
    // English is handed out where a name or a note is read.
    const skillTree = typeof SkillTreeClassAlias === "object" && SkillTreeClassAlias;
    [
        [skillTree && skillTree.SkillDataType, "message"],
        [skillTree && skillTree.SkillDataType, "helpMessage"],
        [skillTree && skillTree.SkillTreeNodeInfo, "helpMessage"],
    ].forEach(([owner, method]) => {
        const original = owner && owner.prototype[method];
        if (original) {
            owner.prototype[method] = function() {
                const text = original.call(this);
                return english(text) || text;
            };
        }
    });

    // NUUN_MenuScreenEX draws the label of the objective, and of the chapter,
    // with no width, then starts their text a set distance on: a label longer
    // than its Japanese ran under the text. Where the label needs more than
    // that distance, the text starts after the label and has that much less
    // room. The window's class cannot be reached until a menu has made one.
    const INFO_ROWS = ["nuun_DrawMenuInfoContentsDestination", "nuun_DrawMenuInfoContentsChapter"];
    const LABEL_GAP = 12;
    const keepInfoLabels = scene => {
        ((scene._windowLayer && scene._windowLayer.children) || []).forEach(window => {
            const kind = Object.getPrototypeOf(window);
            if (typeof kind[INFO_ROWS[0]] !== "function" || Object.prototype.hasOwnProperty.call(kind, "_dlaInfoRows")) {
                return;
            }
            kind._dlaInfoRows = true;
            INFO_ROWS.filter(row => typeof kind[row] === "function").forEach(row => {
                const drawRow = kind[row];
                kind[row] = function(data, x, y, width) {
                    let over = 0;
                    const label = draw =>
                        function(text, labelX, labelY, maxWidth, align) {
                            if (!maxWidth && data && data.ParamName) {
                                const needed = this.textWidth(english(text) || text) + LABEL_GAP;
                                over = Math.max(0, needed - this.systemWidth(data.SystemItemWidth, width));
                            }
                            return draw.call(this, text, labelX, labelY, maxWidth, align);
                        };
                    const value = draw =>
                        function(text, textX, textY, room) {
                            return draw.call(this, text, textX + over, textY, room - over);
                        };
                    return during(this, "drawText", label, () =>
                        during(this, "drawTextEx", value, () => drawRow.call(this, data, x, y, width))
                    );
                };
            });
            if (typeof window.refresh === "function") {
                window.refresh();
            }
        });
    };
    const _menuStart = Scene_MenuBase.prototype.start;
    Scene_MenuBase.prototype.start = function() {
        _menuStart.call(this);
        keepInfoLabels(this);
    };

    // An image with words drawn into it (a character's introduction, a map's
    // title) has no text to replace. The launcher puts English versions under
    // dla-translation/img, laid out and named as the game's own img folder,
    // and the game is handed those in their place. They are plain PNG files:
    // a game that keeps its images encrypted is told, for the moment one of
    // them starts loading, that it does not.
    const englishImages = new Set();
    const listImages = (directory, prefix) => {
        let entries = [];
        try {
            entries = fs.readdirSync(directory, { withFileTypes: true });
        } catch (error) {
            // No English images.
        }
        entries.forEach(entry => {
            if (entry.isDirectory()) {
                listImages(path.join(directory, entry.name), `${prefix}${entry.name}/`);
            } else if (entry.name.toLowerCase().endsWith(".png")) {
                englishImages.add(prefix + entry.name.slice(0, -4));
            }
        });
    };
    listImages(path.join(folder, "img"), "");
    if (englishImages.size) {
        const plain = load => {
            const owner = IS_MZ ? Utils : Decrypter;
            const before = owner.hasEncryptedImages;
            owner.hasEncryptedImages = IS_MZ ? () => false : false;
            try {
                return load();
            } finally {
                owner.hasEncryptedImages = before;
            }
        };
        const _loadBitmap = ImageManager.loadBitmap;
        ImageManager.loadBitmap = function(directory, filename, hue, smooth) {
            const name = String(directory).replace(/^img\//, "") + filename;
            if (!filename || !String(directory).startsWith("img/") || !englishImages.has(name)) {
                return _loadBitmap.apply(this, arguments);
            }
            const url = `dla-translation/img/${name.split("/").map(encodeURIComponent).join("/")}.png`;
            if (IS_MZ) {
                return plain(() => this.loadBitmapFromUrl(url));
            }
            const bitmap = plain(() => this.loadNormalBitmap(url, hue || 0));
            bitmap.smooth = smooth;
            return bitmap;
        };
        log(`images in English: ${englishImages.size}`);
    }

    // A help window has a set number of rows, two in most games, and draws
    // no more. English that needs more is put into the rows there are, in
    // lines longer than the window is wide, and each line is squeezed into
    // it where it is drawn (drawFitted): no row of a description is lost
    // until its letters would be under six tenths of their width.
    const _setHelpText = Window_Help.prototype.setText;
    Window_Help.prototype.setText = function(text) {
        if (text && !JAPANESE.test(text)) {
            this.resetFontSettings();
            const room = this.contentsWidth() - 8;
            const rows = Math.max(1, Math.floor(this.contentsHeight() / this.lineHeight()));
            const needed = wrapBlock(this, text, room).split("\n").length;
            if (needed > rows) {
                const flat = text.replace(/\s*\n\s*/g, " ");
                for (let wider = 1.05; wider <= 1 / SQUEEZE_MOST + 0.001; wider += 0.05) {
                    text = wrap(this, flat, room * wider);
                    if (text.split("\n").length <= rows) {
                        break;
                    }
                }
            } else if (!text.includes("\n")) {
                text = wrap(this, text, room);
            }
        }
        _setHelpText.call(this, text);
    };

    // The catch-all: Japanese that other plugins draw from their own settings
    // or commands (a menu command, a hint in a corner). A known translation is
    // drawn instead; a new one is asked for and shows the next time the text
    // is drawn. The message window is left alone: it draws its text a few
    // letters at a time as it types it out.
    let drawingMessage = 0;
    const _messageUpdate = Window_Message.prototype.update;
    Window_Message.prototype.update = function() {
        drawingMessage++;
        try {
            _messageUpdate.call(this);
        } finally {
            drawingMessage--;
        }
    };
    const _drawText = Bitmap.prototype.drawText;
    Bitmap.prototype.drawText = function(text, x, y, maxWidth, lineHeight, align) {
        if (!drawingMessage && typeof text === "string" && text.length > 1 && JAPANESE.test(text)) {
            const translated = english(text);
            if (translated) {
                text = translated;
            } else {
                want(text, "addon");
            }
        }
        return _drawText.call(this, text, x, y, maxWidth, lineHeight, align);
    };

    // Menu words and the database: every Japanese string, with where it
    // lives, so a translation is put in place the moment it arrives.
    const places = new Map();
    const place = (owner, key) => {
        const text = owner[key];
        if (!needsTranslation(text)) {
            return null;
        }
        if (!places.has(text)) {
            places.set(text, []);
        }
        places.get(text).push([owner, key]);
        return text;
    };
    const applyDatabase = text => {
        const translated = english(text);
        if (translated) {
            (places.get(text) || []).forEach(([owner, key]) => (owner[key] = translated));
            (pages.get(text) || []).forEach(page => (page.owner[page.key] = shownPage(page)));
        }
    };
    // A glossary page is a title and paragraphs, each under its own indent
    // and apart by empty lines. Each is translated on its own, as one
    // paragraph, and the page is put together with the lines and indents it
    // had: translated whole it would come back as one paragraph.
    const INDENT = /^[ \u3000]*/;
    const pageBlocks = original => {
        const lines = original.split("\n");
        const blocks = [];
        for (let at = 0; at < lines.length; ) {
            const indent = INDENT.exec(lines[at])[0];
            let end = at + 1;
            if (indent.length < lines[at].length) {
                while (
                    end < lines.length &&
                    INDENT.exec(lines[end])[0] === indent &&
                    indent.length < lines[end].length
                ) {
                    end++;
                }
            }
            const own = lines.slice(at, end);
            blocks.push({ indent, lines: own, text: own.map(line => line.slice(indent.length)).join("\n") });
            at = end;
        }
        return blocks;
    };
    const composePage = original =>
        pageBlocks(original)
            .map(block => {
                const translated = english(block.text);
                return translated ? block.indent + translated : block.lines.join("\n");
            })
            .join("\n");
    // A page a plugin draws a line at a time, each squeezed into its width,
    // is put into lines of that width here. Measuring takes a window, and a
    // window takes the game's objects: the database is read before they
    // exist, so these pages are put into lines once the game has made them.
    // The window has shown it has the rows of the longest of these pages
    // in Japanese: English that would take more gets wider lines instead, up
    // to half as wide again, and the plugin squeezes them into its width.
    const widePages = [];
    const rowsOf = page => page.split("\n").filter(Boolean).length;
    const shownPage = page => {
        const composed = composePage(page.original);
        if (!page.width || !$gameSystem || JAPANESE.test(composed)) {
            return composed;
        }
        const window = measuringWindow();
        const room = Math.max(...widePages.map(other => rowsOf(other.original)));
        let wrapped = wrapBlock(window, composed, page.width);
        for (let wider = 1.1; rowsOf(wrapped) > room && wider <= 1.5; wider += 0.1) {
            wrapped = wrapBlock(window, composed, page.width * wider);
        }
        return wrapped;
    };
    const pages = new Map();
    const rememberPage = (owner, key, width = 0) => {
        const original = owner[key];
        if (!needsTranslation(original)) {
            return;
        }
        const page = { owner, key, original, width };
        if (width) {
            widePages.push(page);
        }
        pageBlocks(original).forEach(({ text }) => {
            if (needsTranslation(text)) {
                if (!pages.has(text)) {
                    pages.set(text, []);
                }
                pages.get(text).push(page);
                want(text, "database");
            }
        });
        owner[key] = shownPage(page);
    };
    const remember = (owner, key, kind) => {
        const text = place(owner, key);
        if (text && settled(text)) {
            applyDatabase(text);
        } else if (text) {
            want(text, kind);
        }
    };
    // Actor names join the name list, so dialogue spells them the same way.
    const rememberName = (owner, key) => {
        const text = place(owner, key);
        if (text && text in store.names) {
            applyDatabase(text);
        } else if (text) {
            wantName(text);
        }
    };

    const DESCRIPTION_TAGS = ["dPlnText", "dText"];
    // The enemy book (NUUN_EnemyBook) draws a note's text under the tags its
    // page settings name for a description (entry 70). The same setting names
    // a tag of picture files for other entries, which stays as it is.
    const ENEMY_BOOK_DESCRIPTION = 70;
    const enemyBookTags = () => {
        const tags = new Set();
        const parsed = value => {
            try {
                return typeof value === "string" ? JSON.parse(value) : value;
            } catch (error) {
                return null;
            }
        };
        Object.values(PluginManager.parameters("NUUN_EnemyBook")).forEach(setting => {
            const list = parsed(setting);
            (Array.isArray(list) ? list : []).forEach(held => {
                const entry = parsed(held);
                if (entry && Number(entry.DateSelect) === ENEMY_BOOK_DESCRIPTION && entry.textMethod) {
                    tags.add(String(entry.textMethod));
                }
            });
        });
        return tags;
    };
    const GLOSSARY_TEXT = /^SG(?:説明|Description|共通説明|CommonDescription)\d*$/;
    const GLOSSARY_CATEGORY = /^SG(?:カテゴリ|Category)$/;
    const rememberDatabase = () => {
        const terms = $dataSystem.terms;
        ["basic", "commands", "params"].forEach(group =>
            (terms[group] || []).forEach((value, index) => remember(terms[group], index, "menu"))
        );
        Object.keys(terms.messages || {}).forEach(key => remember(terms.messages, key, "menu"));
        ["equipTypes", "skillTypes", "weaponTypes", "armorTypes", "elements"].forEach(group =>
            ($dataSystem[group] || []).forEach((value, index) => remember($dataSystem[group], index, "menu"))
        );
        remember($dataSystem, "currencyUnit", "menu");
        remember($dataSystem, "gameTitle", "menu");
        ($dataActors || []).forEach(actor => actor && rememberName(actor, "name"));
        const fields = ["name", "nickname", "profile", "description", "message1", "message2", "message3", "message4"];
        [$dataActors, $dataClasses, $dataItems, $dataWeapons, $dataArmors, $dataSkills, $dataStates, $dataEnemies].forEach(
            table =>
                (table || []).forEach(
                    row =>
                        row &&
                        fields.forEach(field => {
                            if (!(table === $dataActors && field === "name")) {
                                remember(row, field, "database");
                            }
                        })
                )
        );
        // A glossary (the SceneGlossary plugin) shows what a row's note says
        // under these tags: the engine has read them into the row's meta.
        [$dataItems, $dataWeapons, $dataArmors].forEach(table =>
            (table || []).forEach(row =>
                Object.keys((row && row.meta) || {}).forEach(tag => {
                    if (GLOSSARY_TEXT.test(tag)) {
                        rememberPage(row.meta, tag);
                    } else if (GLOSSARY_CATEGORY.test(tag) && typeof row.meta[tag] === "string") {
                        // The glossary compares these names: they stay as
                        // they are, and their English is drawn.
                        row.meta[tag].split(",").forEach(category => want(category, "menu"));
                    }
                })
            )
        );
        // The extended descriptions of TMDescriptionEx: a row's note holds
        // them, and each line is drawn squeezed into the right pane.
        const descriptions = typeof TMPlugin === "object" && TMPlugin && TMPlugin.DescriptionEx;
        if (descriptions) {
            const width = Number(descriptions.RightPaneWidth) || 0;
            [$dataItems, $dataWeapons, $dataArmors, $dataSkills].forEach(table =>
                (table || []).forEach(row =>
                    DESCRIPTION_TAGS.forEach(tag => row && row.meta && rememberPage(row.meta, tag, width))
                )
            );
        }
        const bookTags = enemyBookTags();
        ($dataEnemies || []).forEach(row => bookTags.forEach(tag => row && row.meta && rememberPage(row.meta, tag)));
        if (skillTree && typeof loadSkillTreeConfig === "function") {
            const config = loadSkillTreeConfig() || {};
            (config.skillTreeTypes || []).forEach(actor =>
                (actor.types || []).forEach(type => {
                    want(type[1], "menu");
                    want(type[2], "database");
                })
            );
            Object.values(config.skillTreeInfo || {}).forEach(skill => want(skill[4], "database"));
        }
        // Other plugins' text that DLA_TranslateSettings found untranslated; used from the next start.
        (window.DLA_ADDON_TEXTS || []).forEach(text => want(text, "addon"));
        (window.DLA_ADDON_PAGES || []).forEach(page => pageBlocks(page).forEach(({ text }) => want(text, "database")));
        log(`menus and database: ${places.size + pages.size} texts, ${pending.size} waiting`);
    };

    const _bootStart = Scene_Boot.prototype.start;
    Scene_Boot.prototype.start = function() {
        rememberDatabase();
        _bootStart.call(this);
        widePages.forEach(page => (page.owner[page.key] = shownPage(page)));
    };

    // Actors copy their names into the save at New Game; read them through the translation.
    ["name", "nickname", "profile"].forEach(method => {
        const original = Game_Actor.prototype[method];
        Game_Actor.prototype[method] = function() {
            const text = original.call(this);
            return english(text) || text;
        };
    });

    // Dialogue of a map is queued when it loads, nearest events first.
    const _onMapLoaded = Scene_Map.prototype.onMapLoaded;
    Scene_Map.prototype.onMapLoaded = function() {
        _onMapLoaded.call(this);
        const mapId = $gameMap.mapId();
        want($dataMap.displayName, "menu");
        ($dataMap.events || []).forEach(event =>
            (event ? event.pages : []).forEach(page => queueList(page.list, 0, { pos: { mapId, x: event.x, y: event.y } }))
        );
        log(`map ${mapId}: ${pending.size} texts waiting`);
    };

    // A small notice while the game waits, and while menu words are translated.
    const statusText = () => {
        if (status.waiting) {
            return "Translating…";
        }
        if (!relay || !(SceneManager._scene instanceof Scene_Title)) {
            return "";
        }
        const menu = [...pending.values()].filter(entry => TIER[entry.kind] === 1).length;
        return menu ? `Menus: ${menu} left` : "";
    };
    class Window_DlaStatus extends Window_Base {
        update() {
            super.update();
            const text = statusText();
            if (text !== this._shown) {
                this._shown = text;
                this.visible = !!text;
                this.contents.clear();
                if (text) {
                    this.drawText(text, 0, 0, this.contentsWidth(), "center");
                    placeStatus(this);
                }
            }
        }
    }
    // A corner none of the game's windows uses: games move their own (a
    // title menu in the top right, a map name in the top left).
    const overlaps = (a, b) =>
        a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
    const placeStatus = status => {
        const layer = SceneManager._scene && SceneManager._scene._windowLayer;
        const others = (layer ? layer.children : []).filter(window => window !== status && window.visible);
        const right = Graphics.boxWidth - status.width;
        const bottom = Graphics.boxHeight - status.height;
        const corners = [
            { x: right, y: 0 },
            { x: 0, y: 0 },
            { x: right, y: bottom },
            { x: 0, y: bottom }
        ];
        const free =
            corners.find(corner =>
                others.every(window => !overlaps(Object.assign({ width: status.width, height: status.height }, corner), window))
            ) || corners[0];
        status.move(free.x, free.y, status.width, status.height);
    };
    const addStatus = scene => {
        const width = 260;
        const height = Window_Base.prototype.fittingHeight(1);
        const x = Graphics.boxWidth - width;
        const status = IS_MZ
            ? new Window_DlaStatus(new Rectangle(x, 0, width, height))
            : new Window_DlaStatus(x, 0, width, height);
        status.visible = false;
        scene.addWindow(status);
    };
    const _createAllWindows = Scene_Map.prototype.createAllWindows;
    Scene_Map.prototype.createAllWindows = function() {
        _createAllWindows.call(this);
        addStatus(this);
    };
    const _titleCreate = Scene_Title.prototype.create;
    Scene_Title.prototype.create = function() {
        _titleCreate.call(this);
        addStatus(this);
    };
})();
