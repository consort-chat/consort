import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const appearanceSettings = vi.hoisted(() => vi.fn());
const setAppearanceSettings = vi.hoisted(() => vi.fn());
const previewApplicationScale = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  appearanceSettings,
  setAppearanceSettings,
  previewApplicationScale,
}));

import { AccessibilitySection } from "./AccessibilitySection";
import { zoomed } from "../lib/scale";

const AS_DRAWN = { applicationScale: 1, textScale: 1 };

function application(): HTMLInputElement {
  return screen.getByRole("slider", { name: /application scale/i });
}

function text(): HTMLInputElement {
  return screen.getByRole("slider", { name: /text size/i });
}

describe("AccessibilitySection", () => {
  beforeEach(() => {
    appearanceSettings.mockReset().mockResolvedValue(AS_DRAWN);
    setAppearanceSettings.mockReset().mockResolvedValue(undefined);
    previewApplicationScale.mockReset().mockResolvedValue(undefined);
    document.documentElement.style.removeProperty("font-size");
  });

  it("draws both sliders at the sizes that were saved", async () => {
    appearanceSettings.mockResolvedValue({
      applicationScale: 1.3,
      textScale: 1.15,
    });
    render(<AccessibilitySection />);

    await waitFor(() => expect(application()).toHaveValue("130"));
    expect(text()).toHaveValue("115");
  });

  it("reads both sizes out in percent beside their sliders", async () => {
    appearanceSettings.mockResolvedValue({
      applicationScale: 1.3,
      textScale: 1.15,
    });
    render(<AccessibilitySection />);

    expect(await screen.findByText("130%")).toBeVisible();
    expect(screen.getByText("115%")).toBeVisible();
  });

  it("scales the window as the application slider moves, before any write", async () => {
    // The whole value of a slider here is watching the size change while it is
    // being dragged. A window that only moved on release would be a number
    // entry with a thumb on it.
    render(<AccessibilitySection />);
    await waitFor(() => expect(application()).toHaveValue("100"));

    fireEvent.change(application(), { target: { value: "150" } });

    expect(previewApplicationScale).toHaveBeenCalledWith(1.5);
    expect(application()).toHaveValue("150");
    expect(setAppearanceSettings).not.toHaveBeenCalled();
  });

  it("sizes the page's words as the text slider moves, before any write", async () => {
    render(<AccessibilitySection />);
    await waitFor(() => expect(text()).toHaveValue("100"));

    fireEvent.change(text(), { target: { value: "125" } });

    expect(document.documentElement.style.fontSize).toBe("125%");
    expect(text()).toHaveValue("125");
    expect(setAppearanceSettings).not.toHaveBeenCalled();
  });

  it("writes the application scale once the pointer stops", async () => {
    render(<AccessibilitySection />);
    await waitFor(() => expect(application()).toHaveValue("100"));

    fireEvent.change(application(), { target: { value: "150" } });

    await waitFor(() =>
      expect(setAppearanceSettings).toHaveBeenCalledWith({
        applicationScale: 1.5,
        textScale: 1,
      }),
    );
  });

  it("writes the text size once the pointer stops", async () => {
    render(<AccessibilitySection />);
    await waitFor(() => expect(text()).toHaveValue("100"));

    fireEvent.change(text(), { target: { value: "125" } });

    await waitFor(() =>
      expect(setAppearanceSettings).toHaveBeenCalledWith({
        applicationScale: 1,
        textScale: 1.25,
      }),
    );
  });

  it("writes once for a slider dragged across the range", async () => {
    // A range input fires an event per step, and writing the settings file on
    // each of them would be a hundred rewrites for one adjustment.
    render(<AccessibilitySection />);
    await waitFor(() => expect(application()).toHaveValue("100"));

    for (const value of ["110", "120", "130", "140"]) {
      fireEvent.change(application(), { target: { value } });
    }

    await waitFor(() => expect(setAppearanceSettings).toHaveBeenCalled());
    expect(setAppearanceSettings).toHaveBeenCalledTimes(1);
    expect(setAppearanceSettings).toHaveBeenCalledWith({
      applicationScale: 1.4,
      textScale: 1,
    });
  });

  it("scales the window on every step of that same drag", async () => {
    // The other half of the pair above: the write is debounced, the drawing
    // must not be.
    render(<AccessibilitySection />);
    await waitFor(() => expect(application()).toHaveValue("100"));

    for (const value of ["110", "120", "130", "140"]) {
      fireEvent.change(application(), { target: { value } });
    }

    expect(previewApplicationScale.mock.calls.map(([scale]) => scale)).toEqual([
      1.1, 1.2, 1.3, 1.4,
    ]);
  });

  it("leaves the two knobs independent of each other", async () => {
    // They are two different things, and a text size that moved the window as
    // well would be the pair collapsing into one control with two thumbs.
    render(<AccessibilitySection />);
    await waitFor(() => expect(text()).toHaveValue("100"));

    fireEvent.change(text(), { target: { value: "140" } });

    expect(previewApplicationScale).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(setAppearanceSettings).toHaveBeenCalledWith({
        applicationScale: 1,
        textScale: 1.4,
      }),
    );
  });

  it("keeps a chosen text size when the application slider moves", async () => {
    // The command takes both, and this screen holds both. A write that sent
    // only the number that changed would quietly put the other back to 100%,
    // and the symptom is somebody losing their text size every time they touch
    // the scale above it.
    appearanceSettings.mockResolvedValue({
      applicationScale: 1,
      textScale: 1.3,
    });
    render(<AccessibilitySection />);
    await waitFor(() => expect(text()).toHaveValue("130"));

    fireEvent.change(application(), { target: { value: "150" } });

    await waitFor(() =>
      expect(setAppearanceSettings).toHaveBeenCalledWith({
        applicationScale: 1.5,
        textScale: 1.3,
      }),
    );
  });

  it("keeps a chosen application scale when the text slider moves", async () => {
    appearanceSettings.mockResolvedValue({
      applicationScale: 1.6,
      textScale: 1,
    });
    render(<AccessibilitySection />);
    await waitFor(() => expect(application()).toHaveValue("160"));

    fireEvent.change(text(), { target: { value: "120" } });

    await waitFor(() =>
      expect(setAppearanceSettings).toHaveBeenCalledWith({
        applicationScale: 1.6,
        textScale: 1.2,
      }),
    );
  });

  it("writes what was still pending when the screen closes", async () => {
    // Somebody who drags a slider and shuts settings straight away has still
    // made the change. Losing the last hundred and fifty milliseconds of it
    // reads as a setting that does not stick.
    const { unmount } = render(<AccessibilitySection />);
    await waitFor(() => expect(application()).toHaveValue("100"));
    fireEvent.change(application(), { target: { value: "170" } });

    unmount();

    expect(setAppearanceSettings).toHaveBeenCalledWith({
      applicationScale: 1.7,
      textScale: 1,
    });
  });

  it("catches up when the keyboard changed the scale underneath it", async () => {
    // Ctrl and plus works with this screen open, and a slider left showing a
    // size the window no longer is would snap it back on the next drag.
    render(<AccessibilitySection />);
    await waitFor(() => expect(application()).toHaveValue("100"));
    appearanceSettings.mockResolvedValue({
      applicationScale: 1.2,
      textScale: 1,
    });

    zoomed();

    await waitFor(() => expect(application()).toHaveValue("120"));
  });

  it("stops listening for that once it is gone", async () => {
    const { unmount } = render(<AccessibilitySection />);
    await waitFor(() => expect(appearanceSettings).toHaveBeenCalled());
    unmount();
    appearanceSettings.mockClear();

    zoomed();

    expect(appearanceSettings).not.toHaveBeenCalled();
  });

  it("says what the keys are, since nothing else on screen would", async () => {
    // Printed rather than only bound, because the shortcut is what somebody
    // reaches for once the window has become too large to read the rest of,
    // and nothing else on screen would tell them it exists.
    render(<AccessibilitySection />);
    await waitFor(() => expect(application()).toHaveValue("100"));

    const keys = screen
      .getAllByText(/^(Ctrl|\+|-|0)$/)
      .map((key) => key.textContent);

    expect(keys).toEqual(expect.arrayContaining(["Ctrl", "+", "-", "0"]));
  });

  it("says so when the sizes cannot be read", async () => {
    appearanceSettings.mockRejectedValue({
      message: "Could not read your settings.",
      detail: "ipc died",
    });
    render(<AccessibilitySection />);

    expect(
      await screen.findByText("Could not read your settings."),
    ).toBeVisible();
  });

  it("says so when a size cannot be written", async () => {
    setAppearanceSettings.mockRejectedValue({
      message: "Could not save your settings.",
      detail: "read-only",
    });
    render(<AccessibilitySection />);
    await waitFor(() => expect(application()).toHaveValue("100"));

    fireEvent.change(application(), { target: { value: "150" } });

    expect(
      await screen.findByText("Could not save your settings."),
    ).toBeVisible();
  });

  it("leaves the window at the size it is showing when the write failed", async () => {
    // The window has already moved. Putting the slider back would leave the
    // two disagreeing, and the one somebody can see is the window.
    setAppearanceSettings.mockRejectedValue({
      message: "Could not save your settings.",
      detail: "read-only",
    });
    render(<AccessibilitySection />);
    await waitFor(() => expect(application()).toHaveValue("100"));

    fireEvent.change(application(), { target: { value: "150" } });
    await screen.findByText("Could not save your settings.");

    expect(application()).toHaveValue("150");
  });
});
