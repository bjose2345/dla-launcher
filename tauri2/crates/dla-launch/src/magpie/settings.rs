//! The settings file the launcher writes for Magpie: one profile and one
//! scaling mode for each game being shown.

use dla_domain::launch::{PictureFilter, Sharpness, WindowScale};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Size {
    pub width: u32,
    pub height: u32,
}

/// A game's window as Windows reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GameWindow {
    /// The program's path as `QueryFullProcessImageNameW` gives it, which is
    /// what Magpie compares a profile's path with.
    pub program: String,
    pub class: String,
    pub picture: Size,
    pub screen: Size,
}

/// What the player chose for a game shown through Magpie.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Shown {
    pub scale: WindowScale,
    pub picture_filter: Option<PictureFilter>,
    pub cursor_hide_delay: Option<u32>,
}

// Magpie's ScalingType and AutoScale, and the "custom" entry of its
// InitialWindowedScaleFactor.
const SCALE_BY_FACTOR: u32 = 0;
const SCALE_TO_FIT: u32 = 1;
const SCALE_TO_FILL: u32 = 3;
const AUTO_SCALE_FULL_SCREEN: u32 = 1;
const AUTO_SCALE_WINDOWED: u32 = 2;
const CUSTOM_WINDOW_FACTOR: u32 = 6;

/// Magpie's settings with one profile and one scaling mode for each game
/// being shown. The first profile is Magpie's default one, left as it is;
/// everything not written here keeps Magpie's own default, its toolbar too.
pub(crate) fn settings(games: &[(GameWindow, Shown)]) -> String {
    let name = |index: usize| format!("DLA Launcher {}", index + 1);
    let modes = games
        .iter()
        .enumerate()
        .map(|(index, (window, shown))| {
            json!({ "name": name(index), "effects": effects(window, *shown) })
        })
        .collect::<Vec<_>>();
    let profiles = std::iter::once(json!({}))
        .chain(games.iter().enumerate().map(|(index, (window, shown))| {
            let mut profile = json!({
                "name": name(index),
                "packaged": false,
                "pathRule": window.program,
                "classNameRule": class_rule(&window.class),
                "scalingMode": index,
            });
            match shown.scale {
                WindowScale::Window { width, height } => {
                    profile["autoScale"] = json!(AUTO_SCALE_WINDOWED);
                    profile["initialWindowedScaleFactor"] = json!(CUSTOM_WINDOW_FACTOR);
                    profile["customInitialWindowedScaleFactor"] =
                        json!(window_factor(window.picture, width, height));
                }
                _ => profile["autoScale"] = json!(AUTO_SCALE_FULL_SCREEN),
            }
            if let Some(delay) = shown.cursor_hide_delay {
                profile["autoHideCursorEnabled"] = json!(true);
                // Magpie takes a tenth of a second to five seconds.
                profile["autoHideCursorDelay"] = json!((f64::from(delay) / 1000.0).clamp(0.1, 5.0));
            }
            profile
        }))
        .collect::<Vec<_>>();
    // Without the tray icon Magpie ignores `-t` and opens its main window.
    let settings = json!({
        "showNotifyIcon": true,
        "autoCheckForUpdates": false,
        "scalingModes": modes,
        "profiles": profiles,
    });
    format!("{settings:#}")
}

/// Full screen with the picture fitted to the monitor or stretched over it,
/// full screen enlarged by whole numbers, or Magpie's window at the size the
/// player chose. Magpie has no "cover the screen and cut the edges", which
/// the Windows options do not offer; a stored one is shown fitted.
fn effects(window: &GameWindow, shown: Shown) -> Vec<Value> {
    let (scaling, factor) = match shown.scale {
        WindowScale::Integer => (SCALE_BY_FACTOR, f64::from(whole_factor(window))),
        WindowScale::Stretch => (SCALE_TO_FILL, 1.0),
        WindowScale::Fit | WindowScale::Fill | WindowScale::Window { .. } => (SCALE_TO_FIT, 1.0),
    };
    let scaled = |name: &str| json!({ "name": name, "scalingType": scaling, "scale": { "x": factor, "y": factor } });
    match (shown.scale, shown.picture_filter) {
        (WindowScale::Integer, _) | (_, None) => vec![scaled("Nearest")],
        (_, Some(PictureFilter::EvenPixels)) => vec![scaled(r"Pixel Art\SharpBilinear")],
        (_, Some(PictureFilter::Smooth)) => vec![scaled("Bilinear")],
        (_, Some(PictureFilter::Fsr(sharpness))) => vec![
            scaled(r"FSR\FSR_EASU"),
            json!({
                "name": r"FSR\FSR_RCAS",
                "parameters": { "sharpness": fsr_sharpness(sharpness) },
            }),
        ],
        (_, Some(PictureFilter::Nis(sharpness))) => {
            let mut effect = scaled(r"NIS\NIS");
            effect["parameters"] = json!({ "sharpness": nis_sharpness(sharpness) });
            vec![effect]
        }
    }
}

