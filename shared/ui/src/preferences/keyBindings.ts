import type { MessageKey } from "../i18n/catalogs";

export const bindingScopes = ["global", "playback", "video", "reader"] as const;

export type BindingScope = (typeof bindingScopes)[number];

export interface BindingAction {
  id: string;
  scope: BindingScope;
  labelKey: MessageKey;
  helpKey?: MessageKey;
  defaults: readonly string[];
  slots: number;
  fixed?: boolean;
}

export const bindingActions: readonly BindingAction[] = [
  { id: "search", scope: "global", labelKey: "settings.action.search", defaults: ["ctrl+k"], slots: 2 },
  { id: "toggleSidebar", scope: "global", labelKey: "settings.action.toggleSidebar", defaults: ["ctrl+b"], slots: 2 },
  { id: "openSettings", scope: "global", labelKey: "settings.action.openSettings", defaults: ["ctrl+,"], slots: 2 },

  { id: "playPause", scope: "playback", labelKey: "settings.action.playPause", helpKey: "settings.action.playPauseHelp", defaults: ["ctrl+space"], slots: 2 },
  { id: "nextTrack", scope: "playback", labelKey: "settings.action.nextTrack", defaults: ["ctrl+arrowright"], slots: 2 },
  { id: "previousTrack", scope: "playback", labelKey: "settings.action.previousTrack", defaults: ["ctrl+arrowleft"], slots: 2 },
  { id: "toggleMute", scope: "playback", labelKey: "settings.action.toggleMute", defaults: ["ctrl+m"], slots: 2 },

  { id: "videoPlayPause", scope: "video", labelKey: "settings.action.playPause", defaults: ["space", "k"], slots: 2 },
  { id: "videoSkipBack", scope: "video", labelKey: "settings.action.skipBack", defaults: ["arrowleft", "j"], slots: 2 },
  { id: "videoSkipForward", scope: "video", labelKey: "settings.action.skipForward", defaults: ["arrowright", "l"], slots: 2 },
  { id: "videoVolumeUp", scope: "video", labelKey: "settings.action.volumeUp", defaults: ["arrowup"], slots: 2 },
  { id: "videoVolumeDown", scope: "video", labelKey: "settings.action.volumeDown", defaults: ["arrowdown"], slots: 2 },
  { id: "videoMute", scope: "video", labelKey: "settings.action.toggleMute", defaults: ["m"], slots: 2 },
  { id: "videoSubtitles", scope: "video", labelKey: "settings.action.subtitles", defaults: ["c"], slots: 2 },
  { id: "videoFullscreen", scope: "video", labelKey: "settings.action.fullscreen", defaults: ["f"], slots: 2 },
  { id: "videoNext", scope: "video", labelKey: "settings.action.nextTrack", defaults: ["n"], slots: 2 },
  { id: "videoPrevious", scope: "video", labelKey: "settings.action.previousTrack", defaults: ["p"], slots: 2 },

  { id: "readerNextPage", scope: "reader", labelKey: "settings.action.nextPage", defaults: ["arrowright"], slots: 2 },
  { id: "readerPreviousPage", scope: "reader", labelKey: "settings.action.previousPage", defaults: ["arrowleft"], slots: 2 },
  { id: "readerScrollBack", scope: "reader", labelKey: "settings.action.scrollBack", defaults: ["arrowup", "pageup"], slots: 2 },
  { id: "readerScrollForward", scope: "reader", labelKey: "settings.action.scrollForward", defaults: ["arrowdown", "pagedown", "space"], slots: 3 },
  { id: "readerZoomIn", scope: "reader", labelKey: "settings.action.zoomIn", defaults: ["plus", "="], slots: 2 },
  { id: "readerZoomOut", scope: "reader", labelKey: "settings.action.zoomOut", defaults: ["-"], slots: 2 },
  { id: "readerResetZoom", scope: "reader", labelKey: "settings.action.resetZoom", defaults: ["0"], slots: 2 },
  { id: "readerClose", scope: "reader", labelKey: "settings.action.close", defaults: ["escape"], slots: 1, fixed: true },
];

export type KeyBindingOverrides = Record<string, string[]>;
export type ResolvedBindings = Record<string, readonly string[]>;

const STORAGE_KEY = "dla-launcher:key-bindings:v1";
const actionsById = new Map(bindingActions.map((action) => [action.id, action]));

