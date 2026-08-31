/**
 * The words the interface puts on things.
 *
 * Here rather than beside the components that use them for two reasons. They
 * are shared: what a channel is called has to read the same in the list and in
 * the heading above it, and two copies is two chances to let a room ID through.
 * And a file that exports both a component and a plain function cannot be hot
 * reloaded by React Fast Refresh, which silently turns every edit into a full
 * page reload.
 */
import type { Call, Channel, Connection, Presence, Standing } from "./api";

/**
 * One short phrase per connection state.
 *
 * A stopped loop is the only case that does not imply a message might still
 * arrive, and a session the homeserver has rejected is the only one the user
 * has to do something about, so those two do not share a label.
 */
export function connectionLabel(connection: Connection): string {
  switch (connection.state) {
    case "connecting":
      return "Connecting";
    case "live":
      return "Connected";
    case "offline":
      return "Reconnecting";
    case "stopped":
      return connection.reason === "sessionEnded"
        ? "Session ended"
        : "Disconnected";
  }
}

/**
 * The letter to draw when there is no picture.
 *
 * The sigil is dropped first. A room ID, a channel name and a user ID all
 * begin with punctuation, and an avatar showing "!" tells nobody anything.
 *
 * Spread rather than `charAt`, because a name can begin with a character that
 * is more than one UTF-16 code unit, and half of one renders as a replacement
 * glyph.
 */
export function initialsOf(name: string): string {
  const first = [...name.replace(/^[!#@]/, "").trim()][0];
  return first ? first.toUpperCase() : "?";
}

/** What an unjoined child is called until the hierarchy request names it. */
const UNKNOWN_CHANNEL = "Unknown channel";

/**
 * What to call a channel.
 *
 * `name` is null only for a room a space lists and this account has never
 * joined, so nothing local knows what it is called. Never its room ID.
 */
export function channelLabel(channel: Channel): string {
  return channel.name ?? UNKNOWN_CHANNEL;
}

/**
 * One short phrase per call state.
 *
 * Deliberately not the same words as `connectionLabel`. Both channels have a
 * state that means "it worked", and putting "Connected" on both would be two
 * different claims wearing one label, one about Matrix and one about a voice
 * channel. "Voice connected" is what Discord says and it is what a person
 * reads as the answer to a different question.
 *
 * A failure gets a fixed phrase rather than the error text. The error is a
 * sentence of its own and gets its own line; this is the label above it, and a
 * label that changes length with the failure would reflow the panel.
 */
export function callLabel(call: Call): string {
  switch (call.state) {
    case "connecting":
      return "Connecting";
    case "connected":
      return "Voice connected";
    case "disconnected":
      return "Not in a voice channel";
    case "failed":
      return "Could not connect";
  }
}

/**
 * How long somebody has been in the call, in words.
 *
 * Rounded down and coarse on purpose. The exact figure is never the question,
 * and a second-by-second reading would be a number that changes while it is
 * being read, on a panel that is open for a few seconds at a time.
 *
 * `now` is passed in rather than read here, so this is a pure function and a
 * test does not have to freeze the clock.
 */
export function elapsedLabel(since: number, now: number): string {
  const seconds = Math.floor((now - since) / 1000);
  // A clock a few seconds ahead of the server would otherwise read as an hour
  // short of a day, going backwards. "Just now" is true of both.
  if (seconds < 60) return "Just now";

  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} ${plural(minutes, "minute")}`;

  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} ${plural(hours, "hour")}`;

  // A call running for days is a channel somebody left open, and the number of
  // hours stops being informative long before this.
  const days = Math.floor(hours / 24);
  return `${days} ${plural(days, "day")}`;
}

function plural(count: number, word: string): string {
  return count === 1 ? word : `${word}s`;
}

/**
 * One short phrase per presence state.
 *
 * "Unknown" is drawn rather than hidden. A homeserver with presence switched
 * off is the ordinary case, and a card that silently omits the line would read
 * as a card that has not finished loading.
 */
export function presenceLabel(presence: Presence): string {
  switch (presence) {
    case "online":
      return "Online";
    case "idle":
      return "Idle";
    case "offline":
      return "Offline";
    case "unknown":
      return "Status unknown";
  }
}

/**
 * What to call somebody's standing, or nothing for an ordinary member.
 *
 * `null` rather than "Member", because a badge on everybody is a badge that
 * says nothing. The label exists to mark the two cases that change what a
 * person can do to you.
 */
export function standingLabel(standing: Standing): string | null {
  switch (standing) {
    case "admin":
      return "Admin";
    case "moderator":
      return "Moderator";
    case "member":
      return null;
  }
}
