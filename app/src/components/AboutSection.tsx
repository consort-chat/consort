import { useCallback, useEffect, useState } from "react";

import {
  appVersion,
  asCommandError,
  updateCheck,
  updatesItself,
  type UpdateLook,
} from "../lib/api";
import "./AboutSection.css";

/**
 * What is running, and whether this build can replace itself.
 *
 * The control is here rather than on the bar across the top because the bar
 * only exists once there is something to offer, and "is there anything?" is a
 * question somebody asks when nothing has been offered.
 */
export function AboutSection() {
  const [version, setVersion] = useState<string | null>(null);
  const [itself, setItself] = useState<boolean | null>(null);
  const [looking, setLooking] = useState(false);
  const [found, setFound] = useState<UpdateLook | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  // No `cancelled` guard: every handler here does nothing but set state, which
  // React drops after an unmount. See COVERAGE.md on checks that earn a place.
  useEffect(() => {
    const fail = (raw: unknown) => setProblem(asCommandError(raw).message);

    void appVersion().then(setVersion, fail);
    void updatesItself().then(setItself, fail);
  }, []);

  const look = useCallback(() => {
    setLooking(true);
    setFound(null);
    void updateCheck().then(
      (answer) => {
        setFound(answer);
        setLooking(false);
      },
      (raw: unknown) => {
        setProblem(asCommandError(raw).message);
        setLooking(false);
      },
    );
  }, []);

  return (
    <div className="about">
      {problem !== null && (
        <p className="about__problem" role="alert">
          {problem}
        </p>
      )}

      {itself === true && (
        <button
          className="button button--small about__check"
          disabled={looking}
          onClick={look}
          type="button"
        >
          Check for updates
        </button>
      )}

      {version !== null && <p className="about__version">Consort {version}</p>}

      {/* Always drawn, so that an answer arriving is a change to a live region
          rather than a new one appearing. */}
      <p className="about__said" role="status">
        {looking ? "Checking for updates." : sentence(found)}
      </p>

      {itself === false && (
        <p className="about__note">
          This build does not update itself. It is replaced wherever it came
          from: your package manager for a Linux package, or another build for
          one you built yourself.
        </p>
      )}
    </div>
  );
}

/**
 * What one look came back with. The release it found is not offered for
 * installing here: the bar that does that is behind this modal, so the
 * sentence says the thing that is in the way instead.
 */
function sentence(found: UpdateLook | null): string {
  if (found === null) return "";
  switch (found.state) {
    case "upToDate":
      return "Consort is up to date.";
    case "ready":
      return `Consort ${found.version} is available. Close settings to install it.`;
    case "failed":
      return found.reason;
  }
}
