import { useEffect, useRef, useState } from "react";

import "./MediaControls.css";

/** A running time, as `m:ss` or `h:mm:ss`. */
export function clock(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "0:00";

  const whole = Math.floor(seconds);
  const parts = [Math.floor(whole / 3600), Math.floor((whole % 3600) / 60), whole % 60];
  const shown = parts[0] === 0 ? parts.slice(1) : parts;

  return shown
    .map((part, index) => (index === 0 ? String(part) : String(part).padStart(2, "0")))
    .join(":");
}

/**
 * The bar under a clip.
 *
 * Written out rather than left to `controls`, and that is not a preference.
 * WebKitGTK's default player is its own shadow DOM, and it puts a fullscreen
 * button in the top left corner of the picture, a speaker in the top right,
 * and a bar across the bottom that says "Error" when a codec is missing. It
 * looks nothing like a browser's because the browser here is not the one
 * anybody has seen. This is the only way to get a control that reads as one.
 *
 * Deliberately small: play, a scrub bar with the time either side, mute, and
 * fullscreen. Playback rate, captions, picture in picture and a volume slider
 * are all things a clip in a chat room has never wanted.
 *
 * Everything it draws is read off the element rather than remembered here. A
 * media element is driven by things other than these buttons, the keyboard and
 * the end of the file among them, so state kept here would drift out of step
 * with the picture beside it.
 */
export function MediaControls({
  media,
  label,
}: {
  /** The element to drive. Null until the player has mounted. */
  media: HTMLVideoElement | null;
  /** What is playing, for the buttons that need naming out loud. */
  label: string;
}) {
  const [playing, setPlaying] = useState(false);
  const [muted, setMuted] = useState(false);
  const [at, setAt] = useState(0);
  const [length, setLength] = useState(0);
  const bar = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (media === null) return;

    function sync() {
      if (media === null) return;
      setPlaying(!media.paused && !media.ended);
      setMuted(media.muted);
      setAt(media.currentTime);
      setLength(Number.isFinite(media.duration) ? media.duration : 0);
    }

    // Every one of these changes something on the bar, and several of them
    // happen without anybody pressing anything: a clip ends, a seek lands, the
    // duration arrives once enough of the file has.
    const events = [
      "play",
      "pause",
      "ended",
      "timeupdate",
      "durationchange",
      "volumechange",
      "seeked",
      "loadedmetadata",
    ];
    for (const event of events) media.addEventListener(event, sync);
    sync();

    return () => {
      for (const event of events) media.removeEventListener(event, sync);
    };
  }, [media]);

  return (
    <div className="controls">
      <button
        type="button"
        className="controls__button"
        aria-label={playing ? `Pause ${label}` : `Play ${label}`}
        onClick={() => {
          if (media === null) return;
          if (media.paused) void media.play().catch(() => {});
          else media.pause();
        }}
      >
        {playing ? "❚❚" : "▶"}
      </button>

      <span className="controls__time">{clock(at)}</span>

      {/*
        A range input rather than a bar with a pointer handler on it. It is
        draggable, arrow-key seekable and announced as a slider without any of
        that being written here, and a scrub bar that only a mouse can move is
        a clip only a mouse can navigate.
      */}
      <input
        ref={bar}
        type="range"
        className="controls__scrub"
        aria-label={`Position in ${label}`}
        min={0}
        max={length === 0 ? 1 : length}
        step="any"
        value={at}
        disabled={length === 0}
        onChange={(event) => {
          if (media === null) return;
          media.currentTime = Number(event.target.value);
        }}
      />

      <span className="controls__time">{clock(length)}</span>

      <button
        type="button"
        className="controls__button"
        aria-label={muted ? `Unmute ${label}` : `Mute ${label}`}
        onClick={() => {
          if (media !== null) media.muted = !media.muted;
        }}
      >
        {muted ? "🔇" : "🔊"}
      </button>

      <button
        type="button"
        className="controls__button"
        aria-label={`Show ${label} full screen`}
        onClick={() => {
          void media?.requestFullscreen?.().catch(() => {});
        }}
      >
        ⛶
      </button>
    </div>
  );
}
