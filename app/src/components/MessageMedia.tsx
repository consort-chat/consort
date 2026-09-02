import { useState } from "react";

import {
  asCommandError,
  mediaUrl,
  saveAttachment,
  type Media,
  type MessageKind,
} from "../lib/api";
import { sizeLabel } from "../lib/labels";
import { canPlay } from "../lib/playable";
import { ImageViewer } from "./ImageViewer";
import { MediaControls } from "./MediaControls";
import "./MessageMedia.css";

/** What to say about a clip this machine has no decoder for. */
const NO_DECODER =
  "This computer has no decoder for this clip. Installing GStreamer's extra " +
  "plugins, gst-libav among them, is what fixes it. Press to save it instead.";

/**
 * The picture, the clip or the file hanging off one message.
 *
 * Nothing here fetches anything. An attachment has a URL on the `consortmedia`
 * scheme, which Rust answers in ranges, so pointing an element at one is an
 * attribute rather than a request: the picture is drawn by the browser's own
 * loader and the clip is streamed and seeked by the media element, neither of
 * them holding the file in JavaScript.
 *
 * ## A picture is drawn, a clip is asked for
 *
 * A picture is drawn as soon as the room is, because that is what a picture in
 * a conversation is for. A clip waits behind its own thumbnail: scrolling back
 * through a room of them would otherwise start a download of every one, and
 * they are the large ones. What is on the card is the still the sender
 * uploaded with it, so the thing being decided on is the picture rather than
 * the filename.
 *
 * A file and a voice note are neither. Consort has no viewer for a spreadsheet
 * and should not pretend to, so both are a card that names what was sent and
 * opens the desktop's Save As window when it is pressed.
 *
 * ## Why a clip can turn into a save card
 *
 * Because a player that cannot play is worse than no player. WebKitGTK decodes
 * through GStreamer, and a machine without `gst-libav` has no H.264 decoder and
 * no AAC decoder, which is a garbled picture, silence, and the word "Error" in
 * a corner. `canPlay` asks before the player is drawn and the element's own
 * `error` catches what the asking got wrong, and either way what is left is
 * the card that saves it, which does work.
 */
