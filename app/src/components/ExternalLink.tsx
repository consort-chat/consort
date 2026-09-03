import type { ReactNode } from "react";

import { asCommandError, openLink } from "../lib/api";

/**
 * An address in a message, drawn as something that can be pressed.
 *
 * The click is always cancelled. The webview holds one page and has no way
 * back to it, so following a link in place would replace Consort with a
 * website and strand whoever pressed it. What happens instead is a command,
 * because opening anything outside the application is Rust's job: the page
 * has `core:default` and nothing else.
 *
 * `href` is still written on the anchor for the ordinary reasons an address
 * belongs there, and is absent for one Consort will not open, which leaves an
 * anchor that is not a link. Both are deliberate: the attribute is what a
 * reader hovers to see where something goes, and dropping it is how a scheme
 * that failed the check stops looking like a destination.
 */
export function ExternalLink({
  href,
  children,
}: {
  href: string | undefined;
  children: ReactNode;
}) {
  return (
    <a
      className="body__link"
      href={href}
      onClick={(event) => {
        event.preventDefault();
        if (href === undefined) return;

        openLink(href).catch((raw: unknown) => {
          console.error("could not open a link", asCommandError(raw).detail);
        });
      }}
    >
      {children}
    </a>
  );
}
