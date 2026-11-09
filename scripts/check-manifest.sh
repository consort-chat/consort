#!/usr/bin/env bash
# What the update manifest has to say for an installed Consort to read it.
#
# The one thing in the update path with no compiler behind it, so every
# assertion here is positive: valid JSON is not a pass, it is the failure.
# Run against the file about to be served. docs/PLAN-self-update.md.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

manifest=${1:?usage: check-manifest.sh <path to latest.json> [public key]}
# The key every shipped binary checks against, unless a caller says otherwise.
pubkey=${2:-$(jq -r '.plugins.updater.pubkey // empty' "$root/app/src-tauri/tauri.conf.json")}

failures=0
fail() {
  echo "  FAIL: $*"
  failures=$((failures + 1))
}

if ! jq -e . "$manifest" >/dev/null 2>&1; then
  echo "$manifest is not JSON. Nothing else can be said about it." >&2
  exit 1
fi

read_field() { jq -r "$1 // empty" "$manifest"; }

# The three the plugin requires. `platforms` is keyed by `{os}-{arch}`.
version=$(read_field '.version')
url=$(read_field '.platforms."windows-x86_64".url')
signature=$(read_field '.platforms."windows-x86_64".signature')

case "$version" in
  [0-9]*.[0-9]*.[0-9]*) ;;
  *) fail "version is '$version', which is not a semver this build compares against" ;;
esac

case "$url" in
  https://github.com/*/releases/download/v"$version"/*-setup.exe)
    ;;
  *)
    fail "url is '$url', which is not this version's installer on its release page"
    ;;
esac

# A `.sig` is base64 of a minisign file, so it decodes and carries minisign's
# untrusted comment. Empty is what a step that signed nothing substitutes.
if [ -z "$signature" ]; then
  fail "there is no signature, so every client will refuse this update"
elif ! printf '%s' "$signature" | base64 -d 2>/dev/null | grep -q 'untrusted comment'; then
  fail "the signature is not the contents of a .sig file the bundler wrote"
fi

# `requireSignedVersion` is on, so the signed version has to be the announced
# one, and no version at all is refused. Both live in minisign's trusted
# comment as tab separated fields, written only by CLI 2.11.5 and newer, so
# this is also what catches app/package.json's floor slipping.
if [ -n "$signature" ]; then
  # `|| :` because pipefail turns a signature that is not base64 into a
  # `set -e` death here, which reads in a release log as the gate crashing.
  comment=$(printf '%s' "$signature" | base64 -d 2>/dev/null \
    | sed -n 's/^trusted comment: //p' || :)
  signed=$(printf '%s' "$comment" | tr '\t' '\n' | sed -n 's/^version://p')
  if [ -z "$signed" ]; then
    fail "the signature records no version, so requireSignedVersion refuses it; app/package.json needs @tauri-apps/cli 2.11.5 or newer"
  elif [ "$signed" != "$version" ] && [ "$signed" != "v$version" ]; then
    fail "the signature was made for '$signed', not for $version"
  fi
fi

# Which key signed it. A signature made by the wrong key is refused by every
# client, after it has downloaded the installer.
key_id() {
  printf '%s' "$1" | base64 -d 2>/dev/null | sed -n 2p | base64 -d 2>/dev/null \
    | od -An -tx1 -N10 | tr -d ' \n' | cut -c5-20 || :
}

ships_with=$(key_id "$pubkey")
signed_by=$(key_id "$signature")
if [ ${#ships_with} -ne 16 ]; then
  fail "plugins.updater.pubkey is not a minisign public key, so no client can check this release"
elif [ ${#signed_by} -eq 16 ] && [ "$signed_by" != "$ships_with" ]; then
  fail "the signature was made by key $signed_by, and shipped binaries only trust $ships_with"
fi

# Optional to the plugin, but not optional here: it is rejected if it is present
# and not RFC 3339, which is a whole release that cannot be offered to anybody.
pub_date=$(read_field '.pub_date')
if [ -n "$pub_date" ]; then
  case "$pub_date" in
    [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]Z) ;;
    *) fail "pub_date is '$pub_date', which the plugin will not parse as RFC 3339" ;;
  esac
fi

# Nothing for Linux, ever: an entry here is all it takes for the plugin to
# reach for `pkexec dpkg -i` on a package apt is tracking. See #47.
if [ "$(jq -r '[.platforms | keys[] | select(startswith("linux") or startswith("darwin"))] | length' "$manifest")" != 0 ]; then
  fail "this manifest offers an update to a platform whose packages Consort does not own"
fi

if [ "$failures" -ne 0 ]; then
  echo
  echo "$failures problem(s) with $manifest. Not serving it." >&2
  exit 1
fi

echo "$manifest is the manifest the plugin reads."
