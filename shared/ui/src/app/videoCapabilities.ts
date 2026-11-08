/**
 * Which picture the host plays video on. A host with a native picture (a
 * webview window on Windows and WebKitGTK, GStreamer over the page on a
 * Chromium build carrying that fallback) answers the native video calls of the
 * gateway; a host without one plays in the page's own `<video>` element, and
 * the player must not ask for a native surface even though the gateway has the
 * methods. The Tauri host asks its build at startup (`read_video_backend`).
 */
export interface VideoCapabilities {
  nativeVideo: boolean;
}

let capabilities: VideoCapabilities = { nativeVideo: true };

export function installVideoCapabilities(next: VideoCapabilities): void {
  capabilities = next;
}

export function videoCapabilities(): VideoCapabilities {
  return capabilities;
}
