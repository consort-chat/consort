import { useMemo } from "react";

import { linkify } from "../lib/links";
import { ExternalLink } from "./ExternalLink";

/**
 * A message that arrived as words, with its addresses found.
 *
 * The other half of `FormattedBody`. A sender who pastes a link usually sends
 * no `formatted_body` at all, because linkifying is something a client does
 * when it draws a message rather than when it sends one, so without this the
 * commonest link in a room is the one nothing can be done with.
 */
export function PlainBody({ text }: { text: string }) {
  const pieces = useMemo(() => linkify(text), [text]);

  return (
    <>
      {pieces.map((piece, index) =>
        piece.href === undefined ? (
          piece.text
        ) : (
          <ExternalLink key={index} href={piece.href}>
            {piece.text}
          </ExternalLink>
        ),
      )}
    </>
  );
}
