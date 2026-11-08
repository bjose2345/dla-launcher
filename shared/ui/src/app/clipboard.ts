type ClipboardWriter = (text: string) => Promise<void>;

let writer: ClipboardWriter = (text) => navigator.clipboard
  ? navigator.clipboard.writeText(text)
  : Promise.reject(new Error("The clipboard is unavailable."));

/**
 * Lets the app copy through its own clipboard (Tauri's clipboard plugin). The
 * page's clipboard API is the fallback, and Chromium refuses it while the page
 * has no focus, as on the first click after coming back from another app.
 */
export function installClipboardWriter(write: ClipboardWriter): void {
  writer = write;
}

export function copyText(text: string): Promise<void> {
  return writer(text);
}
