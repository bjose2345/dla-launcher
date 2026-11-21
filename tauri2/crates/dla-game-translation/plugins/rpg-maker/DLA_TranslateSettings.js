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
    // hold JSON (structs and lists), e.g. "messages.0.text"; `*` is every entry
    // of a list or a struct. "list:" marks words apart by commas, each
    // translated on its own. "page:" marks a page of several paragraphs, left
    // as it is here: DLA_Translate draws its English. Names a plugin compares
    // (a key, a category, an id) have no rule.
    // This list is JSON: the launcher reads it from this file, to translate
    // these settings ahead of play.
    const RULES = {
        "ARTM_TMDescriptionExMZ": [
            "secretItemA", "secretItemB", "consumableText", "occasionText", "scopeText", "speedText",
            "successRateText", "repeatsText", "tpGainText", "hitTypeText", "priceText", "mpCostText",
            "tpCostText", "requiredWtypeText", "effectText", "traitText", "plainText",
            "effectTextRecoverHp", "effectTextRecoverMp", "effectTextGainTp", "effectTextAddState",
            "effectTextRemoveState", "effectTextAddBuff", "effectTextAddDebuff", "effectTextRemoveBuff",
            "effectTextRemoveDebuff", "effectTextSpecial", "effectTextGrow", "effectTextLearnSkill",
            "damageTextDamageHp", "damageTextDamageMp", "damageTextRecoverHp", "damageTextRecoverMp",
            "damageTextDrainHp", "damageTextDrainMp", "traitTextElementRate", "traitTextDebuffRate",
            "traitTextStateRate", "traitTextStateResist", "traitTextAttackElement", "traitTextAttackState",
            "traitTextAttackSpeed", "traitTextAttackTimes", "traitTextStypeAdd", "traitTextStypeSeal",
            "traitTextSkillAdd", "traitTextSkillSeal", "traitTextEquipWtype", "traitTextEquipAtype",
            "traitTextEquipSeal", "traitTextActionPlus", "elementFooter", "costExTextHp", "costExTextItem",
            "costExTextExp", "costExTextGold", "passiveAlwaysText", "passiveTpText", "passiveTpText2",
            "passiveStateText", "list:xparamText", "list:sparamText", "list:consumableValue",
            "list:occasionValue", "list:scopeValue", "list:hitTypeValue", "list:slotTypeValue",
            "list:specialFlagValue", "list:partyAbilityValue"
        ],
        "AlchemySystem": ["MenuAlchemyText", "NeedMaterialText", "NeedPriceText", "TargetItemText"],
        "AutoBattle": ["PartyCommandName"],
        "CustomizeConfigItem": ["SwitchOptions.*.Name", "SwitchOptions.*.OnText", "SwitchOptions.*.OffText"],
        "DarkPlasma_TextLog": ["choiceFormat", "choiceCancelText"],
        "FilterControllerMZ": ["enabledAll-Text"],
        "KEN_BattleStateInformation": ["states.*.description", "partyCommandText", "turnText", "stateNothingText"],
        "MPP_DuplicateName": ["Duplicate Message", "Error Message", "Prohibited Message"],
        "MenuSubCommand": ["subCommands.*.Name", "subCommands.*.ParentName"],
        "NUUN_Chapter": ["ChapterList.*.ChapterText"],
        "NUUN_Destination": ["DestinationList.*.DestinationText"],
        "NUUN_EnemyBook": [
            "CommandName", "EnemyInfoCommandName", "EnemyBookCategory.*.CategoryName",
            "PercentContent.*.ContentName", "PageSetting.*.PageCategoryName",
            "AnalyzeSkillMode.*.AnalyzeMissMessage", "*.*.paramName"
        ],
        "NUUN_ExpItem": ["ExpMessageUp", "ExpMessageDown", "ExpMessageNoEffect"],
        "NUUN_MenuScreenEX": [
            "MenuLayout.*.*.*.ParamName", "HelpList.*.HelpCommandName", "HelpList.*.HelpCommandText"
        ],
        "NUUN_StatusScreen": ["*.*.ParamName"],
        "NUUN_popUp": [
            "AddBuffPopUpText", "AddDebuffPopUpText", "RemovedBuffPopUpText", "RemovedDebuffPopUpText"
        ],
        "QuestSystem": ["QuestDatas.*.Title", "page:QuestDatas.*.DetailNote", "Text.*"],
        "SealActorCommand": ["disableSign"],
        "ShopScene_Extension": ["NoneItemText"],
        "SkillTree": [
            "MenuSkillTreeText", "NeedSpText", "OpenedNodeText", "NodeOpenConfirmationText",
            "NodeOpenYesText", "NodeOpenNoText", "BattleEndGetSpText", "LevelUpGetSpText"
        ],
        "TorigoyaMZ_Achievement2": [
            "baseAchievementData.*.title", "baseAchievementData.*.description",
            "baseAchievementData.*.hint", "popupMessage", "titleMenuText"
        ],
        "TorigoyaMZ_NotifyMessage_AddonGetItem": [
            "baseGainSingleMessage", "baseGainMultiMessage", "baseGainMoneyMessage"
        ]
    };

    // Words a plugin's own code draws where a setting leaves a label empty.
    // They are not settings, so none is changed here: DLA_Translate draws
    // their English. This list is JSON as well, read by the launcher too.
    const DEFAULTS = {
        "NUUN_EnemyBook": [
            "会心率", "会心回避率", "魔法回避率", "魔法反射率", "反撃率", "HP再生率", "MP再生率", "TP再生率", "狙われ率", "防御効果率",
            "回復効果率", "薬の知識", "MP消費率", "TPチャージ率", "物理ダメージ率", "魔法ダメージ率", "床ダメージ率", "獲得経験率", "獲得金額",
            "倒した数", "ターン", "←→キー：ページ切り替え", "耐性属性", "弱点属性", "無効属性", "耐性ステート", "弱点ステート", "無効ステート",
            "弱点デバフ", "耐性デバフ", "ドロップアイテム", "盗めるアイテム", "条件ドロップアイテム", "使用スキル", "属性耐性", "ステート耐性"
        ],
        "NUUN_MenuScreenEX": [
            "会心率", "会心回避率", "魔法回避率", "魔法反射率", "反撃率", "HP再生率", "MP再生率", "TP再生率", "狙われ率", "防御効果率",
            "回復効果率", "薬の知識", "MP消費率", "TPチャージ率", "物理ダメージ率", "魔法ダメージ率", "床ダメージ率", "獲得経験率"
        ],
        "NUUN_StatusScreen": [
            "会心率", "会心回避率", "魔法回避率", "魔法反射率", "反撃率", "HP再生率", "MP再生率", "TP再生率", "狙われ率", "防御効果率",
            "回復効果率", "薬の知識", "MP消費率", "TPチャージ率", "物理ダメージ率", "魔法ダメージ率", "床ダメージ率", "獲得経験率",
            "QWキー:キャラの切替", "ΛVボタン / QWキー:キャラの切替", "←→キー:項目の切替", "<>ボタン / ←→キー:項目の切替"
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
    const pages = [];

    // A setting may hold its text as a string of code: "…" (a note) or '…'.
    const literal = value => {
        if (value.startsWith('"')) {
            try {
                const text = JSON.parse(value);
                if (typeof text === "string") {
                    return { text, held: JSON.stringify };
                }
            } catch (error) {
                // Text that starts with a quote, then.
            }
        }
        if (/^'[^'\\]*'$/.test(value)) {
            return {
                text: value.slice(1, -1),
                held: text => `'${text.replace(/\\/g, "\\\\").replace(/'/g, "\\'")}'`
            };
        }
        return { text: value, held: text => text };
    };

    // What a plugin fills in (`%1`, `{choice}`): English without one of them
    // would lose what goes there, so the Japanese stays.
    const FILLED_IN = /%\d+|\{[A-Za-z]+\}/g;
    const english = text => {
        if (!JAPANESE.test(text)) {
            return null;
        }
        const translated = texts[text];
        if (!translated) {
            missing.push(text);
            return null;
        }
        return (text.match(FILLED_IN) || []).every(part => translated.includes(part)) ? translated : null;
    };
    const SHAPES = {
        text: value => english(value),
        list: value => {
            const words = value.split(",");
            const shown = words.map(word => {
                const translated = english(word);
                return translated ? translated.replace(/,/g, " ") : word;
            });
            return shown.some((word, index) => word !== words[index]) ? shown.join(",") : null;
        },
        page: value => {
            if (JAPANESE.test(value)) {
                pages.push(value);
            }
            return null;
        }
    };

    // Puts the English in at a path, following settings that hold JSON.
    // Answers whether anything changed, so only such settings are written.
    const visit = (owner, keys, shape) => {
        const [key, ...rest] = keys;
        let changed = false;
        (key === "*" ? Object.keys(owner) : [key]).forEach(name => {
            const value = owner[name];
            if (rest.length === 0) {
                if (typeof value === "string") {
                    const { text, held } = literal(value);
                    const shown = shape(text);
                    if (shown) {
                        owner[name] = held(shown);
                        changed = true;
                    }
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
            if (inner && typeof inner === "object" && visit(inner, rest, shape)) {
                if (encoded) {
                    owner[name] = JSON.stringify(inner);
                }
                changed = true;
            }
        });
        return changed;
    };

    for (const [plugin, paths] of Object.entries(RULES)) {
        const settings = PluginManager.parameters(plugin);
        paths.forEach(rule => {
            const marked = /^(list|page):(.+)$/.exec(rule);
            visit(settings, (marked ? marked[2] : rule).split("."), SHAPES[marked ? marked[1] : "text"]);
        });
    }
    for (const [plugin, words] of Object.entries(DEFAULTS)) {
        if ($plugins.some(listed => listed.name === plugin && listed.status)) {
            words.filter(word => !texts[word]).forEach(word => missing.push(word));
        }
    }
    window.DLA_ADDON_TEXTS = missing;
    window.DLA_ADDON_PAGES = pages;
})();
