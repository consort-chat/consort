import { useState } from "react";

import {
  HOME_ID,
  type Channel,
  type Connection,
  type KeyBackup,
  type Profile,
  type Rooms,
  type TokenStorage,
  type Verification,
  type VerificationFlow,
} from "../lib/api";
import { ChannelList, channelLabel } from "./ChannelList";
import { SpaceRail } from "./SpaceRail";
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

/** What the main pane says about whatever is selected. */
function paneDetail(channel: Channel | null): string {
  if (channel === null) return "Messages come after voice.";
  return channel.kind === "voice"
    ? "Joining a voice channel comes next."
    : "Messages come after voice.";
}

/** The heading of the main pane: the channel, or the honest absence of one. */
function paneHeadline(channel: Channel | null): string {
  if (channel === null) return "Nothing here yet";
  // The hash is the text channel's, and only the text channel's. It is how
  // every client anybody already uses says which of the two this is.
  return channel.kind === "voice"
    ? channelLabel(channel)
    : `#${channelLabel(channel)}`;
}

interface Props {
  profile: Profile;
  rooms: Rooms;
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
  rooms,
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
  const [spaceId, setSpaceId] = useState(HOME_ID);
  const [channelId, setChannelId] = useState<string | null>(null);

  /*
    Both selections are derived rather than trusted. A space can be left and a
    channel can be removed from one while either is selected, and the room list
    that says so arrives as a whole new tree. Looking the selection up in that
    tree every render means a selection that no longer exists simply stops
    being one, instead of leaving the shell pointing at a room that is gone.

    Falling back to the first entry rather than to `HOME_ID` because Home is
    always the first entry, and this way there is one fact about the order
    rather than two.
  */
  const space =
    rooms.spaces.find((candidate) => candidate.id === spaceId) ??
    rooms.spaces[0] ??
    null;
  const channel =
    space?.channels.find((candidate) => candidate.id === channelId) ?? null;

  function selectSpace(id: string) {
    setSpaceId(id);
    // A channel belongs to the space it was picked in. Carrying the selection
    // across would leave a channel highlighted in a list it is not in.
    setChannelId(null);
  }

  return (
    <div className="shell">
      <SpaceRail
        spaces={rooms.spaces}
        selectedId={space?.id ?? HOME_ID}
        onSelect={selectSpace}
      />

      <div className="shell__sidebar">
        <div className="shell__channels">
          {space !== null && (
            <ChannelList
              space={space}
              selectedId={channel?.id ?? null}
              onSelect={setChannelId}
            />
          )}
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
            It names the selected channel, and says there is nothing here when
            nothing is selected, which is the state the app opens in.
          */}
          <h1 className="shell__empty-headline">{paneHeadline(channel)}</h1>
          <p className="shell__empty-detail">{paneDetail(channel)}</p>
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
