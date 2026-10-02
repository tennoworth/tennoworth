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
//   rust/tennoworth-desktop/Cargo.toml   AUTHORITATIVE. CARGO_PKG_VERSION,
//                                             what the app reports and what the
//                                             updater compares against, and
//                                             (with no `version` in
//                                             tauri.conf.json) what Tauri writes
//                                             into the bundle.
//   rust/Cargo.lock                      derived, machine-written
//
// Usage:
//   bun scripts/release.ts snapshot [--host wfm]
//   bun scripts/release.ts snapshot-check [--release] [--dir <path>]
//   bun scripts/release.ts prepare <major|minor|patch|X.Y.Z>
//   bun scripts/release.ts check [--release X.Y.Z]
//   bun scripts/release.ts notes [--release] [X.Y.Z]
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { ROOT, CARGO_TOML, CHANGELOG, SEMVER, read, write, fail } from "./release/files";
import { cmdSnapshot, cmdSnapshotCheck, validateSnapshotDirectory, printSnapshotSummary } from "./release/snapshot";
import { allPins, cargoTomlVersion, publishedVersions, compareVersions, nextVersion } from "./release/versions";
import { APP_NOTES_SINCE, appNotesCatalog, syncAppNotes, releaseNotesBody, releaseNotesTemplate, validateReleaseNotes, type ReleaseNotesSummary } from "./release/notes";

export { splitSnapshotFrame, validateSnapshotValues, type SnapshotSummary } from "./release/snapshot";
export { appNotesCatalog, releaseNotesBody, releaseNotesTemplate, validateReleaseNotes, type ReleaseNotesSummary } from "./release/notes";

function checkedReleaseNotes(version: string): { body: string; summary: ReleaseNotesSummary } {
  const body = releaseNotesBody(read(CHANGELOG), version);
  syncAppNotes(true);
  return { body: body.replace(/ <!-- app-note (\{[^\n]+\}) -->$/gm, ""), summary: validateReleaseNotes(body, version) };
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
  let notesNeedValidation = expected !== null;

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
      if (cmp > 0) notesNeedValidation = true;
    }
  }

  if (notesNeedValidation) {
    try {
      const { summary } = checkedReleaseNotes(version);
      console.log(
        `release notes for ${version}: ${summary.icon} ${summary.changeCount} changes ` +
          `across ${summary.categoryCount} categories`,
      );
    } catch (error) {
      console.error(`release notes for ${version}: ${(error as Error).message}`);
      bad = true;
    }
  }

  try {
    syncAppNotes(true);
    const catalog = appNotesCatalog(read(CHANGELOG), version);
    for (const released of published) {
      if (compareVersions(released, APP_NOTES_SINCE) > 0 && compareVersions(released, version) <= 0 && !catalog.releases.some(r => r.version === released)) throw new Error(`published release ${released} is missing from bundled app notes`);
    }
  } catch (error) { console.error((error as Error).message); bad = true; }
  if (bad) process.exit(1);
  console.log(
    `version ${version} agrees across ${pins.length} pins` +
      (expected ? ` and matches the requested release` : "") +
      ".",
  );
}

// ---------------------------------------------------------------------------
// prepare

function cmdPrepare(argv: string[]) {
  const bump = argv[0] ?? fail("prepare needs <major|minor|patch|X.Y.Z>");
  try {
    printSnapshotSummary(
      validateSnapshotDirectory(join(ROOT, "frontend/public"), true),
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
    cwd: join(ROOT, "rust"),
    stdio: "inherit",
  });

  // Deliberately invalid placeholders make release-note authors choose the
  // contextual icon, summary, categories and honest change count. Both the PR
  // gate and release preflight reject the scaffold until it is complete.
  const today = new Date().toISOString().slice(0, 10);
  const section = releaseNotesTemplate(next, today);
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
  console.log(
    `${CHANGELOG}: opened the structured section for ${next} - replace every placeholder.`,
  );

  console.log(
    `\nDone. Review the diff, write the changelog entry, and commit all of it ` +
      `together:\n  git add -A && git commit -m "desktop ${next}"\n` +
      `Open the release-preparation PR into develop, then promote it to main ` +
      `and dispatch release-desktop with version=${next} and expected_sha set to the full approved promotion commit.`,
  );
}

// ---------------------------------------------------------------------------
// notes

function cmdNotes(argv: string[]) {
  let release = false;
  let version: string | null = null;
  for (const arg of argv) {
    if (arg === "--release") {
      release = true;
    } else if (version === null) {
      version = arg;
    } else {
      fail(`unexpected notes argument: ${arg}`);
    }
  }
  version ??= cargoTomlVersion();
  if (!existsSync(join(ROOT, CHANGELOG))) {
    fail(`${CHANGELOG} does not exist - run \`prepare\` first`);
  }
  let body: string;
  try {
    body = release
      ? checkedReleaseNotes(version).body
      : releaseNotesBody(read(CHANGELOG), version);
  } catch (error) {
    fail((error as Error).message);
  }
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
    case "app-notes":
      try { syncAppNotes(rest.includes("--check")); } catch (error) { fail((error as Error).message); }
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
          "  bun scripts/release.ts notes [--release] [X.Y.Z]\n" +
          "  bun scripts/release.ts app-notes [--check]",
      );
      process.exit(1);
  }
}
