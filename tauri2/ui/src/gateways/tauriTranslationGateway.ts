import type {
  GameTranslationView,
  TranslationGateway,
} from "@dla-launcher/shared-ui/translation";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";

export const tauriTranslationGateway: TranslationGateway = {
  readSettings: () => invoke("read_translation_settings"),
  saveSettings: ({ translator, deeplKey, prepareOnlyWhenIdle }) =>
    invoke("save_translation_settings", { translator, deeplKey, prepareOnlyWhenIdle }),
  testTranslator: (translator, deeplKey) => invoke("test_translator", { translator, deeplKey }),
  readGame: (installationId) => invoke("read_game_translation", { installationId }),
  turnOn: (installationId) => invoke("turn_on_game_translation", { installationId }),
  show: (installationId, shown) => invoke("show_game_translation", { installationId, shown }),
  remove: (installationId) => invoke("remove_game_translation", { installationId }),
  prepare: (installationId) => invoke("prepare_game_translation", { installationId }),
  translateAgain: (installationId) => invoke("translate_game_again", { installationId }),
  pause: (installationId) => invoke("pause_game_translation", { installationId }),
  checkPack: (installationId) => invoke("check_translation_pack", { installationId }),
  downloadPack: (installationId) => invoke("download_translation_pack", { installationId }),
  translatedWorks: () => invoke("read_translated_works"),
  subscribe(listener) {
    const unlisten = listen<GameTranslationView>("game-translation-changed", (event) =>
      listener(event.payload),
    );
    return () => void unlisten.then((stop) => stop());
  },
  openWebsite: (url) => openUrl(url),
};
