// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PresentationProvider } from "../../preferences/PresentationProvider";
import type { CatalogDetailGateway, CatalogWork } from "../catalog/types";
import type { ScanInstall, ScanInstallGateway } from "../scanner/types";
import type { TranslationGateway } from "../translation/types";
import { LibraryManageDrawer, recommendedKey, type ManageTab } from "./LibraryManageDrawer";
import type {
  Installation,
  InstallationHealthReport,
  InstallationReviewRequest,
  LaunchOptions,
  LaunchOptionsRequest,
  LibraryGateway,
  OwnWorkDetails,
} from "./types";

afterEach(() => {
  cleanup();
});

describe("LibraryManageDrawer", () => {
  it("shows General and Installed files and saves a new name as soon as it is typed", async () => {
    const gateway = libraryGateway();
    renderDrawer(gateway);

    expect(await screen.findByRole("dialog", { name: "Kunoichi Red" })).toBeTruthy();
    expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual(["General", "Installed files"]);
    expect(screen.getByText("Changes save as you make them")).toBeTruthy();

    const name = screen.getByLabelText("Name");
    fireEvent.change(name, { target: { value: "My ninja game" } });
    fireEvent.blur(name);

    await waitFor(() => expect(gateway.saveReview).toHaveBeenCalledOnce());
    expect(savedRequest(gateway).customTitle).toBe("My ninja game");
    expect(await screen.findByText("Saved")).toBeTruthy();
  });

  it("adds a Translation tab for games where the launcher can translate", async () => {
    const translation = {
      readSettings: vi.fn().mockResolvedValue({ translator: null, keySaved: false, prepareOnlyWhenIdle: false }),
      readGame: vi.fn().mockResolvedValue({
        installationId: "installation-1",
        engine: "rpg_maker_mz",
        engineLabel: "RPG Maker MZ",
        supported: true,
        on: false,
        shown: false,
        translatorReady: true,
        preparation: null,
      }),
      subscribe: vi.fn(() => () => undefined),
    } as unknown as TranslationGateway;
    renderDrawer(libraryGateway(), { translationGateway: translation, tab: "translation" });

    expect(await screen.findByText("Play this game in English")).toBeTruthy();
    expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual([
      "General",
      "Installed files",
      "Translation",
    ]);
  });

  it("holds the Translation tab's Play back while the game starts and stops the game while it runs", async () => {
    const translation = {
      readSettings: vi.fn().mockResolvedValue({ translator: null, keySaved: false, prepareOnlyWhenIdle: false }),
      readGame: vi.fn().mockResolvedValue({
        installationId: "installation-1",
        engine: "wolf_rpg",
        engineLabel: "WOLF RPG Editor",
        supported: true,
        on: true,
        shown: true,
        translatorReady: true,
        preparation: {
          state: "ready",
          pausing: false,
          groups: [{ group: "dialogue", done: 875, total: 875 }],
          done: 875,
          total: 875,
          secondsLeft: null,
          preparedAt: "2026-10-29T12:00:00Z",
          preparedWith: "qwen3:4b-instruct",
          missing: 0,
          problem: null,
          error: null,
        },
        pack: null,
      }),
      subscribe: vi.fn(() => () => undefined),
    } as unknown as TranslationGateway;
    const gateway = libraryGateway();
    type Launch = Awaited<ReturnType<LibraryGateway["launchInstallation"]>>;
    const running = { id: "launch-1", installationId: "installation-1", status: "running" } as Launch;
    let started: (launch: Launch) => void = () => undefined;
    vi.mocked(gateway.launchInstallation).mockReturnValue(new Promise<Launch>((resolve) => {
      started = resolve;
    }));
    renderDrawer(gateway, { translationGateway: translation, tab: "translation" });

    fireEvent.click(await screen.findByRole("button", { name: "Play" }));
    await waitFor(() => expect(gateway.launchInstallation).toHaveBeenCalledWith("installation-1"));
    const starting = screen.getByRole("button", { name: "Play" }) as HTMLButtonElement;
    expect(starting.disabled).toBe(true);
    expect(starting.getAttribute("aria-busy")).toBe("true");

    vi.mocked(gateway.listRecentLaunches).mockResolvedValue([running]);
    started(running);
    fireEvent.click(await screen.findByRole("button", { name: "Stop" }));
    await waitFor(() => expect(gateway.stopLaunch).toHaveBeenCalledWith("launch-1"));
    expect(screen.getByText("Running")).toBeTruthy();

    vi.mocked(gateway.listRecentLaunches).mockResolvedValue([
      { ...running, status: "exited" },
      { ...running, id: "launch-2", installationId: "installation-2" },
    ]);
    fireEvent.click(screen.getByRole("tab", { name: "General" }));
    fireEvent.click(screen.getByRole("tab", { name: "Translation" }));
    await waitFor(() => expect(gateway.listRecentLaunches).toHaveBeenCalledTimes(4));
    expect((await screen.findByRole("button", { name: "Play" }) as HTMLButtonElement).disabled).toBe(false);
  });

  it("says in Translation that a work already including English needs no translating", async () => {
    const translation = {
      readSettings: vi.fn().mockResolvedValue({ translator: null, keySaved: false, prepareOnlyWhenIdle: false }),
      readGame: vi.fn().mockResolvedValue({
        installationId: "installation-1",
        engine: "rpg_maker_mz",
        engineLabel: "RPG Maker MZ",
        supported: true,
        on: false,
        shown: false,
        translatorReady: true,
        preparation: null,
        pack: null,
      }),
      subscribe: vi.fn(() => () => undefined),
    } as unknown as TranslationGateway;
    const languages = [
      { code: "japanese", name: "日本語", nameEnglish: "Japanese" },
      { code: "english", name: "英語", nameEnglish: "English" },
    ];
    renderDrawer(libraryGateway(), {
      translationGateway: translation,
      tab: "translation",
      readWork: async () => ({ supportedLanguages: languages }),
    });

    expect(await screen.findByText("This work already includes English, so it doesn't need translating.")).toBeTruthy();
    expect(screen.getByRole("tab", { name: "Translation" }).getAttribute("aria-selected")).toBe("true");
    expect(screen.queryByRole("button", { name: "Turn on translation" })).toBeNull();
  });

  it("fixes the match by picking a catalog work", async () => {
    const gateway = libraryGateway();
    renderDrawer(gateway);

    fireEvent.click(await screen.findByRole("button", { name: "Fix match…" }));
    fireEvent.change(screen.getByLabelText("Find a work in the catalog"), { target: { value: "RJ01000004" } });
    fireEvent.click(await screen.findByRole("button", { name: /Her Secret/ }, { timeout: 2_000 }));

    await waitFor(() => expect(gateway.saveReview).toHaveBeenCalledOnce());
    expect(savedRequest(gateway).catalogIdentity).toEqual({ kind: "catalog_work", workCode: "RJ01000004" });
  });

  it("shows a work outside the catalog with the user's own details", async () => {
    const installation = gameInstallation();
    installation.detection.catalogIdentity = null;
    installation.overrides.catalogIdentity = { kind: "unidentified" };
    const own: OwnWorkDetails = {
      installationId: installation.id,
      circle: "My circle",
      code: "D_123456",
      releaseDate: null,
      kind: "game",
      tags: [],
      description: null,
      cover: "cover-1",
      samples: [],
      updatedAt: "2026-10-28T00:00:00Z",
    };
    renderDrawer(libraryGateway({ installation, own }));

    const dialog = await screen.findByRole("dialog", { name: "KUNOICHI RED" });
    expect(await within(dialog).findByText("My circle · D_123456")).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Metadata" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Game" }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByRole("img", { name: "Cover" }).getAttribute("src")).toBe("art:installation-1:cover-1");
  });

  it("shows an unknown work with question marks and a way to find it", async () => {
    const installation = gameInstallation();
    installation.detection.catalogIdentity = null;
    installation.overrides.catalogIdentity = { kind: "unidentified" };
    renderDrawer(libraryGateway({ installation }));

    expect(await screen.findByRole("dialog", { name: "KUNOICHI RED" })).toBeTruthy();
    expect(screen.getAllByText("Unknown work").length).toBeGreaterThan(0);
    expect(screen.getAllByText("?").length).toBe(2);
    expect(screen.getByRole("button", { name: "Find in catalog…" })).toBeTruthy();
  });

  it("chooses what the work starts with, marking the recommended file", async () => {
    const gateway = libraryGateway();
    renderDrawer(gateway);

    const startsWith = await screen.findByLabelText("Starts with") as HTMLSelectElement;
    expect([...startsWith.options].map((option) => option.textContent)).toEqual([
      "Game.exe · Launch executable (recommended)",
      "Config.exe · Launch executable",
    ]);
    fireEvent.change(startsWith, { target: { value: "launch_executable|path:KUNOICHI RED/Config.exe" } });

    await waitFor(() => expect(gateway.saveReview).toHaveBeenCalledOnce());
    expect(savedRequest(gateway).preferredAction).toEqual({
      action: "launch_executable",
      target: { kind: "relative_path", path: "KUNOICHI RED/Config.exe" },
    });
  });

  it("shows the work's own folder without the Windows path marker and opens it", async () => {
    const installation = gameInstallation();
    installation.rootPath = "\\\\?\\D:\\Games";
    const gateway = libraryGateway({ installation });
    renderDrawer(gateway, { tab: "files" });

    expect(await screen.findByText("D:\\Games\\KUNOICHI RED")).toBeTruthy();
    expect(screen.getByText("2 files · 200 B · Executable 2")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Browse…" }));
    await waitFor(() => expect(gateway.openInstallationFolder).toHaveBeenCalledWith("installation-1"));
  });

  it("counts a single file in the singular", async () => {
    const installation = gameInstallation();
    installation.detection.contentItems = [contentItem("KUNOICHI RED/Game.exe")];
    renderDrawer(libraryGateway({
      installation,
      health: healthReport({ expectedFiles: 1, presentFiles: 1 }),
    }), { tab: "files" });

    expect(await screen.findByText("1 file · 100 B · Executable 1")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Verify integrity of files" }));
    expect(await screen.findByText("The file is present.")).toBeTruthy();
  });

  it("verifies the files and reports the result in plain words", async () => {
    const gateway = libraryGateway();
    renderDrawer(gateway, { tab: "files" });

    expect(screen.queryByText("Healthy")).toBeNull();
    fireEvent.click(await screen.findByRole("button", { name: "Verify integrity of files" }));

    await waitFor(() => expect(gateway.verifyInstallation).toHaveBeenCalledWith("installation-1"));
    expect(await screen.findByText("All 14 files are present.")).toBeTruthy();
  });

  it("finds a file by name, changes its type and puts it back", async () => {
    const installation = gameInstallation();
    installation.detection.contentItems.push({ ...contentItem("KUNOICHI RED/img/title.png"), mediaType: "image" });
    const gateway = libraryGateway({ installation });
    renderDrawer(gateway, { tab: "files" });

    expect(await screen.findByText("No file has been changed by hand.")).toBeTruthy();
    // Folders start closed; a search opens the ones with a match.
    expect(screen.queryByText("Config.exe")).toBeNull();
    fireEvent.change(screen.getByRole("searchbox", { name: "Find a file by name" }), { target: { value: "config" } });
    fireEvent.click(await screen.findByText("Config.exe"));
    expect(screen.queryByText("title.png")).toBeNull();

    const editor = await screen.findByRole("group", { name: "KUNOICHI RED/Config.exe" });
    fireEvent.change(within(editor).getByRole("combobox", { name: "Type" }), { target: { value: "unknown" } });
    await waitFor(() => expect(savedRequest(gateway).contentItems).toEqual([
      { relativePath: "KUNOICHI RED/Config.exe", mediaType: "unknown", ignored: false, order: null },
    ]));
    expect(await screen.findByText("Unknown content", { selector: ".library-changed-what" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    await waitFor(() => expect(vi.mocked(gateway.saveReview).mock.calls[1]![0].contentItems).toEqual([]));
    expect(await screen.findByText("No file has been changed by hand.")).toBeTruthy();
  });

  it("hides a file from the Library and says so in the list of changes", async () => {
    const gateway = libraryGateway();
    renderDrawer(gateway, { tab: "files" });

    fireEvent.click(await screen.findByRole("button", { name: "Open all folders" }));
    fireEvent.click(await screen.findByText("Game.exe"));
    fireEvent.click(within(await screen.findByRole("group", { name: "KUNOICHI RED/Game.exe" }))
      .getByRole("checkbox", { name: "Hide in the Library" }));

    await waitFor(() => expect(savedRequest(gateway).contentItems).toEqual([
      { relativePath: "KUNOICHI RED/Game.exe", mediaType: null, ignored: true, order: null },
    ]));
    expect(await screen.findByText("Hidden in the Library", { selector: ".library-changed-what" })).toBeTruthy();
  });

  it("runs a game in a Japanese locale on Windows, fetching the tool when turned on", async () => {
    const gateway = libraryGateway();
    const options = (japaneseLocale: boolean): LaunchOptions => (
      { japaneseLocale, japaneseLocaleAvailable: true, missingRuntimes: [], videoPlayer: null, audioPlayer: null, windowScale: null, enlarger: null, refreshRate: null, pictureFilter: null, performanceOverlay: false, performanceOverlayAvailable: false, cursorHideDelay: null }
    );
    gateway.readLaunchOptions = vi.fn().mockResolvedValue(options(false));
    let finish: (value: LaunchOptions) => void = () => undefined;
    gateway.saveLaunchOptions = vi.fn(() => new Promise<LaunchOptions>((resolve) => { finish = resolve; }));
    renderDrawer(gateway);

    const toggle = await screen.findByRole("switch", { name: /Run in Japanese locale/ });
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    fireEvent.click(toggle);
    expect(await screen.findByText("Downloading Locale Remulator…")).toBeTruthy();
    expect(gateway.saveLaunchOptions).toHaveBeenCalledWith("installation-1", { japaneseLocale: true, videoPlayer: null, audioPlayer: null, windowScale: null, refreshRate: null, pictureFilter: null, performanceOverlay: false, cursorHideDelay: null });
    finish(options(true));
    await waitFor(() => expect(toggle.getAttribute("aria-checked")).toBe("true"));
    expect(screen.queryByText(/Visual C\+\+ Redistributable/)).toBeNull();
  });

  it("lets a video work choose a player of its own and clear it again", async () => {
    const installation = gameInstallation();
    installation.detection.contentItems = [contentItem("Movie/opening.mp4", "video")];
    installation.detection.launchCandidates = [];
    installation.overrides.preferredAction = null;
    const gateway = libraryGateway({ installation });
    const options = (videoPlayer: string | null): LaunchOptions => (
      { japaneseLocale: false, japaneseLocaleAvailable: false, missingRuntimes: [], videoPlayer, audioPlayer: null, windowScale: null, enlarger: null, refreshRate: null, pictureFilter: null, performanceOverlay: false, performanceOverlayAvailable: false, cursorHideDelay: null }
    );
    gateway.readLaunchOptions = vi.fn().mockResolvedValue(options(null));
    gateway.saveLaunchOptions = vi.fn(async (_id: string, request: { videoPlayer: string | null }) => options(request.videoPlayer));
    gateway.selectMediaPlayer = vi.fn().mockResolvedValue("/usr/bin/vlc");
    renderDrawer(gateway);

    expect(await screen.findByText("Built-in player")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Choose…" }));
    await waitFor(() => expect(gateway.saveLaunchOptions).toHaveBeenCalledWith("installation-1", { japaneseLocale: false, videoPlayer: "/usr/bin/vlc", audioPlayer: null, windowScale: null, refreshRate: null, pictureFilter: null, performanceOverlay: false, cursorHideDelay: null }));
    expect(await screen.findByText(/vlc/)).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    await waitFor(() => expect(gateway.saveLaunchOptions).toHaveBeenCalledWith("installation-1", { japaneseLocale: false, videoPlayer: null, audioPlayer: null, windowScale: null, refreshRate: null, pictureFilter: null, performanceOverlay: false, cursorHideDelay: null }));
    expect(await screen.findByText("Built-in player")).toBeTruthy();
  });

  it("says when Windows lacks the Visual C++ runtime and links Microsoft's download", async () => {
    const gateway = libraryGateway();
    gateway.readLaunchOptions = vi.fn().mockResolvedValue({
      japaneseLocale: true,
      japaneseLocaleAvailable: true,
      missingRuntimes: ["x86"],
      videoPlayer: null,
      audioPlayer: null,
      windowScale: null,
      enlarger: null,
      refreshRate: null,
      pictureFilter: null,
      performanceOverlay: false,
      performanceOverlayAvailable: false,
      cursorHideDelay: null,
    } satisfies LaunchOptions);
    gateway.openWebsite = vi.fn().mockResolvedValue(undefined);
    renderDrawer(gateway);

    expect(await screen.findByText(/doesn't have yet\. Install it, then start the game\./)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Download (64-bit)" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Download (32-bit)" }));
    expect(gateway.openWebsite).toHaveBeenCalledWith("https://aka.ms/vs/17/release/vc_redist.x86.exe");
  });

  it("leaves the Japanese locale out where Wine runs every game in Japanese", async () => {
    const gateway = libraryGateway();
    gateway.readLaunchOptions = vi.fn().mockResolvedValue(
      { japaneseLocale: false, japaneseLocaleAvailable: false, missingRuntimes: [], videoPlayer: null, audioPlayer: null, windowScale: null, enlarger: null, refreshRate: null, pictureFilter: null, performanceOverlay: false, performanceOverlayAvailable: false, cursorHideDelay: null } satisfies LaunchOptions,
    );
    renderDrawer(gateway);

    await waitFor(() => expect(gateway.readLaunchOptions).toHaveBeenCalled());
    expect(screen.queryByRole("switch", { name: /Run in Japanese locale/ })).toBeNull();
    expect(screen.queryByRole("combobox", { name: "Resolution" })).toBeNull();
  });

  it("offers a resolution, refresh rate and picture for an app in the Display tab where Linux has gamescope", async () => {
    const gateway = libraryGateway();
    const options = (
      windowScale: LaunchOptions["windowScale"],
      refreshRate: number | null,
      pictureFilter: LaunchOptions["pictureFilter"],
      extras: Partial<LaunchOptions> = {},
    ): LaunchOptions => ({
      japaneseLocale: false, japaneseLocaleAvailable: false, missingRuntimes: [], videoPlayer: null, audioPlayer: null,
      windowScale, enlarger: "gamescope", refreshRate, pictureFilter,
      performanceOverlay: false, performanceOverlayAvailable: true, cursorHideDelay: null, ...extras,
    });
    const saved = (
      windowScale: LaunchOptions["windowScale"],
      refreshRate: number | null,
      pictureFilter: LaunchOptions["pictureFilter"],
      extras: Partial<LaunchOptionsRequest> = {},
    ) => ({
      japaneseLocale: false, videoPlayer: null, audioPlayer: null, windowScale, refreshRate, pictureFilter,
      performanceOverlay: false, cursorHideDelay: null, ...extras,
    });
    gateway.readLaunchOptions = vi.fn().mockResolvedValue(options(null, null, null));
    gateway.saveLaunchOptions = vi.fn(async (_id: string, request: LaunchOptionsRequest) => (
      options(request.windowScale, request.refreshRate, request.pictureFilter, {
        performanceOverlay: request.performanceOverlay,
        cursorHideDelay: request.cursorHideDelay,
      })
    ));
    renderDrawer(gateway, { tab: "display" });

    expect(await screen.findByText(/Experimental/)).toBeTruthy();
    const choice = screen.getByRole("combobox", { name: "Resolution" }) as HTMLSelectElement;
    const rate = screen.getByRole("combobox", { name: "Refresh rate" }) as HTMLSelectElement;
    const picture = screen.getByRole("combobox", { name: "Picture" }) as HTMLSelectElement;
    expect(choice.value).toBe("");
    expect(rate.disabled).toBe(true);
    expect(picture.disabled).toBe(true);
    expect(screen.getByRole("option", { name: "Original" })).toBeTruthy();
    expect(screen.getByRole("option", { name: "1920 × 1440" })).toBeTruthy();
    expect(screen.getByRole("option", { name: "Full screen (stretched)" })).toBeTruthy();
    fireEvent.change(choice, { target: { value: "1920x1440" } });
    await waitFor(() => expect(gateway.saveLaunchOptions).toHaveBeenCalledWith("installation-1", saved("1920x1440", null, null)));
    await waitFor(() => expect(choice.value).toBe("1920x1440"));
    await waitFor(() => expect(rate.disabled).toBe(false));
    expect(rate.value).toBe("120");
    fireEvent.change(rate, { target: { value: "60" } });
    await waitFor(() => expect(gateway.saveLaunchOptions).toHaveBeenCalledWith("installation-1", saved("1920x1440", 60, null)));
    await waitFor(() => expect(rate.value).toBe("60"));

    expect(picture.value).toBe("");
    expect(screen.queryByRole("combobox", { name: "Sharpness" })).toBeNull();
    fireEvent.change(picture, { target: { value: "even" } });
    await waitFor(() => expect(gateway.saveLaunchOptions).toHaveBeenCalledWith("installation-1", saved("1920x1440", 60, "even")));
    await waitFor(() => expect(picture.value).toBe("even"));
    expect(screen.queryByRole("combobox", { name: "Sharpness" })).toBeNull();
    fireEvent.change(picture, { target: { value: "smooth" } });
    await waitFor(() => expect(gateway.saveLaunchOptions).toHaveBeenCalledWith("installation-1", saved("1920x1440", 60, "smooth")));
    await waitFor(() => expect(picture.value).toBe("smooth"));
    fireEvent.change(picture, { target: { value: "fsr" } });
    await waitFor(() => expect(gateway.saveLaunchOptions).toHaveBeenCalledWith("installation-1", saved("1920x1440", 60, "fsr-medium")));
    const sharpness = await screen.findByRole("combobox", { name: "Sharpness" }) as HTMLSelectElement;
    await waitFor(() => expect(sharpness.disabled).toBe(false));
    expect(sharpness.value).toBe("medium");
    fireEvent.change(sharpness, { target: { value: "strong" } });
    await waitFor(() => expect(gateway.saveLaunchOptions).toHaveBeenCalledWith("installation-1", saved("1920x1440", 60, "fsr-strong")));

    const cursor = screen.getByRole("combobox", { name: "Hide the cursor" }) as HTMLSelectElement;
    expect(cursor.value).toBe("");
    fireEvent.change(cursor, { target: { value: "3000" } });
    await waitFor(() => expect(gateway.saveLaunchOptions).toHaveBeenCalledWith("installation-1", saved("1920x1440", 60, "fsr-strong", { cursorHideDelay: 3000 })));
    await waitFor(() => expect(cursor.value).toBe("3000"));
    const overlay = screen.getByRole("switch", { name: /Show frame rate and load over the game/ });
    fireEvent.click(overlay);
    await waitFor(() => expect(gateway.saveLaunchOptions).toHaveBeenCalledWith("installation-1", saved("1920x1440", 60, "fsr-strong", { cursorHideDelay: 3000, performanceOverlay: true })));

    // Whole steps always keep hard pixels; the saved choice waits for another resolution.
    await waitFor(() => expect(choice.disabled).toBe(false));
    fireEvent.change(choice, { target: { value: "integer" } });
    await waitFor(() => expect(picture.disabled).toBe(true));
    expect(picture.value).toBe("");
    expect(screen.queryByRole("combobox", { name: "Sharpness" })).toBeNull();
  });

  it("offers Magpie's choices in the Display tab on Windows", async () => {
    const gateway = libraryGateway();
    const options = (extras: Partial<LaunchOptions> = {}): LaunchOptions => ({
      japaneseLocale: false, japaneseLocaleAvailable: true, missingRuntimes: [], videoPlayer: null, audioPlayer: null,
      windowScale: null, enlarger: "magpie", refreshRate: null, pictureFilter: null,
      performanceOverlay: false, performanceOverlayAvailable: false, cursorHideDelay: null, ...extras,
    });
    gateway.readLaunchOptions = vi.fn().mockResolvedValue(options());
    gateway.saveLaunchOptions = vi.fn(async (_id: string, request: LaunchOptionsRequest) => options({
      windowScale: request.windowScale,
      pictureFilter: request.pictureFilter,
      cursorHideDelay: request.cursorHideDelay,
    }));
    renderDrawer(gateway, { tab: "display" });

    const choice = await screen.findByRole("combobox", { name: "Resolution" }) as HTMLSelectElement;
    expect(screen.getByText(/Uses Magpie \(GPL-3\.0\)/)).toBeTruthy();
    expect(screen.getByRole("option", { name: "Full screen" })).toBeTruthy();
    expect(screen.getByRole("option", { name: "Full screen (stretched)" })).toBeTruthy();
    expect(screen.getByRole("option", { name: "Full screen (sharp pixels)" })).toBeTruthy();
    expect(screen.getByRole("option", { name: "1920 × 1440" })).toBeTruthy();
    expect(screen.queryByRole("option", { name: /edges cut off/ })).toBeNull();
    expect(screen.queryByRole("combobox", { name: "Refresh rate" })).toBeNull();
    expect(screen.queryByRole("switch", { name: /Show frame rate and load over the game/ })).toBeNull();
    expect(screen.getByRole("option", { name: "Never" })).toBeTruthy();
    expect(screen.queryByRole("option", { name: "After 10 seconds without moving" })).toBeNull();

    fireEvent.change(choice, { target: { value: "fit" } });
    await waitFor(() => expect(gateway.saveLaunchOptions).toHaveBeenCalledWith("installation-1", {
      japaneseLocale: false, videoPlayer: null, audioPlayer: null, windowScale: "fit", refreshRate: null,
      pictureFilter: null, performanceOverlay: false, cursorHideDelay: null,
    }));
    await waitFor(() => expect(choice.value).toBe("fit"));
    const picture = screen.getByRole("combobox", { name: "Picture" }) as HTMLSelectElement;
    await waitFor(() => expect(picture.disabled).toBe(false));
    fireEvent.change(picture, { target: { value: "smooth" } });
    await waitFor(() => expect(picture.value).toBe("smooth"));
    const cursor = screen.getByRole("combobox", { name: "Hide the cursor" }) as HTMLSelectElement;
    await waitFor(() => expect(cursor.disabled).toBe(false));
    fireEvent.change(cursor, { target: { value: "1000" } });
    await waitFor(() => expect(gateway.saveLaunchOptions).toHaveBeenLastCalledWith("installation-1", {
      japaneseLocale: false, videoPlayer: null, audioPlayer: null, windowScale: "fit", refreshRate: null,
      pictureFilter: "smooth", performanceOverlay: false, cursorHideDelay: 1000,
    }));
  });

  it("keeps the Display tab away from works that are not apps", async () => {
    const installation = gameInstallation();
    installation.detection.contentItems = [contentItem("Movie/opening.mp4", "video")];
    installation.detection.launchCandidates = [];
    installation.overrides.preferredAction = null;
    renderDrawer(libraryGateway({ installation }));

    expect(await screen.findByRole("tab", { name: "General" })).toBeTruthy();
    expect(screen.queryByRole("tab", { name: "Display" })).toBeNull();
  });

  it("offers Repair only when files are missing and can be repaired", async () => {
    renderDrawer(libraryGateway({
      health: healthReport({ state: "repairable", repairable: true, missingFiles: 3 }),
    }), { tab: "files" });

    expect(await screen.findByText("3 files are missing or changed.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Repair" })).toBeTruthy();
  });

  it("gives Japanese file names back when an older install read them wrongly", async () => {
    const gateway = {
      ...libraryGateway(),
      checkFileNames: vi.fn().mockResolvedValue({ installationId: "installation-1", wrong: 1216, fixed: 0 }),
      fixFileNames: vi.fn().mockResolvedValue({ installationId: "installation-1", wrong: 0, fixed: 1216 }),
    } as unknown as LibraryGateway;
    renderDrawer(gateway, { tab: "files" });

    expect(await screen.findByText("1,216 file names were unpacked wrong")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Fix file names" }));

    await waitFor(() => expect(gateway.fixFileNames).toHaveBeenCalledWith("installation-1"));
    expect(await screen.findByText("Fixed 1,216 file names.")).toBeTruthy();
    expect(screen.queryByText("1,216 file names were unpacked wrong")).toBeNull();
  });

  it("installs an archive that was added but never unpacked", async () => {
    const installation = gameInstallation();
    installation.detection.packageInspection = packageInspection();
    const installs = scanInstalls();
    renderDrawer(libraryGateway({ installation }), { tab: "files", installs });

    expect(await screen.findByText("This archive isn't installed yet")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Install" }));

    await waitFor(() => expect(installs.installArchive).toHaveBeenCalledWith("installation-1"));
  });

  it("asks what to do when the install folder already has the work's folder", async () => {
    const installation = gameInstallation();
    installation.detection.packageInspection = packageInspection();
    const installs = scanInstalls([{
      installationId: "installation-1",
      key: "",
      name: "RJ137267.zip",
      workCode: "RJ137267",
      status: "failed",
      detail: "the install folder already has a folder named RJ137267",
      conflict: { folderName: "RJ137267", keepBothName: "RJ137267 (2)", replaceable: true },
    }]);
    renderDrawer(libraryGateway({ installation }), { tab: "files", installs });

    expect(await screen.findByText("Your install folder already has a folder named RJ137267.")).toBeTruthy();
    expect(screen.getByText("Keep both installs this work as RJ137267 (2) and leaves the other folder as it is.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Install" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Replace" }));
    expect(installs.installArchive).not.toHaveBeenCalled();
    const confirm = screen.getByRole("alertdialog", {
      name: "Delete the folder RJ137267 and everything in it, saves included, and install this work in its place?",
    });
    fireEvent.click(within(confirm).getByRole("button", { name: "Cancel" }));

    fireEvent.click(screen.getByRole("button", { name: "Keep both" }));
    await waitFor(() => expect(installs.installArchive).toHaveBeenCalledWith("installation-1", "keep_both"));
  });

  it("replaces a taken folder only after the warning, and never another work's", async () => {
    const installation = gameInstallation();
    installation.detection.packageInspection = packageInspection();
    const conflict = { folderName: "RJ137267", keepBothName: "RJ137267 (2)", replaceable: true };
    const failed: ScanInstall = {
      installationId: "installation-1",
      key: "",
      name: "RJ137267.zip",
      workCode: "RJ137267",
      status: "failed",
      detail: null,
      conflict,
    };
    const installs = scanInstalls([failed]);
    const first = renderDrawer(libraryGateway({ installation }), { tab: "files", installs });

    fireEvent.click(await screen.findByRole("button", { name: "Replace" }));
    fireEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Replace" }));
    await waitFor(() => expect(installs.installArchive).toHaveBeenCalledWith("installation-1", "replace_existing"));
    first.unmount();

    renderDrawer(libraryGateway({ installation }), {
      tab: "files",
      installs: scanInstalls([{ ...failed, conflict: { ...conflict, replaceable: false } }]),
    });
    expect(await screen.findByText(/belongs to another work in your Library/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Keep both" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Replace" })).toBeNull();
  });

  it("confirms before uninstalling and closes once the work is gone", async () => {
    const gateway = libraryGateway();
    const onClose = vi.fn();
    renderDrawer(gateway, { tab: "files", onClose });

    fireEvent.click(await screen.findByRole("button", { name: "Uninstall…" }));
    const confirmation = screen.getByRole("alertdialog");
    expect(gateway.uninstallInstallation).not.toHaveBeenCalled();

    fireEvent.click(within(confirmation).getByRole("button", { name: "Uninstall" }));
    await waitFor(() => expect(gateway.uninstallInstallation).toHaveBeenCalledWith("installation-1"));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("closes with Escape", async () => {
    const onClose = vi.fn();
    renderDrawer(libraryGateway(), { onClose });

    await screen.findByRole("dialog", { name: "Kunoichi Red" });
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });

  it("recommends the most confident start action, the first among equals", () => {
    const candidates = gameInstallation().detection.launchCandidates;
    expect(recommendedKey(candidates)).toBe("launch_executable|path:KUNOICHI RED/Game.exe");
    expect(recommendedKey([])).toBeNull();
  });
});

const works: Record<string, CatalogWork> = {
  RJ01000002: work("RJ01000002", "Kunoichi Red"),
  RJ01000004: work("RJ01000004", "Her Secret"),
};

function renderDrawer(
  gateway: LibraryGateway,
  options: {
    tab?: ManageTab;
    onClose?: () => void;
    installs?: ScanInstallGateway;
    translationGateway?: TranslationGateway;
    readWork?: (code: string) => Promise<unknown>;
  } = {},
) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <PresentationProvider>
        <LibraryManageDrawer
          installationId="installation-1"
          initialTab={options.tab}
          gateway={gateway}
          catalogGateway={{
            readWorks: vi.fn(async (codes: string[]) => codes.flatMap((code) => works[code] ? [works[code]] : [])),
            ...(options.readWork ? { read: options.readWork as CatalogDetailGateway["read"] } : {}),
          }}
          installs={options.installs}
          translationGateway={options.translationGateway}
          onClose={options.onClose ?? (() => undefined)}
        />
      </PresentationProvider>
    </QueryClientProvider>,
  );
}

function savedRequest(gateway: LibraryGateway): InstallationReviewRequest {
  return vi.mocked(gateway.saveReview).mock.calls[0]![0];
}

function scanInstalls(installs: ScanInstall[] = []): ScanInstallGateway {
  const view = { revision: 1, installFolder: "/home/developer/DLA Library", installs };
  return {
    readInstalls: vi.fn().mockResolvedValue(view),
    subscribeInstalls: vi.fn().mockResolvedValue(() => undefined),
    selectInstallFolder: vi.fn().mockResolvedValue(view),
    installArchive: vi.fn().mockResolvedValue({ ...view, revision: 2 }),
    cancelInstall: vi.fn().mockResolvedValue(view),
  };
}

function libraryGateway(overrides: {
  installation?: Installation;
  health?: InstallationHealthReport;
  own?: OwnWorkDetails;
} = {}): LibraryGateway {
  const installation = overrides.installation ?? gameInstallation();
  return {
    readInstallation: vi.fn().mockResolvedValue(installation),
    readOwnWorkDetails: vi.fn().mockResolvedValue(overrides.own ?? null),
    ownWorkArtworkUrl: vi.fn((installationId: string, artworkId: string) => `art:${installationId}:${artworkId}`),
    readInstallationHealth: vi.fn().mockResolvedValue(overrides.health ?? healthReport()),
    readPreparedPackage: vi.fn().mockResolvedValue(null),
    readPackagePreparationProgress: vi.fn().mockResolvedValue(null),
    subscribePackagePreparationProgress: vi.fn().mockResolvedValue(() => undefined),
    saveReview: vi.fn(async (request: InstallationReviewRequest) => ({
      ...installation,
      overrides: { ...installation.overrides, ...request, reviewedAt: "2026-10-08T10:00:00Z" },
    })),
    verifyInstallation: vi.fn().mockResolvedValue(overrides.health ?? healthReport()),
    rescanInstallation: vi.fn().mockResolvedValue(healthReport()),
    repairInstallation: vi.fn().mockResolvedValue(healthReport()),
    locateInstallation: vi.fn().mockResolvedValue(healthReport()),
    selectInstallationLocation: vi.fn().mockResolvedValue(null),
    cleanupMaintenance: vi.fn(),
    removeInstallation: vi.fn().mockResolvedValue(undefined),
    uninstallInstallation: vi.fn().mockResolvedValue(undefined),
    openInstallationFolder: vi.fn().mockResolvedValue(undefined),
    launchInstallation: vi.fn().mockResolvedValue(undefined),
    stopLaunch: vi.fn().mockResolvedValue(undefined),
    listRecentLaunches: vi.fn().mockResolvedValue([]),
  } as unknown as LibraryGateway;
}

function gameInstallation(): Installation {
  return {
    id: "installation-1",
    scanRootId: "root-1",
    rootPath: "/home/developer/My Works",
    platform: "linux",
    status: "ready",
    detection: {
      sourceScanSessionId: "session-1",
      catalogIdentity: { workCode: "RJ01000002", confidence: "strong", reasonCodes: ["code_in_directory_name"] },
      suggestedStatus: "needs_review",
      contentItems: [
        contentItem("KUNOICHI RED/Config.exe"),
        contentItem("KUNOICHI RED/Game.exe"),
      ],
      launchCandidates: [
        {
          id: "launch-game",
          action: "launch_executable",
          target: { kind: "relative_path", path: "KUNOICHI RED/Game.exe" },
          supportedPlatforms: ["windows"],
          confidence: "high",
          reasonCodes: ["preferred_executable_name"],
        },
        {
          id: "launch-config",
          action: "launch_executable",
          target: { kind: "relative_path", path: "KUNOICHI RED/Config.exe" },
          supportedPlatforms: ["windows"],
          confidence: "medium",
          reasonCodes: ["executable_candidate"],
        },
      ],
      packageInspection: null,
    },
    overrides: {
      catalogIdentity: null,
      customTitle: null,
      preferredAction: {
        action: "launch_executable",
        target: { kind: "relative_path", path: "KUNOICHI RED/Game.exe" },
      },
      contentItems: [],
      reviewedAt: "2026-10-08T09:00:00Z",
    },
    discoveredAt: "2026-10-08T09:00:00Z",
    updatedAt: "2026-10-08T09:00:00Z",
  };
}

function contentItem(
  relativePath: string,
  mediaType: Installation["detection"]["contentItems"][number]["mediaType"] = "executable",
): Installation["detection"]["contentItems"][number] {
  return {
    relativePath,
    pathKey: relativePath,
    mediaType,
    sizeBytes: 100,
    modifiedAt: null,
    confidence: "high",
    reasonCodes: ["executable_extension"],
  };
}

function packageInspection(): NonNullable<Installation["detection"]["packageInspection"]> {
  return {
    source: {
      scanEntryId: "archive-1",
      kind: "archive",
      relativePath: "RJ01000002.zip",
      sizeBytes: 512,
      sha256: "a".repeat(64),
    },
    format: "zip",
    safety: "safe",
    entryCount: 2,
    fileCount: 2,
    directoryCount: 1,
    totalCompressedBytes: 512,
    totalUncompressedBytes: 1_024,
    commonRoot: null,
    issues: [],
    classification: {
      contentKind: "windows_game",
      engine: null,
      platform: "windows",
      confidence: "high",
      reasonCodes: ["executable_dominant"],
      contentRoot: null,
      launchCandidates: [],
    },
    installPlan: {
      requiresExtraction: true,
      contentRoot: null,
      preferredAction: null,
      archiveRetention: "keep",
    },
    inspectedAt: "2026-10-08T09:00:00Z",
  };
}

function healthReport(overrides: Partial<InstallationHealthReport> = {}): InstallationHealthReport {
  return {
    installationId: "installation-1",
    state: "healthy",
    managed: true,
    repairable: false,
    checkedRoot: "/home/developer/DLA Library/RJ01000002",
    checkedAt: "2026-10-08T09:30:00Z",
    expectedFiles: 14,
    presentFiles: 14,
    missingFiles: 0,
    modifiedFiles: 0,
    inaccessibleFiles: 0,
    unexpectedFiles: 0,
    issues: [],
    ...overrides,
  };
}

function work(code: string, title: string): CatalogWork {
  return {
    code,
    sourceCode: code,
    title,
    titleEnglish: "",
    addedDate: "2026-10-01",
    releaseDate: "2026-10-01",
    updatedDate: "2026-10-01",
    ageRating: "all_ages",
    releaseType: "game",
    mainImageUrls: [],
    thumbnailUrls: [],
    circles: [],
    categories: [],
    tags: [],
    synthetic: true,
  };
}
