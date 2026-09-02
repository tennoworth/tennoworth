#!/usr/bin/env bun
// The one thing that writes a version, and the one thing that checks one.
//
// The desktop version used to live in six places kept in step by hand, with a
// release-time guard as the only enforcement. Drift was silent and surfaced on
// user machines: 0.3.3 sat under a 0.3.6 desktop for three releases, and 0.3.5
// and 0.3.6 both shipped a Cargo.lock naming the previous version, which made
// them unbuildable from source (`cargo build --frozen` refuses to rewrite a
// lock).
//
// Two pins remain:
//
//   companion/tennoworth-desktop/Cargo.toml   AUTHORITATIVE. CARGO_PKG_VERSION,
//                                             what the app reports and what the
//                                             updater compares against, and
//                                             (with no `version` in
//                                             tauri.conf.json) what Tauri writes
//                                             into the bundle.
//   companion/Cargo.lock                      derived, machine-written
//
// Usage:
//   bun scripts/release.ts snapshot [--host wfm]
//   bun scripts/release.ts snapshot-check [--release] [--dir <path>]
//   bun scripts/release.ts prepare <major|minor|patch|X.Y.Z>
//   bun scripts/release.ts check [--release X.Y.Z]
//   bun scripts/release.ts notes [X.Y.Z]
import { Buffer } from "node:buffer";
import { execFileSync } from "node:child_process";
import {
  chmodSync,
  existsSync,
  readFileSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, join, resolve } from "node:path";

const ROOT = new URL("..", import.meta.url).pathname.replace(/\/$/, "");

const CARGO_TOML = "companion/tennoworth-desktop/Cargo.toml";
const CARGO_LOCK = "companion/Cargo.lock";
const CHANGELOG = "CHANGELOG.md";
const MARKET_SNAPSHOT = "prototype/public/market.json";
const WFSTAT_CATALOG = "prototype/public/wfstat-catalog.json";
const RELEASE_SNAPSHOT_MAX_AGE_HOURS = 24;
const MAX_SNAPSHOT_FILE_BYTES = 16 * 1024 * 1024;

const REQUIRED_MARKET_KEYS = [
  "updated_at",
  "platform",
  "item_count",
  "catalog_count",
  "source",
  "catalog",
  "items",
  "path_to_info",
  "set_to_parts",
  "relic_rewards",
  "vault_status",
  "baro",
  "surface_fetched_at",
] as const;

// Strict X.Y.Z with no leading zeros. Prerelease and build-metadata suffixes
// are rejected on purpose, not for lack of a regex: the updater has ONE
// endpoint, and semver orders 0.4.0-beta.1 above 0.3.8, so publishing a
// prerelease today would offer it to every stable install. Prereleases unlock
// when a separate beta endpoint exists.
const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;

const read = (rel: string) => readFileSync(join(ROOT, rel), "utf8");
const write = (rel: string, text: string) =>
  writeFileSync(join(ROOT, rel), text);

function fail(message: string): never {
  console.error(`error: ${message}`);
  process.exit(1);
}

type JsonObject = Record<string, unknown>;

export interface SnapshotSummary {
  updatedAt: string;
  itemCount: number;
  catalogCount: number;
  resolverCount: number;
}

function object(value: unknown, name: string): JsonObject {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`${name} must be an object`);
  }
  return value as JsonObject;
}

