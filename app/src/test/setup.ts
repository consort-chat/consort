/**
 * Test setup, loaded before every file.
 *
 * Three jobs. Bring in jest-dom's matchers so assertions read as `toBeVisible`
 * rather than a chain of property checks, make sure nothing in the suite can
 * reach the real Tauri IPC (`@tauri-apps/api/core` is mocked per test file,
 * and a component that slips an unmocked `invoke` through should fail loudly
 * rather than hang), and stand in for the part of the Tauri runtime that is
 * not IPC.
 *
 * That last one matters because `@tauri-apps/api` is a shell: the real
 * implementations are injected into the webview at startup and hang off
 * `window.__TAURI_INTERNALS__`, which jsdom has no way to produce. Without it
 * a component that draws an attachment dies inside a dependency, naming
 * nothing. `mockConvertFileSrc` is Tauri's own stand-in, taken rather than
 * written because the thing it reproduces is a platform difference and a
 * hand-rolled one would drift from the rule it is supposed to be checking.
 */
import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { mockConvertFileSrc } from "@tauri-apps/api/mocks";
import { afterEach, beforeEach, vi } from "vitest";

// Linux unless a test says otherwise, and reset here rather than at the end so
// that a test which changed it cannot leak a platform into the next one.
beforeEach(() => {
  mockConvertFileSrc("linux");
});

afterEach(() => {
  cleanup();
});

// jsdom does not implement this either, and it does no layout to implement it
// against, so a component that scrolls one of its own children into view dies
// on the call. `mock.contexts` is what a test reads to see which child.
if (!Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = vi.fn();
}

// jsdom does not implement this, and React logs a warning without it.
if (!window.matchMedia) {
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  }));
}
