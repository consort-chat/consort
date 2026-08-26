import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const verificationAccept = vi.hoisted(() => vi.fn());
const verificationStartSas = vi.hoisted(() => vi.fn());
const verificationConfirm = vi.hoisted(() => vi.fn());
const verificationMismatch = vi.hoisted(() => vi.fn());
const verificationCancel = vi.hoisted(() => vi.fn());

vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  verificationAccept,
  verificationStartSas,
  verificationConfirm,
  verificationMismatch,
  verificationCancel,
}));

import { VerificationFlowPanel } from "./VerificationFlow";
import type { VerificationFlow, VerificationFlowState } from "../lib/api";

const emoji = [
  { symbol: "🐶", description: "Dog" },
  { symbol: "🐱", description: "Cat" },
  { symbol: "🦁", description: "Lion" },
  { symbol: "🐎", description: "Horse" },
  { symbol: "🦄", description: "Unicorn" },
  { symbol: "🐷", description: "Pig" },
  { symbol: "🐘", description: "Elephant" },
];

function flow(
  state: VerificationFlowState,
  weStarted = false,
): VerificationFlow {
  return {
    flowId: "the-only-flow",
    otherUserId: "@bob:example.org",
    isSelfVerification: true,
    weStarted,
    state,
  };
}

function show(state: VerificationFlowState, onDismiss = vi.fn()) {
  render(<VerificationFlowPanel flow={flow(state)} onDismiss={onDismiss} />);
}

/** The same panel, for a flow this session asked for. */
function showOurs(state: VerificationFlowState, onDismiss = vi.fn()) {
  render(
    <VerificationFlowPanel flow={flow(state, true)} onDismiss={onDismiss} />,
  );
}

beforeEach(() => {
  for (const action of [
    verificationAccept,
    verificationStartSas,
    verificationConfirm,
    verificationMismatch,
    verificationCancel,
  ]) {
    action.mockReset().mockResolvedValue(undefined);
  }
  vi.spyOn(console, "error").mockImplementation(() => {});
});

describe("a request waiting for an answer", () => {
  it("says another of your own sessions is asking", () => {
    show({ kind: "requested" });

    expect(screen.getByRole("status")).toHaveTextContent(
      /another of your sessions/i,
    );
  });

  it("names the other person when it is not your own session", () => {
    // Out of scope to start one, but a request from somebody else can still
    // arrive, and calling them "your session" would be a lie about whose keys
    // are being signed.
    render(
      <VerificationFlowPanel
        flow={{ ...flow({ kind: "requested" }), isSelfVerification: false }}
        onDismiss={vi.fn()}
      />,
    );

    expect(screen.getByRole("status")).toHaveTextContent("@bob:example.org");
  });

  it("accepts the flow it was given, by name", async () => {
    show({ kind: "requested" });

    fireEvent.click(screen.getByRole("button", { name: /verify/i }));

    expect(verificationAccept).toHaveBeenCalledWith(
      "@bob:example.org",
      "the-only-flow",
    );
  });

  it("declines through cancel rather than pretending to", () => {
    show({ kind: "requested" });

    fireEvent.click(screen.getByRole("button", { name: /not now/i }));

    expect(verificationCancel).toHaveBeenCalledWith(
      "@bob:example.org",
      "the-only-flow",
    );
  });

  it("offers nothing to compare yet", () => {
    show({ kind: "requested" });

    expect(screen.queryByRole("button", { name: /match/i })).toBeNull();
  });
});

describe("waiting for the comparison to start", () => {
  it("offers to start it from this side", () => {
    // Whoever asked normally starts it. When they do not, both sides wait
    // forever unless one of them can.
    show({ kind: "ready" });

    fireEvent.click(screen.getByRole("button", { name: /show the emoji/i }));

    expect(verificationStartSas).toHaveBeenCalledWith(
      "@bob:example.org",
      "the-only-flow",
    );
  });

  it("does not offer to start it once it has started", () => {
    show({ kind: "waiting" });

    expect(screen.queryByRole("button", { name: /show the emoji/i })).toBeNull();
  });

  it("says it is waiting rather than showing an empty row of emoji", () => {
    // `emoji()` is `None` until the keys are exchanged. An empty grid where
    // seven pictures belong reads as a broken screen.
    show({ kind: "waiting" });

    expect(screen.getByRole("status")).toHaveTextContent(/wait/i);
  });
});

