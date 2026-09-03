import { createElement, useMemo, type ReactNode } from "react";

import { mxcUrl } from "../lib/api";

/**
 * The elements this build draws, and what it draws them as.
 *
 * Everything markdown produces plus the rest of the tags the specification
 * lists for `m.room.message`, minus `font` and the colour attributes, which
 * are a typography decision nobody has made.
 *
 * A tag that is not here is not drawn, but what is inside it still is. See
 * [`render`] for why that is the safe half of the rule rather than the
 * dangerous one.
 */
const DRAWN: Record<string, string> = {
  p: "p",
  br: "br",
  hr: "hr",
  h1: "h1",
  h2: "h2",
  h3: "h3",
  h4: "h4",
  h5: "h5",
  h6: "h6",
  em: "em",
  i: "em",
  strong: "strong",
  b: "strong",
  del: "del",
  s: "del",
  strike: "del",
  code: "code",
  pre: "pre",
  blockquote: "blockquote",
  ul: "ul",
  ol: "ol",
  li: "li",
  table: "table",
  thead: "thead",
  tbody: "tbody",
  tr: "tr",
  th: "th",
  td: "td",
  caption: "caption",
  a: "a",
  img: "img",
};

/**
 * The schemes an address may have before it is drawn at all.
 *
 * `javascript:` is the one everybody names, but the rule is the other way
 * round: an allow-list, so a scheme nobody here has thought about is refused
 * rather than waved through.
 */
const REACHABLE = ["http:", "https:", "mailto:"];

/**
 * Whether an address names a person, and the fragment says which.
 *
 * A `matrix.to` link is how every client writes a mention, and the same shape
 * also names rooms and events, which the sigil tells apart: `@` is somebody,
 * `!` and `#` are a room, and `$` is a message. Only the first is a mention.
 */
function mentioned(raw: string | null): boolean {
  if (raw === null) return false;
  try {
    const address = new URL(raw);
    return (
      address.hostname === "matrix.to" && address.hash.startsWith("#/@")
    );
  } catch {
    return false;
  }
}

/** An address worth putting on an anchor, or nothing. */
function reachable(raw: string | null): string | undefined {
  if (raw === null) return undefined;
  try {
    return REACHABLE.includes(new URL(raw).protocol) ? raw : undefined;
  } catch {
    // Not an absolute address. A relative one means nothing here: there is no
    // site to be relative to.
    return undefined;
  }
}

/** One node as something React can draw, or nothing. */
function draw(node: Node, key: string): ReactNode {
  if (node.nodeType === Node.TEXT_NODE) return node.textContent;
  if (!(node instanceof Element)) return null;

  const children = [...node.childNodes].map((child, index) =>
    draw(child, `${key}.${index}`),
  );
  const tag = DRAWN[node.localName];

  // Not drawn, but not thrown away either. Whatever is inside was still said,
  // and this is the whole of what makes dropping an unknown tag safe rather
  // than lossy.
  if (tag === undefined) return children;

  if (tag === "a") {
    const href = node.getAttribute("href");
    /*
      A mention is drawn as a name rather than as a destination, because that
      is what it is: pressing it goes nowhere, and an underlined blue word
      promises otherwise.

      The at sign is put back when the sender left it off, which every client
      does: the pill's text is the display name, so "bragoodle" arrives where
      "@bragoodle" was meant and the word reads as a noun.
    */
    const mention = mentioned(href);
    const named = mention && !(node.textContent ?? "").startsWith("@");

    return (
      <a
        key={key}
        className={mention ? "timeline__mention" : undefined}
        href={reachable(href)}
        // The webview holds one page and has no way back to it. Following a
        // link would replace Consort with a website and strand whoever
        // clicked. Opening one outside the application needs a Tauri plugin
        // and so a capability this build does not grant.
        onClick={(event) => event.preventDefault()}
      >
        {named && "@"}
        {children}
      </a>
    );
  }

  if (tag === "img") {
    /*
      Only an `mxc://`, and that is the whole security rule here rather than a
      convenience. An `img` the sender chose the address of is a request the
      reader's machine makes to a server the sender picked: a read receipt
      nobody asked for, with an IP address attached. Refusing every other
      scheme means the only thing a message can point at is the homeserver
      this account is already talking to.

      The content security policy says the same thing again in
      `tauri.conf.json`, and neither is a reason to drop the other: this is
      what decides, and that is what catches a mistake made here.
    */
    const src = mxcUrl(node.getAttribute("src") ?? "");
    if (src === undefined) return null;

    /*
      A custom emoji is drawn at the height of the words around it, whatever
      size the sender's client wrote on it. `data-mx-emoticon` is what says it
      is one, which is MSC2545 and what every client that sends them uses.
    */
    const emoticon = node.hasAttribute("data-mx-emoticon");
    return (
      <img
        key={key}
        className={emoticon ? "body__emoticon" : "body__image"}
        src={src}
        // The shortcode, which is what a sender writes and what somebody
        // reading with a screen reader needs. Empty rather than absent when
        // there is none: an image with no description is decoration.
        alt={node.getAttribute("alt") ?? node.getAttribute("title") ?? ""}
        title={node.getAttribute("title") ?? undefined}
      />
    );
  }

  // Void elements take no children, and React throws rather than ignoring
  // them.
  if (tag === "br" || tag === "hr") return createElement(tag, { key });

  return createElement(tag, { key }, children);
}

/**
 * A message's `formatted_body`, drawn.
 *
 * ## Why this is not `dangerouslySetInnerHTML`
 *
 * Because the HTML is a stranger's. It arrives from a homeserver, written by
 * whoever sent the message, and `consort_matrix` deliberately passes it along
 * without sanitising it. What happens instead is one step further than
 * sanitising: the string is parsed into a document that is never attached to
 * anything, and the elements drawn are built here from an allow-list. Nothing
 * that came off the wire is ever handed back to the HTML parser that owns the
 * page, so there is no ordering of tags, no malformed nesting and no encoding
 * trick that can end with a script in the document. React escapes every piece
 * of text on the way in, and every attribute is dropped on the floor except
 * the four that are read by name: an anchor's address, and an image's source,
 * description and tooltip. Each of those is checked here before it is used.
 *
 * `DOMParser` is what makes that cheap: the document it returns is inert.
 * Scripts in it do not run and images in it are never fetched, which is the
 * same property every HTML sanitiser in a browser is built on.
 *
 * ## Why the caller supplies the box
 *
 * A fragment, so this can go inside whatever the timeline wants. It returns
 * block elements, so the container has to be one that may hold them, which a
 * paragraph is not.
 */
export function FormattedBody({ html }: { html: string }) {
  const drawn = useMemo(() => {
    const parsed = new DOMParser().parseFromString(html, "text/html");
    return [...parsed.body.childNodes].map((node, index) =>
      draw(node, String(index)),
    );
  }, [html]);

  return <>{drawn}</>;
}
