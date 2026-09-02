import { createElement, useMemo, type ReactNode } from "react";

/**
 * The elements this build draws, and what it draws them as.
 *
 * Everything markdown produces plus the rest of the tags the specification
 * lists for `m.room.message`, minus the ones that would need something this
 * build has not got: `img` has no way to fetch an `mxc://`, and `font` and the
 * colour attributes are a typography decision nobody has made.
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
};

/**
 * The schemes an address may have before it is drawn at all.
 *
 * `javascript:` is the one everybody names, but the rule is the other way
 * round: an allow-list, so a scheme nobody here has thought about is refused
 * rather than waved through.
 */
const REACHABLE = ["http:", "https:", "mailto:"];

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
    return (
      <a
        key={key}
        href={reachable(node.getAttribute("href"))}
        // The webview holds one page and has no way back to it. Following a
        // link would replace Consort with a website and strand whoever
        // clicked. Opening one outside the application needs a Tauri plugin
        // and so a capability this build does not grant.
        onClick={(event) => event.preventDefault()}
      >
        {children}
      </a>
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
 * of text on the way in, and every attribute except an anchor's address is
 * dropped on the floor.
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
