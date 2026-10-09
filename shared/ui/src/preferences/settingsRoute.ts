export const settingsTabs = ["general", "library", "display", "controls"] as const;

export type SettingsTab = (typeof settingsTabs)[number];

export function parseSettingsTab(value: unknown): SettingsTab {
  return settingsTabs.find((tab) => tab === value) ?? "general";
}

export interface SettingsRouteSearch {
  tab: SettingsTab;
  legacyAbout?: true;
}

export function parseSettingsSearch(search: Record<string, unknown>): SettingsRouteSearch {
  if (search.tab === "about") return { tab: "general", legacyAbout: true };
  return { tab: parseSettingsTab(search.tab) };
}
