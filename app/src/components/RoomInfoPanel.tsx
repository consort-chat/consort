import { useEffect, useState } from "react";

import { asCommandError, roomCopyLink, type Channel } from "../lib/api";
import { channelHeading } from "../lib/labels";
import { RoomAvatar } from "./RoomAvatar";
import { COPIED_FOR } from "./RoomTimeline";
import "./RoomInfoPanel.css";

/**
 * What a room is, beside the room.
 *
 * Opened from the heading, which until now was a line of text that did
 * nothing. Everything here is already in the snapshot the shell holds, so
 * nothing is fetched and nothing can fail to arrive: the panel is the same
 * facts the header has room for one line of, drawn at a size that can hold all
 * of them.
 *
 * A column beside the room rather than a dialog over it, for the reason the
 * thread panel is one: what somebody is reading when they ask what a room is
 * about is the room, and a panel that covered it would hide the answer to the
 * question behind it.
 */
export function RoomInfoPanel({
  channel,
  onClose,
}: {
  /** The room this is about, straight off the snapshot the shell holds. */
  channel: Channel;
  onClose: () => void;
}) {
  const [copied, setCopied] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  /*
    Escape shuts the panel, which is what it does to everything else here that
    can be dismissed.

    On `window` for the reason the thread panel's is: the picture viewer, the
    settings dialog, the reaction picker and a person's card all catch the key
    at the document and stop it there, one step nearer the press. Listening
    further out is what leaves them the press while any of them is open over
    this, and what leaves the composer the press while it has a reply or an
    attachment to clear. One press, one thing.

    Nothing guards on the panel being open because the shell only mounts this
    while it is, so the listener goes with it.
  */
  useEffect(() => {
    function shut(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      onClose();
    }

    window.addEventListener("keydown", shut);
    return () => {
      window.removeEventListener("keydown", shut);
    };
  }, [onClose]);

  // The tick on the copy control, put back after a moment. The same length the
  // one on a message holds for, because two ticks on one screen that fade at
  // different speeds is a difference nobody could see the reason for.
  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), COPIED_FOR);
    return () => window.clearTimeout(timer);
  }, [copied]);

  const name = channelHeading(channel);

  function copyLink() {
    setProblem(null);
    void roomCopyLink(channel.id)
      .then(() => setCopied(true))
      .catch((raw: unknown) => {
        setProblem(asCommandError(raw).message);
      });
  }

  return (
    <aside className="info" aria-label={`About ${name}`}>
      <div className="info__head">
        <h2 className="info__title">Room info</h2>
        <button
          type="button"
          className="info__close"
          aria-label="Close room info"
          onClick={onClose}
        >
          &times;
        </button>
      </div>

      <div className="info__scroll">
        <div className="info__identity">
          <RoomAvatar
            roomId={channel.id}
            name={name}
            avatar={channel.avatar}
            className="info__avatar"
          />
          <p className="info__name">{name}</p>
          {/*
            The address the room published, under the name it goes by. Absent
            for most private rooms, and absent is drawn as nothing: a room with
            no published address has not failed to have one.
          */}
          {channel.alias !== undefined && (
            <p className="info__alias">{channel.alias}</p>
          )}
        </div>

        {/*
          The whole topic, wrapped, which is the thing the header cannot do.
          `pre-wrap` because a topic is free text and the line breaks somebody
          put in one are part of what they wrote.
        */}
        <section className="info__section" aria-labelledby="info-topic">
          <h3 className="info__heading" id="info-topic">
            Topic
          </h3>
          {channel.topic === undefined ? (
            <p className="info__none">This room has not set a topic.</p>
          ) : (
            <p className="info__topic">{channel.topic}</p>
          )}
        </section>

        <button type="button" className="info__copy" onClick={copyLink}>
          {copied ? "Link copied" : "Copy room link"}
        </button>

        {problem !== null && (
          <p className="info__problem" role="alert">
            {problem}
          </p>
        )}
      </div>
    </aside>
  );
}
