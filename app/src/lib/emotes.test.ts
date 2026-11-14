import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";

const emojiPacks = vi.hoisted(() => vi.fn());
vi.mock("./api", () => ({ emojiPacks }));

import { useEmoteNames } from "./emotes";

const ROOM = "!general:example.org";

const CAT = "mxc://example.org/cat";
const PARROT = "mxc://example.org/parrot";

/** One pack, as the command answers with it. */
function pack(id: string, images: [string, string][]) {
  return {
    id,
    images: images.map(([shortcode, url]) => ({ shortcode, url })),
  };
}

beforeEach(() => {
  emojiPacks.mockReset().mockResolvedValue([]);
});

describe("what a custom emoji in this room is called", () => {
  it("knows nothing before the answer arrives", () => {
    const { result } = renderHook(() => useEmoteNames(ROOM));

    expect(result.current.size).toBe(0);
  });

  it("names every image in every pack by its address", async () => {
    emojiPacks.mockResolvedValue([
      pack("account", [["blobcat", CAT]]),
      pack(`${ROOM}/`, [["partyparrot", PARROT]]),
    ]);

    const { result } = renderHook(() => useEmoteNames(ROOM));

    await waitFor(() => expect(result.current.size).toBe(2));
    expect(result.current.get(CAT)).toBe("blobcat");
    expect(result.current.get(PARROT)).toBe("partyparrot");
  });

  it("keeps the first name an address was given", async () => {
    // The same image in two packs, called two things. The packs arrive in a
    // deliberate order, so the first answer is the one to keep: a map that
    // took the last would name an emoji after whichever room it also happens
    // to live in.
    emojiPacks.mockResolvedValue([
      pack("account", [["blobcat", CAT]]),
      pack(`${ROOM}/`, [["cat", CAT]]),
    ]);

    const { result } = renderHook(() => useEmoteNames(ROOM));

    await waitFor(() => expect(result.current.size).toBe(1));
    expect(result.current.get(CAT)).toBe("blobcat");
  });

  it("asks about the room it was given", async () => {
    renderHook(() => useEmoteNames(ROOM));

    await waitFor(() => expect(emojiPacks).toHaveBeenCalledWith(ROOM));
  });

  it("asks again when the room changes", async () => {
    const { rerender } = renderHook(({ room }) => useEmoteNames(room), {
      initialProps: { room: ROOM },
    });
    await waitFor(() => expect(emojiPacks).toHaveBeenCalledWith(ROOM));

    rerender({ room: "!other:example.org" });

    await waitFor(() =>
      expect(emojiPacks).toHaveBeenCalledWith("!other:example.org"),
    );
  });

  it("asks nothing when there is no room to ask about", () => {
    // The thread panel draws with no thread open, which is an empty room ID.
    renderHook(() => useEmoteNames(""));

    expect(emojiPacks).not.toHaveBeenCalled();
  });

  it("knows nothing rather than failing when the packs cannot be read", async () => {
    // A homeserver that would not answer must not take the conversation with
    // it. What is lost is a name on a pill, which falls back to saying what
    // kind of thing it is.
    emojiPacks.mockImplementation(() => Promise.reject(new Error("nope")));

    const { result } = renderHook(() => useEmoteNames(ROOM));

    await waitFor(() => expect(emojiPacks).toHaveBeenCalled());
    expect(result.current.size).toBe(0);
  });

  it("forgets what it knew when it moves to a room it cannot read", async () => {
    // Otherwise the names from the room somebody just left stay on the pills
    // of the room they arrived in.
    emojiPacks.mockResolvedValue([pack("account", [["blobcat", CAT]])]);
    const { result, rerender } = renderHook(
      ({ room }) => useEmoteNames(room),
      { initialProps: { room: ROOM } },
    );
    await waitFor(() => expect(result.current.size).toBe(1));

    emojiPacks.mockImplementation(() => Promise.reject(new Error("nope")));
    rerender({ room: "!other:example.org" });

    await waitFor(() => expect(result.current.size).toBe(0));
  });
});
