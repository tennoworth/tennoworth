#!/usr/bin/env node
// Gate for the TENOWORTH_PROBE UI smoke run (ui-smoke.yml): the probe drives
// the real Tauri webview against a synthetic fixture and writes its evidence
// as JSON. This asserts the app booted into Tauri IPC mode, the sell view
// rendered its scan CTA, and the run logged no console/CSP violations - the
// failure class a static gate cannot see (a default-vs-named import that
// binds undefined compiles fine and no-ops the feature).
//
// Usage: node scripts/check-probe-report.mjs <report.json>
import { readFileSync } from "node:fs";

const path = process.argv[2];
if (!path) {
  console.error("usage: check-probe-report.mjs <report.json>");
  process.exit(2);
}
let report;
try {
  report = JSON.parse(readFileSync(path, "utf8"));
} catch (e) {
  console.error("could not read/parse probe report " + path + ": " + e.message);
  process.exit(1);
}

const problems = [];
if (report.done !== true) problems.push("done is not true (run did not complete)");
if (report.fatal) problems.push("fatal: " + String(report.fatal).slice(0, 300));
if (!Array.isArray(report.consoleErrors) || report.consoleErrors.length > 0)
  problems.push("consoleErrors: " + JSON.stringify(report.consoleErrors));
if (!Array.isArray(report.cspViolations) || report.cspViolations.length > 0)
  problems.push("cspViolations: " + JSON.stringify(report.cspViolations));
if (report.appMounted !== true) problems.push("appMounted is not true (SPA did not mount)");
if (report.desktopBadge !== true) problems.push("desktopBadge is not true (SPA is not in Tauri IPC mode)");
if (report.scanButtonFound !== true) problems.push("scanButtonFound is not true (sell-view scan CTA did not render)");

if (problems.length > 0) {
  console.error("Probe smoke gate FAILED:");
  for (const p of problems) console.error("  - " + p);
  console.error("full report: " + JSON.stringify(report).slice(0, 4000));
  process.exit(1);
}
console.log(
  "Probe smoke gate ok: runtag=" + (report.runtag || "?") +
  " done=true consoleErrors=0 cspViolations=0 desktopBadge=true scanButtonFound=true",
);
