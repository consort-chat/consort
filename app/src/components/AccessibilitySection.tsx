import { useCallback, useEffect, useRef, useState } from "react";

import {
  appearanceSettings,
  asCommandError,
  previewApplicationScale,
  setAppearanceSettings,
  type AppearanceSettings,
} from "../lib/api";
import {
  APPLICATION_SCALE,
  TEXT_SCALE,
  applyTextScale,
  onZoomed,
  percent,
  type Range,
} from "../lib/scale";
import "./AccessibilitySection.css";

/**
 * How long after the last move to write the file, in milliseconds.
 *
 * The same delay the voice sliders use, and for the same reason: short enough
 * not to read as a delay, long enough that a drag across the range is one write
 * rather than a hundred.
 */
const WRITE_AFTER = 150;

/**
 * One size, with a slider and a readout.
 *
 * Works in whole percent rather than in the multiplier underneath, because a
 * range input steps in integers and 80 to 200 by 10 is a ladder a person can
 * feel. The multiplier goes back out at the edge of the control.
 */
function ScaleSlider({
  id,
  label,
  scale,
  range,
  onChange,
  describedBy,
}: {
  id: string;
  label: string;
  scale: number;
  range: Range;
  onChange: (scale: number) => void;
  describedBy: string;
}) {
  return (
    <div className="a11y-scale">
      <label className="a11y-scale__label" htmlFor={id}>
        {label}
      </label>
      <input
        id={id}
        className="a11y-scale__slider"
        type="range"
        min={Math.round(range.min * 100)}
        max={Math.round(range.max * 100)}
        step={Math.round(range.step * 100)}
        value={Math.round(scale * 100)}
        aria-describedby={describedBy}
        onChange={(event) => onChange(Number(event.target.value) / 100)}
      />
      {/*
        `aria-hidden`, because the range input announces its own value and a
        second reading of the same number is noise. This is for the eye.
      */}
      <output className="a11y-scale__value" htmlFor={id} aria-hidden="true">
        {percent(scale)}
      </output>
    </div>
  );
}

/**
 * How big Consort is drawn.
 *
 * Two controls, and keeping them apart is the point of the screen. Application
 * scale is the webview's zoom: it moves everything, pictures and avatars and
 * borders included, and it is what Ctrl and plus do from anywhere. Text size is
 * a root font size on top of that: it moves the words and the spacing around
 * them and leaves a picture somebody sent at the size they sent it. Somebody
 * who wants larger words on a screen that already fits wants the second one,
 * and a single control would have chosen for them.
 *
 * Called Accessibility rather than Appearance deliberately. Reduced motion is
 * the next thing that belongs here, wanted already by the flash on a jump and
 * by the chat effects work, and the name leaves room for it.
 *
 * Both sliders apply as they move and write once the pointer stops. Watching
 * the size change while dragging is the whole reason this is a slider rather
 * than a number, and a write per pointer move would be the settings file
 * rewritten a hundred times for one adjustment.
 */
export function AccessibilitySection() {
  const [settings, setSettings] = useState<AppearanceSettings | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  /*
    The last value a slider produced, and the timer that will write it.

    Refs rather than state: nothing renders differently while a write is
    pending, and a re-render per pixel of drag is what the debounce is here to
    avoid in the first place.
  */
  const pending = useRef<AppearanceSettings | null>(null);
  const writing = useRef<ReturnType<typeof setTimeout> | null>(null);

  const reload = useCallback(async () => {
    const current = await appearanceSettings();
    setSettings(current);
    // So the page and the slider cannot disagree. Ordinarily this is the size
    // already applied at launch and setting it again changes nothing.
    applyTextScale(current.textScale);
  }, []);

  useEffect(() => {
    let cancelled = false;
    void reload().catch((raw: unknown) => {
      if (!cancelled) setProblem(asCommandError(raw).message);
    });

    return () => {
      cancelled = true;
    };
  }, [reload]);

  /*
    Ctrl and plus works with this screen open, and moves the same scale the
    first slider does. Without this the slider would sit at a size the window no
    longer is, and the next drag of it would snap the window back to that.

    Read again rather than told what changed: the file is the authority on the
    size, and `App.tsx` announces only after its write has landed.
  */
  useEffect(
    () =>
      onZoomed(() => {
        void reload().catch((raw: unknown) => {
          setProblem(asCommandError(raw).message);
        });
      }),
    [reload],
  );

  const flush = useCallback(async () => {
    const next = pending.current;
    if (next === null) return;
    pending.current = null;

    try {
      await setAppearanceSettings(next);
    } catch (raw: unknown) {
      /*
        Said, and nothing put back. The window is already the size the slider
        shows, so moving the slider back would leave the two disagreeing, and
        the one somebody can see is the window.
      */
      setProblem(asCommandError(raw).message);
    }
  }, []);

  // Somebody who drags a slider and shuts settings immediately has still made
  // the change. Without this the timer goes with the component and the last
  // hundred and fifty milliseconds of intent is lost, which reads as a setting
  // that does not stick.
  useEffect(
    () => () => {
      if (writing.current === null) return;
      clearTimeout(writing.current);
      writing.current = null;
      void flush();
    },
    [flush],
  );

  /** Draw at `next`, and write it once the moving stops. */
  function slide(next: AppearanceSettings) {
    setProblem(null);
    setSettings(next);
    pending.current = next;

    if (writing.current !== null) clearTimeout(writing.current);
    writing.current = setTimeout(() => {
      writing.current = null;
      void flush();
    }, WRITE_AFTER);
  }

  /**
   * The window's own zoom, which only Rust can set.
   *
   * Separate from the text size below rather than one function taking either,
   * for the reason the voice section splits its two: the two are applied by
   * different halves of the application and a single function taking both
   * shapes would read as one knob.
   */
  function slideApplication(applicationScale: number) {
    if (settings === null) return;
    void previewApplicationScale(applicationScale);
    slide({ ...settings, applicationScale });
  }

  /** The root font size, which only the page can set. */
  function slideText(textScale: number) {
    if (settings === null) return;
    applyTextScale(textScale);
    slide({ ...settings, textScale });
  }

  return (
    <div className="a11y">
      {problem !== null && (
        <p className="a11y__problem" role="alert">
          {problem}
        </p>
      )}

      {settings !== null && (
        <>
          <div className="a11y__field">
            <ScaleSlider
              id="a11y-application-scale"
              label="Application scale"
              scale={settings.applicationScale}
              range={APPLICATION_SCALE}
              onChange={slideApplication}
              describedBy="a11y-application-scale-note"
            />
            <p className="a11y__note" id="a11y-application-scale-note">
              Everything at once, the way zoom works in a browser: words,
              avatars, pictures and the space around them. Useful on a display
              where Consort comes out small.
            </p>
            <p className="a11y__keys">
              <kbd>Ctrl</kbd> <kbd>+</kbd> and <kbd>Ctrl</kbd> <kbd>-</kbd>
              {" change this from anywhere, and "}
              <kbd>Ctrl</kbd> <kbd>0</kbd> puts it back to 100%.
            </p>
          </div>

          <div className="a11y__field">
            <ScaleSlider
              id="a11y-text-scale"
              label="Text size"
              scale={settings.textScale}
              range={TEXT_SCALE}
              onChange={slideText}
              describedBy="a11y-text-scale-note"
            />
            <p className="a11y__note" id="a11y-text-scale-note">
              The words only, and the spacing that follows them. Pictures and
              avatars stay the size they are, so this is the one to reach for if
              the window already fits and the text is what is too small.
            </p>
          </div>
        </>
      )}
    </div>
  );
}
