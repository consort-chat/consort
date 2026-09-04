import { useCallback, useEffect, useMemo, useState } from "react";

import {
  HOME_ID,
  NOBODY,
  asCommandError,
  callRoomId,
  roomAt,
  type Call,
  type CallRefused,
  type Channel,
  type Connection,
  type KeyBackup,
  type Profile,
  type Rooms,
  type SelfAudio,
  type TokenStorage,
  type Verification,
  type VerificationFlow,
} from "../lib/api";
import { channelLabel } from "../lib/labels";
import type { PlaceTarget } from "../lib/matrixTo";
import { RoomLinksContext, type RoomLinks } from "../lib/roomLinks";
import { CallPanel } from "./CallPanel";
import { CallRefusedNotice } from "./CallRefusedNotice";
import { ChannelList } from "./ChannelList";
import { RoomTimeline } from "./RoomTimeline";
import { SettingsModal } from "./SettingsModal";
import { SidebarToggle } from "./SidebarToggle";
import { SpaceRail } from "./SpaceRail";
import {
  ThreadPanel,
  clampThreadWidth,
  defaultThreadWidth,
} from "./ThreadPanel";
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

/** What the main pane says when no channel is selected. */
function paneDetail(): string {
  return "Pick a channel to read it. Clicking a voice one joins it as well.";
}

/**
 * What the channel a call is about is named.
 *
 * Searched across every space rather than the selected one. A voice channel
 * stays joined while somebody browses elsewhere, which is the whole point of a
 * panel that is always on screen, so the channel it names is regularly not in
 * the list beside it.
 *
 * Null when nothing local knows, which is a room the account has not joined.
 * The panel draws a placeholder; it never draws a room ID.
 */
function nameOfCalledChannel(rooms: Rooms, call: Call): string | null {
  const roomId = callRoomId(call);
  if (roomId === null) return null;
  return nameOfChannel(rooms, roomId);
}

/// What a room is called, across every space, or null when nothing knows.
function nameOfChannel(rooms: Rooms, roomId: string): string | null {
  for (const space of rooms.spaces) {
    const found = space.channels.find((channel) => channel.id === roomId);
    if (found) return channelLabel(found);
  }
  return null;
}

/**
 * What to write on a link into Matrix, the way the shell writes a channel.
 *
 * The hash included, because that is how a text channel is named everywhere
 * else here and a badge saying `general` beside a heading saying `#general`
 * reads as two different rooms.
 *
 * Null for an alias, always. An alias is a name a homeserver holds and only a
 * homeserver can say which room it currently is, which is a question the badge
 * does not ask: it asks one when it is pressed.
 */
