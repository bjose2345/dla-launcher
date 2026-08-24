import { beforeEach } from "vitest";

class TestResizeObserver implements ResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

beforeEach(() => {
  if (typeof window === "undefined") return;

  if (!("ResizeObserver" in window)) {
    Object.defineProperty(window, "ResizeObserver", {
      configurable: true,
      writable: true,
      value: TestResizeObserver,
    });
  }

  if (typeof window.matchMedia !== "function") {
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      writable: true,
      value: (query: string): MediaQueryList => ({
        matches: false,
        media: query,
        onchange: null,
        addEventListener: () => {},
        removeEventListener: () => {},
        addListener: () => {},
        removeListener: () => {},
        dispatchEvent: () => false,
      }),
    });
  }

  const values = new Map<string, string>();
  const storage: Storage = {
    clear: () => values.clear(),
    getItem: (key) => values.get(key) ?? null,
    key: (index) => [...values.keys()][index] ?? null,
    get length() {
      return values.size;
    },
    removeItem: (key) => void values.delete(key),
    setItem: (key, value) => void values.set(key, value),
  };

  Object.defineProperty(window, "localStorage", {
    configurable: true,
    value: storage,
  });
});
