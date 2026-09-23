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
    /*
      One stylesheet, processed rather than stubbed.

      Vitest replaces CSS imports with nothing by default, which is right for
      almost everything here: no test should turn on a colour. The exception is
      a target size. WCAG 2.5.8 asks 24 by 24 CSS pixels of anything you have
      to hit, this repository has been under it twice (#82, and #102 is still
      open), and both times the control took its height from the words in it,
      which is the shape where reading the rule and believing it is exactly
      what fails. With the file processed, jsdom resolves the cascade and the
      floor can be measured through the same import the component uses, so a
      rule that stops matching fails the test as loudly as a number that drops.
    */
    css: { include: [/CallPanel\.css$/] },
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
