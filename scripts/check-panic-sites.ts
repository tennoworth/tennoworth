#!/usr/bin/env bun
// Gate: no unwrap()/expect() in production code outside the allowlist.
//
// Why this gate exists: unwrap/expect on the scan, listing, and command paths
// turns a recoverable condition into a crash (Tauri 2.x does not catch command
// panics), and a panicked thread poisons every Mutex it held - so one panic can
// brick all DB/session state for the rest of the process. The allowlist is the
// sites that are unreachable by construction (compile-time regexes, build-time,
// top-level main, the probe harness). When you FIX a site, remove it from the
// allowlist in the same commit - the list is meant to shrink.
// unwrap() in tests is idiomatic and stays; this gate only sees production code.
//
// Usage: bun scripts/check-panic-sites.ts [--list]
//   --list prints every production site found (file:line kind snippet) without
//          failing - run it after a refactor to regenerate the allowlist.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const ROOT = new URL("..", import.meta.url).pathname.replace(/\/$/, "");
const DIRS = [
  "companion/wfm-core/src",
  "companion/wfm-client/src",
  "companion/market-math/src",
  "companion/tennoworth-desktop",
  "companion/wfm-scrape/src",
];

// file:line of the legitimate production sites. Shrink, don't grow.
const ALLOWLIST = new Set([
  "companion/tennoworth-desktop/build.rs:12",
  "companion/tennoworth-desktop/build.rs:17",
  // The end-of-main .expect("error while running tauri application") — its
  // line drifts whenever setup() grows (252 -> 278 probe block, -> 294 UA
  // identity + watch checker, -> 309 EE.log tailer); same site, drifted key.
  "companion/tennoworth-desktop/src/main.rs:309",
  "companion/tennoworth-desktop/src/probe.rs:308",
  "companion/wfm-core/src/auth.rs:125",
  "companion/wfm-core/src/scan.rs:91",
  "companion/wfm-core/src/scan.rs:92",
  "companion/wfm-core/src/scan.rs:93",
]);

function walk(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    if (statSync(p).isDirectory()) walk(p, out);
    else if (entry.endsWith(".rs")) out.push(p);
  }
  return out;
}

// Replace string/char literals, comments and raw strings with spaces so braces
// inside them can't break depth tracking. Newlines are preserved so line
// numbers stay aligned. Handles multi-line raw strings and block comments.
function cleanSource(src) {
  const out = new Array(src.length).fill(" ");
  let i = 0;
  const n = src.length;
  const copyNewlines = (a, b) => { for (let j = a; j < b; j++) if (src[j] === "\n") out[j] = "\n"; };
  while (i < n) {
    const ch = src[i];
    const nx = src[i + 1];
    if (ch === "/" && nx === "/") { while (i < n && src[i] !== "\n") i++; continue; }
    if (ch === "/" && nx === "*") {
      let j = i + 2;
      while (j < n && !(src[j] === "*" && src[j + 1] === "/")) j++;
      copyNewlines(i, j);
      i = Math.min(j + 2, n);
      continue;
    }
    if (ch === "r" || ch === "b") {
      let k = i + (ch === "b" && nx === "r" ? 2 : 1);
      let hashes = 0;
      while (src[k] === "#") { hashes++; k++; }
      if (src[k] === '"') {
        const close = '"' + "#".repeat(hashes);
        let j = k + 1;
        while (j < n) { if (src[j] === '"' && src.slice(j, j + 1 + hashes) === close) break; j++; }
        copyNewlines(i, Math.min(j + 1 + hashes, n));
        i = Math.min(j + 1 + hashes, n);
        continue;
      }
    }
    if (ch === '"') {
      let j = i + 1;
      while (j < n && src[j] !== '"' && src[j] !== "\n") { if (src[j] === "\\") j++; j++; }
      if (j < n && src[j] === '"') j++;
      copyNewlines(i, j);
      i = j;
      continue;
    }
    if (ch === "'") {
      // char literal, or a lifetime ('a) which has no closing quote
      let j = i + 1;
      let closed = false;
      while (j < n && src[j] !== "\n") {
        if (src[j] === "\\") j += 2;
        else if (src[j] === "'") { closed = true; break; }
        else j++;
      }
      if (closed) { i = j + 1; continue; }
    }
    out[i] = ch;
    i++;
  }
  return out.join("");
}

