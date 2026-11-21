const assert = require("node:assert/strict");
const { createHash } = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

const source = fs.readFileSync(path.join(__dirname, "DLA_Translate.js"), "utf8");
const start = source.indexOf("    const swapLate =");
const end = source.indexOf("    const _command102 =", start);
assert(start > 0 && end > start);
const key = text => createHash("sha256").update(text).digest("hex").slice(0, 32);

function setup({ translated = true, saved = false } = {}) {
    const japanese = "元の台詞";
    const speaker = "アキ";
    const list = [
        { code: 101, parameters: ["", 0, 0, 2, speaker] },
        { code: 401, parameters: [japanese] },
        { code: 0, parameters: [] },
    ];
    const fix = { map: 1, event: 2, page: 0, command: 0, key: key(japanese), expected_speaker: key(speaker), text: "Yuki" };
    const message = {
        _texts: [],
        isBusy: () => false,
        setSpeakerName(name) { this.speaker = name; },
    };
    function Interpreter() {
        this._mapId = 1;
        this._eventId = 2;
        this._index = 0;
        this._list = saved ? structuredClone(list) : list;
    }
    Interpreter.prototype.command101 = function (params) {
        message.speaker = params[4];
        this._index++;
        return true;
    };
    const english = new Map([[speaker, "Aki"]]);
    if (translated) english.set(japanese, "The translated line.");
    const window = { isOpen: () => true, isAnySubWindowActive: () => false, startMessage() { this.started = true; } };
    const context = {
        require,
        IS_MZ: true,
        speakerOverrides: [fix],
        textKey: key,
        Game_Interpreter: Interpreter,
        $gameMessage: message,
        $dataMap: { events: [null, null, { pages: [{ list }] }] },
        messageAt: (commands, index) => commands.slice(index + 1).filter(command => command.code === 401).map(command => command.parameters[0]).join("\n"),
        choicesAfter: () => [],
        waitFor: () => false,
        english: text => english.get(text),
        SceneManager: { _scene: { _messageWindow: window } },
        log: () => {},
    };
    vm.runInNewContext(source.slice(start, end) + "\nglobalThis.swapLate = swapLate;", context);
    const interpreter = new Interpreter();
    return { context, interpreter, fix, list, message, english, window, japanese, speaker,
        run: () => interpreter.command101(interpreter._list[interpreter._index].parameters) };
}

test("corrects one message before the engine advances its command index", () => {
    const state = setup();
    assert.equal(state.run(), true);
    assert.equal(state.interpreter._index, 1);
    assert.equal(state.message.speaker, "Yuki");
    assert.equal(state.message._texts[0], "The translated line.");
    assert.equal(state.list[0].parameters[4], state.speaker);
});

test("matches a message restored from a saved interpreter", () => {
    const state = setup({ saved: true });
    state.run();
    assert.equal(state.message.speaker, "Yuki");
});

for (const field of ["map", "event", "page", "command", "key", "expected_speaker"]) {
    test(`does not apply when ${field} differs`, () => {
        const state = setup();
        state.fix[field] = typeof state.fix[field] === "number" ? 9 : "0".repeat(32);
        state.run();
        assert.equal(state.message.speaker, "Aki");
    });
}

test("does not affect a common event with the same message and event number", () => {
    const state = setup({ saved: true });
    state.interpreter._list.push({ code: 0, parameters: [] });
    state.run();
    assert.equal(state.message.speaker, "Aki");
});

test("keeps the correction when the translated body arrives late", () => {
    const state = setup({ translated: false });
    state.run();
    state.message.speaker = "Aki";
    state.english.set(state.japanese, "The translated line.");
    state.context.swapLate();
    assert.equal(state.message.speaker, "Yuki");
    assert.equal(state.message._texts[0], "The translated line.");
    assert.equal(state.window.started, true);
});

test("keeps the corrections when the plugin saves its own copy", () => {
    const begin = source.indexOf('    const saved = read("translations.json")');
    const finish = source.indexOf("    // A code of letters with nothing in brackets", begin);
    assert(begin > 0 && finish > begin);
    const fix = { map: 1, event: 2, page: 0, command: 0, key: key("元の台詞"), expected_speaker: key("アキ"), text: "Yuki" };
    const written = {};
    const context = {
        require,
        path,
        folder: "/game/dla-translation",
        read: () => ({ texts: { "はい": "Yes" }, names: {}, speakers: [fix] }),
        fs: {
            writeFileSync: (file, content) => { written[file] = content; },
            renameSync: (from, to) => { written[to] = written[from]; },
        },
        setTimeout: run => run(),
        clearTimeout: () => {},
        log: () => {},
    };
    vm.runInNewContext(source.slice(begin, finish) + "\nstore.texts['いいえ'] = 'No'; save();", context);
    const copy = JSON.parse(written[path.join("/game/dla-translation", "translations.json")]);
    assert.deepEqual(copy.speakers, [fix]);
    assert.deepEqual(copy.texts, { "はい": "Yes", "いいえ": "No" });
});