export function validateSnapshotValues(
  marketValue: unknown,
  resolverValue: unknown,
  options: { maxAgeHours?: number; nowMs?: number } = {},
): SnapshotSummary {
  const market = object(marketValue, "market.json");
  const missing = REQUIRED_MARKET_KEYS.filter((key) => !(key in market));
  if (missing.length > 0) {
    throw new Error(`market.json is missing required keys: ${missing.join(", ")}`);
  }

  const items = object(market.items, "market.json items");
  const catalog = object(market.catalog, "market.json catalog");
  const itemCount = Object.keys(items).length;
  const catalogCount = Object.keys(catalog).length;
  if (itemCount === 0) throw new Error("market.json items are empty");
  if (catalogCount === 0) throw new Error("market.json catalog is empty");
  if (!Number.isInteger(market.item_count) || market.item_count !== itemCount) {
    throw new Error(
      `market.json item_count is ${String(market.item_count)}, but items contains ${itemCount}`,
    );
  }
  if (!Number.isInteger(market.catalog_count) || market.catalog_count !== catalogCount) {
    throw new Error(
      `market.json catalog_count is ${String(market.catalog_count)}, but catalog contains ${catalogCount}`,
    );
  }

  if (!Array.isArray(resolverValue) || resolverValue.length === 0) {
    throw new Error("wfstat-catalog.json must be a non-empty array");
  }
  for (let i = 0; i < resolverValue.length; i++) {
    const entry = resolverValue[i];
    if (
      !Array.isArray(entry) ||
      entry.length !== 2 ||
      typeof entry[0] !== "string" ||
      entry[0].length === 0 ||
      typeof entry[1] !== "object" ||
      entry[1] === null ||
      Array.isArray(entry[1])
    ) {
      throw new Error(`wfstat-catalog.json entry ${i} is not a [path, info] pair`);
    }
  }

  if (typeof market.updated_at !== "string") {
    throw new Error("market.json updated_at must be an ISO timestamp");
  }
  const updatedMs = Date.parse(market.updated_at);
  if (!Number.isFinite(updatedMs)) {
    throw new Error(`market.json updated_at is invalid: ${market.updated_at}`);
  }
  const nowMs = options.nowMs ?? Date.now();
  const ageMs = nowMs - updatedMs;
  if (ageMs < -10 * 60 * 1000) {
    throw new Error(`market.json updated_at is in the future: ${market.updated_at}`);
  }
  if (options.maxAgeHours !== undefined && ageMs > options.maxAgeHours * 60 * 60 * 1000) {
    throw new Error(
      `market.json is ${(ageMs / 3_600_000).toFixed(1)} h old; release limit is ${options.maxAgeHours} h. ` +
        "Run `bun scripts/release.ts snapshot` during release preparation.",
    );
  }

  return {
    updatedAt: market.updated_at,
    itemCount,
    catalogCount,
    resolverCount: resolverValue.length,
  };
}

function parseJson(path: string): unknown {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    throw new Error(`${path} is not valid JSON: ${(error as Error).message}`);
  }
}

function validateSnapshotDirectory(dir: string, release: boolean): SnapshotSummary {
  return validateSnapshotValues(
    parseJson(join(dir, "market.json")),
    parseJson(join(dir, "wfstat-catalog.json")),
    release ? { maxAgeHours: RELEASE_SNAPSHOT_MAX_AGE_HOURS } : {},
  );
}

function printSnapshotSummary(summary: SnapshotSummary) {
  console.log(
    `snapshot ${summary.updatedAt}: ${summary.itemCount} items, ` +
      `${summary.catalogCount} market names, ${summary.resolverCount} resolver paths`,
  );
}

function cmdSnapshotCheck(argv: string[]) {
  let release = false;
  let dir = join(ROOT, "prototype/public");
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--release") {
      release = true;
    } else if (argv[i] === "--dir") {
      dir = resolve(ROOT, argv[++i] ?? fail("--dir needs a path"));
    } else {
      fail(`unknown snapshot-check flag: ${argv[i]}`);
    }
  }

  try {
    printSnapshotSummary(validateSnapshotDirectory(dir, release));
  } catch (error) {
    fail((error as Error).message);
  }
}

const REMOTE_SNAPSHOT_SCRIPT = `set -euo pipefail
APP=/srv/wfm/app
exec 9<"$APP"
if ! flock -n 9; then
  echo "ABORT: wfm-scrape currently owns the snapshot lock; retry after it finishes." >&2
  exit 75
fi
state=$(systemctl show wfm-scrape.service -p ActiveState --value 2>/dev/null || true)
case "\${state:-unknown}" in
  inactive|failed) ;;
  *) echo "ABORT: wfm-scrape.service is \${state:-unknown}; retry after it finishes." >&2; exit 75 ;;
esac
cd "$APP/prototype/public"
market_size=$(stat -c %s market.json)
resolver_size=$(stat -c %s wfstat-catalog.json)
printf '%s\\n%s\\n' "$market_size" "$resolver_size"
cat market.json wfstat-catalog.json
`;

