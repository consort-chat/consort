import { useCallback, useEffect, useRef, useState } from "react";

import { LoginScreen } from "./components/LoginScreen";
import { SignedIn } from "./components/SignedIn";
import { Splash } from "./components/Splash";
import {
  appearanceSettings,
  asCommandError,
  quit,
  sessionStatus,
  setAppearanceSettings,
  type Profile,
} from "./lib/api";
import {
  APPLICATION_SCALE,
  applyTextScale,
  stepped,
  zoomed,
  zoomIntent,
} from "./lib/scale";

type View =
  | { name: "checking" }
  | { name: "signedOut" }
  | { name: "signedIn"; profile: Profile };

export function App() {
  const [view, setView] = useState<View>({ name: "checking" });

  useEffect(() => {
    let cancelled = false;

    sessionStatus()
      .then((status) => {
        if (cancelled) return;
        setView(
          status.status === "signedIn"
            ? { name: "signedIn", profile: status.profile }
            : { name: "signedOut" },
        );
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        // The Rust side already treats an unrestorable session as signed out,
        // so reaching here means the command itself failed. There is nothing
        // useful to show but the login form.
        console.error("session_status failed", asCommandError(error));
        setView({ name: "signedOut" });
      });

    return () => {
      cancelled = true;
    };
  }, []);

  /*
    Ctrl+Q closes Consort, as it does in every other application on this
    desktop.

    Here rather than in the shell because all three views below are somewhere
    somebody can be stuck: a login that will not go through and a splash
    waiting on a homeserver are exactly when a way out is wanted, and a quit
    key that only worked once you were signed in would be missing then.

    Deliberately not filtered by what has focus, which is the one thing the
    Ctrl+V handler in `RoomTimeline` does do. A paste aimed at a settings field
    is not the room's; a quit is nobody's in particular, and a quit key that
    silently did nothing depending on where the caret sat would be worse than
    no quit key at all.

    `preventDefault` because WebKitGTK may have its own opinion about this
    combination, and the answer is ours.
  */
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (!event.ctrlKey || event.key !== "q") return;
      event.preventDefault();
      void quit();
    }

    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  /*
    The text half of the chosen size, as early as the page can apply it.

    The other half is already in place by now. The webview's zoom is Rust's to
    set and `lib::run`'s setup does it before this page has painted anything,
    which is why nothing here touches it. A root font size cannot be done from
    there: only the page can set one, so this is the earliest it can happen, and
    the screen it settles on is the splash.

    Failing is logged and nothing else. The window is still the size it was,
    which is a size somebody can read, and there is nothing useful to put in
    front of them about a font size that stayed where it was.
  */
  useEffect(() => {
    let cancelled = false;

    appearanceSettings()
      .then((appearance) => {
        if (cancelled) return;
        applyTextScale(appearance.textScale);
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        console.error("appearance_settings failed", asCommandError(error));
      });

    return () => {
      cancelled = true;
    };
  }, []);

  /*
    Ctrl and plus, Ctrl and minus, Ctrl and zero.

    Here for the reason Ctrl+Q is: somebody who cannot read the window needs the
    key to work on the splash and the login screen too, and the settings screen
    that would otherwise fix it is inside the window they cannot read. Not
    filtered by focus either, because that is what these keys do in a browser.

    The current size is read from Rust on each press rather than remembered.
    That is one IPC call per keystroke, which is nothing, and it is what keeps
    this from writing a stale text size back over one somebody has just changed
    in Settings. `zoomed` then tells an open slider to go and look again.

    One press at a time, through `queue`. These keys repeat when held, faster
    than a round trip, and two overlapping read-modify-writes would both start
    from the same size: one of the two presses would simply not happen. Chained,
    each press reads the size the one before it wrote, so holding the key walks
    the ladder a step at a time however fast it repeats. The catch is inside the
    link rather than around the chain, because a chain that rejected would
    swallow every press after the first failure.

    Only the application scale moves. Ctrl and zero puts it back to 1 and leaves
    the text size exactly as it is: these are two knobs, and a reset that
    silently moved the other would be them becoming one.
  */
  const queue = useRef<Promise<void>>(Promise.resolve());

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      const intent = zoomIntent(event);
      if (intent === null) return;
      // `preventDefault` because WebKitGTK has its own opinion about these
      // combinations, and the answer is ours.
      event.preventDefault();

      queue.current = queue.current.then(async () => {
        try {
          const current = await appearanceSettings();
          const applicationScale =
            intent === "reset"
              ? 1
              : stepped(current.applicationScale, APPLICATION_SCALE, intent);
          if (applicationScale === current.applicationScale) return;

          await setAppearanceSettings({ ...current, applicationScale });
          zoomed();
        } catch (error: unknown) {
          console.error("appearance_settings failed", asCommandError(error));
        }
      });
    }

    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const handleSignedIn = useCallback((profile: Profile) => {
    setView({ name: "signedIn", profile });
  }, []);

  const handleSignedOut = useCallback(() => {
    setView({ name: "signedOut" });
  }, []);

  switch (view.name) {
    case "checking":
      return <Splash />;
    case "signedOut":
      return <LoginScreen onSignedIn={handleSignedIn} />;
    case "signedIn":
      return <SignedIn profile={view.profile} onSignedOut={handleSignedOut} />;
  }
}
