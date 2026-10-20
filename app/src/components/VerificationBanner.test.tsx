import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const verificationOtherSessionsExist = vi.hoisted(() => vi.fn());
const verificationRecoveryExists = vi.hoisted(() => vi.fn());
const verificationVerifyThisSession = vi.hoisted(() => vi.fn());
const verificationWarningDismissed = vi.hoisted(() => vi.fn());
const dismissVerificationWarning = vi.hoisted(() => vi.fn());

vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  verificationOtherSessionsExist,
  verificationRecoveryExists,
  verificationVerifyThisSession,
  verificationWarningDismissed,
  dismissVerificationWarning,
}));

import { VerificationBanner } from "./VerificationBanner";

function show(
  state: "unknown" | "verified" | "unverified" = "unverified",
  canStart = true,
) {
  render(
    <VerificationBanner state={state} canStart={canStart} />,
  );
}

/** The way out of the long form, once the banner has worked out it has one. */
function carryOn(): Promise<HTMLElement> {
  return screen.findByRole("button", { name: /continue without verifying/i });
}

beforeEach(() => {
  // Another session is signed in and nobody has a recovery key, which is the
  // common shape and the one that draws both the emoji route and this one.
  verificationOtherSessionsExist.mockReset().mockResolvedValue(true);
  verificationRecoveryExists.mockReset().mockResolvedValue(false);
  verificationVerifyThisSession.mockReset().mockResolvedValue(undefined);
  // Nobody has answered yet, so the warning starts in full.
  verificationWarningDismissed.mockReset().mockResolvedValue(false);
  dismissVerificationWarning.mockReset().mockResolvedValue(undefined);
  vi.spyOn(console, "error").mockImplementation(() => {});
});

describe("a session nobody has verified", () => {
  it("offers a way to carry on without verifying", async () => {
    // #59. Every other route out of this banner needs something somebody may
    // not have: a second device, or a key they kept. With neither, the banner
    // was a wall with nothing on the far side of it.
    show();

    expect(await carryOn()).toBeVisible();
  });

  it("asks whether this session has already answered", async () => {
    // Nothing is passed: which session this is about is Rust's to read off the
    // client, so there is no device id here to get wrong.
    show();

    await waitFor(() =>
      expect(verificationWarningDismissed).toHaveBeenCalledWith(),
    );
  });

  it("offers neither form until it knows which one to draw", async () => {
    // The gap between mounting and the answer coming back. Without the gate
    // the short form is what the opening render draws, so a session that has
    // never answered would flash one line and a Verify button where the
    // warning belongs.
    verificationWarningDismissed.mockReturnValue(new Promise(() => {}));
    show();

    expect(
      await screen.findByText("This session is not verified."),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", { name: /continue without verifying/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /^verify$/i }),
    ).not.toBeInTheDocument();
  });

  it("keeps the warning in full if it cannot tell whether this was answered", async () => {
    // The only direction that matters here. An unreadable settings file must
    // not be a way to silence a standing warning about somebody's messages, so
    // not knowing is treated as not answered.
    verificationWarningDismissed.mockRejectedValue({
      message: "The settings file could not be read.",
    });
    show();

    expect(await carryOn()).toBeVisible();
    expect(
      screen.queryByRole("button", { name: /^verify$/i }),
    ).not.toBeInTheDocument();
  });

  it("does not dismiss anything on the first press", async () => {
    // The press opens a question. A control that acted on the first press
    // would be reached by a hand that is already somewhere it did not mean to
    // be, and what it settles is how much warning somebody gets about their
    // own messages.
    show();

    await userEvent.click(await carryOn());

    expect(dismissVerificationWarning).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog")).toBeVisible();
  });

  it("says what staying unverified costs before it is agreed to", async () => {
    show();

    await userEvent.click(await carryOn());
    const asked = screen.getByRole("dialog");

    // The three things that stay true afterwards. Any one of them missing is
    // somebody agreeing to something they were not told.
    expect(asked).toHaveTextContent(/before you signed in/i);
    expect(asked).toHaveTextContent(/encrypted voice channels/i);
    expect(asked).toHaveTextContent(/other people/i);
  });

  it("writes nothing if the question is cancelled", async () => {
    show();

    await userEvent.click(await carryOn());
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));

    expect(dismissVerificationWarning).not.toHaveBeenCalled();
    expect(await carryOn()).toBeVisible();
  });
});