const displayNames: Record<string, string> = {
  arrowleft: "←",
  arrowright: "→",
  arrowup: "↑",
  arrowdown: "↓",
  plus: "+",
  space: "Space",
  escape: "Esc",
  pageup: "Page Up",
  pagedown: "Page Down",
  enter: "Enter",
  backspace: "Backspace",
  tab: "Tab",
  delete: "Delete",
  home: "Home",
  end: "End",
};

export function eventCombo(event: KeyboardEvent): string | null {
  const key = event.key;
  if (
    key === "Control"
    || key === "Meta"
    || key === "Alt"
    || key === "Shift"
    || key === "Dead"
    || key === "Process"
    || key === "Unidentified"
  ) return null;
  const parts: string[] = [];
  if (event.ctrlKey || event.metaKey) parts.push("ctrl");
  if (event.altKey) parts.push("alt");
  if (event.shiftKey && key !== "+") parts.push("shift");
  parts.push(key === " " ? "space" : key === "+" ? "plus" : key.toLowerCase());
  return parts.join("+");
}

export function formatCombo(combo: string): string {
  const normalized = normalizeCombo(combo) ?? combo;
  return normalized
    .split("+")
    .map((part) => {
      if (part === "ctrl") return "Ctrl";
      if (part === "alt") return "Alt";
      if (part === "shift") return "Shift";
      const named = displayNames[part];
      if (named) return named;
      return part.length === 1 ? part.toUpperCase() : part;
    })
    .join(" ");
}

export function ariaKeyShortcuts(combos: readonly string[]): string | undefined {
  const shortcuts = combos.flatMap((combo) => {
    const normalized = normalizeCombo(combo);
    if (!normalized) return [];
    const parts = normalized.split("+");
    const hasControl = parts[0] === "ctrl";
    const variants = hasControl ? ["Control", "Meta"] : [null];
    return variants.map((control) => {
      const formatted = parts.flatMap((part) => {
        if (part === "ctrl") return control ? [control] : [];
        if (part === "alt") return ["Alt"];
        if (part === "shift") return ["Shift"];
        return [ariaKeyName(part)];
      });
      return formatted.join("+");
    });
  });
  return shortcuts.length ? shortcuts.join(" ") : undefined;
}

export function readKeyBindingOverrides(): KeyBindingOverrides {
  if (typeof window === "undefined") return {};
  try {
    const stored = JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? "{}") as unknown;
    return sanitizeOverrides(stored);
  } catch {
    return {};
  }
}

export function writeKeyBindingOverrides(overrides: KeyBindingOverrides): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(overrides));
  } catch {
    return;
  }
}

export function sanitizeOverrides(value: unknown): KeyBindingOverrides {
  if (!value || typeof value !== "object") return {};
  const overrides: KeyBindingOverrides = {};
  for (const [id, combos] of Object.entries(value as Record<string, unknown>)) {
    const action = actionsById.get(id);
    if (!action || action.fixed || !Array.isArray(combos)) continue;
    const cleaned = [...new Set(combos.flatMap((combo) => {
      if (typeof combo !== "string") return [];
      const normalized = normalizeCombo(combo);
      return normalized ? [normalized] : [];
    }))].slice(0, action.slots);
    if (!sameBindings(cleaned, action.defaults)) overrides[id] = cleaned;
  }
  return overrides;
}

export function resolveBindings(overrides: KeyBindingOverrides): ResolvedBindings {
  const resolved: ResolvedBindings = {};
  for (const action of bindingActions) {
    const override = action.fixed ? undefined : overrides[action.id];
    resolved[action.id] = override ?? action.defaults;
  }
  const ordered = [
    ...bindingActions.filter((action) => action.fixed),
    ...bindingActions.filter((action) => !action.fixed && Object.hasOwn(overrides, action.id)),
    ...bindingActions.filter((action) => !action.fixed && !Object.hasOwn(overrides, action.id)),
  ];
  const claimed: Array<{ action: BindingAction; combo: string }> = [];
  for (const action of ordered) {
    const accepted = bindingsForAction(resolved, action.id).filter((combo) => !claimed.some((owner) => (
      owner.combo === combo && scopesOverlap(owner.action.scope, action.scope)
    )));
    resolved[action.id] = accepted;
    claimed.push(...accepted.map((combo) => ({ action, combo })));
  }
  return resolved;
}

export function bindingsForAction(resolved: ResolvedBindings, actionId: string): readonly string[] {
  return resolved[actionId] ?? [];
}

const alwaysLiveScopes: readonly BindingScope[] = ["global", "playback"];

export function scopesOverlap(left: BindingScope, right: BindingScope): boolean {
  if (left === right) return true;
  return alwaysLiveScopes.includes(left) || alwaysLiveScopes.includes(right);
}

