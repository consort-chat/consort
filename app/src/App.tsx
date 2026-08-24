import { useCallback, useEffect, useState } from "react";

import { LoginScreen } from "./components/LoginScreen";
import { SignedIn } from "./components/SignedIn";
import { Splash } from "./components/Splash";
import { asCommandError, sessionStatus, type Profile } from "./lib/api";

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
