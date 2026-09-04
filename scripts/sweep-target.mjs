#!/usr/bin/env node

// Keeps the Cargo target directory bounded. Runs ahead of every `pnpm tauri`.
//
// Cargo never deletes anything. Every artifact is hash-suffixed by its
// fingerprint, so a dependency bump, a feature change or a rustflag edit
// orphans the previous set rather than replacing it. One generation of
// `debug/deps` on this workspace is roughly 20 GB, and left alone the
// directory reached 184 GB in nine days. Issue #48 has the measurements.
//
// Three things accumulate, and no single tool addresses all three:
//
//   deps/ and build/   `cargo sweep --maxsize` evicts oldest first, so the
//                      generation currently being linked against survives and
//                      the abandoned ones go.
//
//   incremental/       Nothing collects this. Cargo prunes old sessions inside
//                      a crate's directory but orphans the whole directory when
//                      the crate's fingerprint changes, and cargo-sweep walks
//                      straight past it: sweeping a throwaway crate to a 1 MB
//                      cap emptied deps and left incremental at its full size.
//                      Pruned here by age. A rebuild rewrites the crate's
//                      session directory, so the mtime tracks real use, and a
//                      directory untouched for days belongs to a fingerprint
//                      nothing builds any more.
//
//   llvm-cov-target/   A second, instrumented target directory that coverage
//                      runs never clean up after. Same age rule.
//
// Nothing here may break the dev loop, so every failure is reported and
// swallowed: a missing cargo-sweep, an unreadable directory, a sweep that
// exits non-zero. The build that follows does not depend on any of it, which
// is also what lets CI run this unchanged with cargo-sweep absent.

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = fileURLToPath(new URL("..", import.meta.url));

// The cap covers the whole directory, which `~/.cargo/config.toml` may point
// somewhere shared with other Rust projects. Everything under it is
// reproducible, so evicting another project's stale artifacts costs a rebuild
// and nothing else.
const maxSize = process.env.CONSORT_TARGET_MAX_SIZE ?? "30GB";
const maxAgeDays = Number(process.env.CONSORT_TARGET_MAX_AGE_DAYS ?? "7");

// Sweeping stats every file in the directory, which is seconds of work at the
// sizes this exists to prevent. Once a day is enough to hold a ceiling, and it
// keeps the common case, restarting the dev build, free.
const intervalHours = Number(process.env.CONSORT_SWEEP_INTERVAL_HOURS ?? "12");

const forced = process.argv.includes("--force");

function targetDirectory() {
  // `--offline` because the dev loop must not wait on the network to find out
  // where its own build output goes, and the matrix-sdk pin is a git
  // dependency that cargo would otherwise be entitled to check.
  const metadata = execFileSync(
    "cargo",
    [
      "metadata",
      "--format-version",
      "1",
      "--no-deps",
      "--offline",
      "--manifest-path",
      join(repoRoot, "Cargo.toml"),
    ],
    { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] },
  );
  return JSON.parse(metadata).target_directory;
}

function sweptRecently(stamp) {
  if (forced || !existsSync(stamp)) return false;
  return Date.now() - statSync(stamp).mtimeMs < intervalHours * 3_600_000;
}

function haveCargoSweep() {
  try {
    execFileSync("cargo", ["sweep", "--version"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

function pruneChildrenOlderThan(directory, cutoff) {
  if (!existsSync(directory)) return 0;
  let removed = 0;
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const path = join(directory, entry.name);
    if (statSync(path).mtimeMs >= cutoff) continue;
    rmSync(path, { recursive: true, force: true });
    removed += 1;
  }
  return removed;
}

// A coverage run rewrites artifacts inside the directory without touching the
// directory itself, so its own mtime says nothing. The newest child is what
// answers whether anybody has asked for coverage lately.
function newestChildMtime(directory) {
  let newest = statSync(directory).mtimeMs;
  for (const entry of readdirSync(directory)) {
    newest = Math.max(newest, statSync(join(directory, entry)).mtimeMs);
  }
  return newest;
}

try {
  const target = targetDirectory();

  // A first build has nothing to sweep, and writing the stamp would be the
  // thing that created the directory.
  if (existsSync(target)) {
    const stamp = join(target, ".consort-sweep-stamp");

    if (!sweptRecently(stamp)) {
      const cutoff = Date.now() - maxAgeDays * 86_400_000;

      let pruned = 0;
      for (const profile of ["debug", "release"]) {
        pruned += pruneChildrenOlderThan(join(target, profile, "incremental"), cutoff);
      }

      const coverage = join(target, "llvm-cov-target");
      if (existsSync(coverage) && newestChildMtime(coverage) < cutoff) {
        rmSync(coverage, { recursive: true, force: true });
        console.log("[sweep-target] dropped an unused coverage target directory");
      }

      if (pruned > 0) {
        console.log(`[sweep-target] pruned ${pruned} stale incremental directories`);
      }

      if (haveCargoSweep()) {
        execFileSync("cargo", ["sweep", "--maxsize", maxSize, repoRoot], { stdio: "inherit" });
      } else {
        console.log(
          "[sweep-target] cargo-sweep is not installed, so deps/ and build/ are uncapped. " +
            "`cargo install cargo-sweep` to fix that.",
        );
      }

      writeFileSync(stamp, "");
    }
  }
} catch (error) {
  console.warn(`[sweep-target] skipped: ${error.message}`);
}
