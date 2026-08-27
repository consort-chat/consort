import { useEffect, useRef, useState } from "react";

import type { Profile } from "../lib/api";
import { MyAccountSection } from "./MyAccountSection";
import { VoiceVideoSection } from "./VoiceVideoSection";
import "./SettingsModal.css";

/** The panes, in the order the sidebar lists them. */
const SECTIONS = [
  { id: "account", label: "My Account" },
  { id: "voice", label: "Voice & Video" },
] as const;

type SectionId = (typeof SECTIONS)[number]["id"];

/** Everything inside the dialog that a Tab can land on, in document order. */
function focusableWithin(root: HTMLElement): HTMLElement[] {
  return Array.from(
    root.querySelectorAll<HTMLElement>(
      'a[href], button:not([disabled]), select:not([disabled]), input:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
    ),
  );
}

interface Props {
  profile: Profile;
  onClose: () => void;
  onSignedOut: () => void;
}

/**
 * Settings, as a modal over the whole application.
 *
 * Full-bleed rather than a small centred card, which is what Discord does and
 * is right for the same reason: this is a place people go to do one thing and
 * leave, and a panel that covers everything removes the question of what is
 * still live behind it. Nothing behind it is: the shell is marked inert while
 * this is up.
 *
 * Three separate pieces of focus handling, and all three are needed. Focus
 * moves in on open, or a keyboard user is still somewhere in the channel list
 * with a dialog they cannot reach. Tab wraps, or it walks straight out of the
 * dialog into that same list. Focus returns on close, or it lands at the top
 * of the document and they tab through the whole shell to get back.
 *
 * `VoiceVideoSection` is mounted only while it is showing. It opens the
 * microphone on mount, and opening settings to change something else should
 * not take the device away from whatever else was using it.
 */
export function SettingsModal({ profile, onClose, onSignedOut }: Props) {
  const [section, setSection] = useState<SectionId>("account");
  const dialog = useRef<HTMLDivElement>(null);

  /*
    Captured during the first render rather than in the effect below.

    By the time an effect runs, React has already committed the DOM, and the
    same commit that mounts this dialog marks the shell behind it inert. That
    blurs whatever had focus, so an effect reading `document.activeElement`
    finds `body` and there is nothing left to give focus back to. A lazy state
    initialiser runs before the commit, while the gear is still focused.
  */
  const [opener] = useState<Element | null>(() => document.activeElement);

  useEffect(() => {
    const first =
      dialog.current === null ? undefined : focusableWithin(dialog.current).at(0);
    first?.focus();

    return () => {
      if (opener instanceof HTMLElement && document.contains(opener)) {
        opener.focus();
      }
    };
  }, []);

  /*
    Escape is bound to the document and not to the dialog.

    A key event goes to whatever has focus, and plenty of what is inside here
    cannot take focus: the headings, the labels, the level meter, the gaps
    between fields. Clicking any of it leaves focus on `body`, and a handler
    bound to the dialog element never sees a keystroke that started outside it.
    Bound to the element, the shortcut works until the first click and then
    quietly stops, which is worse than not having it.

    Safe to have at the document because nothing else is listening: the shell
    behind this is inert while it is up.
  */
  useEffect(() => {
    function onEscape(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      onClose();
    }

    document.addEventListener("keydown", onEscape);
    return () => document.removeEventListener("keydown", onEscape);
  }, [onClose]);

  // Tab only. Escape is handled at the document, above, because it has to
  // work when focus is on nothing in particular; Tab by definition does not.
  function onKeyDown(event: React.KeyboardEvent<HTMLDivElement>) {
    if (event.key !== "Tab" || dialog.current === null) return;

    const focusable = focusableWithin(dialog.current);
    if (focusable.length === 0) return;

    const first = focusable.at(0);
    const last = focusable.at(-1);
    if (first === undefined || last === undefined) return;
    const active = document.activeElement;

    // Only the two ends are handled. Everything between them is the browser's
    // own tab order, which is better than anything reimplemented here.
    if (event.shiftKey && active === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  }

  return (
    <div
      className="settings"
      data-testid="settings-backdrop"
      /*
        `mousedown` rather than `click`. A click that starts inside on a control
        and finishes out here counts as a click on the backdrop, and closing on
        that throws away whatever was being done. Comparing target to
        currentTarget on the press is what makes "clicked the backdrop" mean
        the backdrop.
      */
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        className="settings__dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Settings"
        ref={dialog}
        onKeyDown={onKeyDown}
      >
        <nav className="settings__nav" aria-label="Settings sections">
          <p className="settings__nav-heading">Settings</p>
          {SECTIONS.map((entry) => (
            <button
              key={entry.id}
              className="settings__nav-item"
              onClick={() => setSection(entry.id)}
              aria-current={section === entry.id ? "page" : undefined}
            >
              {entry.label}
            </button>
          ))}
        </nav>

        <div className="settings__pane">
          <div className="settings__pane-head">
            <h2 className="settings__title">
              {SECTIONS.find((entry) => entry.id === section)?.label}
            </h2>
            {/*
              The keyboard shortcut is printed under the button rather than
              only bound. It is the fastest way out and nothing else on screen
              would tell you it exists.
            */}
            <button
              className="settings__close"
              onClick={onClose}
              aria-label="Close settings"
              title="Close settings"
            >
              <span className="settings__close-mark" aria-hidden="true">
                ✕
              </span>
              <span className="settings__close-hint" aria-hidden="true">
                ESC
              </span>
            </button>
          </div>

          <div className="settings__body">
            {section === "account" && (
              <MyAccountSection profile={profile} onSignedOut={onSignedOut} />
            )}
            {section === "voice" && <VoiceVideoSection />}
          </div>
        </div>
      </div>
    </div>
  );
}