export function MessageMedia({
  kind,
  media,
}: {
  kind: Extract<MessageKind, "image" | "video" | "file" | "audio">;
  media: Media;
}) {
  const [playing, setPlaying] = useState(false);
  const [opened, setOpened] = useState(false);
  const [undecodable, setUndecodable] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  /*
    The element, once it exists, so the control bar can drive it. State rather
    than a ref, because the bar has to re-render when it arrives and a ref
    changing is not a render.
  */
  const [player, setPlayer] = useState<HTMLVideoElement | null>(null);

  const source = mediaUrl(media.source);
  const weight = sizeLabel(media.size);

  /**
   * Write it wherever the person chooses.
   *
   * Silent when they close the window without choosing. That is a change of
   * mind rather than a failure, and reporting it would be telling somebody off
   * for one.
   */
  function save() {
    setProblem(null);
    saveAttachment(media.source, media.name)
      .then((path) => {
        if (path !== null) setSaved(path);
      })
      .catch((raw: unknown) => {
        setProblem(asCommandError(raw).message);
      });
  }

  const receipt = (
    <>
      {saved !== null && <p className="media__saved">Saved to {saved}</p>}
      {problem !== null && (
        <p className="media__problem" role="alert">
          {problem}
        </p>
      )}
    </>
  );

  const card = (why?: string) => (
    <>
      <button type="button" className="media__file" onClick={save}>
        <span className="media__file-name">{media.name}</span>
        {weight !== null && <span className="media__file-size">{weight}</span>}
      </button>
      {why !== undefined && <p className="media__note">{why}</p>}
      {receipt}
    </>
  );

  if (kind === "file" || kind === "audio") return card();

  if (kind === "video") {
    // Asked before the player is drawn rather than after it fails, so a room
    // full of clips on a machine that cannot decode them says so once per
    // clip instead of playing garbage at somebody.
    if (undecodable || canPlay(media.mime) === "no") return card(NO_DECODER);

    if (!playing) {
      return (
        <button
          type="button"
          className="media__play"
          style={shapeOf(media)}
          aria-label={`Play ${media.name}`}
          onClick={() => setPlaying(true)}
        >
          {media.thumbnail !== undefined && (
            <img
              className="media__poster"
              src={mediaUrl(media.thumbnail)}
              alt=""
            />
          )}
          <span className="media__play-mark" aria-hidden="true">
            ▶
          </span>
          <span className="media__play-name">{media.name}</span>
          {weight !== null && (
            <span className="media__play-size">{weight}</span>
          )}
        </button>
      );
    }

    return (
      <div className="media__player">
        {/*
          `autoPlay` rather than a `play()` call, because by the time this
          mounts the press that asked for it is one task old and WebKit will
          have expired the gesture, which is a clip that starts muted or not
          at all. The attribute is evaluated against the same activation the
          press granted.

          `controls` is off: what WebKitGTK draws for it is its own shadow DOM
          and looks nothing like a browser's. See `MediaControls`.
        */}
        <video
          ref={setPlayer}
          className="media__video"
          src={source}
          autoPlay
          onError={() => setUndecodable(true)}
        />
        <MediaControls media={player} label={media.name} />
      </div>
    );
  }

  /*
    A button rather than an `img` with a click handler, so the picture is
    reachable from the keyboard and announces itself as something that opens.
    In a room capped at 480 by 340, a screenshot of anything with words in it
    cannot be read until it is opened.

    The frame carries the whole box and the picture fills it, which is one
    direction and cannot loop. See `boxOf` below, and `MessageMedia.css` for
    what the other direction did.
  */
  return (
    <>
      <button
        type="button"
        className="media__frame"
        style={boxOf(media)}
        aria-label={`Open ${media.name}`}
        onClick={() => setOpened(true)}
      >
        <img
          className="media__image"
          src={source}
          alt={media.name}
          width={media.width}
          height={media.height}
        />
      </button>
      {opened && <ImageViewer media={media} onClose={() => setOpened(false)} />}
    </>
  );
}

/** The widest a picture may be drawn in a room, in pixels. */
const WIDEST = 480;

/**
 * The tallest a picture may be drawn, in pixels.
 *
 * Short enough that one screenshot does not push the message under it off the
 * bottom of a room somebody is reading.
 */
const TALLEST = 340;

/**
 * How wide to draw a picture's frame.
 *
 * Every term but the percentage is worked out here, because a percentage is
 * the one thing only the browser can resolve and mixing the two is what broke
 * this twice. A `min()` holding a percentage cannot be resolved while a
 * shrink-to-fit box is being worked out, so the whole cap was dropped and the
 * frame took the width of the column; and a frame that fixed its own shape
 * while the picture inside took a percentage of that fed each layout pass the
 * previous pass's answer, which is the picture that grew a pixel at a time.
 *
 * The number is the smallest of three: the cap a room can spare, the picture's
 * own width so nothing is drawn larger than it was sent, and the width that
 * keeps it inside `TALLEST`. The percentage is what keeps it inside a narrow
 * column, which in the thread panel is most of them.
 *
 * Empty when the sender said nothing about the shape, which leaves the frame
 * hugging whatever arrives under the stylesheet's own cap. Guessing a ratio
 * would be worse than the jump.
 */
function boxOf(media: Media): { width?: string } {
  const { width, height } = media;
  if (width === undefined || height === undefined || height === 0) return {};

  const room = Math.min(WIDEST, width, Math.round((TALLEST * width) / height));
  return { width: `min(100%, ${room}px)` };
}

/**
 * The shape of the card a clip waits behind.
 *
 * Empty for a clip whose sender said nothing about its shape, which leaves the
 * card at its minimum height. Guessing a ratio would be worse than the jump: a
 * tall clip drawn in a wide box moves twice.
 *
 * Only the clip's card takes this. A picture carries its measurements on the
 * `img` itself.
 */
function shapeOf(media: Media): { aspectRatio?: string } {
  if (media.width === undefined || media.height === undefined) return {};
  return { aspectRatio: `${media.width} / ${media.height}` };
}