export function conflictsWith(
  resolved: ResolvedBindings,
  actionId: string,
  combo: string,
): string | null {
  return bindingConflicts(resolved, actionId, combo)[0]?.id ?? null;
}

export function bindingConflicts(
  resolved: ResolvedBindings,
  actionId: string,
  combo: string,
): readonly BindingAction[] {
  const action = actionsById.get(actionId);
  const normalized = normalizeCombo(combo);
  if (!action || !normalized) return [];
  return bindingActions.filter((other) => (
    other.id !== actionId
    && scopesOverlap(action.scope, other.scope)
    && bindingsForAction(resolved, other.id).includes(normalized)
  ));
}

export function assignBinding(
  overrides: KeyBindingOverrides,
  resolved: ResolvedBindings,
  actionId: string,
  slot: number,
  combo: string,
): KeyBindingOverrides {
  const action = actionsById.get(actionId);
  const normalized = normalizeCombo(combo);
  if (!action || action.fixed || !normalized) return overrides;
  const current = [...bindingsForAction(resolved, actionId)];
  if (current.includes(normalized)) return overrides;
  const conflicts = bindingConflicts(resolved, actionId, normalized);
  if (conflicts.some((conflict) => conflict.fixed)) return overrides;
  const next: KeyBindingOverrides = { ...overrides };
  for (const conflict of conflicts) {
    next[conflict.id] = bindingsForAction(resolved, conflict.id)
      .filter((value) => value !== normalized);
  }
  while (current.length <= slot) current.push("");
  current[slot] = normalized;
  next[actionId] = current.filter((value) => value.length > 0).slice(0, action.slots);
  return next;
}

export function clearBinding(
  overrides: KeyBindingOverrides,
  resolved: ResolvedBindings,
  actionId: string,
  slot: number,
): KeyBindingOverrides {
  const action = actionsById.get(actionId);
  if (!action || action.fixed) return overrides;
  const current = [...bindingsForAction(resolved, actionId)];
  current.splice(slot, 1);
  return { ...overrides, [actionId]: current };
}

export function resetBinding(overrides: KeyBindingOverrides, actionId: string): KeyBindingOverrides {
  const next = { ...overrides };
  delete next[actionId];
  return next;
}

export function actionsInScope(scope: BindingScope): readonly BindingAction[] {
  return bindingActions.filter((action) => action.scope === scope);
}

export function matchBinding(
  resolved: ResolvedBindings,
  scope: BindingScope,
  combo: string,
): string | null {
  for (const action of bindingActions) {
    if (action.scope !== scope) continue;
    if (bindingsForAction(resolved, action.id).includes(combo)) return action.id;
  }
  return null;
}

export function isInteractiveTarget(target: EventTarget | null): boolean {
  return target instanceof Element && Boolean(target.closest(
    "button, a[href], input, select, textarea, [contenteditable]:not([contenteditable='false']), [role='button'], [role='combobox'], [role='link'], [role='slider'], [role='textbox']",
  ));
}

function normalizeCombo(combo: string): string | null {
  const value = combo.trim().toLowerCase();
  if (value === "+") return "plus";
  if (!value || value.length > 80) return null;
  const parts = value.split("+");
  const key = parts.pop();
  if (!key || key.trim() !== key || key === "ctrl" || key === "meta" || key === "alt" || key === "shift") {
    return null;
  }
  const modifiers = new Set<string>();
  for (const modifier of parts) {
    const canonical = modifier === "meta" ? "ctrl" : modifier;
    if (canonical !== "ctrl" && canonical !== "alt" && canonical !== "shift") return null;
    modifiers.add(canonical);
  }
  return ["ctrl", "alt", "shift"]
    .filter((modifier) => modifiers.has(modifier))
    .concat(key === " " ? "space" : key === "+" ? "plus" : key)
    .join("+");
}

function ariaKeyName(key: string): string {
  const names: Record<string, string> = {
    arrowleft: "ArrowLeft",
    arrowright: "ArrowRight",
    arrowup: "ArrowUp",
    arrowdown: "ArrowDown",
    backspace: "Backspace",
    delete: "Delete",
    end: "End",
    enter: "Enter",
    escape: "Escape",
    home: "Home",
    pagedown: "PageDown",
    pageup: "PageUp",
    plus: "Plus",
    space: "Space",
    tab: "Tab",
  };
  return names[key] ?? (key.length === 1 ? key.toUpperCase() : key);
}

function sameBindings(left: readonly string[], right: readonly string[]): boolean {
  return left.length === right.length && left.every((binding, index) => binding === right[index]);
}
