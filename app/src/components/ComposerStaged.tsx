import { PaperclipIcon } from "./ComposerAttach";
import "./ComposerStaged.css";

/**
 * The one attachment waiting in a composer, and where its bytes are.
 *
 * Two shapes because there are two answers to that. A file somebody picked or
 * dropped is a path Rust will read at the moment they press send. A screenshot
 * off the clipboard has no path, so Rust holds the picture itself and there is
 * nothing here to address it by. Neither one puts bytes in the page.
 *
 * One at a time. More would change the picker, the line below, and what a
 * failure halfway through a send means, all at once.
 */
export type Staged =
  | { kind: "file"; name: string; size: number; path: string }
  | { kind: "pasted"; name: string; size: number };

/** What a staged attachment weighs, in words rather than in bytes. */
export function weigh(bytes: number): string {
  const units = ["bytes", "KB", "MB", "GB"];
  let at = 0;
  let size = bytes;
  while (size >= 1024 && at < units.length - 1) {
    size /= 1024;
    at += 1;
  }
  // Whole numbers for bytes, because "1.0 bytes" is nonsense, and one decimal
  // for everything else, because a 4 MB screenshot and a 4.7 MB one are worth
  // telling apart.
  return at === 0 ? `${size} ${units[at]}` : `${size.toFixed(1)} ${units[at]}`;
}

/**
 * What is about to be sent, above the box that captions it.
 *
 * Drawn on the same terms as the reply line: a staged attachment is a thing
 * somebody can forget they did, and a composer that says nothing about it sends
 * a photo with the next sentence typed into it.
 */
export function ComposerStaged({
  staged,
  onStop,
}: {
  staged: Staged;
  /** Take it back out of the composer. The caller owns what was staged. */
  onStop: () => void;
}) {
  return (
    <div className="composer-staged">
      <PaperclipIcon className="composer-staged__glyph" />
      <span className="composer-staged__name">{staged.name}</span>
      <span className="composer-staged__size">{weigh(staged.size)}</span>
      <button
        type="button"
        className="composer-staged__stop"
        aria-label={`Do not send ${staged.name}`}
        onClick={onStop}
      >
        &times;
      </button>
    </div>
  );
}
