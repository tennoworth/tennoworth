import { existsSync } from "node:fs";
import { join } from "node:path";
import { ROOT, CHANGELOG, SEMVER, read, write, object } from "./files";
import { cargoTomlVersion, compareVersions } from "./versions";

export interface ReleaseNotesSummary {
  icon: string;
  changeCount: number;
  categoryCount: number;
}

const RELEASE_NOTE_EMOJI = /\p{Extended_Pictographic}/gu;
const RELEASE_SECTION_HEADING = /^## \d+\.\d+\.\d+(?:\s|$)/;

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

export function releaseNotesBody(changelog: string, version: string): string {
  const lines = changelog.split("\n");
  const start = lines.findIndex(
    (line) => line.startsWith(`## ${version} `) || line.trim() === `## ${version}`,
  );
  if (start === -1) throw new Error(`${CHANGELOG} has no section for ${version}`);
  const rest = lines.slice(start + 1);
  const end = rest.findIndex((line) => RELEASE_SECTION_HEADING.test(line));
  const body = (end === -1 ? rest : rest.slice(0, end)).join("\n").trim();
  if (!body) throw new Error(`the ${version} section in ${CHANGELOG} is empty`);
  return body;
}

export function validateReleaseNotes(body: string, version: string): ReleaseNotesSummary {
  body = body.replace(/ <!-- app-note (\{[^\n]+\}) -->$/gm, "");
  const lines = body.replace(/\r\n/g, "\n").split("\n");
  if (/<!--|\b(?:EMOJI|TODO|TBD)\b|\[Category\]/i.test(body)) {
    throw new Error("release notes still contain template placeholders");
  }

  const first = lines.findIndex((line) => line.trim().length > 0);
  const titlePattern = new RegExp(
    `^# (\\p{Extended_Pictographic}\\uFE0F?) TennoWorth Desktop ${escapeRegExp(version)}$`,
    "u",
  );
  const title = first === -1 ? null : lines[first].match(titlePattern);
  if (!title) {
    throw new Error(
      `the first line must be "# <one contextual emoji> TennoWorth Desktop ${version}"`,
    );
  }

  const emojiCount = [...body.matchAll(RELEASE_NOTE_EMOJI)].length;
  if (emojiCount !== 1) {
    throw new Error(
      `release notes must contain exactly one emoji, in the title; found ${emojiCount}`,
    );
  }

  const changelogHeadings = lines
    .map((line, index) => ({ line, index }))
    .filter(({ line }) => line.startsWith("## Changelog"));
  if (changelogHeadings.length !== 1) {
    throw new Error('release notes need exactly one "## Changelog (N)" heading');
  }
  const countMatch = changelogHeadings[0].line.match(/^## Changelog \((\d+)\)$/);
  if (!countMatch) {
    throw new Error('the changelog heading must use "## Changelog (N)" with a number');
  }
  const changelogAt = changelogHeadings[0].index;

  const intro = lines
    .slice(first + 1, changelogAt)
    .join("\n")
    .trim();
  if (/^#/m.test(intro)) {
    throw new Error("the introduction must be prose, before the changelog heading");
  }
  const introParagraphs = intro
    .split(/\n\s*\n/)
    .map((paragraph) => paragraph.trim())
    .filter(Boolean);
  if (introParagraphs.length < 2) {
    throw new Error(
      "release notes need an announcement paragraph and a separate user-facing summary",
    );
  }

  const updatingHeadings = lines
    .map((line, index) => ({ line, index }))
    .filter(({ line }) => line.trim() === "## Updating");
  if (updatingHeadings.length !== 1 || updatingHeadings[0].index <= changelogAt) {
    throw new Error('release notes need one "## Updating" section after the changelog');
  }
  const updatingAt = updatingHeadings[0].index;

  const changelogLines = lines.slice(changelogAt + 1, updatingAt);
  const firstChangelogLine = changelogLines.find((line) => line.trim().length > 0);
  if (!firstChangelogLine?.startsWith("### ")) {
    throw new Error("the changelog must begin with a plain category heading such as \"### Linux\"");
  }
  const categories = changelogLines.filter((line) => line.startsWith("### "));
  if (categories.length === 0) {
    throw new Error("the changelog needs at least one category heading");
  }
  if (categories.some((line) => /\p{Extended_Pictographic}/u.test(line))) {
    throw new Error("category headings must be plain text; keep the single emoji in the title");
  }

  if (changelogLines.some((line) => /^-\s*$/.test(line))) {
    throw new Error("the changelog contains an empty bullet");
  }
  const changes = changelogLines.filter((line) => /^- \S/.test(line));
  if (changes.length === 0) throw new Error("the changelog needs at least one change bullet");
  for (const [index, line] of changelogLines.entries()) {
    if (!line.startsWith("### ")) continue;
    const nextCategory = changelogLines.findIndex(
      (candidate, candidateIndex) => candidateIndex > index && candidate.startsWith("### "),
    );
    const categoryBody = changelogLines.slice(
      index + 1,
      nextCategory === -1 ? changelogLines.length : nextCategory,
    );
    if (!categoryBody.some((candidate) => /^- \S/.test(candidate))) {
      throw new Error(`the category "${line.slice(4)}" has no change bullets`);
    }
  }
  const declaredCount = Number(countMatch[1]);
  if (declaredCount !== changes.length) {
    throw new Error(
      `the changelog declares ${declaredCount} changes but contains ${changes.length} top-level bullets`,
    );
  }

  const updatingLines = lines.slice(updatingAt + 1);
  if (updatingLines.some((line) => line.startsWith("## "))) {
    throw new Error('"## Updating" must be the final section');
  }
  if (!updatingLines.some((line) => line.trim().length > 0)) {
    throw new Error('the "## Updating" section is empty');
  }

  return {
    icon: title[1],
    changeCount: changes.length,
    categoryCount: categories.length,
  };
}

// App notes are authored on the changelog bullet they describe, then bundled offline.
const APP_NOTES_PATH = "rust/tennoworth-desktop/resources/update-notes.json";
export const APP_NOTES_SINCE = "0.7.1";
const APP_ANNOTATION = / <!-- app-note (\{[^\n]+\}) -->$/;
interface AppChange {
  id: string; kind: "improved" | "fixed" | "action"; title: string; body: string;
  platforms: string[]; supersedes: string[];
}
export function appNotesCatalog(changelog: string, version: string) {
  if (!SEMVER.test(version)) throw new Error("invalid app notes version");
  const headers = [...changelog.matchAll(/^## (\d+\.\d+\.\d+) - (\d{4}-\d{2}-\d{2})\s*$/gm)];
  const versions = new Set<string>();
  const releases: { version: string; date: string; changes: AppChange[] }[] = [];
  for (const header of headers) {
    const release = header[1];
    if (!SEMVER.test(release) || release.split('.').some(n => !Number.isSafeInteger(Number(n)))) throw new Error("invalid release version");
    if (versions.has(release)) throw new Error(`duplicate release ${release}`);
    versions.add(release);
    if (compareVersions(release, APP_NOTES_SINCE) <= 0) continue;
    if (compareVersions(release, version) > 0) throw new Error(`future app notes ${release} exceed installed ${version}`);
    if (!Number.isFinite(Date.parse(header[2])) || new Date(header[2]).toISOString().slice(0, 10) !== header[2]) throw new Error("invalid release date");
    const body = releaseNotesBody(changelog, release);
    const changes: AppChange[] = [];
    const ids = new Set<string>();
    for (const line of body.split('\n')) {
      const annotation = line.match(APP_ANNOTATION);
      if (!annotation) continue;
      const meta = object(JSON.parse(annotation[1]), 'app-note');
      if (Object.keys(meta).some(k => !['id', 'kind', 'platforms', 'supersedes'].includes(k))) throw new Error('unknown app-note field');
      const text = line.replace(APP_ANNOTATION, '').match(/^- \*\*(.+?)\*\* (.+)$/);
      if (!text || text[1].length > 100 || text[2].length > 600 || /[<>\r\n]/.test(text[1] + text[2])) throw new Error('app-note needs a short plain-text title and explanation');
      if (typeof meta.id !== 'string' || !/^[a-z][a-z0-9-]{0,79}$/.test(meta.id) || ids.has(meta.id) || meta.id === 'replace-me') throw new Error('invalid or duplicate app-note ID');
      ids.add(meta.id);
      if (!['improved', 'fixed', 'action'].includes(String(meta.kind))) throw new Error('invalid app-note kind');
      const platforms = meta.platforms ?? ['windows', 'linux'];
      if (!Array.isArray(platforms) || !platforms.length || platforms.some(p => !['windows', 'linux'].includes(p)) || new Set(platforms).size !== platforms.length) throw new Error('invalid app-note platforms');
      const supersedes = meta.supersedes ?? [];
      if (!Array.isArray(supersedes) || supersedes.some(id => typeof id !== 'string' || !/^[a-z][a-z0-9-]{0,79}$/.test(id) || id === meta.id)) throw new Error('invalid app-note supersession');
      changes.push({ id: meta.id, kind: meta.kind as AppChange['kind'], title: text[1], body: text[2], platforms, supersedes });
    }
    if (!changes.length) throw new Error(`release ${release} has no app notes`);
    validateReleaseNotes(body, release);
    releases.push({ version: release, date: header[2], changes });
  }
  releases.sort((a, b) => compareVersions(a.version, b.version));
  if (!releases.some(r => r.version === version)) throw new Error(`installed version ${version} has no app notes`);
  const earlier = new Map<string, AppChange>();
  for (const [index, release] of releases.entries()) {
    for (const change of release.changes) {
      const prior = earlier.get(change.id);
      if (prior && (prior.kind !== change.kind || JSON.stringify(prior.platforms) !== JSON.stringify(change.platforms))) throw new Error(`inconsistent app-note ID ${change.id}`);
      earlier.set(change.id, change);
    }
    for (const change of release.changes) for (const id of change.supersedes) {
      const prior = releases.slice(0, index).flatMap(r => r.changes).filter(c => c.id === id);
      if (!prior.length || prior.some(c => c.kind === 'action')) throw new Error(`supersession ${id} must name an earlier non-action change`);
    }
  }
  return { version, coverage_since: APP_NOTES_SINCE, releases };
}
export function syncAppNotes(check: boolean) {
  const generated = JSON.stringify(appNotesCatalog(read(CHANGELOG), cargoTomlVersion()), null, 2) + '\n';
  if (check) {
    if (!existsSync(join(ROOT, APP_NOTES_PATH)) || read(APP_NOTES_PATH) !== generated) throw new Error('bundled app notes are stale; run bun scripts/release.ts app-notes');
  } else write(APP_NOTES_PATH, generated);
}

export function releaseNotesTemplate(version: string, date: string): string {
  return `## ${version} - ${date}\n\n` +
    `# EMOJI TennoWorth Desktop ${version}\n\n` +
    `TennoWorth Desktop ${version} is ready.\n\n` +
    `<!-- Explain in one short paragraph who benefits and why this release matters. -->\n\n` +
    `## Changelog (N)\n\n` +
    `### Category\n\n` +
    `- **Short user-facing title** Explain what changed in plain English. <!-- app-note {"id":"replace-me","kind":"improved"} -->\n\n` +
    `## Updating\n\n` +
    `TennoWorth checks for updates automatically at launch and every 30 minutes while it is open. ` +
    `Downloads for Windows and Linux are available in the assets below.\n\n`;
}