describe("agreeing to carry on", () => {
  /** Press through the question for the device under test. */
  async function agree() {
    await userEvent.click(await carryOn());
    await userEvent.click(
      screen.getByRole("button", { name: /continue unverified/i }),
    );
  }

  it("records the answer, naming no device of its own", async () => {
    show();

    await agree();

    await waitFor(() =>
      expect(dismissVerificationWarning).toHaveBeenCalledWith(),
    );
  });

  it("takes the routes and the question away", async () => {
    show();

    await agree();

    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: "Verify this session" }),
      ).not.toBeInTheDocument(),
    );
    expect(
      screen.queryByRole("button", { name: /continue without verifying/i }),
    ).not.toBeInTheDocument();
  });

  it("still says the session is not verified", async () => {
    // The line that must survive. Agreeing to run unverified is not the same
    // as being told everything is fine, and a banner that went silent here
    // would be the application claiming the second thing.
    show();

    await agree();

    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: /continue without verifying/i }),
      ).not.toBeInTheDocument(),
    );
    expect(screen.getByText("This session is not verified.")).toBeVisible();
  });

  it("leaves the way back one press away", async () => {
    show();

    await agree();

    const back = await screen.findByRole("button", { name: /^verify$/i });
    await userEvent.click(back);

    expect(
      await screen.findByRole("button", { name: "Verify this session" }),
    ).toBeVisible();
  });

  it("keeps the warning in full when the answer could not be saved", async () => {
    // Fail safe. Being wrong this way shows somebody a paragraph they have
    // read; being wrong the other way hides a standing warning on the strength
    // of a choice nothing recorded, so it would be back at the next launch
    // with no explanation.
    dismissVerificationWarning.mockRejectedValue({
      message: "The settings file could not be written.",
    });
    show();

    await agree();

    expect(
      await screen.findByText("The settings file could not be written."),
    ).toBeVisible();
    expect(await carryOn()).toBeVisible();
  });
});

describe("a session that has already answered", () => {
  beforeEach(() => {
    verificationWarningDismissed.mockResolvedValue(true);
  });

  it("is drawn short, and never asked again", async () => {
    show();

    expect(
      await screen.findByText("This session is not verified."),
    ).toBeVisible();
    await waitFor(() => expect(verificationWarningDismissed).toHaveBeenCalled());
    expect(
      screen.queryByRole("button", { name: /continue without verifying/i }),
    ).not.toBeInTheDocument();
  });

  it("does not go asking the homeserver about routes nobody is being offered", async () => {
    // `verificationOtherSessionsExist` reaches the homeserver when the store
    // cannot answer. Asking for a paragraph that is not being drawn is a
    // request nobody wanted.
    show();

    await waitFor(() => expect(verificationWarningDismissed).toHaveBeenCalled());

    expect(verificationOtherSessionsExist).not.toHaveBeenCalled();
    expect(verificationRecoveryExists).not.toHaveBeenCalled();
  });

  it("brings the routes back when asked", async () => {
    show();

    await userEvent.click(await screen.findByRole("button", { name: /^verify$/i }));

    expect(
      await screen.findByRole("button", { name: "Verify this session" }),
    ).toBeVisible();
  });
});

describe("the states that are not an unverified session", () => {
  it("draws nothing at all for a verified one", async () => {
    show("verified");

    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(verificationWarningDismissed).not.toHaveBeenCalled();
  });

  it("offers nothing to agree to while nothing has looked yet", async () => {
    // `unknown` is the launch state. Offering to accept a cost before anything
    // has established there is one is asking somebody to answer a question
    // that has no answer yet.
    show("unknown");

    expect(
      await screen.findByText("Checking whether this session is verified."),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", { name: /continue without verifying/i }),
    ).not.toBeInTheDocument();
  });
});