function nameOfLinkedRoom(rooms: Rooms, roomOrAlias: string): string | null {
  for (const space of rooms.spaces) {
    const found = space.channels.find((channel) => channel.id === roomOrAlias);
    if (found === undefined) continue;
    return found.kind === "voice"
      ? channelLabel(found)
      : `#${channelLabel(found)}`;
  }
  return null;
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
  call: Call;
  /** Whether this session has muted or deafened itself. */
  selfAudio: SelfAudio;
  /**
   * Who in the current call is talking, by Matrix user ID.
   *
   * Passed down rather than subscribed to here, so that one subscription
   * serves the whole screen: this changes several times a second, and a
   * listener per component would be that many re-renders of everything.
   *
   * Optional, and nobody by default. A screen with no call to describe should
   * not have to invent an empty set to say so.
   */
  speaking?: ReadonlySet<string>;
  /** Why this session cannot play the call, if it cannot. */
  audioProblem?: string | null;
  verification: Verification;
  keyBackup: KeyBackup;
  storage: TokenStorage | null;
  flows: VerificationFlow[];
  canStartVerification: boolean;
  onDismissFlow: (flowId: string) => void;
  onJoinVoice: (roomId: string) => void;
  onLeaveVoice: () => void;
  onSetMuted: (muted: boolean) => void;
  onSetDeafened: (deafened: boolean) => void;
  onSetAway: (away: boolean) => void;
  /**
   * A voice channel that was clicked and not joined, or null.
   *
   * Held by the caller rather than here, because it is dismissed rather than
   * superseded: a component that owned it would clear it on every re-render
   * caused by anything else in the shell.
   */
  callRefused: CallRefused | null;
  onDismissRefusal: () => void;
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
 * The main pane holds the selected room, and above it whatever has to be said
 * about the session itself: a verification flow in progress, whether this
 * session is verified, whether room keys survive it, and where the access
 * token ended up. None of that fits in a sixty pixel strip, which is why none
 * of it is in one, and none of it is drawn at all when there is nothing to
 * say: an empty box still takes the space around it.
 */
export function AppShell({
  profile,
  rooms,
  connection,
  call,
  selfAudio,
  speaking = NOBODY,
  audioProblem = null,
  verification,
  keyBackup,
  storage,
  flows,
  canStartVerification,
  onDismissFlow,
  onJoinVoice,
  onLeaveVoice,
  onSetMuted,
  onSetDeafened,
  onSetAway,
  callRefused,
  onDismissRefusal,
  onSignedOut,
}: Props) {
  const [spaceId, setSpaceId] = useState(HOME_ID);
  const [channelId, setChannelId] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  /*
    Whether the channel list is folded away. Here rather than in the list,
    because the control that brings it back has to be somewhere the folding did
    not hide, which is the pane.
  */
  const [folded, setFolded] = useState(false);
  /*
    How wide the thread panel is. Here rather than in the panel, because the
    panel draws nothing while none is open: a width it owned would go back to
    the default every time somebody shut a thread.
  */
  const [threadWidth, setThreadWidth] = useState(defaultThreadWidth);
  /*
    The message a link in a message asked to be shown, handed to the room that
    holds it. A fresh object per press, so following the same link twice lights
    the message up twice.
  */
  const [focus, setFocus] = useState<{ eventId: string } | null>(null);
  /* Why the last link somebody pressed went nowhere, if it did. */
  const [linkProblem, setLinkProblem] = useState<string | null>(null);

  // So a panel dragged wide on a large window is not wider than a small one.
  useEffect(() => {
    const settle = () => setThreadWidth(clampThreadWidth);
    window.addEventListener("resize", settle);
    return () => window.removeEventListener("resize", settle);
  }, []);

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

  /**
   * Open a channel, and join it if it is a voice one.
   *
   * Both, rather than either. Clicking a voice channel in every client anybody
   * already uses connects to it, and the selection still moves because the
   * main pane is the only thing that names what was clicked.
   *
   * Joining the channel already joined is not filtered out here. The call
   * thread answers it by re-announcing the call it is in, which is the right
   * answer to a click from an interface that may be asking precisely because
   * it has lost track of where it is.
   */
  /**
   * Show a room, wherever it lives.
   *
   * The rail entry is worked out from the room rather than passed in, because
   * a caller with a room ID has no reason to know which space it hangs under:
   * a direct message is under Home, a channel is under whichever space claims
   * it, and both arrive here the same way. A room in no rail entry at all is a
   * room this account has just left, and the selection is left where it is.
   */
  const openRoom = useCallback(
    (roomId: string): boolean => {
      const holder = rooms.spaces.find((candidate) =>
        candidate.channels.some((channel) => channel.id === roomId),
      );
      if (holder === undefined) return false;
      setSpaceId(holder.id);
      setChannelId(roomId);
      return true;
    },
    [rooms],
  );

  /**
   * Show whatever a `matrix.to` link in a message points at.
   *
   * The address is resolved in Rust, because an alias is a name a homeserver
   * holds and only a homeserver can say which room it currently is. What comes
   * back is a room this account is in, or a sentence saying why there is
   * nowhere to go.
   */
  const follow = useCallback(
    async (target: PlaceTarget) => {
      setLinkProblem(null);
      try {
        const roomId = await roomAt(target.roomOrAlias);
        if (!openRoom(roomId)) {
          // The account is in the room and the rail has not caught up, which
          // is a room joined a moment ago from somewhere else. Saying so beats
          // a control that appears to do nothing.
          setLinkProblem(
            "That room is not in the list yet. Try again in a moment.",
          );
          return;
        }
        setFocus(target.kind === "message" ? { eventId: target.eventId } : null);
      } catch (raw: unknown) {
        setLinkProblem(asCommandError(raw).message);
      }
    },
    [openRoom],
  );

  const links = useMemo<RoomLinks>(
    () => ({
      nameOf: (roomOrAlias) => nameOfLinkedRoom(rooms, roomOrAlias),
      open: (target) => void follow(target),
    }),
    [rooms, follow],
  );

  function selectChannel(id: string) {
    setChannelId(id);

    const chosen = space?.channels.find((candidate) => candidate.id === id);
    if (chosen?.kind === "voice") onJoinVoice(id);
  }

  /*
    Whether anything below has something to draw. Written out here rather than
    left to the four components, because each of them decides for itself to
    render nothing and a wrapper cannot see that: what it gets either way is a
    box with no children, which still takes its share of the gap.
  */
  const announcing =
    flows.length > 0 ||
    callRefused !== null ||
    linkProblem !== null ||
    verification.state !== "verified" ||
    keyBackup.state === "missing" ||
    (storage !== null && !storage.isPreferred);

  return (
    <RoomLinksContext.Provider value={links}>
      {/*
        Inert rather than merely covered. The dialog's own focus trap keeps
        Tab inside it; this is the other half, and it is what stops a pointer
        or a screen reader reaching a channel list that is still rendered
        behind a full-screen panel.
      */}
      <div
        className="shell"
        inert={settingsOpen}
        {...(folded ? { "data-sidebar": "folded" } : {})}
      >
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
              call={call}
              speaking={speaking}
              selfId={profile.user_id}
              onSelect={selectChannel}
              onOpenRoom={openRoom}
              onFold={() => setFolded(true)}
            />
          )}
        </div>
        {/*
          Between the channel list and the account strip, which is where every
          client that has one puts it: the list scrolls and these two do not,
          so the bottom of the column is the part that is always on screen.
        */}
        <CallPanel
          call={call}
          channelName={nameOfCalledChannel(rooms, call)}
          selfAudio={selfAudio}
          audioProblem={audioProblem}
          onDisconnect={onLeaveVoice}
          onSetMuted={onSetMuted}
          onSetDeafened={onSetDeafened}
          onSetAway={onSetAway}
        />
        <UserPanel
          profile={profile}
          connection={connection}
          onOpenSettings={() => setSettingsOpen(true)}
        />
      </div>

      <main className="shell__main">
        {/*
          Absent rather than empty. An empty box still takes the gap either
          side of it, which for a session with nothing wrong with it was a
          band of dead space above every room name.
        */}
        {announcing && (
        <div className="shell__alerts">
          {flows.map((flow) => (
            <VerificationFlowPanel
              key={flow.flowId}
              flow={flow}
              onDismiss={() => onDismissFlow(flow.flowId)}
            />
          ))}

          {/*
            Above the verification banner, and deliberately. This is the thing
            that just happened; the banner below it is the standing state and
            carries the way out of it, which is why this one does not repeat
            either route.
          */}
          {callRefused !== null && (
            <CallRefusedNotice
              refusal={callRefused}
              channelName={nameOfChannel(rooms, callRefused.roomId)}
              onDismiss={onDismissRefusal}
            />
          )}

          {/*
            Dismissed rather than timed out. A link that went nowhere is a
            thing somebody did and got no result from, and a sentence that
            disappears while it is being read is worse than no sentence.
          */}
          {linkProblem !== null && (
            <p className="shell__notice shell__notice--alert" role="alert">
              {linkProblem}
              <button
                type="button"
                className="shell__notice-dismiss"
                aria-label="Dismiss"
                onClick={() => setLinkProblem(null)}
              >
                &times;
              </button>
            </p>
          )}

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
        )}

        {/*
          The page's `h1` lives in whichever of these is drawn. It names the
          selected channel, and says there is nothing here when nothing is
          selected, which is the state the app opens in.

          Keyed by room, so switching channels remounts rather than reusing:
          the scroll position, the draft and the resolved names all belong to
          the room they were for, and carrying any of them across would put one
          room's half-typed sentence under another room's name.
        */}
        {channel === null ? (
          <div className="shell__empty">
            {/*
              Its own copy of the control, because this pane has no header to
              put one in. Folding the list before picking a channel would
              otherwise be a one-way door.
            */}
            {folded && (
              <div className="shell__unfold">
                <SidebarToggle folded onToggle={() => setFolded(false)} />
              </div>
            )}
            <h1 className="shell__empty-headline">{paneHeadline(channel)}</h1>
            <p className="shell__empty-detail">{paneDetail()}</p>
          </div>
        ) : (
          <RoomTimeline
            key={channel.id}
            channel={channel}
            selfId={profile.user_id}
            focus={focus}
            onOpenRoom={openRoom}
            {...(folded ? { onUnfold: () => setFolded(false) } : {})}
          />
        )}
      </main>

      {/*
        A column of its own rather than something over the room, and a sibling
        of the pane rather than a child of it: a thread is read alongside the
        conversation it came out of, and one drawn on top would cover the
        messages somebody opened it to compare against. It draws nothing at all
        while none is open, so the track it sits in costs nothing.
      */}
      <ThreadPanel
        selfId={profile.user_id}
        onOpenRoom={openRoom}
        width={threadWidth}
        onResize={setThreadWidth}
      />
      </div>

      {settingsOpen && (
        <SettingsModal
          profile={profile}
          onClose={() => setSettingsOpen(false)}
          onSignedOut={onSignedOut}
        />
      )}
    </RoomLinksContext.Provider>
  );
}
