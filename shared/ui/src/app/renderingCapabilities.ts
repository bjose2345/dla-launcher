/**
 * What the webview renders cheaply. WebKitGTK composites a `filter: blur()`
 * over a large layer in software, so the shared UI blurs on a canvas unless
 * the host says the webview is Chromium.
 */
export interface RenderingCapabilities {
  cssBlur: boolean;
}

let capabilities: RenderingCapabilities = { cssBlur: false };

export function installRenderingCapabilities(next: RenderingCapabilities): void {
  capabilities = next;
}

export function renderingCapabilities(): RenderingCapabilities {
  return capabilities;
}