/// Both effects take 0, the least, to 1, the most; Medium is each one's own
/// default.
fn fsr_sharpness(sharpness: Sharpness) -> f64 {
    match sharpness {
        Sharpness::Soft => 0.6,
        Sharpness::Medium => 0.87,
        Sharpness::Strong => 1.0,
    }
}

fn nis_sharpness(sharpness: Sharpness) -> f64 {
    match sharpness {
        Sharpness::Soft => 0.25,
        Sharpness::Medium => 0.5,
        Sharpness::Strong => 0.75,
    }
}

fn whole_factor(window: &GameWindow) -> u32 {
    let across = window.screen.width / window.picture.width.max(1);
    let down = window.screen.height / window.picture.height.max(1);
    across.min(down).max(1)
}

fn window_factor(picture: Size, width: u32, height: u32) -> f64 {
    let across = f64::from(width) / f64::from(picture.width.max(1));
    let down = f64::from(height) / f64::from(picture.height.max(1));
    across.min(down).max(1.0)
}

/// The class name as Magpie compares it. Magpie rewrites the names that
/// change from one start to the next before it looks for a profile, so a
/// profile holding the window's real name would never match: NW.js games
/// (RPG Maker MV and MZ) count their `Chrome_WidgetWin_` windows up, WPF
/// appends a new GUID every time, and TeknoParrot numbers its `XWindow_`.
fn class_rule(class: &str) -> String {
    let numbered = |prefix: &str| {
        class
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.chars().all(|character| character.is_ascii_digit()))
    };
    if let Some(name) = wpf_window_name(class) {
        name.to_owned()
    } else if numbered("Chrome_WidgetWin_") {
        "Chrome_WidgetWin_1".to_owned()
    } else if numbered("XWindow_") {
        "XWindow_0".to_owned()
    } else {
        class.to_owned()
    }
}

