import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    env: {
      TZ: "UTC",
    },
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: ["./src/test/setup.ts"],
    environmentOptions: {
      jsdom: {
        url: "http://localhost/",
      },
    },
  },
});