export function splitSnapshotFrame(frame: Buffer): { market: Buffer; resolver: Buffer } {
  const firstNewline = frame.indexOf(0x0a);
  const secondNewline = firstNewline === -1 ? -1 : frame.indexOf(0x0a, firstNewline + 1);
  if (firstNewline < 1 || secondNewline < firstNewline + 2) {
    throw new Error("production returned an invalid snapshot frame header");
  }
  const headers = [frame.subarray(0, firstNewline), frame.subarray(firstNewline + 1, secondNewline)]
    .map((part) => part.toString("ascii"));
  if (headers.some((header) => !/^\d+$/.test(header))) {
    throw new Error(`production returned invalid snapshot sizes: ${headers.join(", ")}`);
  }
  const sizes = headers.map(Number);
  if (sizes.some((size) => !Number.isSafeInteger(size) || size <= 0 || size > MAX_SNAPSHOT_FILE_BYTES)) {
    throw new Error(`production returned invalid snapshot sizes: ${sizes.join(", ")}`);
  }
  const payloadAt = secondNewline + 1;
  if (frame.length !== payloadAt + sizes[0] + sizes[1]) {
    throw new Error("production snapshot frame was truncated or had trailing data");
  }
  return {
    market: frame.subarray(payloadAt, payloadAt + sizes[0]),
    resolver: frame.subarray(payloadAt + sizes[0]),
  };
}

function atomicReplace(path: string, contents: Buffer) {
  const temporary = join(dirname(path), `.${basename(path)}.${process.pid}.tmp`);
  try {
    writeFileSync(temporary, contents);
    chmodSync(temporary, statSync(path).mode & 0o777);
    renameSync(temporary, path);
  } finally {
    rmSync(temporary, { force: true });
  }
}

function cmdSnapshot(argv: string[]) {
  let host = "wfm";
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--host") {
      host = argv[++i] ?? fail("--host needs an SSH host or alias");
    } else {
      fail(`unknown snapshot flag: ${argv[i]}`);
    }
  }
  if (!/^[A-Za-z0-9_.@:-]+$/.test(host) || host.startsWith("-")) {
    fail(`invalid SSH host: ${host}`);
  }

  const tracked = [MARKET_SNAPSHOT, WFSTAT_CATALOG];
  const dirty = execFileSync("git", ["status", "--porcelain=v1", "--", ...tracked], {
    cwd: ROOT,
    encoding: "utf8",
  }).trim();
  if (dirty) {
    fail(`refusing to overwrite locally modified snapshot files:\n${dirty}`);
  }

  let frame: Buffer;
  try {
    frame = execFileSync(
      "ssh",
      ["-o", "BatchMode=yes", host, "bash", "-s"],
      {
        input: REMOTE_SNAPSHOT_SCRIPT,
        maxBuffer: 2 * MAX_SNAPSHOT_FILE_BYTES + 1024,
        stdio: ["pipe", "pipe", "inherit"],
      },
    );
  } catch (error) {
    const status = (error as { status?: number }).status;
    if (status === 75) fail("production scrape is active; retry when it finishes");
    fail(`could not copy the production snapshot from ${host}`);
  }

  let parts: { market: Buffer; resolver: Buffer };
  try {
    parts = splitSnapshotFrame(frame);
    const summary = validateSnapshotValues(
      JSON.parse(parts.market.toString("utf8")),
      JSON.parse(parts.resolver.toString("utf8")),
      { maxAgeHours: RELEASE_SNAPSHOT_MAX_AGE_HOURS },
    );
    printSnapshotSummary(summary);
  } catch (error) {
    fail(`production snapshot rejected: ${(error as Error).message}`);
  }

  // Match the generator's publication order: catalog first, market.json last
  // as the anchor that says the pair is ready.
  const marketPath = join(ROOT, MARKET_SNAPSHOT);
  const resolverPath = join(ROOT, WFSTAT_CATALOG);
  const originalMarket = readFileSync(marketPath);
  const originalResolver = readFileSync(resolverPath);
  try {
    atomicReplace(resolverPath, parts.resolver);
    atomicReplace(marketPath, parts.market);
  } catch (error) {
    try {
      atomicReplace(resolverPath, originalResolver);
      atomicReplace(marketPath, originalMarket);
    } catch (rollbackError) {
      fail(
        `snapshot install failed (${(error as Error).message}) and rollback failed: ` +
          (rollbackError as Error).message,
      );
    }
    fail(`snapshot install failed; restored previous pair: ${(error as Error).message}`);
  }
  console.log(`copied ${MARKET_SNAPSHOT} and ${WFSTAT_CATALOG} from ${host}`);
  console.log("include both files in the desktop release-preparation commit");
}

