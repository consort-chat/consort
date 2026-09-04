import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const openLink = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  openLink,
}));

import { PlainBody } from "./PlainBody";

beforeEach(() => {
  openLink.mockReset().mockResolvedValue(undefined);
});

describe("PlainBody", () => {
  it("says what was said", () => {
    render(<PlainBody text="nothing to see here" />);

    expect(screen.getByText("nothing to see here")).toBeVisible();
  });

  it("draws an address somebody pasted as something to press", () => {
    // The complaint this exists for. A pasted link arrives as plain text,
    // because linkifying is what a client does when it draws a message rather
    // than when it sends one.
    render(<PlainBody text="have a look at https://example.org/x" />);

    expect(screen.getByRole("link", { name: "https://example.org/x" })).toHaveAttribute(
      "href",
      "https://example.org/x",
    );
  });

  it("asks Rust to open one rather than following it", async () => {
    render(<PlainBody text="https://example.org/x" />);

    await userEvent.click(screen.getByRole("link"));

    expect(openLink).toHaveBeenCalledWith("https://example.org/x");
  });

  it("never navigates the page itself", async () => {
    // The webview holds one page and has no way back to it, so a link that
    // was followed in place would be a one-way trip out of Consort.
    render(<PlainBody text="https://example.org/x" />);

    const clicked = new MouseEvent("click", { bubbles: true, cancelable: true });
    screen.getByRole("link").dispatchEvent(clicked);

    expect(clicked.defaultPrevented).toBe(true);
  });

  it("draws a pasted message address as somewhere to go", () => {
    // Exactly what Consort's own Copy link produces, pasted into a room. It
    // arrives as plain words with no formatting at all, and as an address
    // nothing outside Matrix can do anything useful with.
    render(
      <PlainBody text="see https://matrix.to/#/!general:example.org/$said:example.org" />,
    );

    expect(screen.queryByRole("link")).toBeNull();
    expect(screen.getByRole("button", { name: "A message" })).toBeVisible();
  });

  it("never hands a pasted matrix address to the desktop's browser", async () => {
    render(<PlainBody text="https://matrix.to/#/#general:example.org" />);

    await userEvent.click(
      screen.getByRole("button", { name: "#general:example.org" }),
    );

    expect(openLink).not.toHaveBeenCalled();
  });

  it("leaves a link to a person as a link", () => {
    // Consort has no card to open from a message body, so the only honest
    // thing left is the address itself.
    render(<PlainBody text="https://matrix.to/#/@ada:example.org" />);

    expect(screen.getByRole("link")).toBeVisible();
  });

  it("says so in the console when a link cannot be opened", async () => {
    // Somebody pressed something and nothing happened. Silence would leave
    // nothing at all to look at.
    const complained = vi.spyOn(console, "error").mockImplementation(() => {});
    openLink.mockRejectedValue({ message: "no browser", detail: "no browser" });
    render(<PlainBody text="https://example.org/x" />);

    await userEvent.click(screen.getByRole("link"));

    expect(complained).toHaveBeenCalled();
    complained.mockRestore();
  });
});
