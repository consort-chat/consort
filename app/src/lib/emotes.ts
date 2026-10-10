import { useEffect, useState } from "react";

import { emojiPacks, type ImagePack } from "./api";

/**
 * No custom emoji, which is where every room starts and where a room with
 * none stays.
 *
 * One frozen instance rather than a fresh `new Map()`, so a component holding
 * it does not get a different object on every render and redraw a list of
 * pills for it. The same reason `NOBODY` in `api.ts` is one.
 */
const NAMELESS: ReadonlyMap<string, string> = new Map();

/**
 * What each custom emoji reachable from `roomId` is called, by its `mxc://`.
 *
 * Backwards on purpose. A pack is a shortcode to an address, and this is the
 * other direction, because that is the question a drawn message asks: MSC2545
 * keys a reaction by the image's own URI, so a pill arrives holding an address
 * and needing a name.
 *
 * Empty until the answer lands, and empty again if it cannot be had. What is
 * lost either way is the name on a pill, which falls back to saying what kind
 * of thing it is: see `nameOfKey` in `MessageGroups`.
 *
 * Nothing is asked for an empty room ID, which is what a thread panel with no
 * thread open holds.
 */
export function useEmoteNames(roomId: string): ReadonlyMap<string, string> {
  const [names, setNames] = useState<ReadonlyMap<string, string>>(NAMELESS);

  useEffect(() => {
    if (roomId === "") return;

    let cancelled = false;
    emojiPacks(roomId)
      .then((packs) => {
        if (!cancelled) setNames(namesIn(packs));
      })
      .catch(() => {
        if (!cancelled) setNames(NAMELESS);
      });

    return () => {
      cancelled = true;
    };
  }, [roomId]);

  return names;
}

/**
 * Every image in `packs`, by address.
 *
 * The first name an address was given wins, which keeps the order the packs
 * arrived in: `consort_matrix::emotes` returns them deliberately, nearest
 * first, and taking the last would name somebody's own emoji after whichever
 * room it also happens to live in.
 */
function namesIn(packs: readonly ImagePack[]): ReadonlyMap<string, string> {
  const names = new Map<string, string>();
  for (const pack of packs) {
    for (const image of pack.images) {
      if (!names.has(image.url)) names.set(image.url, image.shortcode);
    }
  }
  return names;
}
