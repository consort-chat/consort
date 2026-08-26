import type {
  Connection,
  KeyBackup,
  Profile,
  TokenStorage,
  Verification,
  VerificationFlow,
} from "../lib/api";
import { UserPanel } from "./UserPanel";
import { VerificationBanner } from "./VerificationBanner";
import { VerificationFlowPanel } from "./VerificationFlow";
import "./AppShell.css";

/**
 * The one thing worth saying about room keys, said only when it is true.
 *
 * Four of the five states get silence, and that is deliberate rather than
 * lazy. `enabled` is the expected case and announcing it would be one more
 * box on a screen that already has two. `preparing` is a state that lasts a
 * second. `unknown` is not knowing, which is not news. `unusable` means there
 * is a backup this session cannot read yet, and the verification banner right
 * above is already saying exactly what to do about that.
 *
 * `missing` is the one nothing else covers. There is no backup, for anybody,
 * and every key this device holds dies with it. Somebody should be told that
 * while they can still do something about it.
 */
function KeyBackupNotice({ state }: { state: KeyBackup["state"] }) {
  if (state !== "missing") return null;

  return (
    <p
      className="shell__notice"
      role="status"
      aria-label="Whether your message keys are backed up"
    >
      This account has no key backup, so the messages you receive here can only
      be read on this device. Set one up from a client that can, and they will
      follow you.
    </p>
  );
}

interface Props {
  profile: Profile;
  connection: Connection;
  verification: Verification;
  keyBackup: KeyBackup;
  storage: TokenStorage | null;
  flows: VerificationFlow[];
  canStartVerification: boolean;
  signingOut: boolean;
  onDismissFlow: (flowId: string) => void;
  onSigningOut: () => void;
  onSignedOut: () => void;
}

/**
 * The signed-in layout: a rail of spaces, that space's channels, and a pane.
 *
 * Three columns, because that is the shape of every client anybody already
 * uses for this, and because the voice work needs a place where a channel is
 * one click from being joined. The first two are fixed width and the third
 * takes what is left, so the furniture stays put when the window changes size.
 *
 * The main pane is empty on purpose. Text messaging is not built, and a pane
 * full of placeholder chrome would imply otherwise. What it does hold is
 * everything that has to be said about the session itself: a verification
 * flow in progress, whether this session is verified, whether room keys
 * survive it, and where the access token ended up. None of that fits in a
 * sixty pixel strip, which is why none of it is in one.
 */
export function AppShell({
  profile,
  connection,
  verification,
  keyBackup,
  storage,
  flows,
  canStartVerification,
  signingOut,
  onDismissFlow,
  onSigningOut,
  onSignedOut,
}: Props) {
  return (
    <div className="shell">
      <nav className="shell__rail" aria-label="Spaces">
        {/* Filled in by the rail. Until then it is furniture with no content. */}
      </nav>

      <div className="shell__sidebar">
        <div className="shell__channels" aria-label="Channels">
          {/* Filled in by the channel list. */}
        </div>
        <UserPanel
          profile={profile}
          connection={connection}
          pending={signingOut}
          onSigningOut={onSigningOut}
          onSignedOut={onSignedOut}
        />
      </div>

      <main className="shell__main">
        <div className="shell__alerts">
          {flows.map((flow) => (
            <VerificationFlowPanel
              key={flow.flowId}
              flow={flow}
              onDismiss={() => onDismissFlow(flow.flowId)}
            />
          ))}

          <VerificationBanner
            state={verification.state}
            canStart={canStartVerification}
          />

          <KeyBackupNotice state={keyBackup.state} />

          {/*
            Shown only when we had to fall back. Storing the token in the
            system keyring is the expected case and does not need announcing;
            storing it in a file is a real, if small, reduction in protection
            and the person it affects should be the one who knows about it.
          */}
          {storage !== null && !storage.isPreferred && (
            <p
              className="shell__notice"
              role="status"
              aria-label="Where your sign-in is stored"
            >
              {storage.description}
            </p>
          )}
        </div>

        <div className="shell__empty">
          {/*
            The page's `h1`. It moved here from the account name, which was
            this screen's heading back when the screen was one centred card.
            When a channel can be selected this becomes its name; until then
            the honest heading is the one that says there is nothing here.
          */}
          <h1 className="shell__empty-headline">Nothing here yet</h1>
          <p className="shell__empty-detail">Messages come after voice.</p>
        </div>

        {/*
          Not decoration. The device ID is the value you need when checking
          cross-signing state or matching this session in a homeserver's device
          list, and that is exactly what the next milestone gets debugged with.
        */}
        <dl className="shell__facts">
          <div>
            <dt>User ID</dt>
            <dd data-selectable>{profile.user_id}</dd>
          </div>
          <div>
            <dt>Device</dt>
            <dd data-selectable>{profile.device_id}</dd>
          </div>
          <div>
            <dt>Homeserver</dt>
            <dd data-selectable>{profile.homeserver}</dd>
          </div>
        </dl>
      </main>
    </div>
  );
}
