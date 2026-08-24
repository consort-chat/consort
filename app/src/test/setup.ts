/**
 * Test setup, loaded before every file.
 *
 * Two jobs. Bring in jest-dom's matchers so assertions read as `toBeVisible`
 * rather than a chain of property checks, and make sure nothing in the suite
 * can reach the real Tauri IPC: `@tauri-apps/api/core` is mocked per test file,
 * and a component that slips an unmocked `invoke` through should fail loudly
 * rather than hang.
 */
import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, vi } from "vitest";

afterEach(() => {
  cleanup();
});

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
