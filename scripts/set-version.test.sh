#!/usr/bin/env bash
# What set-version.sh has to keep getting right.
#
# Every assertion here is about an anchor, because a sed that matches nothing
# exits 0 and leaves the file alone. That is the failure worth covering:
# somebody reformats package.json, the next release still reports success, and
# the Windows job then looks for an installer named after the previous version
# and cannot find it. Nothing in between says anything.
#
# The fixtures are the repository's own files rather than samples written out
# here, so a reformat fails this test rather than a release.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
root=$PWD
script=$root/scripts/set-version.sh

failures=0
fail() {
  echo "  FAIL: $*"
  failures=$((failures + 1))
}

# The seven files, copied into a tree of their own so a test can rewrite them.
fixture() {
  local tmp
  tmp=$(mktemp -d)
  mkdir -p "$tmp/app/src-tauri" "$tmp/packaging/aur" "$tmp/packaging/arch"
  cp "$root/Cargo.toml" "$tmp/Cargo.toml"
  cp "$root/Cargo.lock" "$tmp/Cargo.lock"
  cp "$root/app/package.json" "$tmp/app/package.json"
  cp "$root/app/src-tauri/tauri.conf.json" "$tmp/app/src-tauri/tauri.conf.json"
  cp "$root/packaging/aur/PKGBUILD" "$tmp/packaging/aur/PKGBUILD"
  cp "$root/packaging/arch/PKGBUILD" "$tmp/packaging/arch/PKGBUILD"
  cp "$root/README.md" "$tmp/README.md"
  printf '%s' "$tmp"
}

has_line() {
  grep -qxF "$2" "$1" || fail "$(basename "$1") has no line '$2'"
}

has_text() {
  grep -qF "$2" "$1" || fail "$(basename "$1") does not contain '$2'"
}

echo "every file carries the new version"
tmp=$(fixture)
"$script" 9.9.9 "$tmp" >/dev/null
has_line "$tmp/Cargo.toml" 'version = "9.9.9"'
has_line "$tmp/app/package.json" '  "version": "9.9.9",'
has_line "$tmp/app/src-tauri/tauri.conf.json" '  "version": "9.9.9",'
# The aur recipe keeps its suffix: makepkg overwrites the whole thing from the
# checkout, so what is written here only has to be the shape of a real answer.
has_line "$tmp/packaging/aur/PKGBUILD" 'pkgver=9.9.9.r0.g0000000'
# The arch recipe does not, because package-arch.yml refuses to build when this
# line and the tag disagree.
has_line "$tmp/packaging/arch/PKGBUILD" 'pkgver=9.9.9'
has_text "$tmp/README.md" '/rpm/Consort-9.9.9-1.x86_64.rpm'
rm -rf "$tmp"

echo "Cargo.lock's workspace members follow, and nothing else in it moves"
tmp=$(fixture)
members=$(grep -c '^name = "consort-' "$tmp/Cargo.lock")
[ "$members" -eq 5 ] || fail "expected five consort crates in Cargo.lock, found $members"
cp "$tmp/Cargo.lock" "$tmp/before.lock"
"$script" 9.9.9 "$tmp" >/dev/null
# Every consort entry, and only those: a dependency list names these five
# without a version, so the version on the line after the name is the whole of
# what cargo would have rewritten.
written=$(awk -v want='version = "9.9.9"' \
  '/^name = "consort-/ { getline; if ($0 == want) n++ } END { print n + 0 }' \
  "$tmp/Cargo.lock")
[ "$written" -eq "$members" ] || fail "$written of $members consort entries were updated"
moved=$(diff "$tmp/before.lock" "$tmp/Cargo.lock" | grep -c '^> ' || true)
[ "$moved" -eq "$members" ] || fail "$moved lines of Cargo.lock changed, expected $members"
rm -rf "$tmp"

echo "a dependency's own version is left alone"
tmp=$(fixture)
"$script" 9.9.9 "$tmp" >/dev/null
# `rust-version` is the reason the workspace version is matched line-anchored,
# and a dependency's version sits indented inside its own table.
has_line "$tmp/Cargo.toml" 'rust-version = "1.97"'
has_text "$tmp/Cargo.toml" 'serde = { version = "1", features = ["derive"] }'
rm -rf "$tmp"

echo "running it twice changes nothing the second time"
tmp=$(fixture)
"$script" 9.9.9 "$tmp" >/dev/null
before=$(find "$tmp" -type f -exec sha256sum {} + | sort)
"$script" 9.9.9 "$tmp" >/dev/null
after=$(find "$tmp" -type f -exec sha256sum {} + | sort)
[ "$before" = "$after" ] || fail "a second run changed something"
rm -rf "$tmp"

echo "a version that is not three numbers is refused"
tmp=$(fixture)
for bad in "" "1.2" "v1.2.3" "1.2.3-rc1" "nonsense"; do
  if "$script" "$bad" "$tmp" >/dev/null 2>&1; then
    fail "accepted '$bad'"
  fi
done
# Refusing has to mean refusing to write, not writing and then complaining.
has_line "$tmp/Cargo.toml" "version = \"$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml")\""
rm -rf "$tmp"

echo "a missing anchor is an error rather than a silent no-op"
tmp=$(fixture)
# What a reformat looks like: the version is still there and still correct, and
# the line it is on no longer matches.
sed -i 's/^  "version": "\(.*\)",$/    "version": "\1",/' "$tmp/app/package.json"
if "$script" 9.9.9 "$tmp" >/dev/null 2>&1; then
  fail "wrote a version into a tree where package.json could not take one"
fi
rm -rf "$tmp"

if [ "$failures" -gt 0 ]; then
  echo
  echo "$failures failed."
  exit 1
fi
echo
echo "All good."
