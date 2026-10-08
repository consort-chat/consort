#!/usr/bin/env bash
# What the update manifest has to say for an installed Consort to read it.
#
# The manifest is the one thing in the update path with no compiler and no type
# behind it. tauri-plugin-updater validates the whole file before it looks at
# the version, so a field with the wrong name is not a degraded update, it is
# every client reporting that the server answered with something unreadable.
#
# Run against the file about to be served, in the release run that built it.
# Every assertion is positive: a manifest that is merely valid JSON is not a
# pass, because that is exactly what the failure looks like.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

manifest=${1:?usage: check-manifest.sh <path to latest.json> [public key]}
# The key every shipped binary checks against, which is the one in the config
# the release was built from unless a caller says otherwise.
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

# The three the plugin requires. `platforms` keyed by `{os}-{arch}`, which is
# what it looks itself up under; Windows is the only build that updates itself.
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

# A `.sig` from the Tauri bundler is base64 of a minisign file, so it decodes
# and it carries the untrusted comment minisign writes. An empty string here is
# the shape this test exists for: the workflow would have substituted one
# silently from a step that produced no signature.
if [ -z "$signature" ]; then
  fail "there is no signature, so every client will refuse this update"
elif ! printf '%s' "$signature" | base64 -d 2>/dev/null | grep -q 'untrusted comment'; then
  fail "the signature is not the contents of a .sig file the bundler wrote"
fi

# `requireSignedVersion` is on, so the version the artifact was signed for has
# to be the version announced here, and a signature carrying no version at all
# is refused outright. Both live in minisign's trusted comment, which the
# signature itself covers, as tab separated `key:value` fields.
#
# The Tauri CLI only started writing `version:` in 2.11.5, so this is also what
# catches app/package.json's floor slipping back below that: the symptom would
# otherwise be every client refusing every update after the release went out.
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

# Which key signed it. minisign compares key ids before it does any crypto, so
# a signature made by a key whose public half is not the one in the binary is
# refused by every client after it has downloaded the installer.
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

# Nothing for Linux, ever. A .deb and an Arch package belong to a package
# manager, and an entry here is all it would take for the plugin to reach for
# `pkexec dpkg -i` on one. See #47 and docs/PLAN-self-update.md.
if [ "$(jq -r '[.platforms | keys[] | select(startswith("linux") or startswith("darwin"))] | length' "$manifest")" != 0 ]; then
  fail "this manifest offers an update to a platform whose packages Consort does not own"
fi

if [ "$failures" -ne 0 ]; then
  echo
  echo "$failures problem(s) with $manifest. Not serving it." >&2
  exit 1
fi

echo "$manifest is the manifest the plugin reads."
