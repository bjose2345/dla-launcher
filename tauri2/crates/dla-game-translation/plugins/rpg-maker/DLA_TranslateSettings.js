/*:
 * @target MZ
 * @plugindesc Machine translation from DLA Launcher: other plugins' settings. Must be first.
 * @author DLA
 * @url https://github.com/bjose2345/dla-launcher
 *
 * @help
 * Added by DLA Launcher together with DLA_Translate, and removed with it.
 *
 * Other plugins read their settings once, as they load, so this plugin goes
 * first in the list. It changes only settings that a rule below names as
 * text the player sees; everything else (button names, file names, switches)
 * is left alone. Settings not translated yet are handed to DLA_Translate,
 * which translates them for the next start.
 */
(() => {
    "use strict";
    if (typeof require !== "function") {
        return;
    }
    // Player-visible settings of known plugins. A path goes into settings that
    // hold JSON (structs and lists), e.g. "messages.0.text".
    const RULES = {
        TorigoyaMZ_NotifyMessage_AddonGetItem: [
            "baseGainSingleMessage",
            "baseGainMultiMessage",
            "baseGainMoneyMessage"
        ]
    };

    const fs = require("fs");
    const path = require("path");
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
    const texts = (read("translations.json") || {}).texts || {};

    const JAPANESE = /[぀-ヿ㐀-鿿ｦ-ﾟ]/;
    const missing = [];

    // Reads or replaces the text at a path, following settings that hold JSON.
    const visit = (owner, keys, replace) => {
        const [key, ...rest] = keys;
        const value = owner[key];
        if (rest.length === 0) {
            if (typeof value === "string" && JAPANESE.test(value)) {
                replace(owner, key, value);
            }
            return;
        }
        let inner = value;
        const encoded = typeof value === "string";
        if (encoded) {
            try {
                inner = JSON.parse(value);
            } catch (error) {
                return;
            }
        }
        if (inner && typeof inner === "object") {
            visit(inner, rest, replace);
            if (encoded) {
                owner[key] = JSON.stringify(inner);
            }
        }
    };

    for (const [plugin, paths] of Object.entries(RULES)) {
        const settings = PluginManager.parameters(plugin);
        paths.forEach(setting =>
            visit(settings, setting.split("."), (owner, key, text) => {
                if (texts[text]) {
                    owner[key] = texts[text];
                } else {
                    missing.push(text);
                }
            })
        );
    }
    window.DLA_ADDON_TEXTS = missing;
})();