// ---------------------------------------------------------------------------
// Reading the pins

/** The authoritative version: the `[package]` version in the desktop crate. */
function cargoTomlVersion(): string {
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
function allPins(): { where: string; version: string }[] {
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
function publishedVersions(): string[] {
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

function compareVersions(a: string, b: string): number {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    if (pa[i] !== pb[i]) return pa[i] - pb[i];
  }
  return 0;
}

// ---------------------------------------------------------------------------
// check

function cmdCheck(argv: string[]) {
  let expected: string | null = null;
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--release") {
      expected = argv[++i] ?? fail("--release needs a version");
    } else {
      fail(`unknown flag: ${argv[i]}`);
    }
  }

  const pins = allPins();
  const version = pins[0].version;
  let bad = false;

  if (!SEMVER.test(version)) {
    console.error(
      `${CARGO_TOML} version "${version}" is not a strict X.Y.Z semver. ` +
        `Prerelease suffixes are not supported until a separate beta updater endpoint exists.`,
    );
    bad = true;
  }

  for (const pin of pins.slice(1)) {
    if (pin.version !== version) {
      console.error(
        `version drift: ${pin.where} says ${pin.version}, ${CARGO_TOML} says ${version}. ` +
          `Run: bun scripts/release.ts prepare ${version}`,
      );
      bad = true;
    }
  }

  if (expected !== null && expected !== version) {
    console.error(
      `this run was asked to release ${expected}, but the repo is pinned at ${version}. ` +
        `Bump every pin in one commit, on main, before dispatching.`,
    );
    bad = true;
  }

  const published = publishedVersions();
  if (published.length === 0) {
    if (expected !== null) {
      // At release time an empty list is far more likely a failed
      // `ls-remote` than a repo with no releases; proceeding would silently
      // drop the monotonicity guard for exactly the run that needs it.
      console.error(
        "no published desktop-v* releases visible. On a PR that is fine; for a release it " +
          "means the tag lookup failed (or this really is the first release - then tag " +
          "desktop-v0.0.0 on the initial commit to seed the history).",
      );
      bad = true;
    } else {
      console.log("no published desktop-v* releases visible - skipping the history check.");
    }
  } else {
    const newest = published[published.length - 1];
    const cmp = compareVersions(version, newest);
    // On a PR the repo legitimately sits AT the newest published version
    // between releases, so only "behind" is drift. `--release` is the release
    // run itself, where equal is also wrong: the updater only ever offers a
    // strictly greater version, so republishing produces a release nobody is
    // offered.
    if (cmp < 0) {
      console.error(
        `${version} is BEHIND the newest published release ${newest}. ` +
          `A release cut from here would never be offered to anyone.`,
      );
      bad = true;
    } else if (expected !== null && cmp === 0) {
      console.error(
        `${version} is already published. The updater only offers a strictly greater version.`,
      );
      bad = true;
    } else {
      console.log(`newest published release: ${newest}`);
    }
  }

  if (bad) process.exit(1);
  console.log(
    `version ${version} agrees across ${pins.length} pins` +
      (expected ? ` and matches the requested release` : "") +
      ".",
  );
}

// ---------------------------------------------------------------------------
// prepare

