import { execFileSync } from "node:child_process";
import { ROOT, CARGO_TOML, CARGO_LOCK, SEMVER, read, fail } from "./files";

// Reading the pins

/** The authoritative version: the `[package]` version in the desktop crate. */
export function cargoTomlVersion(): string {
  // Anchored to the first `version = "..."` at column 0, which is the
  // [package] one - dependency versions are all inline in `{ version = ... }`
  // tables or indented, so they cannot match.
  const m = read(CARGO_TOML).match(/^version\s*=\s*"([^"]+)"/m);
  if (!m) fail(`no [package] version found in ${CARGO_TOML}`);
  return m[1];
}

/** The version Cargo.lock records for the desktop crate's own entry. */
function cargoLockVersion(): string {
  const m = read(CARGO_LOCK).match(
    /\[\[package\]\]\nname = "tennoworth-desktop"\nversion = "([^"]+)"/,
  );
  if (!m) fail(`no tennoworth-desktop entry found in ${CARGO_LOCK}`);
  return m[1];
}

/** Every pin, as `{ where, version }`, with Cargo.toml first. */
export function allPins(): { where: string; version: string }[] {
  return [
    { where: CARGO_TOML, version: cargoTomlVersion() },
    { where: CARGO_LOCK, version: cargoLockVersion() },
  ];
}

// ---------------------------------------------------------------------------
// Published history

/**
 * Every published desktop version, newest last.
 *
 * Local tags first, because that works offline and in a full clone. CI checks
 * out at depth 1 with no tags, so fall back to asking the remote rather than
 * making every consumer deepen its checkout. An empty list is not an error -
 * a fork or a fresh clone legitimately has no release history, and a version
 * check that hard-fails there would block contributors over nothing.
 */
export function publishedVersions(): string[] {
  const collect = (out: string) =>
    out
      .split("\n")
      .map((line) => line.trim().split(/\s+/).pop() ?? "")
      .map((ref) => ref.replace(/^refs\/tags\//, "").replace(/\^\{\}$/, ""))
      .filter((tag) => tag.startsWith("desktop-v"))
      .map((tag) => tag.slice("desktop-v".length))
      .filter((v) => SEMVER.test(v));

  const git = (args: string[]) => {
    try {
      // stderr ignored: a missing remote is an expected miss on the way to
      // the next candidate, not something to print git's four-line
      // "Could not read from remote repository" complaint about.
      return execFileSync("git", args, {
        cwd: ROOT,
        encoding: "utf8",
        stdio: ["ignore", "pipe", "ignore"],
      });
    } catch {
      return "";
    }
  };

  let versions = collect(git(["tag", "--list", "desktop-v*"]));
  if (versions.length === 0) {
    // CI checks out at depth 1 with no tags, so ask the remote instead of
    // making every consumer deepen its checkout. `origin` first because that
    // is what actions/checkout sets up; a local clone may instead call the
    // GitHub remote `github` (with `origin` pointing at a private mirror).
    for (const remote of ["origin", "github"]) {
      versions = collect(git(["ls-remote", "--tags", remote, "desktop-v*"]));
      if (versions.length > 0) break;
    }
  }
  return versions.sort(compareVersions);
}

export function compareVersions(a: string, b: string): number {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    if (pa[i] !== pb[i]) return pa[i] - pb[i];
  }
  return 0;
}

// ---------------------------------------------------------------------------
// Release notes

export function nextVersion(current: string, bump: string): string {
  if (SEMVER.test(bump)) return bump;
  const [major, minor, patch] = current.split(".").map(Number);
  // Pre-1.0 policy (stated in full in CHANGELOG.md's header). The minor digit
  // is deliberately expensive: 1.0 has to mean something, so it is not a
  // counter of how much work happened.
  //   patch  the default - fixes, deps, internal work, AND ordinary features
  //          and UI work. A new view is a patch.
  //   minor  only when the product changes shape: a distribution channel
  //          added/removed, a persisted-format or updater change, package
  //          identity, or a compatibility break.
  //   major  1.0 only.
  // Also: a change confined to frontend/ ships to tennoworth.app via
  // continuous deployment and needs no desktop release at all.
  switch (bump) {
    case "major":
      return `${major + 1}.0.0`;
    case "minor":
      return `${major}.${minor + 1}.0`;
    case "patch":
      return `${major}.${minor}.${patch + 1}`;
    default:
      return fail(`"${bump}" is not major, minor, patch, or an X.Y.Z version`);
  }
}

