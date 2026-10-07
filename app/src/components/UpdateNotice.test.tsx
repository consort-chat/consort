import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { UpdateNotice } from "./UpdateNotice";
import type { Call, Update } from "../lib/api";

const api = vi.hoisted(() => ({
  updatesItself: vi.fn(),
  updateInstall: vi.fn(),
  onUpdate: vi.fn(),
  onCall: vi.fn(),
  resendState: vi.fn(),
}));

vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  ...api,
}));

/** Hand a listener back, and keep the handler so a test can push to it. */
function channel<T>() {
  let push: ((value: T) => void) | null = null;
  const unlisten = vi.fn();
  const register = vi.fn((handler: (value: T) => void) => {
    push = handler;
    return Promise.resolve(unlisten);
  });
  return {
    register,
    unlisten,
    /// Wrapped in `act` so the state it sets is applied before the assertion
    /// that follows. Without it an assertion about what did not change reads the
    /// render from before the event and passes either way.
    say(value: T) {
      if (push === null) throw new Error("nothing subscribed");
      const send = push;
      act(() => send(value));
    },
  };
}

const READY: Update = {
  state: "ready",
  version: "0.12.0",
  notes: "https://example.invalid/v0.12.0",
};

let updates: ReturnType<typeof channel<Update>>;
let calls: ReturnType<typeof channel<Call>>;

beforeEach(() => {
  vi.clearAllMocks();
  updates = channel<Update>();
  calls = channel<Call>();
  api.updatesItself.mockResolvedValue(true);
  api.updateInstall.mockResolvedValue(undefined);
  api.resendState.mockResolvedValue(undefined);
  api.onUpdate.mockImplementation(updates.register);
  api.onCall.mockImplementation(calls.register);
});

describe("UpdateNotice", () => {
  it("subscribes to nothing at all in a build that does not update itself", async () => {
    // A .deb or an Arch package. Not a disabled bar: no bar, and no channel
    // either, because a package manager owns updating there.
    api.updatesItself.mockResolvedValue(false);

    render(<UpdateNotice />);

    await waitFor(() => expect(api.updatesItself).toHaveBeenCalled());
    expect(api.onUpdate).not.toHaveBeenCalled();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("asks for the state it may have missed, once subscribed", async () => {
    // The poll starts with the process and the webview subscribes whenever its
    // JavaScript gets round to it. Whatever was said first went to nobody.
    render(<UpdateNotice />);

    await waitFor(() => expect(api.resendState).toHaveBeenCalledOnce());
    expect(api.onUpdate).toHaveBeenCalledOnce();
  });

  it("offers a release it is told about", async () => {
    render(<UpdateNotice />);
    await waitFor(() => expect(api.onUpdate).toHaveBeenCalled());

    updates.say(READY);

    expect(await screen.findByText(/0\.12\.0/)).toBeVisible();
  });

  it("installs on a press", async () => {
    render(<UpdateNotice />);
    await waitFor(() => expect(api.onUpdate).toHaveBeenCalled());
    updates.say(READY);

    await userEvent.click(
      await screen.findByRole("button", { name: /update and restart/i }),
    );

    expect(api.updateInstall).toHaveBeenCalledOnce();
  });

  it("will not install while a call is up", async () => {
    render(<UpdateNotice />);
    await waitFor(() => expect(api.onCall).toHaveBeenCalled());
    updates.say(READY);
    calls.say({
      state: "connected",
      roomId: "!lounge:example.org",
      participants: [],
      trouble: null,
    });

    const button = await screen.findByRole("button", {
      name: /update and restart/i,
    });
    await waitFor(() => expect(button).toBeDisabled());
    expect(api.updateInstall).not.toHaveBeenCalled();
  });

  it("offers it again once the call is over", async () => {
    render(<UpdateNotice />);
    await waitFor(() => expect(api.onCall).toHaveBeenCalled());
    updates.say(READY);
    calls.say({
      state: "connected",
      roomId: "!lounge:example.org",
      participants: [],
      trouble: null,
    });

    calls.say({ state: "disconnected" });

    const button = await screen.findByRole("button", {
      name: /update and restart/i,
    });
    await waitFor(() => expect(button).toBeEnabled());
  });

  it("shows the refusal when Rust turns the install down", async () => {
    // The gate is Rust's, and it is checked again after the download. A webview
    // that disagreed with it has to render what Rust said rather than its own
    // idea of whether a call was up.
    api.updateInstall.mockRejectedValue({
      message: "Leave the call first.",
      detail: "an update was refused",
    });
    render(<UpdateNotice />);
    await waitFor(() => expect(api.onUpdate).toHaveBeenCalled());
    updates.say(READY);

    await userEvent.click(
      await screen.findByRole("button", { name: /update and restart/i }),
    );

    expect(await screen.findByText("Leave the call first.")).toBeVisible();
  });

  it("does not withdraw an offer because a later look failed", async () => {
    // A laptop that goes offline. The poll runs every six hours, and a look that
    // could not reach GitHub says nothing about whether 0.12.0 exists: it still
    // does, and replacing the offer with an error would take away the one thing
    // on screen somebody could act on.
    render(<UpdateNotice />);
    await waitFor(() => expect(api.onUpdate).toHaveBeenCalled());
    updates.say(READY);
    await screen.findByText(/0\.12\.0/);

    updates.say({
      state: "failed",
      reason: "Consort could not reach the update server.",
    });

    expect(screen.getByText(/0\.12\.0/)).toBeVisible();
    expect(screen.queryByText(/could not reach/)).toBeNull();
  });

  it("reports a failure when there was nothing on offer", async () => {
    render(<UpdateNotice />);
    await waitFor(() => expect(api.onUpdate).toHaveBeenCalled());

    updates.say({
      state: "failed",
      reason: "Consort could not reach the update server.",
    });

    expect(
      await screen.findByText("Consort could not reach the update server."),
    ).toBeVisible();
  });

  it("stays dismissed for this session", async () => {
    render(<UpdateNotice />);
    await waitFor(() => expect(api.onUpdate).toHaveBeenCalled());
    updates.say(READY);
    await screen.findByRole("status");

    await userEvent.click(screen.getByRole("button", { name: /not now/i }));

    expect(screen.queryByRole("status")).toBeNull();
    // A repeat of the same offer, which the six-hourly poll produces, must not
    // bring it back.
    updates.say(READY);
    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
  });

  it("comes back for a version that was not the dismissed one", async () => {
    render(<UpdateNotice />);
    await waitFor(() => expect(api.onUpdate).toHaveBeenCalled());
    updates.say(READY);
    await screen.findByRole("status");
    await userEvent.click(screen.getByRole("button", { name: /not now/i }));

    updates.say({ ...READY, version: "0.13.0" });

    expect(await screen.findByText(/0\.13\.0/)).toBeVisible();
  });

  it("stops listening when it goes away", async () => {
    const { unmount } = render(<UpdateNotice />);
    await waitFor(() => expect(api.resendState).toHaveBeenCalled());

    unmount();

    await waitFor(() => expect(updates.unlisten).toHaveBeenCalledOnce());
    expect(calls.unlisten).toHaveBeenCalledOnce();
  });
});
