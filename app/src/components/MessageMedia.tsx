import { useEffect, useState } from "react";

import { asCommandError, timelineMedia, type Media, type MessageKind } from "../lib/api";
import { sizeLabel } from "../lib/labels";
import "./MessageMedia.css";

/**
 * The picture or the clip hanging off one message.
 *
 * The bytes arrive as bytes and become a blob here, which is the whole reason
 * this is not another data URL: a photograph is megabytes, and encoding it
 * would add a third to that before this side had to hold it as a string. The
 * blob is let go of when the message leaves the room.
 *
 * A file and a voice note are neither: they are a card naming what was sent
 * and what it weighs, because Consort has no viewer for a spreadsheet and
 * should not pretend to.
 *
 * ## A picture is fetched, a clip is asked for
 *
 * A picture is drawn as soon as the room is, because that is what a picture in
 * a conversation is for. A clip waits: scrolling back through a room of them
 * would otherwise be a download of every one, and they are the large ones. The
 * button says what it is called and what it will cost before anybody commits
 * to it.
 */
export function MessageMedia({
  kind,
  media,
}: {
  kind: Extract<MessageKind, "image" | "video" | "file" | "audio">;
  media: Media;
}) {
  const [url, setUrl] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [wanted, setWanted] = useState(kind === "image");
  const name = media.name;

  useEffect(() => {
    if (!wanted) return;

    let live = true;
    let held: string | null = null;

    void timelineMedia(media.source)
      .then((bytes) => {
        // The type the sender named, which Rust kept only if it named a
        // picture or a clip. A browser sniffs a picture without it and often
        // will not play a clip without it.
        const fresh = URL.createObjectURL(
          new Blob([bytes], { type: media.mime ?? "" }),
        );
        if (!live) {
          URL.revokeObjectURL(fresh);
          return;
        }
        held = fresh;
        setUrl(fresh);
      })
      .catch((raw: unknown) => {
        if (live) setProblem(asCommandError(raw).message);
      });

    return () => {
      live = false;
      if (held !== null) URL.revokeObjectURL(held);
    };
  }, [wanted, media.source, media.mime]);

  if (problem !== null) {
    return <p className="media__problem">{problem}</p>;
  }

  // Neither is played and neither is looked at, so neither is fetched. What
  // Consort can honestly offer for a spreadsheet is its name and its weight.
  if (kind === "file" || kind === "audio") {
    const weight = sizeLabel(media.size);
    return (
      <p className="media__file">
        <span className="media__file-name">{name}</span>
        {weight !== null && <span className="media__file-size">{weight}</span>}
      </p>
    );
  }

  if (kind === "video") {
    if (!wanted) {
      const weight = sizeLabel(media.size);
      return (
        <button
          type="button"
          className="media__play"
          onClick={() => setWanted(true)}
        >
          <span className="media__play-name">{name}</span>
          {weight !== null && <span className="media__play-size">{weight}</span>}
        </button>
      );
    }

    return url === null ? (
      <p className="media__waiting">Loading {name}...</p>
    ) : (
      <video className="media__video" src={url} controls autoPlay />
    );
  }

  return (
    <div className="media__frame" style={shapeOf(media)}>
      {url !== null && <img className="media__image" src={url} alt={name} />}
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
