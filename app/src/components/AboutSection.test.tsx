import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const appVersion = vi.hoisted(() => vi.fn());
const updatesItself = vi.hoisted(() => vi.fn());
const updateCheck = vi.hoisted(() => vi.fn());

vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  appVersion,
  updatesItself,
  updateCheck,
}));

import { AboutSection } from "./AboutSection";

/** The control, which half of these tests are about the absence of. */
const checkButton = () => screen.queryByRole("button", { name: /check for updates/i });

describe("AboutSection", () => {
  beforeEach(() => {
    appVersion.mockReset().mockResolvedValue("0.12.0");
    updatesItself.mockReset().mockResolvedValue(true);
    updateCheck.mockReset().mockResolvedValue({ state: "upToDate" });
  });

  it("says which version is running", async () => {
    render(<AboutSection />);

    expect(await screen.findByText(/0\.12\.0/)).toBeVisible();
  });

  it("looks when asked, and says so when there is nothing to find", async () => {
    render(<AboutSection />);
    await waitFor(() => expect(checkButton()).not.toBeNull());

    await userEvent.click(checkButton() as HTMLElement);

    expect(await screen.findByText(/up to date/i)).toBeVisible();
    expect(updateCheck).toHaveBeenCalledTimes(1);
  });

  it("names the release it found, and what to do about it", async () => {
    // The bar that installs it is behind this modal, so the sentence has to
    // say the one thing that is in the way.
    updateCheck.mockResolvedValue({
      state: "ready",
      version: "0.13.0",
      notes: "https://example.invalid",
    });
    render(<AboutSection />);
    await waitFor(() => expect(checkButton()).not.toBeNull());

    await userEvent.click(checkButton() as HTMLElement);

    expect(await screen.findByText(/0\.13\.0 is available/)).toBeVisible();
    expect(screen.getByText(/close settings/i)).toBeVisible();
  });

  it("passes on what went wrong when a look does not work", async () => {
    updateCheck.mockResolvedValue({
      state: "failed",
      reason: "Consort could not reach the update server.",
    });
    render(<AboutSection />);
    await waitFor(() => expect(checkButton()).not.toBeNull());

    await userEvent.click(checkButton() as HTMLElement);

    expect(
      await screen.findByText("Consort could not reach the update server."),
    ).toBeVisible();
  });

  it("says it is looking while it looks", async () => {
    // A press that reports nothing for the length of a network round trip
    // reads as a press that did nothing.
    updateCheck.mockReturnValue(new Promise(() => {}));
    render(<AboutSection />);
    await waitFor(() => expect(checkButton()).not.toBeNull());

    await userEvent.click(checkButton() as HTMLElement);

    expect(await screen.findByText(/checking/i)).toBeVisible();
  });

  it("does not start a second look while one is running", async () => {
    updateCheck.mockReturnValue(new Promise(() => {}));
    render(<AboutSection />);
    await waitFor(() => expect(checkButton()).not.toBeNull());

    await userEvent.click(checkButton() as HTMLElement);
    await userEvent.click(checkButton() as HTMLElement);

    expect(updateCheck).toHaveBeenCalledTimes(1);
  });

  it("says so when the look could not even be asked for", async () => {
    // Not the same as a look that failed: the command answers with a state of
    // its own for that. This is the IPC itself going away under it.
    updateCheck.mockImplementation(() =>
      Promise.reject({ message: "no answer from Consort", detail: "" }),
    );
    render(<AboutSection />);
    await waitFor(() => expect(checkButton()).not.toBeNull());

    await userEvent.click(checkButton() as HTMLElement);

    expect(await screen.findByText("no answer from Consort")).toBeVisible();
    await waitFor(() => expect(checkButton()).not.toBeDisabled());
  });

  it("offers no control at all in a build that does not update itself", async () => {
    // Every Linux package and every build from source. A disabled button here
    // is a bug report about something working as intended.
    updatesItself.mockResolvedValue(false);
    render(<AboutSection />);

    expect(await screen.findByText(/package manager/i)).toBeVisible();
    expect(checkButton()).toBeNull();
  });

  it("still says which version is running in that build", async () => {
    updatesItself.mockResolvedValue(false);
    render(<AboutSection />);

    expect(await screen.findByText(/0\.12\.0/)).toBeVisible();
  });

  it("claims nothing about updating when the question is not answered", async () => {
    // Neither a control nor a sentence about package managers: an unanswered
    // question is not an answer, and guessing one would print something false.
    updatesItself.mockImplementation(() => Promise.reject({ message: "no answer", detail: "" }));
    render(<AboutSection />);

    expect(await screen.findByText("no answer")).toBeVisible();
    expect(checkButton()).toBeNull();
    expect(screen.queryByText(/package manager/i)).toBeNull();
  });

  it("says so when it cannot even read its own version", async () => {
    appVersion.mockImplementation(() => Promise.reject({ message: "no version", detail: "" }));
    render(<AboutSection />);

    expect(await screen.findByText("no version")).toBeVisible();
  });
});
