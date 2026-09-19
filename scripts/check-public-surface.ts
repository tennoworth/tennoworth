#!/usr/bin/env bun
// Gate for the public surface of the repository.
//
// The GitHub repo is source, user documentation, packaging recipes and CI.
// Research notes, findings, investigations, audits, spike write-ups, plans and
// maintainer/ops runbooks are maintainer-local: `/.planning/` is their single
// home and `.gitignore` keeps it out of git. AGENTS.md states the rule; this is
// what makes it fail closed, because a rule with no gate is a comment.
//
// WHAT IT VALIDATES, AND AGAINST WHAT
//   File list and contents come from the git index (`git ls-files`,
//   `git show :<path>`), so it checks what a checkout receives, not what an
//   editor currently holds. A `git add -f` past the ignore rules is exactly the
//   failure this catches, and `.gitignore` alone cannot see it.
//
// WHAT IT DOES NOT CHECK
//   * Whether a document reads as research. A note deliberately published as
//     user documentation is user documentation; this checks where maintainer
//     material lives, not its vocabulary. Do not add a filename denylist here:
//     `docs/rust-toolkit-2026-lessons.md` is tracked, public and linked.
//   * Binary assets and data files under docs/ (screenshots, ruleset JSON).
//
// When this fails, the fix is not to weaken it. A tracked document belongs to
// the public surface: reference it from README, AGENTS.md, CONTRIBUTING.md or
// another published document. Material only the maintainer needs moves to
// `.planning/` and leaves git.
//
// Usage: bun scripts/check-public-surface.ts   (from the repository root)
import { posix, resolve } from "node:path";

const ROOT = resolve(import.meta.dir, "..");
const INTERNAL_HOMES = [".planning/", ".research/", ".mockups/", "docs/mockups/"];
const problems: string[] = [];

function git(args: string[]) {
  return Bun.spawnSync(["git", ...args], { cwd: ROOT });
}

const listed = git(["ls-files", "-z"]);
if (listed.exitCode !== 0) {
  console.error("check-public-surface: git ls-files failed; run from a git checkout");
  process.exit(2);
}
const tracked = listed.stdout.toString().split("\0").filter(Boolean);

// 1. Nothing under a maintainer-local home is tracked.
for (const path of tracked) {
  const home = INTERNAL_HOMES.find((candidate) => path.startsWith(candidate));
  if (home) {
    problems.push(
      `${path} is tracked, but ${home} is maintainer-local and gitignored: ` +
        `keep the file where it is and remove it from git (git rm --cached ${path})`,
    );
  }
}

// 2. Every tracked document under docs/ is referenced from outside docs/, so a
//    document cannot be added without becoming part of the published surface.
//    A nested document may be referenced by its directory instead. A reference
//    from another document does not count - that is the shape a leak takes.
const outside = tracked.filter((path) => !path.startsWith("docs/") && path.endsWith(".md"));
const outsideText = outside
  .map((path) => git(["show", `:${path}`]).stdout.toString())
  .join("\n");

const documents = tracked.filter((path) => path.startsWith("docs/") && path.endsWith(".md"));
for (const doc of documents) {
  const dir = posix.dirname(doc);
  const referenced =
    outsideText.includes(doc) || (dir !== "docs" && outsideText.includes(`${dir}/`));
  if (!referenced) {
    problems.push(
      `${doc} is tracked but nothing outside docs/ references it: publish it from ` +
        `README, AGENTS.md or CONTRIBUTING.md, or move it to .planning/`,
    );
  }
}

if (problems.length > 0) {
  console.error("");
  for (const problem of problems) console.error(`  ${problem}`);
  console.error(`\ncheck-public-surface: ${problems.length} problem(s)`);
  process.exit(1);
}

console.log(
  `check-public-surface: ok (${documents.length} tracked documents, ${tracked.length} tracked files)`,
);