const TEST_ATTR = new RegExp(String.raw`#\[cfg\(test\)\]|#\[tokio::test\]|#\[test\]`);
const MOD = new RegExp(String.raw`^\s*(pub(\([^)]*\))?\s+)?mod\s+(\w+)`);
const FN = new RegExp(String.raw`^\s*(pub(\([^)]*\))?\s+)?(async\s+)?(unsafe\s+)?fn\s+(\w+)`);
const UNWRAP = new RegExp(String.raw`([^A-Za-z0-9_])unwrap\(\)`);
const EXPECT = new RegExp(String.raw`([^A-Za-z0-9_])expect\(("[^"]*")?\)`);

// Classify every unwrap()/expect() site in one file; returns production ones
// as { line, kind, text }. Mirrors the 2026-08-13 panic audit classifier:
// #[test] / #[tokio::test] / #[cfg(test)] items (and any mod tests) open a
// test region at their own brace depth; deeper lines are test code.
function productionSites(src) {
  const clean = cleanSource(src);
  const lines = clean.split("\n");
  const rawLines = src.split("\n");
  let depth = 0;
  let pendingTest = false;
  const regions = [];
  const sites = [];
  for (let i = 0; i < lines.length; i++) {
    const stripped = lines[i];
    if (TEST_ATTR.test(stripped)) pendingTest = true;
    const item = stripped.replace(/^(\s*#\[[^\]]*\]\s*)+/, "");
    const mm = item.match(MOD);
    const fm = item.match(FN);
    if (mm) {
      if (pendingTest || mm[3] === "tests") regions.push({ d: depth });
      pendingTest = false;
    } else if (fm) {
      if (pendingTest) regions.push({ d: depth });
      pendingTest = false;
    }
    const uw = rawLines[i].match(UNWRAP);
    const ex = rawLines[i].match(EXPECT);
    if (uw || ex) {
      const inTest = regions.some((r) => depth > r.d);
      if (!inTest) sites.push({ line: i + 1, kind: uw ? "unwrap" : "expect", text: rawLines[i].trim().slice(0, 90) });
    }
    let d = 0;
    for (const ch of stripped) {
      if (ch === "{") d++;
      else if (ch === "}") d--;
    }
    depth += d;
    for (let k = regions.length - 1; k >= 0; k--) {
      if (depth <= regions[k].d) regions.splice(k, 1);
    }
  }
  return sites;
}

const listOnly = process.argv.includes("--list");
const found = [];
for (const dir of DIRS) {
  for (const file of walk(join(ROOT, dir))) {
    const src = readFileSync(file, "utf8");
    for (const site of productionSites(src)) {
      found.push({ key: relative(ROOT, file) + ":" + site.line, ...site });
    }
  }
}

if (listOnly) {
  for (const s of found) console.log(s.key + "  " + s.kind + "  " + s.text);
  console.log("\n" + found.length + " production sites total.");
  process.exit(0);
}

const violations = found.filter((s) => !ALLOWLIST.has(s.key));
if (violations.length > 0) {
  console.error("Panic-site gate: production unwrap()/expect() outside the allowlist:");
  for (const v of violations) console.error("  " + v.key + "  " + v.kind + "  " + v.text);
  console.error("\nAllowlisted (shrink, don't grow):");
  for (const a of [...ALLOWLIST].sort()) console.error("  " + a);
  console.error("\nFix the sites above (return an error instead of panicking) or, if a site");
  console.error("is unreachable by construction, add it to ALLOWLIST - and say why in a");
  console.error("comment in the same commit.");
  process.exit(1);
}
console.log("Panic-site gate ok - " + found.length + " production sites, all allowlisted (" + ALLOWLIST.size + ").");