describe("a verification this session asked for", () => {
  it("says it is waiting for the other session to answer", () => {
    // The responder's `waiting` means the two are agreeing on algorithms. Ours
    // means nobody has picked the request up yet, and telling somebody to sit
    // tight when what they need to do is go and tap accept on their phone is
    // the difference between a flow that finishes and one that times out.
    showOurs({ kind: "waiting" });

    expect(screen.getByRole("status")).toHaveTextContent(/other session/i);
  });

  it("does not ask this side to accept its own request", () => {
    showOurs({ kind: "waiting" });

    expect(screen.queryByRole("button", { name: /^verify$/i })).toBeNull();
  });

  it("does not offer to start a comparison it already started", () => {
    // The Rust side sends `m.key.verification.start` by itself when it was the
    // one that asked, so this button would be a second start.
    showOurs({ kind: "ready" });

    expect(screen.queryByRole("button", { name: /show the emoji/i })).toBeNull();
  });

  it("can still be called off", () => {
    showOurs({ kind: "waiting" });

    fireEvent.click(screen.getByRole("button", { name: /cancel/i }));

    expect(verificationCancel).toHaveBeenCalledWith(
      "@bob:example.org",
      "the-only-flow",
    );
  });

  it("shows the same emoji screen as the other direction", () => {
    // The point of the phase. Two directions, one comparison screen.
    showOurs({ kind: "comparing", emoji, decimals: [1234, 5678, 9012] });

    expect(screen.getAllByRole("img")).toHaveLength(7);
    expect(screen.getByRole("button", { name: /they match/i })).toBeTruthy();
  });
});

describe("comparing", () => {
  const comparing: VerificationFlowState = {
    kind: "comparing",
    emoji,
    decimals: [1234, 5678, 9012],
  };

  it("shows all seven pictures with their words", () => {
    show(comparing);

    for (const pair of emoji) {
      expect(screen.getByText(pair.description)).toBeInTheDocument();
    }
    expect(screen.getAllByRole("img")).toHaveLength(7);
  });

  it("gives each picture an accessible name, since an emoji alone has none", () => {
    show(comparing);

    expect(screen.getByRole("img", { name: "Dog" })).toBeInTheDocument();
  });

  it("confirms a match", () => {
    show(comparing);

    fireEvent.click(screen.getByRole("button", { name: /they match/i }));

    expect(verificationConfirm).toHaveBeenCalledWith(
      "@bob:example.org",
      "the-only-flow",
    );
  });

  it("reports a mismatch as a mismatch and not as a plain cancel", () => {
    // Not cosmetic. A mismatch sends `m.mismatched_sas`, which tells the other
    // side somebody may be intercepting, and a plain cancel says somebody
    // changed their mind.
    show(comparing);

    fireEvent.click(screen.getByRole("button", { name: /do not match/i }));

    expect(verificationMismatch).toHaveBeenCalledWith(
      "@bob:example.org",
      "the-only-flow",
    );
    expect(verificationCancel).not.toHaveBeenCalled();
  });

  it("falls back to the numbers when the other side cannot do emoji", () => {
    show({ kind: "comparing", emoji: [], decimals: [1234, 5678, 9012] });

    expect(screen.getByRole("status")).toHaveTextContent("1234");
    expect(screen.getByRole("status")).toHaveTextContent("9012");
    expect(screen.queryAllByRole("img")).toHaveLength(0);
  });

  it("shows the emoji rather than the numbers when it has both", () => {
    show(comparing);

    expect(screen.getByRole("status")).not.toHaveTextContent("5678");
  });
});

