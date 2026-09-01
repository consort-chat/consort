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
 * Whether an encrypted call joined right now would be audible, mirrored from
 * `consort_matrix::calls::CallReadiness`.
 *
 * Not a rewording of `Verification`, and the difference is the whole reason
 * this exists. Media keys travel to-device under the SDK's identity-based
 * sharing strategy, which refuses in two distinct shapes, and each sends a
 * person somewhere different. `noIdentity` means the *account* has no
 * cross-signing identity, fixed by setting up recovery on any client.
 * `sessionUnverified` means the account has one and this device is not trusted
 * against it, fixed here. Collapsing them tells somebody who has already done
 * the first thing to go and do it again.
 *
 * There is no "not known yet". `Verification` needs one because it republishes
 * a state it watches; this is a question with an answer, and the watcher does
 * not report until it has one.
 */
export type CallReadiness =
  | { state: "ready" }
  | { state: "noIdentity" }
  | { state: "sessionUnverified" };

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
  /**
   * Whether they have muted themselves.
   *
   * Only ever true for somebody in the call this session is also in. It comes
   * from the media layer, which learns it from the SFU. Room state carries
   * nothing like it, so a person listed from a channel this session is not
   * sitting in is reported unmuted because nothing there could say otherwise,
   * not because anything checked.
   *
   * Optional on the wire and defaulted here, so a payload written before the
   * field existed still parses.
   */
  muted?: boolean;
  /**
   * Whether they have stopped listening to the call.
   *
   * Only ever true of another Consort client. Deafening is built out of one
   * session's own subscriptions, so nothing in MatrixRTC or LiveKit reports
   * it and Consort clients tell each other over the call's data channel.
   * Somebody in Element Call is reported as merely muted, which is what
   * deafening looks like from outside.
   *
   * Implies `muted`, because deafening mutes. Kept separate so the interface
   * can say which of the two somebody chose.
   */
  deafened?: boolean;
  /**
   * Whether they have said they are not at their computer.
   *
   * Carried the same way `deafened` is, between Consort clients over the
   * call's data channel, and true under the same rule: every one of their
   * memberships said so.
   *
   * Implies the microphone is off and implies nothing about `deafened`.
   * Somebody away with their headphones on can still hear their name, which
   * is the entire difference between walking away and leaving.
   */
  away?: boolean;
  /**
   * Whether a camera of theirs is live.
   *
   * True only for somebody publishing a camera that is not muted. Somebody on
   * two devices is on camera if either of them is, which is the opposite fold
   * to `muted` and right for the reason behind it: both are chosen so the icon
   * never claims less exposure than there is.
   *
   * Known only for the call this session is sitting in. Room state carries
   * nothing like it, so a person listed from there is reported without a
   * camera because nothing checked, not because anything found one off, and
   * the interface draws no camera at all for them rather than a confident
   * cross.
   */
  camera?: boolean;
  /**
   * When they joined the call, in milliseconds since the Unix epoch.
   *
   * The SFU's own record rather than the moment this session noticed them, so
   * it is still right for people who were already in the call when we arrived.
   * Absent for anybody listed from room state, for anybody whose media has not
   * appeared yet, and against a server too old to report it.
   *
   * Somebody on two devices joined when the first of them did.
   */
  since?: number;
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
 * Listen for changes in whether a call from this session could be heard.
 *
 * Same contract as `onVerification`: the channel name matches
 * `AppEvent::CALL_READINESS`, and the returned function stops listening.
 *
 * Watched rather than asked once because the answer moves in the direction
 * that matters. Every session starts unable to distribute media keys, and
 * verifying it is precisely what changes that, so a value read at startup
 * would say "no" for the rest of a session in which somebody fixed it.
 */
export function onCallReadiness(
  handler: (state: CallReadiness) => void,
): Promise<UnlistenFn> {
  return listen<CallReadiness>("call-readiness", (event) =>
    handler(event.payload),
  );
}

/**
 * A voice channel that was clicked and not joined, mirrored from
 * `crate::events::CallRefused`.
 *
 * Deliberately not on the `call` channel. That one carries what this session
 * is currently doing, and somebody sitting in one voice channel who clicks a
 * second one and is refused is still sitting in the first.
 */
export interface CallRefused {
  roomId: string;
  readiness: CallReadiness;
}

/**
 * Listen for a join that was refused before it was attempted.
 *
 * An incident rather than state, so `resendState` never repeats it: a
 * complaint about a click made twenty minutes ago is not news to somebody who
 * has since verified. The standing answer lives on `onCallReadiness`.
 */
