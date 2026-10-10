import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { UpdateBar, progress } from "./UpdateBar";
import type { Update } from "../lib/api";

const READY: Update = {
  state: "ready",
  version: "0.12.0",
  notes: "https://github.com/consort-chat/consort/releases/tag/v0.12.0",
};

function bar(update: Update | null, inACall = false) {
  const onInstall = vi.fn();
  const onDismiss = vi.fn();
  render(
    <UpdateBar
      update={update}
      inACall={inACall}
      onInstall={onInstall}
      onDismiss={onDismiss}
    />,
  );
  return { onInstall, onDismiss };
}

describe("UpdateBar", () => {
  it("draws nothing at all when nothing was ever said", () => {
    // A packaged build, which never speaks on the channel. Not a disabled
    // control and not an "updates unavailable" notice: those invite a bug
    // report about a thing that is working as intended.
    bar(null);

    expect(screen.queryByRole("status")).toBeNull();
  });

  it("draws nothing when this is already the newest release", () => {
    bar({ state: "upToDate" });

    expect(screen.queryByRole("status")).toBeNull();
  });

  it("names the version on offer", () => {
    bar(READY);

    expect(screen.getByText(/0\.12\.0/)).toBeVisible();
  });

  it("installs on a press", async () => {
    const { onInstall } = bar(READY);

    await userEvent.click(screen.getByRole("button", { name: /update/i }));

    expect(onInstall).toHaveBeenCalledOnce();
  });

  it("refuses to install during a call, and says why", async () => {
    // Consort is a voice client. An update that restarts it mid-sentence is a
    // defect, so the control says what is in the way rather than going grey.
    const { onInstall } = bar(READY, true);

    const button = screen.getByRole("button", { name: /update/i });
    expect(button).toBeDisabled();
    expect(screen.getByText(/call/i)).toBeVisible();

    await userEvent.click(button);
    expect(onInstall).not.toHaveBeenCalled();
  });

  it("can be dismissed", async () => {
    const { onDismiss } = bar(READY);

    await userEvent.click(screen.getByRole("button", { name: /not now/i }));

    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it("offers no press while bytes are moving", () => {
    bar({ state: "downloading", received: 500_000, total: 1_000_000 });

    expect(screen.queryByRole("button", { name: /^update$/i })).toBeNull();
    expect(screen.getByText(/50%/)).toBeVisible();
  });

  it("says a length is unknown rather than inventing one", () => {
    bar({ state: "downloading", received: 500_000, total: null });

    expect(screen.getByText(/Downloading/)).toBeVisible();
    expect(screen.queryByText(/%/)).toBeNull();
  });

  it("shows a failure as the sentence Rust wrote", () => {
    bar({ state: "failed", reason: "Consort could not reach the update server." });

    expect(
      screen.getByText("Consort could not reach the update server."),
    ).toBeVisible();
  });
});

describe("progress", () => {
  it("is null when the server sent no length", () => {
    expect(progress(10, null)).toBeNull();
  });

  it("is null rather than infinite when the length is zero", () => {
    expect(progress(10, 0)).toBeNull();
  });

  it("rounds to a whole percent", () => {
    expect(progress(1, 3)).toBe(33);
  });

  it("never exceeds a hundred, however many bytes arrive", () => {
    // Content-Length is whatever the server said, and it has been wrong before.
    expect(progress(300, 100)).toBe(100);
  });
});