describe("after answering", () => {
  it("says it is waiting for the other side", () => {
    show({ kind: "confirmed" });

    expect(screen.getByRole("status")).toHaveTextContent(/other/i);
  });

  it("offers nothing further to press", () => {
    show({ kind: "confirmed" });

    expect(
      screen.queryByRole("button", { name: /they match/i }),
    ).toBeNull();
  });
});

describe("the end of a flow", () => {
  it("reports success", () => {
    show({ kind: "done" });

    expect(screen.getByRole("status")).toHaveTextContent(/verified/i);
  });

  it("can be dismissed once it is over", () => {
    const onDismiss = vi.fn();
    show({ kind: "done" }, onDismiss);

    fireEvent.click(screen.getByRole("button", { name: /dismiss/i }));

    expect(onDismiss).toHaveBeenCalled();
  });

  it("cannot be dismissed while it is still running", () => {
    show({ kind: "comparing", emoji, decimals: [1, 2, 3] });

    expect(screen.queryByRole("button", { name: /dismiss/i })).toBeNull();
  });

  it("says who declined when somebody did", () => {
    show({
      kind: "cancelled",
      reason: "declined",
      detail: "The user cancelled the verification.",
      byUs: false,
    });

    expect(screen.getByRole("status")).toHaveTextContent(/other session/i);
  });

  it("says it was you when it was", () => {
    show({
      kind: "cancelled",
      reason: "declined",
      detail: "The user cancelled the verification.",
      byUs: true,
    });

    expect(screen.getByRole("status")).toHaveTextContent(/you/i);
  });

  it("treats a mismatch as the serious answer it is", () => {
    show({
      kind: "cancelled",
      reason: "mismatch",
      detail: "The SAS did not match.",
      byUs: false,
    });

    const panel = screen.getByRole("status");
    expect(panel).toHaveTextContent(/did not match/i);
    expect(panel).toHaveAttribute("data-outcome", "alarming");
  });

  it("does not treat another of your sessions answering as a problem", () => {
    // The common case with self-verification: the request goes to every
    // device, and the ones that did not answer are told it was accepted.
    show({
      kind: "cancelled",
      reason: "acceptedElsewhere",
      detail: "A m.key.verification.request was accepted by a different device.",
      byUs: false,
    });

    const panel = screen.getByRole("status");
    expect(panel).toHaveTextContent(/another of your sessions/i);
    expect(panel).not.toHaveAttribute("data-outcome", "alarming");
  });

  it("says an expired flow expired", () => {
    show({
      kind: "cancelled",
      reason: "timedOut",
      detail: "The verification process timed out.",
      byUs: false,
    });

    expect(screen.getByRole("status")).toHaveTextContent(/expired/i);
  });

  it("never renders the developer wording, whatever the reason", () => {
    show({
      kind: "cancelled",
      reason: "other",
      detail: "The device received an unexpected message.",
      byUs: false,
    });

    const panel = screen.getByRole("status");
    expect(panel).not.toHaveTextContent("unexpected message");
    expect(panel).toHaveTextContent(/ended/i);
  });
});

describe("when an action fails", () => {
  it("says so instead of leaving the button looking like it worked", async () => {
    // The likeliest failure by far, and not a bug: the flow expired, the other
    // side cancelled, or another session answered between the panel drawing
    // this button and somebody pressing it.
    verificationAccept.mockRejectedValue({
      message: "That verification is no longer waiting for an answer.",
      detail: "verification flow the-only-flow is no longer active",
    });
    show({ kind: "requested" });

    fireEvent.click(screen.getByRole("button", { name: /verify/i }));

    expect(
      await screen.findByText(/no longer waiting for an answer/i),
    ).toBeInTheDocument();
  });

  it("keeps the underlying text out of the interface", async () => {
    verificationAccept.mockRejectedValue({
      message: "That verification is no longer waiting for an answer.",
      detail: "verification flow the-only-flow is no longer active",
    });
    show({ kind: "requested" });

    fireEvent.click(screen.getByRole("button", { name: /verify/i }));
    await screen.findByText(/no longer waiting/i);

    expect(screen.getByRole("status")).not.toHaveTextContent(
      "is no longer active",
    );
  });
});
