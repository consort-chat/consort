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

**The `-git` suffix.** AUR convention: a package built from the latest VCS
commit takes the suffix and needs a `pkgver()` function so `yay` can tell that
an upgrade exists. The plain `consort` package that builds a tag instead is
`packaging/arch/PKGBUILD`, and would be published as a second AUR repo under
that name. The `provides`/`conflicts` pair here is what stops both installing
at once, and is why that one needs no such pair of its own.

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
than from what Tauri packages usually list, so namcap should be quiet. Two
things it says are expected and are not defects:

- `W: Unused shared library '/usr/lib64/ld-linux-x86-64.so.2'`. rustc lists the
  dynamic loader as an explicit `DT_NEEDED` entry. Every Rust binary on Arch
  produces this warning and there is nothing to change.
- `I: Missing Contributor tag`. There are no previous contributors to credit
  yet.

Anything else it reports is right and the PKGBUILD is wrong. `hicolor-icon-theme`
is in `depends` because namcap asked for it: the package writes into a directory
tree that another package owns.

### If you cannot get root for a chroot

`extra-x86_64-build` needs root, because it builds a container. A weaker but
useful substitute runs makepkg with a `PATH` that has been cut down to only the
binaries a clean chroot would contain, which is what catches the missing
makedepend:

```sh
# Everything base-devel pulls in, plus the declared makedepends, and nothing
# else. Symlink each of those packages' /usr/bin entries into one directory,
# then build with PATH pointing only at it.
PATH=/path/to/that/directory makepkg -f -d
```

This is how `cmake` was ruled out. `aws-lc-sys` carries the `cmake` crate as an
unconditional build dependency, so `cmake v0.1.58` compiles on every build and
it looks like the binary is needed. It is not: on `x86_64-unknown-linux-gnu` the
crate ships pre-generated bindings, `is_bindgen_required()` is false, and the
builder falls through to the `cc` path. Verified by building with no `cmake`,
`clang`, `go` or `nasm` anywhere on `PATH`. What the restricted `PATH` does not
model is a missing *library* or header, so it does not replace the chroot.

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

**`--features custom-protocol` is not optional.** tauri's build script computes
`dev = !custom_protocol`, so without it a `--release` build is still a dev
build: it ignores the frontend embedded at compile time and fetches `devUrl`
(`http://localhost:1420`) at runtime, giving a window that says "Could not
connect to localhost". Nothing static catches this. It compiles, links, has the
right title, icon and `WM_CLASS`, and every `DT_NEEDED` entry resolves. The only
way to see it is to run the binary and look. The tauri CLI passes
`--features tauri/custom-protocol` itself, which is why `tauri build` and the
resulting deb and rpm were never affected, and why only this PKGBUILD needs the
flag.

**`!lto` is required, not a preference.** makepkg's `lto` option puts
`-flto=auto` in `CFLAGS`, the `cc` crate passes it to gcc when building
aws-lc-sys, gcc emits GIMPLE bytecode instead of machine code, and lld cannot
read it. Every `aws_lc_*` symbol then comes out undefined. `lto = "thin"` in the
workspace release profile is untouched by this, so Rust-side LTO still happens.

**`--remap-path-prefix` is set by hand.** Rust puts `file!()` paths in panic and
tracing call sites as `.rodata` string literals, which survive `strip = true`.
Without the remap the package ships around 700 absolute paths naming whoever
built it. makepkg sets this remap only in `DEBUG_RUSTFLAGS`, which `!debug`
disables, so `build()` and `check()` set it themselves. The two must agree
character for character or cargo refingerprints and recompiles everything.
