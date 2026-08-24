# AUR packaging

`PKGBUILD` builds Consort from the latest commit on `main`. It is not published
to the AUR yet. Build it locally with `makepkg -si`.

## What the AUR actually requires

The AUR hosts build scripts, not binaries. There is no upload of a compiled
artifact anywhere in the process, and no review queue: you push a git repo and
it is live. The requirements are correspondingly mechanical.

**An account with an SSH key.** Register at https://aur.archlinux.org and add a
public key. Publishing is `git push` over SSH and there is no other route.

**A repo whose only tracked files are the packaging ones.** For us that is
`PKGBUILD` and `.SRCINFO`. The AUR repo is separate from this one:

```sh
git clone ssh://aur@aur.archlinux.org/consort-git.git
```

An empty clone with a warning is what a name nobody has claimed looks like.

**`.SRCINFO`, committed and current.** A server-side hook rejects any push
without it, and rejects one that disagrees with the PKGBUILD. It is generated,
never hand-edited:

```sh
makepkg --printsrcinfo > .SRCINFO
```

Regenerate it in the same commit as every PKGBUILD change. For a `-git`
package, run `makepkg` once first so `pkgver()` resolves against a real
checkout, otherwise you publish the placeholder version.

**A unique package name.** `consort-git` is free at the time of writing, but
check, because names are first come first served.

**The `-git` suffix, for now.** AUR convention: a package built from the latest
VCS commit takes the suffix and needs a `pkgver()` function so `yay` can tell
that an upgrade exists. A tagged release later gets a plain `consort` package
alongside it. The `provides`/`conflicts` pair in the PKGBUILD is what stops
both installing at once.

## Before pushing anything

Two checks, neither optional:

```sh
# Builds in a clean chroot, which is the only way to catch a missing
# makedepend that happens to be installed on your machine.
pacman -S devtools
extra-x86_64-build

# Lints metadata: dependencies you declared but do not link, ones you link but
# did not declare, file permissions, .desktop correctness.
pacman -S namcap
namcap PKGBUILD
namcap consort-git-*.pkg.tar.zst
```

`depends` in the PKGBUILD came from `readelf -d` on the built binary rather
than from what Tauri packages usually list, so namcap should be quiet. If it is
not, it is right and the list is wrong.

## Things specific to this package

**The frontend must be built before cargo.** Tauri embeds `app/dist` into the
binary at compile time and `cargo build` does not run `beforeBuildCommand`.
Skip it and the package compiles, installs, launches, and shows a white
rectangle.

**`rust-toolchain.toml` is deleted in `prepare()`.** A clean chroot uses Arch's
`rust` and ignores the file, so this looks unnecessary. It is not: `yay` builds
on the user's own machine, rustup shims do honour the file, and cargo would
then try to download the pinned toolchain from inside `build()` where there is
meant to be no network.

**`sqlite` is a real dependency.** matrix-sdk-sqlite links the system library
instead of bundling it. It is easy to leave out because nothing fails until the
crypto store is opened.
