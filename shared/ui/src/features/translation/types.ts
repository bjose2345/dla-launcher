/** The player's translator: a local model behind the OpenAI-compatible API, or DeepL. */
export type TranslatorSettings =
  | { kind: "localModel"; address: string; model: string }
  | { kind: "deepl"; freePlan: boolean };

export interface TranslationSettings {
  translator: TranslatorSettings | null;
  /** The DeepL key itself never comes back from the launcher. */
  deeplKeySaved: boolean;
  prepareOnlyWhenIdle: boolean;
}

export interface SaveTranslationSettings {
  translator: TranslatorSettings | null;
  /** Undefined keeps the saved key; an empty string forgets it. */
  deeplKey?: string;
  prepareOnlyWhenIdle?: boolean;
}

export interface TranslatorTestResult {
  source: string;
  english: string;
  seconds: number;
}

export type PreparationState =
  | "preparing"
  | "waiting"
  | "paused"
  | "ready"
  | "needs_translator"
  | "failed";

export type TextGroup = "menus" | "names_and_descriptions" | "dialogue";

/** Why a preparation stopped. */
export type PreparationProblem =
  | "translator_unreachable"
  | "translator_refused"
  | "translator_reply"
  | "game_files"
  | "other";

export interface GroupProgress {
  group: TextGroup;
  done: number;
  total: number;
}

export interface PreparationView {
  state: PreparationState;
  groups: GroupProgress[];
  done: number;
  total: number;
  secondsLeft: number | null;
  preparedAt: string | null;
  preparedWith: string | null;
  /** Texts the translator gave nothing usable for in the last run. */
  missing: number;
  problem: PreparationProblem | null;
  /** The technical detail behind the problem. */
  error: string | null;
}

export interface PackSummary {
  lines: number;
  /** When the pack was made, as YYYY-MM-DD. */
  made: string;
}

/** Why the player cannot have the Archive's translation yet. */
export type PackAccess = "for_elite" | "sign_in";

/** The translation from the Archive: one newer than the one in use, and the one in use. */
export interface PackView {
  available: PackSummary | null;
  installed: PackSummary | null;
  access: PackAccess | null;
}

export interface GameTranslationView {
  installationId: string;
  engine: string | null;
  engineLabel: string | null;
  supported: boolean;
  on: boolean;
  shown: boolean;
  translatorReady: boolean;
  preparation: PreparationView | null;
  pack: PackView | null;
}

export interface TranslationGateway {
  readSettings(): Promise<TranslationSettings>;
  saveSettings(request: SaveTranslationSettings): Promise<TranslationSettings>;
  /** Translates one line with these settings, without saving them. */
  testTranslator(translator: TranslatorSettings, deeplKey?: string): Promise<TranslatorTestResult>;
  readGame(installationId: string): Promise<GameTranslationView>;
  turnOn(installationId: string): Promise<GameTranslationView>;
  show(installationId: string, shown: boolean): Promise<GameTranslationView>;
  remove(installationId: string): Promise<void>;
  /** Translates the game ahead of play, or resumes; after an update, only new lines. */
  prepare(installationId: string): Promise<GameTranslationView>;
  pause(installationId: string): Promise<GameTranslationView>;
  /** Translates again the lines the player's translator made, keeping packs' lines. */
  translateAgain(installationId: string): Promise<GameTranslationView>;
  /** Asks the Archive whether it has a translation of the game. */
  checkPack(installationId: string): Promise<GameTranslationView>;
  downloadPack(installationId: string): Promise<GameTranslationView>;
  /** Every work the Archive has a translation of, and the day its newest was made. */
  translatedWorks(): Promise<Record<string, string>>;
  /** Progress while a game is prepared; returns the unsubscribe function. */
  subscribe(listener: (view: GameTranslationView) => void): () => void;
  /** Opens a translator's website in the browser. */
  openWebsite(url: string): Promise<void>;
}

export const localModelDefaults = {
  address: "http://127.0.0.1:11434/v1",
  model: "qwen3:4b-instruct",
} as const;
