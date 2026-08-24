import "./Splash.css";

/**
 * Shown while the stored session is restored.
 *
 * Deliberately quiet and without a spinner. A restore is usually fast enough
 * that a spinner appears and vanishes as a flicker, which reads as a glitch.
 * The mark fades in slowly instead, so a fast restore looks like nothing
 * happened and a slow one looks intentional.
 */
export function Splash() {
  return (
    <div className="splash">
      <div className="splash__mark" aria-hidden="true" />
      <p className="splash__label">Signing you in</p>
    </div>
  );
}
