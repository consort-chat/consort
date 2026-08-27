/**
 * Typed wrappers over the Rust commands.
 *
 * The types here are hand-mirrored from `app/src-tauri/src/commands.rs`. They
 * are the seam where a Rust change becomes a TypeScript compile error, so keep
 * them in step with that file rather than reaching for `any` at a call site.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface Profile {
  user_id: string;
  device_id: string;
  homeserver: string;
  display_name: string | null;
  avatar_url: string | null;
}

export type SessionStatus =
  | { status: "signedOut" }
  | { status: "signedIn"; profile: Profile };

/** Where the Rust side is keeping the access token on this machine. */
export interface TokenStorage {
  kind: "keyring" | "file" | "memory";
  /** One sentence, already written for a person. Safe to render as-is. */
  description: string;
  /** False when no system keyring was available and a file was used instead. */
  isPreferred: boolean;
}

/**
 * What the sync loop is doing, mirrored from `consort_matrix::sync`.
 *
 * `stopped` means it will not restart on its own, so it is the only state
 * where the interface should stop implying that a message might arrive.
 */
export type Connection =
  | { state: "connecting" }
  | { state: "live" }
  | { state: "offline"; attempt: number; retryInSeconds: number }
  | { state: "stopped"; reason: "signedOut" | "sessionEnded" | "failed" };

/**
 * Whether this session is verified, mirrored from
 * `consort_matrix::verification`.
 *
 * Three states rather than a boolean because the third one is real: the SDK
 * has not always worked out the answer yet, and rendering "not yet known" as
 * either answer is a lie. `unknown` is the state the interface starts in.
 */
export type Verification =
  | { state: "unknown" }
  | { state: "verified" }
  | { state: "unverified" };

/**
 * What is happening to this session's room keys, mirrored from
 * `consort_matrix::backup`.
 *
 * Five states rather than a boolean because "no backup is active here" is two
 * different pieces of news. `unusable` means one exists and this session
 * cannot read it, which verification fixes and which is the ordinary state of
 * a session nobody has verified yet. `missing` means there is nothing to read,
 * for anybody, ever. Only the second is worth interrupting somebody about.
 */
export type KeyBackup =
  | { state: "enabled" }
  | { state: "preparing" }
  | { state: "unusable" }
  | { state: "missing" }
  | { state: "unknown" };

/**
 * Which column a channel belongs in, mirrored from `consort_matrix::rooms`.
 *
 * Two values and no "unknown". A room that does not announce itself as a call
 * is a text room, which is what the spec implies by having no room type at all
 * for the ordinary case.
 */
export type ChannelKind = "text" | "voice";

/**
 * One person connected to a voice channel.
 *
 * Per human, not per device: somebody on a laptop and a phone appears once.
 * Rust does that deduplication, because it is the side that knows a membership
 * is per device in the first place.
 */
export interface Participant {
  /** A user id. Half of the key `memberAvatar` takes; the room is the other. */
  id: string;
  /**
   * What to call them. Never null, unlike a channel name: when there is no
   * display name this is the user id, which is still a name a person knows.
   */
  name: string;
}

/** One room under a rail entry. */
export interface Channel {
  id: string;
  /**
   * Null only for a room a space lists that this account has never joined, so
   * nothing local knows what it is called. Render a placeholder, never the id:
   * `!AbCdEf...` is not a channel name.
   */
  name: string | null;
  kind: ChannelKind;
  /** An `mxc://` URI. Pass it nowhere; call `roomAvatar(id)` for the image. */
  avatar: string | null;
  /** False for a listed room this account is not in. Show it, unavailable. */
  joined: boolean;
  /**
   * Who is connected right now, oldest membership first. Draw in this order:
   * it is stable across renders, and re-sorting would make the list move under
   * the pointer whenever anybody joined.
   *
   * Empty for a text channel, for a voice channel nobody is in, and for a room
   * this account has not joined, whose call state it cannot see at all.
   */
  participants: Participant[];
}

