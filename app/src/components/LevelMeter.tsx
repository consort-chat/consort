import "./LevelMeter.css";

interface Props {
  /** Peak of the last batch of frames, 0 to 1. */
  level: number;
  /** Whether the voice gate is letting audio through right now. */
  open: boolean;
  /** Whether the microphone is open at all. */
  running: boolean;
  /** Whether the gate is deciding, or everything is going out. */
  voiceActivity: boolean;
}

/**
 * What the caption says, which is the part somebody who cannot see the bar
 * relies on.
 *
 * Four sentences rather than one, because a bar sitting at zero means several
 * completely different things and only some of them are a problem.
 * "Listening" with nothing moving is a working microphone in a quiet room.
 * "Not running" is a microphone that never opened. Collapsing them is how
 * somebody spends twenty minutes debugging silence that was never there.
 *
 * With the gate off, "We can hear you" would be true on every frame and would
 * therefore say nothing. What is worth saying instead is that nothing is being
 * held back, because that is the thing somebody has chosen and the thing they
 * might have forgotten choosing.
 */
function caption(running: boolean, open: boolean, voiceActivity: boolean): string {
  if (!running) return "Not running.";
  if (!voiceActivity) return "Sending everything the microphone hears.";
  if (open) return "We can hear you.";
  return "Listening. Say something.";
}

/**
 * The input level, as a bar and as a sentence.
 *
 * Both halves matter. The bar is the fast one: it moves twenty times a second
 * and shows at a glance whether a microphone is alive. The sentence is the one
 * that survives not being able to see it, and the one that separates the two
 * ways a still bar can happen.
 *
 * The gate state is marked on the fill as an attribute as well as a colour.
 * Mint is the presence colour throughout Consort and this is the one place it
 * would be carrying meaning on its own, which it must not.
 */
export function LevelMeter({ level, open, running, voiceActivity }: Props) {
  // Clamped here rather than trusted. The number crossed a process boundary,
  // and a fill wider than its track escapes the panel it is drawn in.
  const percent = Math.min(100, Math.max(0, level * 100));

  return (
    <div className="level-meter" data-running={running}>
      {/*
        Decoration, and hidden as such. It updates twenty times a second, so
        exposing it as a live value would read a number aloud continuously. The
        caption below is the accessible half and it changes only when the gate
        does.
      */}
      <div className="level-meter__track" aria-hidden="true">
        <div
          className="level-meter__fill"
          data-open={open}
          style={{ width: `${percent}%` }}
        />
      </div>

      <p className="level-meter__caption" role="status">
        {caption(running, open, voiceActivity)}
      </p>
    </div>
  );
}
