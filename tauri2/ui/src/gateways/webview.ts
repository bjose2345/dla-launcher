/**
 * The engine behind the page. The Chromium build and Windows' WebView2 are
 * Chromium; the system webview on Linux is WebKitGTK, the one that needs the
 * render watchdog and cannot blur a large layer cheaply.
 */
export const chromiumWebview = __DLA_WEBVIEW__ === "chromium" || /Windows NT/.test(navigator.userAgent);

export const webkitGtkWebview = __DLA_WEBVIEW__ === "system"
  && /Linux/.test(navigator.userAgent)
  && !/Android/.test(navigator.userAgent);
