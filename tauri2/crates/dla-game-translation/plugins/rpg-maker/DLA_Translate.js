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
    let saveTimer = null;
    const save = () => {
        clearTimeout(saveTimer);
        saveTimer = setTimeout(() => {
            const file = path.join(folder, "translations.json");
            try {
                fs.writeFileSync(`${file}.tmp`, JSON.stringify({ version: 1, texts: store.texts, names: store.names }));
                fs.renameSync(`${file}.tmp`, file);
            } catch (error) {
                log(`could not save translations: ${error.message}`);
            }
        }, 2000);
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

    const choicesAfter = (list, index) => {
        let i = index + 1;
        while (list[i] && list[i].code === 401) {
            i++;
        }
        return list[i] && list[i].code === 102 ? list[i].parameters[0] : [];
    };

    const queueList = (list, from, options) => {
        for (let i = from; i < (list || []).length; i++) {
            const command = list[i];
            if (command.code === 101) {
                wantName(command.parameters[4]);
                want(messageAt(list, i), "dialogue", options);
            } else if (command.code === 102) {
                command.parameters[0].forEach(choice => want(choice, "choice", options));
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
        if (late.speaker && english(late.speaker)) {
            $gameMessage.setSpeakerName(english(late.speaker));
        }
        window.pause = false;
        window.startMessage();
        log("swapped in English that came late");
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
        const result = _command101.call(this, params);
        const translated = english(text);
        if (translated) {
            $gameMessage._texts = [translated];
            $gameMessage._dlaTranslated = true;
        } else {
            $gameMessage._dlaLate = { text, speaker };
        }
        if (speaker && english(speaker)) {
            $gameMessage.setSpeakerName(english(speaker));
        }
        return result;
    };

    const _command102 = Game_Interpreter.prototype.command102;
    Game_Interpreter.prototype.command102 = function(params) {
        if ($gameMessage.isBusy() || waitFor(this, (params || this._params)[0])) {
            return false;
        }
        return _command102.call(this, params);
    };

    const _setupChoices = Game_Interpreter.prototype.setupChoices;
    Game_Interpreter.prototype.setupChoices = function(params) {
        _setupChoices.call(this, [params[0].map(choice => english(choice) || choice), ...params.slice(1)]);
    };

    const _clear = Game_Message.prototype.clear;
    Game_Message.prototype.clear = function() {
        _clear.call(this);
        this._dlaTranslated = false;
        this._dlaLate = null;
    };

    // English is longer than the Japanese it replaces: wrap it to the width.
    const visible = text => text.replace(/\x1b(?:[A-Z]+(?:\[[^\]]*\])?|[^A-Za-z])/gi, "");
    // A line's width, each part at the size \{ and \} set as the window draws
    // it, and the size at its end.
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
                width += window.textWidth(visible(part));
            }
        });
        contents.fontSize = before;
        return { width, size };
    };
    const wrap = (window, text, width) => {
        const lines = [];
        let line = "";
        let size = window.contents.fontSize;
        window
            .convertEscapeCharacters(text)
            .split(" ")
            .forEach(word => {
                const candidate = line ? `${line} ${word}` : word;
                if (line && measure(window, candidate, size).width > width) {
                    lines.push(line);
                    size = measure(window, line, size).size;
                    line = word;
                } else {
                    line = candidate;
                }
            });
        lines.push(line);
        return lines.join("\n");
    };

    const _startMessage = Window_Message.prototype.startMessage;
    Window_Message.prototype.startMessage = function() {
        if ($gameMessage._dlaTranslated) {
            this.resetFontSettings();
            const width = this.contentsWidth() - this.newLineX({ rtl: false }) - 8;
            $gameMessage._texts = [wrap(this, $gameMessage._texts.join(" "), width)];
        }
        _startMessage.call(this);
    };

    const _setHelpText = Window_Help.prototype.setText;
    Window_Help.prototype.setText = function(text) {
        if (text && !JAPANESE.test(text) && !text.includes("\n")) {
            this.resetFontSettings();
            text = wrap(this, text, this.contentsWidth() - 8);
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
        }
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
        // Other plugins' text that DLA_TranslateSettings found untranslated; used from the next start.
        (window.DLA_ADDON_TEXTS || []).forEach(text => want(text, "addon"));
        log(`menus and database: ${places.size} texts, ${pending.size} waiting`);
    };

    const _bootStart = Scene_Boot.prototype.start;
    Scene_Boot.prototype.start = function() {
        rememberDatabase();
        _bootStart.call(this);
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
