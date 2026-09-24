#!/usr/bin/env bash
# What the release notes have to keep saying about who wrote them.
#
# Attribution is the kind of thing that fails by being absent. Every handle in
# cliff.toml's template sits inside an `{% if %}`, so nothing about a missing
# GitHub answer looks like an error: git-cliff exits 0, the notes render
# cleanly, and the only difference is that nobody is credited. That is the
# failure this exists to catch, which is why every assertion below is a
# positive one. Rendering without crashing is not a pass.
#
# Run against the repository's own history and the real GitHub API, for the
# same reason set-version.test.sh uses the repository's own files: the thing
# under test is this tree's cliff.toml against this project's commits, and a
# fixture would only prove that a fixture still parses. It follows that
# `--offline`, `GIT_CLIFF_OFFLINE` and an unreachable API all turn this red,
# which is the point.
#
# Nothing here can be broken by a future pull request. The strict per-line
# check reads a released range that is closed forever, and the rest assert
# facts about contributions already in the history, so a contributor whose
# email GitHub cannot match to an account fails no test and blocks no release.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
root=$PWD

if ! command -v git-cliff >/dev/null 2>&1; then
  echo "git-cliff is not on PATH. The release workflow pins the version it" >&2
  echo "uses; see .github/workflows/release.yml." >&2
  exit 1
fi

failures=0
fail() {
  echo "  FAIL: $*"
  failures=$((failures + 1))
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# PR #89 is the only pull request this repository has had from somebody other
# than its owner, so it is the one place where crediting the wrong person, or
# nobody, would be visible rather than theoretical.
newcomer=bernalalexis-try
their_line='- **timeline:** Show joins, leaves, invites, kicks and bans in the room (@bernalalexis-try)'

# A range between two tags that already exist. It cannot grow, so the strict
# check below cannot start failing because of somebody else's commit.
frozen=v0.6.0..v0.7.0
frozen_lines=9

render() {
  local out=$1
  shift
  if ! git-cliff --config "$root/cliff.toml" "$@" >"$tmp/$out" 2>"$tmp/$out.err"; then
    echo "git-cliff failed on: $*" >&2
    cat "$tmp/$out.err" >&2
    exit 1
  fi
}

# The whole changelog, as the release workflow regenerates it, and the same
# thing with the strip the notes and the tag message both get.
render whole
render stripped --strip all
render frozen --strip all "$frozen"

echo "a first time contributor is named"
if ! grep -qxF -- "- @$newcomer made their first contribution" "$tmp/whole"; then
  fail "the changelog does not name @$newcomer as a first time contributor"
fi

echo "naming them survives the strip the release page gets"
# `--strip all` drops the header and the footer, and the notes and the tag
# message are both made with it. Anything moved out of the body therefore stops
# reaching the release page, and nothing reports that it has.
kept=$(grep -c '^### New contributors$' "$tmp/whole" || true)
[ "$kept" -gt 0 ] || fail "the changelog has no New contributors section to strip"
stripped=$(grep -c '^### New contributors$' "$tmp/stripped" || true)
if [ "$stripped" -ne "$kept" ]; then
  fail "--strip all left $stripped of $kept New contributors sections, so the section is in the footer rather than the body"
fi

echo "every change line in a closed release says who wrote it"
# Counted first. A check that only looks for lines without a handle passes on
# a range that rendered nothing at all, which is the shape of the failure.
counted=$(grep -c '^- ' "$tmp/frozen" || true)
if [ "$counted" -ne "$frozen_lines" ]; then
  fail "$frozen rendered $counted change lines, expected $frozen_lines"
fi
bare=$(grep -E '^- ' "$tmp/frozen" | grep -vE ' \(@[A-Za-z0-9._-]+\)$' || true)
if [ -n "$bare" ]; then
  fail "$frozen has change lines with nobody on them:"
  echo "$bare" | sed 's/^/    /'
fi

echo "the handle on a line is whoever wrote that line"
if ! grep -qxF -- "$their_line" "$tmp/stripped"; then
  fail "the line PR #89 added does not carry @$newcomer:"
  { grep -F 'joins, leaves, invites' "$tmp/stripped" || echo "(the line is not in the changelog at all)"; } | sed 's/^/    /'
fi
# Whoever wrote it, rather than one name repeated: an attribution taken from
# the release instead of from the commit would pass everything above.
handles=$({ grep -oE '\(@[A-Za-z0-9._-]+\)$' "$tmp/stripped" || true; } | sort -u | wc -l)
if [ "$handles" -lt 2 ]; then
  fail "the whole changelog names $handles distinct handles, so the lines are not being credited individually"
fi

if [ "$failures" -gt 0 ]; then
  echo
  echo "$failures failed."
  echo
  echo "If every one of them failed, the notes carry no attribution at all."
  echo "That is a template in cliff.toml that asks for none, or --offline,"
  echo "GIT_CLIFF_OFFLINE, or GitHub not answering."
  exit 1
fi
echo
echo "All good."
