/**
 * Typed wrappers over the Rust commands.
 *
 * The types here are hand-mirrored from `app/src-tauri/src/commands.rs`. They
 * are the seam where a Rust change becomes a TypeScript compile error, so keep
 * them in step with that file rather than reaching for `any` at a call site.
 */
import { invoke } from "@tauri-apps/api/core";

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
 */
export function asCommandError(error: unknown): CommandError {
  if (
    typeof error === "object" &&
    error !== null &&
    "message" in error &&
    typeof (error as CommandError).message === "string"
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
