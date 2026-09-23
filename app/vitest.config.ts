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
    restoreMocks: true,
    /*
      Two runs of the same suite, split on whether the test wants CSS.

      Vitest replaces CSS imports with nothing, which is right for almost
      everything: no test should turn on a colour. A target size is the
      exception. WCAG 2.5.8 asks 24 by 24 CSS pixels of anything there is to
      press, the row under a message has been under it three times (#82, and
      #102 twice over), and every time the control took its height from the
      words or the glyph inside it, which is the shape where reading the
      stylesheet and believing it is exactly what fails. Processed, jsdom
      resolves the cascade and the floor can be read back.

      It cannot be switched on for everything, which is why this is a split
      rather than one flag. `RoomTimeline.css` hides the message toolbar until
      it is hovered, and `pointer-events: none` makes `userEvent` refuse to
      press those controls: 65 tests fail on styling that is doing its job. So
      `.css.test.tsx` is the name for a test that measures a stylesheet, and it
      is the only kind that gets one.
    */
    projects: [
      {
        extends: true,
        test: {
          name: "behaviour",
          include: ["src/**/*.test.{ts,tsx}"],
          exclude: ["src/**/*.css.test.tsx"],
        },
      },
      {
        extends: true,
        test: {
          name: "styles",
          include: ["src/**/*.css.test.tsx"],
          css: true,
        },
      },
    ],
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