/** One entry in the left rail, and the channels underneath it. */
export interface Space {
  /** A room id, or the literal `"home"`. Every real room id starts with `!`. */
  id: string;
  name: string;
  avatar: string | null;
  /**
   * Already sorted, following MSC1772: by `order` where a space set one, then
   * by when the room was added, then by id. Do not re-sort. Filtering by
   * `kind` to draw the two groups preserves it; sorting each group separately
   * does not.
   */
  channels: Channel[];
}

/**
 * Everything the shell draws, mirrored from `consort_matrix::rooms`.
 *
 * The whole tree arrives every time any part of it changes, so replace the
 * previous value rather than merging into it. `spaces[0]` is always Home, the
 * entry holding rooms that belong to no joined space, and it is present even
 * when it is empty.
 */
export interface Rooms {
  spaces: Space[];
}

/** The id of the Home rail entry. Cannot collide: room ids begin with `!`. */
export const HOME_ID = "home";

/** One of the seven pictures both devices compare. */
export interface EmojiPair {
  symbol: string;
  /** The English word from the spec's table, for reading down a phone line. */
  description: string;
}

/**
 * Why a verification flow ended without verifying anything.
 *
 * `mismatch` is the one that matters and the only one to look alarmed about:
 * it is what the whole comparison exists to detect. `acceptedElsewhere` is the
 * opposite and is easy to get wrong. A request goes to every device on the
 * account, so every device that did not answer is told the flow was accepted,
 * and rendering that as a failure reports a problem to somebody whose
 * verification is going fine on their phone.
 */
export type CancelReason =
  | "declined"
  | "mismatch"
  | "timedOut"
  | "acceptedElsewhere"
  | "other";

/**
 * Where a verification flow has got to, mirrored from
 * `consort_matrix::verification::dto`.
 *
 * `ready` and `waiting` are not the same. `ready` means both sides agreed and
 * nobody has started the comparison, which is where the "show me the emoji"
 * button belongs. `waiting` means it has started and the keys are in flight,
 * which is a spinner. Merging them puts a button on a screen where pressing it
 * does nothing.
 */
export type VerificationFlowState =
  | { kind: "requested" }
  | { kind: "ready" }
  | { kind: "waiting" }
  | { kind: "comparing"; emoji: EmojiPair[]; decimals: [number, number, number] }
  | { kind: "confirmed" }
  | { kind: "done" }
  | {
      kind: "cancelled";
      reason: CancelReason;
      /** Developer English from the SDK. For the console, never the screen. */
      detail: string;
      byUs: boolean;
    };

/** One verification flow, addressed by the pair every action takes. */
export interface VerificationFlow {
  flowId: string;
  otherUserId: string;
  isSelfVerification: boolean;
  /**
   * Whether this session asked, rather than being asked.
   *
   * Decides both the sentence and the buttons. Once a request turns into an
   * emoji comparison the two directions look identical, so this is the only
   * thing left to tell them apart.
   */
  weStarted: boolean;
  state: VerificationFlowState;
}

/** The `CommandError` shape every command rejects with. */
export interface CommandError {
  /** Written for a person. Safe to render. */
  message: string;
  /** Underlying error text. For the console, not the interface. */
  detail: string;
}

/**
 * Tauri rejects with whatever the command returned, so a rejection is a
 * `CommandError` and not an `Error`. Narrow before touching `.message`, since
 * a genuine JS exception can also land here.
 *
 * The `instanceof Error` exclusion is not redundant. An `Error` also has a
 * string `message`, so a plain duck-type check accepts one and the UI ends up
 * rendering "Cannot read properties of undefined" at the user as though the
 * homeserver had said it. A JS exception is a bug in this code, and what a
 * person should see for that is the generic sentence, with the real text left
 * in the console where it is useful.
 *
 * The emptiness check is the same idea. An error object whose message is `""`
 * satisfies every type check and renders as blank space.
 */
export function asCommandError(error: unknown): CommandError {
  if (
    typeof error === "object" &&
    error !== null &&
    !(error instanceof Error) &&
    "message" in error &&
    typeof (error as CommandError).message === "string" &&
    (error as CommandError).message !== ""
  ) {
    return error as CommandError;
  }
  return {
    message: "Something went wrong.",
    detail: error instanceof Error ? error.message : String(error),
  };
}