/// `HwndWrapper[<name>;;<GUID>]` gives `<name>`.
fn wpf_window_name(class: &str) -> Option<&str> {
    let inner = class.strip_prefix("HwndWrapper[")?.strip_suffix(']')?;
    let (name, guid) = inner.rsplit_once(";;")?;
    let groups = guid.split('-').collect::<Vec<_>>();
    let shaped = groups.iter().map(|group| group.len()).eq([8, 4, 4, 4, 12])
        && groups.iter().all(|group| {
            group
                .chars()
                .all(|character| matches!(character, '0'..='9' | 'a'..='f' | ','))
        });
    shaped.then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wolf_game() -> GameWindow {
        GameWindow {
            program: r"D:\Library\RJ1\sys\Game.exe".to_owned(),
            class: "ゲーム  System Start-up.".to_owned(),
            picture: Size {
                width: 640,
                height: 480,
            },
            screen: Size {
                width: 3840,
                height: 2160,
            },
        }
    }

    fn shown(scale: WindowScale) -> Shown {
        Shown {
            scale,
            picture_filter: None,
            cursor_hide_delay: None,
        }
    }

    fn written(games: &[(GameWindow, Shown)]) -> Value {
        serde_json::from_str(&settings(games)).expect("settings are JSON")
    }

    #[test]
    fn full_screen_gets_a_profile_for_exactly_that_window() {
        let settings = written(&[(wolf_game(), shown(WindowScale::Fit))]);

        assert_eq!(settings["showNotifyIcon"], true);
        assert_eq!(settings["autoCheckForUpdates"], false);
        assert_eq!(settings["profiles"][0], json!({}));
        let profile = &settings["profiles"][1];
        assert_eq!(profile["packaged"], false);
        assert_eq!(profile["pathRule"], r"D:\Library\RJ1\sys\Game.exe");
        assert_eq!(profile["classNameRule"], "ゲーム  System Start-up.");
        assert_eq!(profile["autoScale"], AUTO_SCALE_FULL_SCREEN);
        assert_eq!(profile["scalingMode"], 0);
        assert!(profile.get("autoHideCursorEnabled").is_none());
        assert!(profile.get("windowedInitialToolbarState").is_none());
        assert_eq!(
            settings["scalingModes"][0]["effects"],
            json!([{ "name": "Nearest", "scalingType": SCALE_TO_FIT, "scale": { "x": 1.0, "y": 1.0 } }])
        );
        assert_eq!(settings["scalingModes"][0]["name"], profile["name"]);
    }

    #[test]
    fn a_window_size_becomes_magpies_window_at_that_factor() {
        let settings = written(&[(
            wolf_game(),
            shown(WindowScale::Window {
                width: 1920,
                height: 1080,
            }),
        )]);

        let profile = &settings["profiles"][1];
        assert_eq!(profile["autoScale"], AUTO_SCALE_WINDOWED);
        assert_eq!(profile["initialWindowedScaleFactor"], CUSTOM_WINDOW_FACTOR);
        assert_eq!(profile["customInitialWindowedScaleFactor"], 2.25);
        assert_eq!(
            settings["scalingModes"][0]["effects"][0]["scalingType"],
            SCALE_TO_FIT
        );
        assert_eq!(
            window_factor(
                Size {
                    width: 1280,
                    height: 960
                },
                1280,
                720
            ),
            1.0
        );
    }

    #[test]
    fn whole_steps_keep_hard_pixels_whatever_picture_was_chosen() {
        let settings = written(&[(
            wolf_game(),
            Shown {
                picture_filter: Some(PictureFilter::Smooth),
                ..shown(WindowScale::Integer)
            },
        )]);

        assert_eq!(
            settings["scalingModes"][0]["effects"],
            json!([{ "name": "Nearest", "scalingType": SCALE_BY_FACTOR, "scale": { "x": 4.0, "y": 4.0 } }])
        );
        let small_screen = GameWindow {
            screen: Size {
                width: 800,
                height: 600,
            },
            ..wolf_game()
        };
        assert_eq!(whole_factor(&small_screen), 1);
    }

    #[test]
    fn draws_the_picture_the_way_the_player_chose() {
        let effects_of = |scale, picture_filter| {
            written(&[(
                wolf_game(),
                Shown {
                    picture_filter: Some(picture_filter),
                    ..shown(scale)
                },
            )])["scalingModes"][0]["effects"]
                .clone()
        };

        let even = effects_of(WindowScale::Fit, PictureFilter::EvenPixels);
        assert_eq!(even[0]["name"], r"Pixel Art\SharpBilinear");
        let smooth = effects_of(WindowScale::Fill, PictureFilter::Smooth);
        assert_eq!(smooth[0]["name"], "Bilinear");
        assert_eq!(smooth[0]["scalingType"], SCALE_TO_FIT);

        let fsr = effects_of(WindowScale::Stretch, PictureFilter::Fsr(Sharpness::Strong));
        assert_eq!(fsr[0]["name"], r"FSR\FSR_EASU");
        assert_eq!(fsr[0]["scalingType"], SCALE_TO_FILL);
        assert_eq!(
            fsr[1],
            json!({ "name": r"FSR\FSR_RCAS", "parameters": { "sharpness": 1.0 } })
        );

        let nis = effects_of(WindowScale::Fit, PictureFilter::Nis(Sharpness::Soft));
        assert_eq!(nis.as_array().map(Vec::len), Some(1));
        assert_eq!(nis[0]["name"], r"NIS\NIS");
        assert_eq!(nis[0]["parameters"]["sharpness"], 0.25);
    }

    #[test]
    fn hides_the_cursor_after_the_chosen_delay_within_magpies_range() {
        let delay_of = |cursor_hide_delay| {
            written(&[(
                wolf_game(),
                Shown {
                    cursor_hide_delay: Some(cursor_hide_delay),
                    ..shown(WindowScale::Fit)
                },
            )])["profiles"][1]
                .clone()
        };

        assert_eq!(delay_of(3000)["autoHideCursorEnabled"], true);
        assert_eq!(delay_of(3000)["autoHideCursorDelay"], 3.0);
        assert_eq!(delay_of(60_000)["autoHideCursorDelay"], 5.0);
    }

    #[test]
    fn every_game_shown_at_once_keeps_its_own_profile_and_mode() {
        let other = GameWindow {
            program: r"D:\Library\RJ2\Game.exe".to_owned(),
            class: "Chrome_WidgetWin_3".to_owned(),
            ..wolf_game()
        };
        let settings = written(&[
            (wolf_game(), shown(WindowScale::Fit)),
            (
                other,
                Shown {
                    picture_filter: Some(PictureFilter::Smooth),
                    ..shown(WindowScale::Fit)
                },
            ),
        ]);

        assert_eq!(settings["profiles"].as_array().map(Vec::len), Some(3));
        assert_eq!(settings["profiles"][2]["scalingMode"], 1);
        assert_eq!(
            settings["profiles"][2]["classNameRule"],
            "Chrome_WidgetWin_1"
        );
        assert_eq!(
            settings["scalingModes"][1]["effects"][0]["name"],
            "Bilinear"
        );
        assert_ne!(
            settings["profiles"][1]["name"],
            settings["profiles"][2]["name"]
        );
    }

    #[test]
    fn names_a_window_class_the_way_magpie_looks_it_up() {
        assert_eq!(class_rule("TkTopLevel"), "TkTopLevel");
        assert_eq!(class_rule("Chrome_WidgetWin_0"), "Chrome_WidgetWin_1");
        assert_eq!(class_rule("Chrome_WidgetWin_12"), "Chrome_WidgetWin_1");
        assert_eq!(class_rule("Chrome_WidgetWin_x"), "Chrome_WidgetWin_x");
        assert_eq!(class_rule("XWindow_4411"), "XWindow_0");
        assert_eq!(
            class_rule("HwndWrapper[Game.exe;;0123abcd-0123-abcd-0123-0123456789ab]"),
            "Game.exe"
        );
        assert_eq!(
            class_rule("HwndWrapper[Game.exe;;not-a-guid]"),
            "HwndWrapper[Game.exe;;not-a-guid]"
        );
    }
}
