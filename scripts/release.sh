#!/usr/bin/env bash
# Cut a release.
#
# The version is not an argument. git-cliff reads the commits since the last
# tag and answers with the next one by the conventional-commit rules: a feat
# moves the minor, a fix moves the patch, a breaking change moves the major.
# That is the whole reason the commit messages are written the way
# CONTRIBUTING.md asks for.
#
# Five files carry the version and none of them reads another, which is what
# this exists to stop being a manual checklist. Pushing is not one of the
# steps: this repository has two remotes and which of them gets a release is a
# decision rather than a formality.
set -euo pipefail

cd "$(dirname "$0")/.."

for tool in git git-cliff cargo; do
  command -v "$tool" >/dev/null || { echo "$tool is not on PATH." >&2; exit 1; }
done

branch=$(git rev-parse --abbrev-ref HEAD)
if [ "$branch" != "main" ]; then
  echo "On $branch. Releases are cut from main." >&2
  exit 1
fi

if ! git diff --quiet || ! git diff --cached --quiet; then
  echo "The tree is dirty. Commit or stash first." >&2
  exit 1
fi

tag=$(git cliff --bumped-version 2>/dev/null)
version=${tag#v}
if [ -z "$version" ]; then
  echo "git-cliff would not name a version. Nothing releasable since the last tag?" >&2
  exit 1
fi

if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  echo "$tag already exists." >&2
  exit 1
fi

echo "Releasing $version."

# The workspace version. Line-anchored, because a dependency's version is
# indented inside its own table and this one is the only bare `version =` in
# the file.
sed -i "s/^version = \".*\"$/version = \"$version\"/" Cargo.toml

sed -i "s/^  \"version\": \".*\",$/  \"version\": \"$version\",/" app/package.json
sed -i "s/^  \"version\": \".*\",$/  \"version\": \"$version\",/" app/src-tauri/tauri.conf.json

# makepkg overwrites this from the checkout, so the number here is only a
# placeholder. It still has to be the right one: it is what a build from a
# tarball reports.
sed -i "s/^pkgver=.*$/pkgver=$version.r0.g0000000/" packaging/aur/PKGBUILD

# The two filenames the install instructions name.
sed -i "s#/deb/Consort_[0-9][^_]*_amd64.deb#/deb/Consort_${version}_amd64.deb#" README.md
sed -i "s#/rpm/Consort-[0-9][^-]*-1.x86_64.rpm#/rpm/Consort-${version}-1.x86_64.rpm#" README.md

# So Cargo.lock's four consort-* entries follow. --offline because this reads
# the manifests it already has and must not go looking for anything.
cargo metadata --offline --format-version 1 >/dev/null

# --tag because the tag does not exist yet: without it everything since the
# last release lands under an "Unreleased" heading.
git cliff --tag "$tag" --output CHANGELOG.md

git commit -qam "chore(release): $version"

# The notes go in the tag object as well as on the release page, so somebody
# reading the history with git alone gets them too.
git cliff --latest --strip all | git tag -a "$tag" -F -

cat <<MSG

Tagged $tag. Nothing is pushed. When you are ready:

  git push forgejo main --follow-tags
  git push origin main --follow-tags

Pushing the tag to origin is what builds the installer and writes the notes.
MSG
