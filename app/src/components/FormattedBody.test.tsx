import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const openLink = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  openLink,
}));

import { FormattedBody } from "./FormattedBody";

beforeEach(() => {
  openLink.mockReset().mockResolvedValue(undefined);
});

/** The rendered elements, so a test can ask what actually reached the page. */
function body(html: string): HTMLElement {
  const { container } = render(<FormattedBody html={html} />);
  return container;
}

describe("FormattedBody", () => {
  it("draws a heading as a heading rather than as its own syntax", () => {
    // The complaint this exists for. Somebody typing "### Heading" was shown
    // the hashes back.
    render(<FormattedBody html="<h3>Heading</h3>" />);

    expect(screen.getByRole("heading", { name: "Heading" })).toBeVisible();
  });

  it("draws emphasis, strength and code", () => {
    const rendered = body(
      "<p><em>soft</em> <strong>loud</strong> <code>terse</code></p>",
    );

    expect(rendered.querySelector("em")).toHaveTextContent("soft");
    expect(rendered.querySelector("strong")).toHaveTextContent("loud");
    expect(rendered.querySelector("code")).toHaveTextContent("terse");
  });

  it("draws a list as a list", () => {
    render(<FormattedBody html="<ul><li>one</li><li>two</li></ul>" />);

    expect(screen.getAllByRole("listitem")).toHaveLength(2);
  });

  it("keeps the words inside an element it does not know", () => {
    // Nothing an allow-list has never heard of is drawn, but the text in it is
    // still what somebody said, and silently swallowing a sentence is worse
    // than drawing it without its box.
    render(<FormattedBody html="<details><summary>still said</summary></details>" />);

    expect(screen.getByText("still said")).toBeVisible();
  });

  it("puts no script in the document, whatever the message said", () => {
    // The reason this component builds elements itself instead of handing the
    // string to `dangerouslySetInnerHTML`. Nothing here is ever parsed into
    // the live document, so there is no arrangement of tags that runs.
    const rendered = body(
      '<p>hello</p><script>window.pwned = true</script><img src="x" onerror="window.pwned = true">',
    );

    expect(rendered.querySelector("script")).toBeNull();
    expect(rendered.querySelector("img")).toBeNull();
    expect((window as unknown as { pwned?: boolean }).pwned).toBeUndefined();
  });

  it("draws an ordinary link, and refuses to be navigated by it", () => {
    // The webview has one page in it and no way back, so a link followed in
    // place would be a one-way trip out of Consort. It opens in the desktop's
    // browser instead; the test below is the other half.
    const rendered = body('<p><a href="https://example.org/x">there</a></p>');
    const link = rendered.querySelector("a");

    expect(link).toHaveAttribute("href", "https://example.org/x");
    const clicked = new MouseEvent("click", { bubbles: true, cancelable: true });
    link?.dispatchEvent(clicked);
    expect(clicked.defaultPrevented).toBe(true);
  });

  it("opens an ordinary link outside the application", async () => {
    body('<p><a href="https://example.org/x">there</a></p>');

    await userEvent.click(screen.getByRole("link", { name: "there" }));

    expect(openLink).toHaveBeenCalledWith("https://example.org/x");
  });

  it("finds an address a sender wrote bare in a formatted message", () => {
    // Formatting and linkifying are separate things a client does. A message
    // in bold with a pasted link in it has the link as plain text.
    const rendered = body("<p><strong>go to https://example.org/x</strong></p>");

    expect(rendered.querySelector("strong a")).toHaveAttribute(
      "href",
      "https://example.org/x",
    );
  });

  it("does not look for an address inside a link", () => {
    // An anchor inside an anchor is invalid, and the address has already been
    // found by the sender.
    const rendered = body(
      '<p><a href="https://example.org/x">https://example.org/x</a></p>',
    );

    expect(rendered.querySelectorAll("a")).toHaveLength(1);
  });

  it("drops the address of a link that is not one to a page", () => {
    // `javascript:` is the obvious one, and the click above is not the only
    // way an anchor is followed.
    const rendered = body('<p><a href="javascript:alert(1)">press</a></p>');

    expect(rendered.querySelector("a")).not.toHaveAttribute("href");
    expect(screen.getByText("press")).toBeVisible();
  });

  it("draws a quote and a code block", () => {
    const rendered = body(
      "<blockquote><p>said before</p></blockquote><pre><code>cargo test</code></pre>",
    );

    expect(rendered.querySelector("blockquote")).toHaveTextContent("said before");
    expect(rendered.querySelector("pre")).toHaveTextContent("cargo test");
  });

  it("draws nothing at all for nothing at all", () => {
    expect(body("").textContent).toBe("");
  });

  it("keeps the @ on somebody's name", () => {
    // The sender writes the pill as a matrix.to link whose text is the display
    // name, so "bragoodle" arrived where "@bragoodle" was meant. The at sign
    // is what says the word is a person rather than a noun.
    const rendered = body(
      '<p>ask <a href="https://matrix.to/#/@bragoodle:example.org">bragoodle</a></p>',
    );

    expect(rendered.querySelector("a")).toHaveTextContent("@bragoodle");
  });

  it("does not double the @ on a name that already has one", () => {
    const rendered = body(
      '<p><a href="https://matrix.to/#/@ada:example.org">@ada:example.org</a></p>',
    );

    expect(rendered.querySelector("a")).toHaveTextContent("@ada:example.org");
    expect(rendered.querySelector("a")?.textContent).not.toContain("@@");
  });

  it("draws a mention as a mention rather than as a destination", () => {
    const rendered = body(
      '<p><a href="https://matrix.to/#/@ada:example.org">Ada</a></p>',
    );

    expect(rendered.querySelector("a")).toHaveClass("timeline__mention");
  });

  it("draws a link to a room as somewhere to go rather than as an address", () => {
    // A matrix.to link can name a room or a message as well as a person, and
    // neither of those is a mention. Nor is either of them a website: following
    // one in a browser lands on matrix.to's own page offering to open a client.
    const rendered = body(
      '<p><a href="https://matrix.to/#/!general:example.org">general</a></p>',
    );

    expect(rendered.querySelector("a")).toBeNull();
    expect(screen.getByRole("button", { name: "general" })).toBeVisible();
  });

  it("names a room link for itself when the sender wrote only the address", () => {
    // What a pasted link inside a markdown link looks like. Nothing was
    // chosen, so nothing is preserved, and the alias is what a person can read.
    const rendered = body(
      '<p><a href="https://matrix.to/#/#general:example.org">https://matrix.to/#/#general:example.org</a></p>',
    );

    expect(rendered.querySelector("a")).toBeNull();
    expect(
      screen.getByRole("button", { name: "#general:example.org" }),
    ).toBeVisible();
  });

  it("keeps the words a sender wrote around a link to a message", () => {
    body(
      '<p><a href="https://matrix.to/#/!general:example.org/$said:example.org">this one</a></p>',
    );

    expect(screen.getByRole("button", { name: "this one" })).toBeVisible();
  });

  it("says a message link is a message when the sender wrote no words", () => {
    body(
      '<p><a href="https://matrix.to/#/!general:example.org/$said:example.org">https://matrix.to/#/!general:example.org/$said:example.org</a></p>',
    );

    expect(screen.getByRole("button", { name: "A message" })).toBeVisible();
  });

  it("never hands a link into Matrix to the desktop's browser", async () => {
    body(
      '<p><a href="https://matrix.to/#/!general:example.org">general</a></p>',
    );

    await userEvent.click(screen.getByRole("button", { name: "general" }));

    expect(openLink).not.toHaveBeenCalled();
  });

  it("draws a custom emoji from the homeserver", () => {
    const rendered = body(
      '<p>nice <img data-mx-emoticon height="32" src="mxc://example.org/abc" alt=":party:" title=":party:"> one</p>',
    );

    const emoji = rendered.querySelector("img");
    expect(emoji).toHaveClass("body__emoticon");
    expect(emoji).toHaveAttribute("alt", ":party:");
    expect(emoji?.getAttribute("src")).toMatch(/^consortmedia:/);
  });

  it("refuses an image pointed anywhere but the homeserver", () => {
    // The whole security rule. An img whose address the sender chose is a
    // request the reader's machine makes to a server the sender picked: a read
    // receipt nobody asked for, with an IP address attached.
    const rendered = body(
      '<p>hello<img src="https://tracker.example/pixel.gif" alt=""></p>',
    );

    expect(rendered.querySelector("img")).toBeNull();
    expect(rendered).toHaveTextContent("hello");
  });

  it("refuses a data URI too", () => {
    // Not a network request, but not an image a message should be able to
    // conjure either, and the allow-list is what says so rather than a list of
    // the schemes somebody thought of.
    const rendered = body(
      '<p><img src="data:image/gif;base64,R0lGODlhAQABAAAAACw=" alt=""></p>',
    );

    expect(rendered.querySelector("img")).toBeNull();
  });

  it("caps an image that is not an emoji", () => {
    const rendered = body('<p><img src="mxc://example.org/abc" alt="a chart"></p>');

    expect(rendered.querySelector("img")).toHaveClass("body__image");
  });

  it("describes an emoji by its title when it carries no alt", () => {
    const rendered = body(
      '<p><img data-mx-emoticon src="mxc://example.org/abc" title=":party:"></p>',
    );

    expect(rendered.querySelector("img")).toHaveAttribute("alt", ":party:");
  });
});
