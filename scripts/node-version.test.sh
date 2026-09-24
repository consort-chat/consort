#!/usr/bin/env bash
# The two Node versions this repository has to keep straight.
#
# One is the Node that builds the frontend: the container ci.yml runs the
# frontend job in, what setup-node installs on the two packaging runners, what
# the PKGBUILDs pull in, and whatever happens to be on a contributor's PATH.
#
# The other is the Node the actions themselves run on, which is not the same
# thing and is what issue #24 was actually about. GitHub deprecated the Node 20
# action runtime. An action pinned to a major whose action.yml still says
# `using: node20` gets force-run on Node 24 and a warning is attached to the
# run, which is how four of them ended up on a release with nobody noticing:
# the warning lands in the run summary rather than in any step, so every job
# stays green and every log reads clean. It stops being a warning the day
# GitHub stops honouring node20, and on that day the thing that breaks is the
# release.
#
# Neither half can be caught by building anything. A workflow file is inert
# until it runs and it only runs after a merge, so a mistake in one is found by
# the release it breaks. That is the same reason the gates in the Hygiene job
# exist, and this runs alongside them.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

failures=0
fail() {
  echo "  FAIL: $*"
  failures=$((failures + 1))
}

# The major every part of this repository is expected to name. One place, so
# that the next bump is one edit here and a list of exactly what disagrees.
node_major=24

# The lowest major of each action whose action.yml says `using: node24`, read
# off that file at that tag rather than assumed from the changelog.
#
# These are floors and not pins: a newer major is fine and passes. Each one is
# the major whose release was the node24 migration itself, which is why they
# differ. actions/upload-artifact is the one to look twice at: its v5 shipped
# before the migration and still declares node20, so v5 is not enough and v6 is
# the floor.
#
# Swatinem/rust-cache is on 2 because its v2 tag already moved to node24 within
# 2.9.x, so there is no newer major to go to. dtolnay/rust-toolchain runs no
# Node at all and says `using: composite`.
floor_for() {
  case "$1" in
    actions/checkout)        echo 5 ;;
    actions/setup-node)      echo 5 ;;
    actions/cache)           echo 5 ;;
    actions/upload-artifact) echo 6 ;;
    Swatinem/rust-cache)     echo 2 ;;
    dtolnay/rust-toolchain)  echo composite ;;
    *)                       echo unknown ;;
  esac
}

echo "every action runs on Node $node_major"

# Resolved before the loop rather than piped into it, so a git that could not
# read the tree fails here instead of reading as a workflow directory with no
# actions in it. An empty result is the failure this whole file is about.
specs=$(git grep -h -E '^[[:space:]]*(- )?uses:' -- .github/workflows \
  | sed -E 's/^[[:space:]]*(- )?uses:[[:space:]]*//' \
  | sort -u) || fail "git grep could not read .github/workflows."

checked=0
while read -r spec; do
  [ -n "$spec" ] || continue
  # A reusable workflow in this repository. The actions inside it are in this
  # same directory and are read on their own.
  case "$spec" in ./*) continue ;; esac

  action=${spec%@*}
  ref=${spec#*@}
  floor=$(floor_for "$action")

  case "$floor" in
    unknown)
      # Failing closed, because an action nobody checked is exactly what put
      # Node 20 in here. Read its action.yml at the tag being pinned, see what
      # `using:` says, and add it above.
      fail "$spec is not in this test's table, so what it runs on is unknown."
      continue
      ;;
    composite)
      checked=$((checked + 1))
      continue
      ;;
  esac

  case "$ref" in
    v[0-9]*) major=${ref#v}; major=${major%%.*} ;;
    *)
      fail "$spec is not pinned to a version tag, so what it runs on cannot be read."
      continue
      ;;
  esac

  if [ "$major" -lt "$floor" ]; then
    fail "$spec still runs on Node 20. $action needs v$floor or newer."
  fi
  checked=$((checked + 1))
done <<< "$specs"

# A floor well under the real count, there only so that a search which matched
# nothing cannot pass this section by having nothing to disagree with.
[ "$checked" -ge 5 ] || fail "only $checked actions were read out of .github/workflows."

echo "the Node version is $node_major everywhere it is named"

# nvm is what the maintainer's machine runs, and it has 20, 24 and 25 unpacked
# side by side, so "whatever node is on PATH" genuinely can be the version this
# issue was about.
if [ -f .nvmrc ]; then
  nvmrc=$(tr -d '[:space:]' < .nvmrc)
  [ "$nvmrc" = "$node_major" ] || fail ".nvmrc says '$nvmrc' rather than '$node_major'."
else
  fail "There is no .nvmrc, so nvm has nothing to read."
fi

# The frontend job runs inside this image rather than on the runner, so this is
# the Node that type checks, tests and builds the frontend on every pull
# request. It is a pin already and the point here is that it stays one.
grep -qF "image: node:$node_major-bookworm" .github/workflows/ci.yml \
  || fail "ci.yml's frontend job is not on node:$node_major-bookworm."

# setup-node with no node-version installs whatever the runner image ships,
# which is a pin in appearance only. Every use of it has to carry one.
setups=$(git grep -h -E '^[[:space:]]*(- )?uses:[[:space:]]*actions/setup-node@' \
  -- .github/workflows | wc -l)
pins=$(git grep -h -E '^[[:space:]]*node-version:' -- .github/workflows | wc -l)
[ "$setups" -eq "$pins" ] \
  || fail "$setups uses of setup-node and $pins node-version lines; one of them names no version."

asked=$(git grep -h -E '^[[:space:]]*node-version:' -- .github/workflows \
  | sed -E "s/.*node-version:[[:space:]]*//" | tr -d "\"'" | sort -u)
[ "$asked" = "$node_major" ] \
  || fail "setup-node is asked for [$(printf '%s' "$asked" | tr '\n' ' ')] rather than $node_major."

# What a clone is told. This is a warning and not a gate, for the reason
# written out in app/pnpm-workspace.yaml: the setting that would make it a gate
# also applies every dependency's range, and jsdom's excludes Node 25
# altogether. Asserted here so that the floor cannot quietly go back to 20.
grep -qF "\"node\": \">=$node_major\"" app/package.json \
  || fail "app/package.json does not ask for node >=$node_major."
grep -qF "engineStrict" app/pnpm-workspace.yaml \
  || fail "app/pnpm-workspace.yaml no longer says why engineStrict is left off."

# The two recipes install their own Node rather than using the caller's, and
# `nodejs` alone accepts whatever the distribution is shipping that week.
for recipe in packaging/arch/PKGBUILD packaging/aur/PKGBUILD; do
  grep -qF "'nodejs>=$node_major'" "$recipe" \
    || fail "$recipe does not ask for nodejs>=$node_major."
done

grep -qF "Node $node_major or newer" docs/BUILDING.md \
  || fail "docs/BUILDING.md does not say Node $node_major or newer."

if [ "$failures" -gt 0 ]; then
  echo
  echo "$failures failed."
  exit 1
fi
echo
echo "All good."
