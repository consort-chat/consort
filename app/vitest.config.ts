import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

/**
 * Separate from vite.config.ts on purpose.
 *
 * The app config fixes the dev server to port 1420 and refuses to move, which
 * is correct for `tauri dev` and wrong for a test run that may happen while
 * the app is open. Keeping the two apart also means a change to the test setup
 * cannot alter what gets built and shipped.
 */
export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
    restoreMocks: true,
    coverage: {
      provider: "v8",
      reporter: ["text-summary", "lcov"],
      include: ["src/**/*.{ts,tsx}"],
      exclude: [
        // The entry point. Three lines that mount React into a div, and
        // exercising them means asserting that ReactDOM works.
        "src/main.tsx",
        "src/test/**",
        "src/**/*.test.{ts,tsx}",
      ],
      thresholds: {
        statements: 90,
        branches: 90,
        functions: 90,
        lines: 90,
      },
    },
  },
});
