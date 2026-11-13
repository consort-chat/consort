# Continuous integration

The workflows live in `.github/workflows/`. `ci.yml` is the checks that run on
every push and pull request. `release.yml` is what the Release button does and
calls the two package workflows once a tag has a release page. `package-deb.yml`
builds the Debian and Ubuntu package and `package-arch.yml` builds the Arch one.

The comments in those files carry a line each. The reasons are here.

## Why it runs on GitHub and not Forgejo

It used to be `.forgejo/workflows/ci.yml`, and Forgejo was the place pull
requests arrived. GitHub is now that place, and it bills nothing for standard
runners on a public repository, so there is no reason to run the same jobs
twice. The Forgejo copy is gone rather than kept in step, and the mirror no
longer runs CI: it falls back to reading `.github/workflows/`, finds
`runs-on: ubuntu-latest`, and has no runner carrying that label.

The free tier is only the standard runners. A larger runner is billed on a
public repository like any other, so nothing may move to one.

## Who can make this run

GitHub runs workflows for a first-time contributor's pull request only if the
repository says so, and the default is "Require approval for first-time
contributors". Leave it there or tighten it. The setting is under Actions in the
repository settings, and it is the equivalent of the approval Forgejo used to
give.

`pull_request_target` is deliberately not used in any workflow here and must not
be added. A workflow using it runs an untrusted branch with repository secrets
in scope.

## The glibc floor, and the container images

The glibc a package links against is the oldest one it will run on.
`ubuntu-latest` is noble, so a package built there needs glibc 2.39. It installs
on Debian 12 and then dies at startup with a version error, which is worse than
not being offered. Debian 12 is glibc 2.36 and carries webkit2gtk-4.1, so a
bookworm container covers Debian 12 and 13 and Ubuntu 24.04 onwards from one
artefact.

Ubuntu 22.04 is out of reach either way: it is glibc 2.35, older than anything
with webkit2gtk-4.1 in it, and 4.0 is not a version Tauri 2 builds against.

Two things follow from it that tie the workflows together:

- It is also what picks the bookworm image for `ci.yml`'s Rust job. There,
  `webrtc-sys` 0.3.51 refuses anything below clang 21 while bookworm ships 14,
  which is why that job installs clang-21 from apt.llvm.org.
- The .deb's system dependency list is `ci.yml`'s Rust job's list, which is what
  makes it known to compile rather than guessed.

The Arch package needs that same clang and is the one job with nowhere to put
an apt source. It gets it from `packaging/arch/PKGBUILD`'s `makedepends`, which
`makepkg --syncdeps` installs before `build()` runs, and Arch's clang is far
past 21. That keeps the floor in the recipe rather than in the workflow, so
somebody building the package by hand gets it too. The `-git` recipe in
`packaging/aur/` carries the same entry and has to be pushed to the AUR
separately.

## The Rust job's system dependencies

webkit2gtk and friends are needed to compile the Tauri crate at all, not just to
run it. libssl and pkg-config are for the C half of aws-lc-sys.
libasound2-dev is cpal's, and is the one that is easy to leave off: every
desktop Linux has the runtime library already, so nothing notices until a
machine that only builds tries to.

Both clangs are installed. clang-21 compiles libwebrtc's C++, and the
unversioned one is what bindgen finds a libclang through.

## One zbus, without its tokio feature

One zbus in the graph serves both Secret Service and whatever else pulls it in.
Its `tokio` feature switches the blocking API over to `block_on`, and the
keyring is read from inside an async command, so turning it on panics at startup
with "Cannot start a runtime from within a runtime". Nothing catches that but
running the application, which is why a CI step checks the feature tree.

The step resolves the graph into a variable rather than piping straight into
grep, so a cargo that failed to answer fails the step instead of reading as a
clean graph.
