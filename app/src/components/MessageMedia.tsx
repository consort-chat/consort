import { useState } from "react";

import { mediaUrl, type Media, type MessageKind } from "../lib/api";
import { sizeLabel } from "../lib/labels";
import "./MessageMedia.css";

/**
 * The picture, the clip or the file hanging off one message.
 *
 * Nothing here fetches anything. An attachment has a URL on the `consortmedia`
 * scheme, which Rust answers in ranges, so pointing an element at one is an
 * attribute rather than a request: the picture is drawn by the browser's own
 * loader and the clip is streamed and seeked by the media element, neither of
 * them holding the file in JavaScript.
 *
 * That is what replaced the blob 0.1.3 built. A blob answers no range request,
 * so a clip could not begin until every byte had crossed the IPC boundary and
 * could not be seeked once it had, and the whole file was then held twice.
 *
 * ## A picture is drawn, a clip is asked for
 *
 * A picture is drawn as soon as the room is, because that is what a picture in
 * a conversation is for. A clip waits: scrolling back through a room of them
 * would otherwise start a download of every one, and they are the large ones.
 * The card says what it is called and what it will cost before anybody commits.
 *
 * A file and a voice note are neither. Consort has no viewer for a spreadsheet
 * and should not pretend to, so both are a card naming what was sent.
 */
export function MessageMedia({
  kind,
  media,
}: {
  kind: Extract<MessageKind, "image" | "video" | "file" | "audio">;
  media: Media;
}) {
  const [playing, setPlaying] = useState(false);
  const source = mediaUrl(media.source);
  const weight = sizeLabel(media.size);

  if (kind === "file" || kind === "audio") {
    return (
      <p className="media__file">
        <span className="media__file-name">{media.name}</span>
        {weight !== null && <span className="media__file-size">{weight}</span>}
      </p>
    );
  }

  if (kind === "video") {
    return playing ? (
      <video className="media__video" src={source} controls autoPlay />
    ) : (
      <button
        type="button"
        className="media__play"
        onClick={() => setPlaying(true)}
      >
        <span className="media__play-name">{media.name}</span>
        {weight !== null && <span className="media__play-size">{weight}</span>}
      </button>
    );
  }

  return (
    <div className="media__frame" style={shapeOf(media)}>
      <img className="media__image" src={source} alt={media.name} />
    </div>
  );
}

/**
 * The space to hold while the bytes are on their way.
 *
 * Empty for an attachment whose sender said nothing about its shape, which
 * leaves the frame sized by what arrives. Guessing a ratio would be worse than
 * the jump: a tall picture drawn in a wide box moves twice.
 */
function shapeOf(media: Media): { aspectRatio?: string } {
  if (media.width === undefined || media.height === undefined) return {};
  return { aspectRatio: `${media.width} / ${media.height}` };
}
