import { useMemo } from "react";

import { linkify } from "../lib/links";
import { matrixTarget } from "../lib/matrixTo";
import { ExternalLink } from "./ExternalLink";
import { MatrixLink } from "./MatrixLink";

/**
 * A message that arrived as words, with its addresses found.
 *
 * The other half of `FormattedBody`. A sender who pastes a link usually sends
 * no `formatted_body` at all, because linkifying is something a client does
 * when it draws a message rather than when it sends one, so without this the
 * commonest link in a room is the one nothing can be done with.
 *
 * That includes the links Consort itself hands out. Copying a message's address
 * and pasting it into a room is exactly this case: plain words, no formatting,
 * and an address that names something in Matrix rather than a website.
 */
export function PlainBody({ text }: { text: string }) {
  const pieces = useMemo(() => linkify(text), [text]);

  return (
    <>
      {pieces.map((piece, index) => {
        if (piece.href === undefined) return piece.text;

        const target = matrixTarget(piece.href);
        if (target !== undefined && target.kind !== "person") {
          return <MatrixLink key={index} target={target} />;
        }

        return (
          <ExternalLink key={index} href={piece.href}>
            {piece.text}
          </ExternalLink>
        );
      })}
    </>
  );
}
