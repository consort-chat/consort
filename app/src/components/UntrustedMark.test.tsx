import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { UntrustedMark } from "./UntrustedMark";
import { senderTrustLabel } from "../lib/labels";

describe("the mark", () => {
  it("carries the sentence as its accessible name", () => {
    render(<UntrustedMark trust="unsignedDevice" />);

    expect(
      screen.getByRole("img", { name: senderTrustLabel("unsignedDevice") }),
    ).toBeInTheDocument();
  });

  it("is reachable by keyboard rather than by hover only", async () => {
    const user = userEvent.setup();
    render(<UntrustedMark trust="unknownDevice" />);

    await user.tab();

    expect(screen.getByRole("img")).toHaveFocus();
  });

  it("says which state it is drawing, for the stylesheet and for a test", () => {
    render(<UntrustedMark trust="verificationViolation" />);

    expect(screen.getByRole("img")).toHaveAttribute(
      "data-trust",
      "verificationViolation",
    );
  });

  it("draws a shape, so the warning is not colour alone", () => {
    const { container } = render(<UntrustedMark trust="mismatchedSender" />);

    const glyph = container.querySelector("svg");
    expect(glyph).not.toBeNull();
    // Hidden from the reader, which already has the sentence as a name.
    expect(glyph).toHaveAttribute("aria-hidden", "true");
  });

  it("repeats the sentence on the pointer without announcing it twice", () => {
    render(<UntrustedMark trust="unsignedDevice" />);

    const tip = screen.getByText(senderTrustLabel("unsignedDevice"), {
      ignore: "[aria-label]",
    });
    expect(tip).toHaveAttribute("aria-hidden", "true");
  });
});