function nextVersion(current: string, bump: string): string {
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
  // Also: a change confined to prototype/ ships to tennoworth.app via
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

function cmdPrepare(argv: string[]) {
  const bump = argv[0] ?? fail("prepare needs <major|minor|patch|X.Y.Z>");
  try {
    printSnapshotSummary(
      validateSnapshotDirectory(join(ROOT, "prototype/public"), true),
    );
  } catch (error) {
    fail((error as Error).message);
  }
  const current = cargoTomlVersion();
  const next = nextVersion(current, bump);
  if (!SEMVER.test(next)) fail(`"${next}" is not a strict X.Y.Z semver`);
  if (compareVersions(next, current) <= 0) {
    fail(`${next} is not greater than the current ${current}`);
  }

  // Cargo.toml - the [package] version only. The replacement is anchored the
  // same way the reader is, so a dependency's version can never be hit.
  write(
    CARGO_TOML,
    read(CARGO_TOML).replace(/^version\s*=\s*"[^"]+"/m, `version = "${next}"`),
  );
  console.log(`${CARGO_TOML}: ${current} -> ${next}`);

  // Cargo.lock - `cargo update --workspace` rewrites only the workspace
  // members' own entries, leaving every dependency resolution alone. This is
  // the step that was missing when 0.3.5 and 0.3.6 shipped a lock naming the
  // previous version, which any --frozen build from source refuses.
  console.log("refreshing Cargo.lock (cargo update --workspace)…");
  execFileSync("cargo", ["update", "--workspace"], {
    cwd: join(ROOT, "companion"),
    stdio: "inherit",
  });

  // CHANGELOG section, opened for the human to fill in. `notes` reads it back
  // for the release body, so an empty section is a visible reminder rather
  // than a silent omission.
  const today = new Date().toISOString().slice(0, 10);
  const section = `## ${next} - ${today}\n\n- \n\n`;
  if (!existsSync(join(ROOT, CHANGELOG))) {
    write(
      CHANGELOG,
      `# Changelog\n\nDesktop releases. Versions are \`desktop-v<version>\` tags. Pre-1.0: patch =\n` +
        `fixes, dependency bumps and internal work; minor = user-facing features, new\n` +
        `distribution channels and compatibility breaks; major is reserved for 1.0.\n\n` +
        section,
    );
  } else {
    const existing = read(CHANGELOG);
    const firstSection = existing.indexOf("\n## ");
    const at = firstSection === -1 ? existing.length : firstSection + 1;
    // Appending after prose (no section yet) needs a separating blank line;
    // inserting before an existing section already sits on one.
    let head = existing.slice(0, at);
    if (firstSection === -1 && !head.endsWith("\n\n")) {
      head = head.replace(/\n*$/, "\n\n");
    }
    write(CHANGELOG, head + section + existing.slice(at));
  }
  console.log(`${CHANGELOG}: opened a section for ${next} - fill it in.`);

  console.log(
    `\nDone. Review the diff, write the changelog entry, and commit all of it ` +
      `together:\n  git add -A && git commit -m "desktop ${next}"\n` +
      `Open the release-preparation PR into develop, then promote it to main ` +
      `and dispatch release-desktop with version=${next}.`,
  );
}

// ---------------------------------------------------------------------------
// notes

function cmdNotes(argv: string[]) {
  const version = argv[0] ?? cargoTomlVersion();
  if (!existsSync(join(ROOT, CHANGELOG))) {
    fail(`${CHANGELOG} does not exist - run \`prepare\` first`);
  }
  const lines = read(CHANGELOG).split("\n");
  const start = lines.findIndex((l) => l.startsWith(`## ${version} `) || l.trim() === `## ${version}`);
  if (start === -1) fail(`${CHANGELOG} has no section for ${version}`);
  const rest = lines.slice(start + 1);
  const end = rest.findIndex((l) => l.startsWith("## "));
  const body = (end === -1 ? rest : rest.slice(0, end)).join("\n").trim();
  if (!body) fail(`the ${version} section in ${CHANGELOG} is empty`);
  console.log(body);
}

// ---------------------------------------------------------------------------

if (import.meta.main) {
  const [command, ...rest] = process.argv.slice(2);
  switch (command) {
    case "snapshot":
      cmdSnapshot(rest);
      break;
    case "snapshot-check":
      cmdSnapshotCheck(rest);
      break;
    case "prepare":
      cmdPrepare(rest);
      break;
    case "check":
      cmdCheck(rest);
      break;
    case "notes":
      cmdNotes(rest);
      break;
    default:
      console.error(
        "usage:\n" +
          "  bun scripts/release.ts snapshot [--host wfm]\n" +
          "  bun scripts/release.ts snapshot-check [--release] [--dir <path>]\n" +
          "  bun scripts/release.ts prepare <major|minor|patch|X.Y.Z>\n" +
          "  bun scripts/release.ts check [--release X.Y.Z]\n" +
          "  bun scripts/release.ts notes [X.Y.Z]",
      );
      process.exit(1);
  }
}
