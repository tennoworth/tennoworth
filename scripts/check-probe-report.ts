#!/usr/bin/env bun
// Gate for the TENNOWORTH_PROBE UI smoke run (ui-smoke.yml): the probe drives
// the real Tauri webview against a synthetic fixture and writes its evidence
// as JSON. This asserts the app booted into Tauri IPC mode, the sell view
// rendered its scan CTA, and the run logged no console/CSP violations - the
// failure class a static gate cannot see (a default-vs-named import that
// binds undefined compiles fine and no-ops the feature).
//
// Usage: bun scripts/check-probe-report.ts <report.json>
import { readFileSync } from "node:fs";
import pacing from "../tests/fixtures/pacing.json";

const path = process.argv[2];
if (!path) {
  console.error("usage: check-probe-report.ts <report.json>");
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
if (report.updateNotesUiVerified !== true) problems.push("installed update notes did not open and close through Settings");
if (report.updateNotesVerified !== true) problems.push("installed update notes were not verified over native IPC");
if (report.wfm?.cancelIdle?.ok !== true) problems.push('WFM cancellation command was unavailable');
const access = report.wfm?.access?.ok === true ? report.wfm.access.value : null;
if (!access || Object.entries(pacing).some(([key, value]) => access.restrictions?.[key] !== value))
  problems.push('WFM access command did not expose the compiled request safeguards');
if (access?.revision !== 0 || typeof access?.queue_count !== 'number')
  problems.push('WFM access status contract is incomplete');
if (report.usageExcluded !== true) problems.push('probe usage reporting was not excluded or private settings were exposed');
if (report.domainRejectedInvalid !== true) problems.push('invalid native domain quantities were not rejected');
const expectedDomainOperations = ['normalize_inventory', 'score_inventory', 'trade_session', 'advisor', 'history', 'relic_plan', 'set_recos', 'ducat_plan', 'build_plan'];
if (JSON.stringify(report.domainOperations) !== JSON.stringify(expectedDomainOperations))
  problems.push('native domain operations did not all match their shared fixtures');
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