export function onCallRefused(
  handler: (refusal: CallRefused) => void,
): Promise<UnlistenFn> {
  return listen<CallRefused>("call-refused", (event) => handler(event.payload));
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
 * Where somebody's own client says they are.
 *
 * `"unknown"` is a real answer rather than a missing one. Most homeservers
 * have presence switched off, because it is the most expensive thing in the
 * protocol, and reading that silence as `"offline"` would put a grey dot on
 * somebody sitting right there.
 */
export type Presence = "online" | "idle" | "offline" | "unknown";

/** What somebody is allowed to do in a room, at the granularity a person cares about. */
export type Standing = "admin" | "moderator" | "member";

/**
 * What can be said about one person in one room beyond their name.
 *
 * Nothing here duplicates the roster: who they are, what they are called,
 * whether they are muted and when they joined the call all arrive with the
 * channel and are on screen before this is asked for.
 */
export interface MemberProfile {
  presence: Presence;
  /** Their own status line, when they set one. */
  status: string | null;
  /**
   * Milliseconds since they last did anything, as the homeserver counts it.
   *
   * Null whenever presence is unknown, and often null even when it is not: the
   * field is optional in the spec and Synapse omits it for people it has not
   * heard from.
   */
  lastActiveAgo: number | null;
  standing: Standing;
}

/**
 * What can be said about one person in one room beyond their name.
 *
 * One request to the homeserver, made when a person's card opens and never on
 * the way to drawing a roster. It does not fail: every part of it degrades to
 * "nothing known" on its own, because none of these facts is worth a dialog in
 * front of somebody who clicked a name out of curiosity.
 */
export function memberProfile(
  roomId: string,
  userId: string,
): Promise<MemberProfile> {
  return invoke<MemberProfile>("member_profile", { roomId, userId });
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

/**
 * One device the host offers, mirrored from `consort_audio::devices`.
 *
 * The name is the whole identity. cpal 0.18 removed the fallible name lookup
 * and offers only a display string, so two identical capture cards are
 * indistinguishable and a saved choice can resolve to the wrong twin. Known,
 * accepted, and the reason `isDefault` is worth carrying separately.
 */
export interface AudioDevice {
  name: string;
  /** Whether the host reports this as the one it would pick. */
  isDefault: boolean;
}

/**
 * What there is in one direction, and what is actually being used.
 *
 * Three facts rather than a list and an index, because the third is the one
 * that is easy to leave out and expensive to have left out.
 */
export interface AudioDeviceList {
  /** Everything worth offering, in host order. Do not re-sort. */
  devices: AudioDevice[];
  /** The device audio will go through. Null only on a machine with none. */
  selected: string | null;
  /**
   * The saved device, when it is not plugged in any more.
   *
   * Null in every other case. Say so when it is not: somebody who chose a
   * headset and is being recorded by a laptop lid microphone should be told,
   * not quietly switched.
   */
  missing: string | null;
}

export interface AudioDeviceReport {
  input: AudioDeviceList;
  output: AudioDeviceList;
}

/**
 * The voice gate's thresholds, mirrored from `consort_audio::gate`.
 *
 * Two thresholds rather than one on purpose. A single threshold chatters: a
 * voice sitting near it opens and shuts the gate every few frames, which
 * clips the start of every other word.
 */
export interface GateConfig {
  /** Speech probability at which the gate opens. */
  openAt: number;
  /** Speech probability below which it may close again. */
  closeAt: number;
  /** Consecutive frames above `openAt` before it opens. */
  attackFrames: number;
  /** How long it stays open after the last frame above `closeAt`. */
  holdMs: number;
  /** Whether the noise suppressor runs. */
  denoise: boolean;
  /**
   * Whether to send only while somebody is talking.
   *
   * Off publishes every frame and makes the thresholds above inert. The model
   * still runs either way, so the level meter reads the same: this is a choice
   * about what gets sent, not about what gets computed.
   */
  voiceActivity: boolean;
}

/**
 * What has been chosen, mirrored from `consort_audio::settings`.
 *
 * Both device names are null until somebody picks one, and null means "use
 * whatever this machine calls its default". That is not the same as writing
 * down today's default by name: the host's answer follows the machine, so
 * plugging in a headset moves the microphone, which is what somebody who
 * never opened this screen expects.
 */
export interface AudioSettings {
  input: string | null;
  output: string | null;
  gate: GateConfig;
  /**
   * Whether a call makes a sound when somebody joins or leaves it.
   *
   * Optional here so a payload written before the field existed still parses,
   * and read as `=== true` wherever it is drawn, because the Rust default is
   * **off**: the chime and the sentence below announce the same arrival, and
   * the chime existed to get somebody's attention for a notification that used
   * to be nothing but the chime.
   */
  callSounds?: boolean;
  /**
   * Whether a call says out loud what `callSounds` only announces.
   *
   * Optional too, but read as `!== false`, because this one defaults on: it is
   * the notification, and the chime in front of it is the optional half. The
   * two are separate in both directions, so a chime with no sentence and a
   * sentence with no chime are both states somebody can ask for.
   */
  callVoices?: boolean;
  /**
   * How loud a call should be, 0 to 100.
   *
   * The master, covering everybody in the call and the notifications above
   * them. Optional and read as `?? 100`, which is the Rust default.
   */
  outputVolume?: number;
  /**
   * How loud the chimes and spoken notifications should be, as a percentage of
   * `outputVolume`.
   *
   * Underneath the master rather than beside it, so turning a call down turns
   * these down with it. Optional and read as `?? 60`: a notification is
   * mastered to be heard on its own and a call is somebody talking three feet
   * from a microphone, so at one level the notification is the loud thing in
   * the room.
   */
  notificationVolume?: number;
  /**
   * How loud one particular person should be, by Matrix user ID.
   *
   * Read-only from here. It is written by `setPersonVolume`, from the menu
   * beside somebody's name in a call, and deliberately not sent back by
   * `setAudioSettings`: this screen does not draw these, and a screen that
   * wrote back what it never read would erase every one of them.
   *
   * Absent means full volume, so the map holds only the people somebody has
   * actually adjusted.
   */
  personVolumes?: Record<string, number>;
}

/**
 * What the microphone test is doing, mirrored from
 * `consort_audio::thread::AudioEvent`.
 *
 * Unlike every other channel here this one carries no state worth replaying.
 * A level is a measurement of a moment, and the Rust side deliberately never
 * repeats the last one to a late subscriber: a bar moving for a microphone
 * that stopped minutes ago is worse than no bar at all.
 *
 * `failed` is an ordinary outcome rather than an exception. A device gets held
 * by another application, or goes away between the list being drawn and the
 * button being pressed, and both are common enough to draw rather than throw.
 *
 * Three devices, one channel. The `tone` states are the output test, the
 * `callAudio` ones are the call's own output, and the rest are the microphone.
 * They are told apart by name rather than shared: a `switch` that treated
 * `toneStarted` as `started` would put the level meter into "running" because
 * somebody pressed the speaker button.
 *
 * `callAudioFailed` is the one worth drawing prominently. Speakers that will
 * not open look exactly like a call nobody is speaking in, and without it
 * somebody spends an evening blaming their microphone.
 */
export type AudioActivity =
  | { state: "started"; device: string }
  | { state: "stopped" }
  | { state: "failed"; error: string }
  | { state: "level"; level: number; probability: number; open: boolean }
  | { state: "toneStarted"; device: string }
  | { state: "toneStopped" }
  | { state: "toneFailed"; error: string }
  | { state: "callAudioStarted"; device: string }
  | { state: "callAudioStopped" }
  | { state: "callAudioFailed"; error: string };

/**
 * What this machine has, and which of it is in use.
 *
 * Ask on every open rather than caching. A device can appear or vanish while
 * the window is up, and a picker drawn from a stale list offers things that
 * are not there.
 */
export function audioDevices(): Promise<AudioDeviceReport> {
  return invoke<AudioDeviceReport>("audio_devices");
}

/** The saved audio choices, or the defaults on first run. */
export function audioSettings(): Promise<AudioSettings> {
  return invoke<AudioSettings>("audio_settings");
}

/**
 * Replace the saved audio choices.
 *
 * Takes the whole object rather than one field. The screen holds all of it,
 * and a partial update would need a merge on the Rust side that could lose a
 * concurrent change for no benefit.
 */
export function setAudioSettings(audio: AudioSettings): Promise<void> {
  return invoke<void>("set_audio_settings", { audio });
}

/**
 * Set how loud one person should be, as a percentage, and remember it.
 *
 * Its own call rather than part of `setAudioSettings`, because it is set from
 * somewhere else entirely: a menu beside a name in a call, which knows one
 * user ID and nothing about devices or gates.
 *
 * Kept per machine, which is the only place it can be kept. Nothing in Matrix
 * carries "that one is too loud in my headphones", and it should not: it is a
 * fact about the room somebody is sitting in rather than about their account.
 * It does survive leaving the call, rejoining, and restarting.
 *
 * `100` removes the entry rather than storing it, because full volume is the
 * absence of a choice. Exactly `100`, not anything at or above it: the range
 * runs to 250, and above full is a boost for somebody who arrives too quiet to
 * be brought up any other way.
 */
export function setPersonVolume(
  userId: string,
  percent: number,
): Promise<void> {
  return invoke<void>("set_person_volume", { userId, percent });
}

/**
 * Open the microphone and start reporting levels.
 *
 * No arguments: the Rust side reads the saved input choice and resolves it
 * against what is plugged in, so a device that has gone falls back to the
 * default rather than refusing. Everything that happens next, including a
 * failure to open, arrives through `onAudio`.
 */
export function audioTestStart(): Promise<void> {
  return invoke<void>("audio_test_start");
}

/** Close the microphone. Safe to call when nothing is running. */
export function audioTestStop(): Promise<void> {
  return invoke<void>("audio_test_stop");
}

/**
 * Play the test chime out of the chosen output.
 *
 * The output picker's only feedback. A microphone can be checked by talking at
 * it and watching the meter move; speakers cannot be checked by anything at
 * all unless something plays.
 *
 * No arguments, for the same reason as `audioTestStart`: the Rust side reads
 * the saved output and resolves it against what is plugged in. What happens
 * next arrives through `onAudio` as `toneStarted`, then `toneStopped` once the
 * chime is over, which takes about a third of a second.
 */
export function audioTonePlay(): Promise<void> {
  return invoke<void>("audio_tone_play");
}

/** Cut the chime short. Safe to call when nothing is playing. */
export function audioToneStop(): Promise<void> {
  return invoke<void>("audio_tone_stop");
}

/**
 * Listen to the microphone test.
 *
 * Same contract as `onConnection`: the channel name matches `AppEvent::AUDIO`,
 * and the returned function stops listening. Unlike those, `resendState` will
 * not repeat anything here, so a component that mounts mid-test hears nothing
 * until the next reading, which is 50 ms away.
 */
export function onAudio(
  handler: (activity: AudioActivity) => void,
): Promise<UnlistenFn> {
  return listen<AudioActivity>("audio", (event) => handler(event.payload));
}

/**
 * Whether this session is in a voice channel, mirrored from
 * `consort_call::CallEvent`.
 *
 * Not to be confused with `Connection`, which is the sync loop. Both have a
 * state called something like "connected" and they are about entirely
 * different things: one is whether Matrix is working at all, the other is
 * whether you are sitting in a voice channel.
 *
 * `connecting` and `failed` carry the room they are about because a second
 * channel can be clicked while the first is still connecting. Without the
 * room, the interface cannot tell whether the message it just received is
 * about the channel it is currently showing.
 *
 * `disconnected` is the state the app opens in and the one every ending
 * arrives at. A channel change never passes through it: the Rust side reports
 * `connecting` for the new channel and leaves the old one behind it, so the
 * panel does not blink out and back between two calls.
 */
export type Call =
  | { state: "connecting"; roomId: string }
  | {
      state: "connected";
      roomId: string;
      /**
       * Who is in the channel, from MatrixRTC signalling rather than room
       * state, oldest membership first.
       *
       * Re-sent, as a whole `connected`, every time somebody joins or leaves.
       * That is deliberate on the Rust side: being in a call and who is in it
       * are one state, so a handler that keeps the latest thing said here has
       * both, and one that missed a change has not also lost track of whether
       * it is in a call.
       *
       * Better than the `participants` on the `Channel` for exactly one
       * channel: this one. It is right in every MatrixRTC generation, where
       * the room-state list is only right in the oldest.
       */
      participants: Participant[];
      /**
       * Why this call cannot be heard, or null when there is nothing wrong.
       *
       * One sentence, already written for a person, so render it as-is. The
       * failure it exists for is the quiet one: every membership publishes,
       * both rosters are right, packets flow, and neither side can decrypt a
       * word. Nothing else on this screen would say so.
       */
      trouble: string | null;
    }
  | { state: "disconnected" }
  | { state: "failed"; roomId: string; error: string };

/** The room a call state is about, or null for the idle one. */
export function callRoomId(call: Call): string | null {
  return call.state === "disconnected" ? null : call.roomId;
}

/**
 * Join the voice channel in `roomId`, leaving whatever call is current.
 *
 * Resolves as soon as the request has been handed to the call thread, which is
 * long before the call exists. Everything about how it went, the failure
 * included, arrives through `onCall`: joining is a sequence of remote steps and
 * the interface needs "working on it" before it needs an answer.
 *
 * The one thing that rejects is asking while signed out, which is not a call
 * that failed but a caller asking at a moment when nothing can be answered.
 */
export function callConnect(roomId: string): Promise<void> {
  return invoke<void>("call_connect", { roomId });
}

/**
 * Leave the voice channel.
 *
 * Safe to call when there is no call and when there is no session. Both are
 * the same thing from here: a disconnect control that outlived what it
 * belonged to.
 */
export function callDisconnect(): Promise<void> {
  return invoke<void>("call_disconnect");
}

/**
 * Listen to this session's voice call.
 *
 * Same contract as `onConnection`: the channel name matches `AppEvent::CALL`,
 * and the returned function stops listening. `resendState` repeats the current
 * one, so a webview that reloaded mid-call finds out it is in a channel.
 */
export function onCall(handler: (call: Call) => void): Promise<UnlistenFn> {
  return listen<Call>("call", (event) => handler(event.payload));
}

/**
 * What this session is doing with its own audio, mirrored from
 * `consort_call::SelfAudio`.
 *
 * Two flags rather than one tri-state, because they are two buttons and either
 * can be pressed while the other is down. `deafened` implies the microphone is
 * off, but it does not imply `muted`: that flag is what somebody pressed, and
 * it is what decides whether undeafening hands the microphone back.
 */
export interface SelfAudio {
  /** Whether the microphone is off because somebody said so. */
  muted: boolean;
  /** Whether this session has stopped receiving everybody else's audio. */
  deafened: boolean;
  /**
   * Whether this session has said nobody is at the computer.
   *
   * Mutes and does not deafen. Optional on the wire so a payload written
   * before the field existed still parses.
   */
  away?: boolean;
}

/** None of the three, which is where every session starts and Rust too. */
export const HEARING: SelfAudio = {
  muted: false,
  deafened: false,
  away: false,
};

/**
 * Whether the microphone is off, for any of the three reasons.
 *
 * Mirrors `SelfAudio::microphone_off` in Rust, and must keep mirroring it: the
 * button draws itself from this, and a disagreement is a microphone icon that
 * says the opposite of what the call is doing.
 */
export function microphoneOff(audio: SelfAudio): boolean {
  return audio.muted || audio.deafened || audio.away === true;
}

/**
 * Listen to whether this session is muted or deafened.
 *
 * A channel of its own rather than a fifth call state, because only the last
 * event per channel is replayed to a webview that reloaded: a mute sent as a
 * call state would evict the call it was pressed during.
 */
export function onSelfAudio(
  handler: (audio: SelfAudio) => void,
): Promise<UnlistenFn> {
  return listen<SelfAudio>("self-audio", (event) => handler(event.payload));
}

/**
 * Nobody is talking, which is where every session starts.
 *
 * A single frozen instance rather than a fresh `new Set()` per default, so a
 * component that falls back to it does not hand React a different object on
 * every render and redraw a list for it.
 */
export const NOBODY: ReadonlySet<string> = new Set();

/**
 * Listen to who in the current call is talking, by Matrix user ID.
 *
 * Its own channel and never replayed, unlike the mute above. This arrives
 * several times a second, and it describes a moment: replaying the last one to
 * a webview that reloaded would leave a ring drawn around somebody who stopped
 * talking before the reload, with nothing to take it off again.
 *
 * The empty list is a real answer and arrives whenever the last person stops.
 *
 * The SFU decides who counts as talking, from the audio it is already
 * receiving. That is one answer for everybody in the call, arrived at the same
 * way, rather than each client guessing from what it happens to be able to
 * measure.
 */
export function onSpeaking(
  handler: (userIds: string[]) => void,
): Promise<UnlistenFn> {
  return listen<string[]>("speaking", (event) => handler(event.payload));
}

/**
 * Mute or unmute this session's microphone.
 *
 * Nothing comes back but the acknowledgement. What the state now is arrives
 * through `onSelfAudio`, so that what is drawn is what the call thread did
 * rather than what was asked of it.
 */
export function callSetMuted(muted: boolean): Promise<void> {
  return invoke<void>("call_set_muted", { muted });
}

/**
 * Stop or resume receiving the audio of everybody else in the call.
 *
 * Mutes on the way, which arrives through `onSelfAudio` as part of the same
 * change. Undeafening does not unmute somebody who had already muted.
 */
export function callSetDeafened(deafened: boolean): Promise<void> {
  return invoke<void>("call_set_deafened", { deafened });
}

/**
 * Say that nobody is at this computer, or that somebody is again.
 *
 * Mutes and deliberately does not deafen. Same contract as the two above: what
 * comes back is the `self-audio` channel saying what the state now is, so this
 * resolves as soon as the request is queued rather than when it has landed.
 */
export function callSetAway(away: boolean): Promise<void> {
  return invoke<void>("call_set_away", { away });
}