export function sessionStatus(): Promise<SessionStatus> {
  return invoke<SessionStatus>("session_status");
}

export function login(
  server: string,
  username: string,
  password: string,
): Promise<Profile> {
  return invoke<Profile>("login", { server, username, password });
}

export function logout(): Promise<void> {
  return invoke<void>("logout");
}

export function tokenStorage(): Promise<TokenStorage> {
  return invoke<TokenStorage>("token_storage");
}

/**
 * Listen for changes in the sync loop's health.
 *
 * Resolves to the function that stops listening. Call it from an effect's
 * cleanup: a listener that outlives its component keeps handling events
 * against unmounted state, and after a sign out and a sign in every event
 * arrives once per leaked listener.
 *
 * The channel name matches `AppEvent::CONNECTION` in
 * `app/src-tauri/src/events.rs`. Tauri does not object to a listener for a
 * channel nothing emits on, so a mismatch here is silence rather than an
 * error.
 */
export function onConnection(
  handler: (state: Connection) => void,
): Promise<UnlistenFn> {
  return listen<Connection>("connection", (event) => handler(event.payload));
}

/**
 * Listen for changes in whether this session is verified.
 *
 * Same contract as `onConnection`: the channel name matches
 * `AppEvent::VERIFICATION`, and the returned function stops listening.
 */
export function onVerification(
  handler: (state: Verification) => void,
): Promise<UnlistenFn> {
  return listen<Verification>("verification", (event) =>
    handler(event.payload),
  );
}

/**
 * Listen for verification flows starting, moving on, and ending.
 *
 * Unlike the two channels above this one carries incidents rather than state,
 * so `resendState` repeats it only while a flow is still running. A flow that
 * has finished is history, and replaying it would put "the emoji did not
 * match" back on screen for something that ended twenty minutes ago.
 */
export function onVerificationFlow(
  handler: (flow: VerificationFlow) => void,
): Promise<UnlistenFn> {
  return listen<VerificationFlow>("verification-flow", (event) =>
    handler(event.payload),
  );
}

/**
 * Listen for changes in whether room keys are being backed up.
 *
 * Same contract as `onConnection`: the channel name matches
 * `AppEvent::KEY_BACKUP`, and the returned function stops listening.
 *
 * A separate channel from `onVerification` because the two can disagree. A
 * verified session with no backup reads every new message and no old one, and
 * folding that into "verified" would bury it.
 */
export function onKeyBackup(
  handler: (state: KeyBackup) => void,
): Promise<UnlistenFn> {
  return listen<KeyBackup>("key-backup", (event) => handler(event.payload));
}

/**
 * Listen for changes to the rooms this account is in.
 *
 * Same contract as `onConnection`: the channel name matches `AppEvent::ROOMS`,
 * and the returned function stops listening.
 *
 * The payload is the whole tree, not a change to it, so assign it rather than
 * patching. That is deliberate on the Rust side: a room list that maintains
 * its own copy from a stream of edits is a room list that drifts out of step
 * with the account.
 */
export function onRooms(handler: (rooms: Rooms) => void): Promise<UnlistenFn> {
  return listen<Rooms>("rooms", (event) => handler(event.payload));
}

/**
 * One room's avatar, as a `data:` URL ready for an `img` src.
 *
 * Resolves to null for a room with no avatar, for Home, and for an image the
 * homeserver would not hand over. All three mean the same thing to a caller:
 * draw initials.
 *
 * Asked for one room at a time rather than carried in `Rooms`, because that
 * value is re-sent in full whenever anything about it changes and image bytes
 * would make that expensive. The Rust side caches on disk, so asking again
 * after a restart does not reach the homeserver, but caching in memory here is
 * still worth it to avoid an IPC round trip per render.
 */
export function roomAvatar(roomId: string): Promise<string | null> {
  return invoke<string | null>("room_avatar", { roomId });
}

