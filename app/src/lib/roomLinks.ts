/**
 * What a message body needs in order to draw a link into Matrix.
 *
 * A context rather than props, because the shell knows the answers and the
 * pills are five levels below it, through two components whose whole job is
 * drawing text.
 *
 * Separate from the component that consumes it so that neither file mixes a
 * component export with a plain one, which is what React Fast Refresh needs in
 * order to keep state across an edit.
 */
import { createContext, useContext } from "react";

import type { PlaceTarget } from "./matrixTo";

export interface RoomLinks {
  /**
   * What this account calls a room, or null when nothing here knows.
   *
   * Null for an alias, always, because an alias is only a room after a
   * homeserver has been asked and nothing asks one to draw a badge. It is also
   * null for a room this account is not in, which is the honest answer: the
   * name of a room you are not in is not something a client has.
   */
  nameOf: (roomOrAlias: string) => string | null;
  /** Show whatever the link points at. */
  open: (target: PlaceTarget) => void;
}

/**
 * Links that go nowhere, for everything drawn outside the shell.
 *
 * Inert rather than absent so that a message body can be rendered on its own,
 * which is what the tests of the two body components do.
 */
export const RoomLinksContext = createContext<RoomLinks>({
  nameOf: () => null,
  open: () => {},
});

export function useRoomLinks(): RoomLinks {
  return useContext(RoomLinksContext);
}
