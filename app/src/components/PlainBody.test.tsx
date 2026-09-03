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