/**
 * One person's avatar in one room, as a `data:` URL ready for an `img` src.
 *
 * Two identifiers rather than one because a Matrix profile is per room:
 * somebody can carry a different picture in every room they are in, and the
 * one to draw beside a voice channel is the one that room knows them by.
 *
 * Resolves to null for somebody with no avatar, somebody the room has never
 * mentioned, and an image the homeserver would not hand over. All three mean
 * the same thing to a caller: draw an initial.
 */
export function memberAvatar(
  roomId: string,
  userId: string,
): Promise<string | null> {
  return invoke<string | null>("member_avatar", { roomId, userId });
}

/**
 * The five things a person can do to a verification flow.
 *
 * All of them take the same pair of identifiers, straight off the event that
 * announced the flow. Nothing on this side holds a handle to anything: the
 * SDK's own registry is keyed by exactly this pair, and naming the flow every
 * time is what lets two verifications be in progress at once.
 *
 * Every one of them can reject, and the likeliest reason is not a bug: flows
 * expire after ten minutes, either side can cancel, and a request another of
 * your sessions answered is dropped. Handle the rejection; do not assume the
 * button is only pressed while the flow is alive.
 */
export function verificationAccept(
  userId: string,
  flowId: string,
): Promise<void> {
  return invoke<void>("verification_accept", { userId, flowId });
}

export function verificationStartSas(
  userId: string,
  flowId: string,
): Promise<void> {
  return invoke<void>("verification_start_sas", { userId, flowId });
}

export function verificationConfirm(
  userId: string,
  flowId: string,
): Promise<void> {
  return invoke<void>("verification_confirm", { userId, flowId });
}

export function verificationMismatch(
  userId: string,
  flowId: string,
): Promise<void> {
  return invoke<void>("verification_mismatch", { userId, flowId });
}

export function verificationCancel(
  userId: string,
  flowId: string,
): Promise<void> {
  return invoke<void>("verification_cancel", { userId, flowId });
}

/**
 * Ask this account's other sessions to verify this one.
 *
 * No arguments, deliberately. It is always this session asking and always the
 * account's own identity being asked, so there is nothing here to get wrong.
 * The flow it starts arrives on the `verification-flow` channel like any
 * other.
 */
export function verificationVerifyThisSession(): Promise<void> {
  return invoke<void>("verification_verify_this_session");
}

/**
 * Whether another of this account's sessions is signed in.
 *
 * Asked before the button is drawn. With nothing else signed in the request
 * can only time out, and offering it anyway spends ten minutes arriving at an
 * answer that was available up front.
 */
export function verificationOtherSessionsExist(): Promise<boolean> {
  return invoke<boolean>("verification_other_sessions_exist");
}

/**
 * Whether this account has a recovery key worth asking for.
 *
 * The other half of the same question, and it decides more of the screen. An
 * account with no secret storage has no key anybody could have written down,
 * and an input box for one sends somebody through a password manager looking
 * for something that was never created.
 */
export function verificationRecoveryExists(): Promise<boolean> {
  return invoke<boolean>("verification_recovery_exists");
}

/**
 * Verify this session with the account's recovery key.
 *
 * The one call here that carries a secret. Do not log it, do not put it in a
 * URL, and do not keep it after this resolves: the Rust side hands it to the
 * SDK, which opens secret storage with it and drops it.
 *
 * Rejecting is the ordinary case and the rejection is worth rendering. Four
 * different things go wrong here and they want four different answers, from
 * "that is not a recovery key" to "that key is fine and it is not this
 * account's". `asCommandError(...).message` already carries the right one.
 */
export function verificationRecover(recoveryKey: string): Promise<void> {
  return invoke<void>("verification_recover", { recoveryKey });
}

/**
 * Ask the Rust side to publish the current state of every channel again.
 *
 * Call it once, after the listeners are attached. Both channels above carry
 * state rather than incidents, and the tasks that publish them start with the
 * session, which on a restored session is before this webview has run any
 * JavaScript at all. Whatever they said in the meantime went to nobody, and
 * without asking for it again the interface sits on its initial guess until
 * something happens to change, which on a healthy session may be never.
 */
export function resendState(): Promise<void> {
  return invoke<void>("resend_state");
}
