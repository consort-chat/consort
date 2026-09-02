/**
 * Whether this machine can actually play a clip, asked before drawing a
 * player for one.
 *
 * WebKitGTK plays media through GStreamer and answers `canPlayType` out of the
 * live plugin registry, so this is a real question with a real answer rather
 * than a guess. On a machine with no `gst-libav` there is no H.264 decoder and
 * no AAC decoder at all, and every mp4 in every room is a garbled picture,
 * silence, and the word "Error" in the corner of a player that will never
 * work. Asking first is the difference between saying that and drawing one.
 *
 * ## Why the codecs are named
 *
 * `canPlayType("video/mp4")` answers "maybe" on a browser with no decoder for
 * anything inside an mp4, because the container is one it knows. The question
 * only becomes answerable when the codecs are named, and what a message
 * carries is the container: so a representative codec string per container is
 * added here.
 *
 * That makes the answer a guess about what is inside the file, and the guess
 * is the overwhelmingly common case for each container. It is checked twice
 * either way: a clip this says nothing about still gets its player, and a
 * player that errors falls back to the same card.
 */

/**
 * The codecs to ask about, per container.
 *
 * One entry each, and the ordinary contents of that container: H.264 and AAC
 * for an mp4, VP8 and Vorbis for a webm. A container not listed here is one
 * with no common answer, and gets no opinion rather than a wrong one.
 */
const TYPICAL: Record<string, string> = {
  "video/mp4": 'video/mp4; codecs="avc1.42E01E, mp4a.40.2"',
  "video/webm": 'video/webm; codecs="vp8, vorbis"',
};

/** Whether a clip will play, as far as anything here can tell. */
export type Playable = "yes" | "no" | "unknown";

/**
 * Whether this machine has what it takes to play `mime`.
 *
 * `"unknown"` for a clip whose sender named no type and for a container with
 * no representative codecs, both of which are a reason to try rather than a
 * reason to refuse.
 */
export function canPlay(mime: string | undefined): Playable {
  if (mime === undefined) return "unknown";

  const container = mime.split(";", 1)[0]?.trim().toLowerCase() ?? "";
  const asked = TYPICAL[container];
  if (asked === undefined) return "unknown";

  // The empty string is the only definite no the API has. "maybe" is taken as
  // a yes, because refusing a clip that would have played is a worse mistake
  // than letting the player try and falling back when it errors.
  return document.createElement("video").canPlayType(asked) === ""
    ? "no"
    : "yes";
}
