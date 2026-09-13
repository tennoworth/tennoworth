#!/usr/bin/env bun
// Gate for the agent instruction files. They are tracked policy with no
// compiler behind them, so the failures worth catching mechanically are the
// ones that rot silently: a link to a file that moved or was never committed,
// and a per-domain file that nothing links to any more.
//
// WHAT IT VALIDATES, AND AGAINST WHAT
//   File list, file contents and byte sizes all come from the git index
//   (`git ls-files`, `git show :<path>`), so this checks what a checkout would
//   actually receive. An unstaged edit is invisible here on purpose - that is
//   what makes it a publication check rather than an editor lint. A link is
//   only satisfied by a *tracked* target, so an untracked file cannot make a
//   link that would break in a clean checkout look healthy.
//
// WHAT IT DOES NOT CHECK
//   * `#fragments` are not resolved against headings.
//   * Backticked text is never treated as a path. These files name commands,
//     globs, URLs, environment variables and placeholders in backticks, and
//     reading those as paths produces false failures.
//   * Content is not checked for semantic preservation, correct scope, or
//     privacy.
//
// Link syntax understood: inline `[text](path)` and reference definitions
// `[label]: path`, with an optional title, outside fenced code blocks.
//
// Usage: bun scripts/check-agent-instructions.ts   (from the repository root)
import { posix, resolve } from "node:path";

const ROOT = resolve(import.meta.dir, "..");
// The root sat at 24.5 KB before this restructuring and 16 KB after it, so warn
// with headroom rather than on every run - an always-on warning gets ignored.
const WARN_BYTES = 20_000;
const ROOT_FILE = "AGENTS.md";

const problems: string[] = [];

function git(args: string[]) {
  return Bun.spawnSync(["git", ...args], { cwd: ROOT });
}

const listed = git(["ls-files", "-z"]);
if (listed.exitCode !== 0) {
  console.error("check-agent-instructions: git ls-files failed; run from a git checkout");
  process.exit(2);
}
const tracked = new Set(listed.stdout.toString().split("\0").filter(Boolean));

function trackedContent(path: string): string | null {
  const shown = git(["show", `:${path}`]);
  return shown.exitCode === 0 ? shown.stdout.toString() : null;
}

const isInstructionFile = (path: string) => posix.basename(path) === "AGENTS.md";

// Git tracks files, not directories, so a link to a directory has to be
// accepted when that directory contains tracked files - GitHub renders it.
const trackedDirs = new Set<string>();
for (const path of tracked) {
  for (let dir = posix.dirname(path); dir !== "." && dir !== "/"; dir = posix.dirname(dir)) {
    trackedDirs.add(dir);
  }
}
const resolvesToTrackedPath = (path: string) => tracked.has(path) || trackedDirs.has(path);

if (!tracked.has(ROOT_FILE)) {
  console.error(`check-agent-instructions: ${ROOT_FILE} is not tracked; nothing to check`);
  process.exit(2);
}

const instructionFiles = [...tracked].filter(isInstructionFile).sort();

function stripFences(text: string): string {
  const kept: string[] = [];
  let fenced = false;
  for (const line of text.split("\n")) {
    if (/^\s*(```|~~~)/.test(line)) {
      fenced = !fenced;
      continue;
    }
    if (!fenced) kept.push(line);
  }
  return kept.join("\n");
}

function linksIn(text: string): string[] {
  const body = stripFences(text);
  const found: string[] = [];
  for (const match of body.matchAll(/^\s*\[[^\]]+\]:\s*<?([^)\s>]+)>?/gm)) found.push(match[1]);
  for (const match of body.matchAll(/\]\(\s*<?([^)\s>]+)>?/g)) found.push(match[1]);
  return found;
}

// A scheme, a bare fragment, or a rooted path is not a reference to a file in
// this repository.
const isExternal = (target: string) =>
  /^[a-z][a-z0-9+.-]*:/i.test(target) || target.startsWith("#") || target.startsWith("/");

const resolveFrom = (file: string, target: string) =>
  posix.normalize(
    posix.join(posix.dirname(file), target.split("#")[0].replace(/\/+$/, "")),
  );

const contents = new Map<string, string>();
for (const file of instructionFiles) {
  const content = trackedContent(file);
  if (content === null) {
    problems.push(`${file} is tracked but unreadable from the index`);
    continue;
  }
  contents.set(file, content);
}

// 1. Every relative link resolves to a tracked file or directory.
for (const [file, content] of contents) {
  for (const raw of linksIn(content)) {
    if (isExternal(raw)) continue;
    const resolved = resolveFrom(file, raw);
    if (!resolvesToTrackedPath(resolved)) {
      problems.push(`${file} links to ${raw} - ${resolved} is not tracked`);
    }
  }
}

// 2. Every instruction file is reachable from the root by following links, so a
//    file cannot be added without becoming discoverable from the router chain.
const reachable = new Set<string>([ROOT_FILE]);
const queue = [ROOT_FILE];
while (queue.length > 0) {
  const current = queue.shift() as string;
  for (const raw of linksIn(contents.get(current) ?? "")) {
    if (isExternal(raw)) continue;
    const target = resolveFrom(current, raw);
    if (isInstructionFile(target) && target !== current && !reachable.has(target)) {
      reachable.add(target);
      queue.push(target);
    }
  }
}
for (const file of instructionFiles) {
  if (!reachable.has(file)) {
    problems.push(`${file} is not reachable from ${ROOT_FILE} by following links`);
  }
}

// 3. Size is telemetry, not a gate: a hard limit rewards compressed prose and
//    arbitrary rewrapping, which is the opposite of the intent.
const sizes: string[] = [];
let total = 0;
for (const [file, content] of contents) {
  const bytes = Buffer.byteLength(content);
  total += bytes;
  sizes.push(`${bytes.toString().padStart(6)}  ${file}`);
  if (bytes > WARN_BYTES) {
    console.warn(`warning: ${file} is ${bytes} bytes; consider whether it still has one focus`);
  }
}
sizes.sort();
console.log(sizes.join("\n"));
console.log(`${total.toString().padStart(6)}  total (${contents.size} files, from the git index)`);

if (problems.length > 0) {
  console.error("");
  for (const problem of problems) console.error(`  ${problem}`);
  console.error(`\ncheck-agent-instructions: ${problems.length} problem(s)`);
  process.exit(1);
}

console.log("\ncheck-agent-instructions: ok");
