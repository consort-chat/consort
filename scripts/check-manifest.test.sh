#!/usr/bin/env bash
# What check-manifest.sh has to keep catching.
#
# Every assertion here names the problem it expects, because the gate's own
# failure mode is passing a manifest for the wrong reason: a glob loosened by
# one character, or a sed that stops matching, turns a check into a formality
# and nothing says so. The gate runs only on a tag, so a formality is found by
# the release it breaks.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
gate=$root/scripts/check-manifest.sh

failures=0
fail() {
  echo "  FAIL: $*"
  failures=$((failures + 1))
}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Nothing below is a key. A minisign .pub and .sig are text whose second line
# is base64 of two algorithm bytes, an eight byte key id, and a payload; these
# build that shape over filler, and no private half exists for any of them.
KEY_ID=$'\x11\x22\x33\x44\x55\x66\x77\x88'

# A minisign file is four lines for a signature and two for a public key, each
# base64 line carrying the bytes above. Built line by line because `base64 -w0`
# writes no trailing newline and a run-together line parses as neither.
lines() { printf '%s\n' "$@" | base64 -w0; }

bytes() { printf "$@" | base64 -w0; }

# <trusted comment>
a_signature() {
  lines "untrusted comment: signature from a fixture" \
    "$(bytes 'ED%s%064d' "$KEY_ID" 0)" \
    "trusted comment: $1" \
    "$(bytes '%064d' 0)"
}

VERSION=0.12.0
SIGNED=$'timestamp:1760000000\tfile:Consort_0.12.0_x64-setup.exe\tversion:0.12.0'
URL=https://github.com/consort-chat/consort/releases/download/v0.12.0/Consort_0.12.0_x64-setup.exe

# <name> [jq edit applied to a manifest that would otherwise pass]
a_manifest() {
  local name=$1 edit=${2:-.}
  jq -n \
    --arg version "$VERSION" \
    --arg url "$URL" \
    --arg signature "$(a_signature "$SIGNED")" \
    '{
       version: $version,
       notes: "https://example.invalid",
       pub_date: "2026-10-08T09:00:00Z",
       platforms: { "windows-x86_64": { url: $url, signature: $signature } }
     }' \
    | jq "$edit" > "$work/$name.json"
  echo "$work/$name.json"
}

passes() {
  local manifest=$1 what=$2 output
  if ! output=$("$gate" "$manifest" 2>&1); then
    fail "$what was rejected: $output"
  fi
}

# <manifest> <what it is> <the words the complaint has to contain>
rejects() {
  local manifest=$1 what=$2 expected=$3 output
  if output=$("$gate" "$manifest" 2>&1); then
    fail "$what was served: $output"
  elif ! printf '%s' "$output" | grep -qF "$expected"; then
    fail "$what was rejected for the wrong reason: $output"
  fi
}

echo "A manifest the plugin can read"
passes "$(a_manifest good)" "the manifest the release run builds"

echo "A version no client can compare against"
rejects "$(a_manifest unversioned '.version = "latest"')" \
  "a non-semver version" "not a semver"

echo "A url pointing somewhere other than this release"
rejects "$(a_manifest elsewhere '.platforms."windows-x86_64".url = "https://example.invalid/setup.exe"')" \
  "an installer hosted elsewhere" "not this version's installer"
rejects "$(a_manifest wrong_tag ".platforms.\"windows-x86_64\".url = \"https://github.com/consort-chat/consort/releases/download/v0.11.0/Consort_0.11.0_x64-setup.exe\"")" \
  "the previous release's installer" "not this version's installer"

echo "A signature that is not one"
rejects "$(a_manifest unsigned '.platforms."windows-x86_64".signature = ""')" \
  "no signature at all" "there is no signature"
rejects "$(a_manifest not_a_sig ".platforms.\"windows-x86_64\".signature = \"$(printf 'not a signature' | base64 -w0)\"")" \
  "base64 of something that is not a .sig" "not the contents of a .sig"

echo "A signature that does not vouch for this version"
rejects "$(a_manifest no_signed_version \
  ".platforms.\"windows-x86_64\".signature = \"$(a_signature $'timestamp:1760000000\tfile:Consort_0.12.0_x64-setup.exe')\"")" \
  "a signature recording no version" "records no version"
rejects "$(a_manifest older_signature \
  ".platforms.\"windows-x86_64\".signature = \"$(a_signature $'timestamp:1760000000\tfile:Consort_0.11.0_x64-setup.exe\tversion:0.11.0')\"")" \
  "a signature made for an earlier release" "was made for '0.11.0'"

echo "A date the plugin will not parse"
rejects "$(a_manifest bad_date '.pub_date = "8 October 2026"')" \
  "a pub_date that is not RFC 3339" "RFC 3339"

echo "A platform whose packages Consort does not own"
rejects "$(a_manifest linux '.platforms."linux-x86_64" = .platforms."windows-x86_64"')" \
  "an entry offering a Linux update" "does not own"
rejects "$(a_manifest no_windows '.platforms = {}')" \
  "a manifest with nothing for Windows" "there is no signature"

if [ "$failures" -ne 0 ]; then
  echo
  echo "$failures problem(s) with $gate." >&2
  exit 1
fi

echo
echo "check-manifest.sh still catches every one of them."
