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

// New releases number their patch X.Y.100 through X.Y.999, so a minor line has
// room for hundreds of small releases before the deliberately expensive minor
// bump. Published history from before the scheme (0.7.9, 0.8.7) stays valid
// strict X.Y.Z: only a version being prepared or released is held to the
// range. 0.8.7 -> 0.8.100 relies on every ordering being numeric, never
// lexical - here, in the desktop crate's update notes and in Tauri's updater.
const FIRST_PATCH = 100;
const LAST_PATCH = 999;

/** Why `version` cannot be prepared or released, or null when it can. */
export function releaseVersionProblem(version: string): string | null {
  if (!SEMVER.test(version)) return `"${version}" is not a strict X.Y.Z version`;
  const patch = Number(version.split(".")[2]);
  if (patch < FIRST_PATCH || patch > LAST_PATCH) {
    return (
      `${version} does not have a three-digit patch: new releases are ` +
      `X.Y.${FIRST_PATCH} to X.Y.${LAST_PATCH}, and X.Y.${LAST_PATCH} is followed by a minor bump`
    );
  }
  return null;
}

export function nextVersion(current: string, bump: string): string {
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
  let next: string;
  if (SEMVER.test(bump)) {
    next = bump;
  } else if (bump === "major") {
    next = `${major + 1}.0.${FIRST_PATCH}`;
  } else if (bump === "minor") {
    next = `${major}.${minor + 1}.${FIRST_PATCH}`;
  } else if (bump === "patch") {
    if (patch >= LAST_PATCH) {
      throw new Error(`${current} is the last patch of ${major}.${minor}; prepare minor instead`);
    }
    // A pre-scheme patch moves to the first three-digit one, not to X.Y.8.
    next = `${major}.${minor}.${Math.max(patch + 1, FIRST_PATCH)}`;
  } else {
    throw new Error(`"${bump}" is not major, minor, patch, or an X.Y.Z version`);
  }
  const problem = releaseVersionProblem(next);
  if (problem) throw new Error(problem);
  if (compareVersions(next, current) <= 0) {
    throw new Error(`${next} is not greater than the current ${current}`);
  }
  return next;
}

