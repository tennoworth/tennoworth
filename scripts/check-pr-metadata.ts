#!/usr/bin/env bun
// Refuses a maintainer pull request that is missing its triage metadata: an
// assignee, exactly one type label, the areas it ships to, and the
// "Desktop X.Y.Z" milestone of the release train it lands in. Release notes and
// milestone history are read from this metadata, and it lapsed for about 75
// pull requests once it lived only in a checklist.
//
// It applies to pull requests from a branch of this repository, which only
// someone with write access can push, and who can therefore set the metadata.
// A fork's author usually cannot, so a maintainer adds it before merging.
// author_association cannot tell them apart: the owner's own pull requests
// report CONTRIBUTOR, because organization membership is private.
//
// Usage: bun scripts/check-pr-metadata.ts (reads GITHUB_EVENT_PATH)

export const TYPE_LABELS = ['bug', 'enhancement', 'documentation', 'maintenance'];
export const AREA_LABELS = ['desktop', 'site', 'pipeline', 'ci', 'release', 'translation'];
const MILESTONE = /^Desktop \d+\.\d+\.\d+$/;

interface PullRequest {
  head?: { repo?: { full_name: string } | null };
  base?: { repo?: { full_name: string } | null };
  assignees?: Array<{ login: string }>;
  labels?: Array<{ name: string }>;
  milestone?: { title: string } | null;
}

export function appliesTo(pr: PullRequest): boolean {
  const head = pr.head?.repo?.full_name;
  return head !== undefined && head === pr.base?.repo?.full_name;
}

export function findMissingMetadata(pr: PullRequest): string[] {
  const problems: string[] = [];
  const labels = (pr.labels ?? []).map((label) => label.name);
  if (!pr.assignees?.length) problems.push('no assignee');
  const types = labels.filter((label) => TYPE_LABELS.includes(label));
  if (types.length !== 1) {
    problems.push(`needs exactly one type label (${TYPE_LABELS.join(', ')}), has ${types.length ? types.join(', ') : 'none'}`);
  }
  // A documentation-only change can ship to no surface at all.
  if (!types.includes('documentation') && !labels.some((label) => AREA_LABELS.includes(label))) {
    problems.push(`needs at least one area label (${AREA_LABELS.join(', ')})`);
  }
  const milestone = pr.milestone?.title;
  if (!milestone) problems.push('no milestone (Desktop X.Y.Z of the next desktop release)');
  else if (!MILESTONE.test(milestone)) problems.push(`milestone "${milestone}" is not "Desktop X.Y.Z"`);
  return problems;
}

async function main(): Promise<number> {
  const eventPath = process.env.GITHUB_EVENT_PATH;
  if (!eventPath) {
    console.error('check-pr-metadata: set GITHUB_EVENT_PATH to a pull_request event payload');
    return 2;
  }
  const pr: PullRequest | undefined = (await Bun.file(eventPath).json()).pull_request;
  if (!pr) {
    console.error('check-pr-metadata: the event carries no pull request');
    return 2;
  }
  if (!appliesTo(pr)) {
    console.log(`check-pr-metadata: skipped for a pull request from ${pr.head?.repo?.full_name ?? 'a deleted fork'}; a maintainer sets the metadata`);
    return 0;
  }
  const problems = findMissingMetadata(pr);
  if (problems.length) {
    console.error('check-pr-metadata: set the pull request metadata (CONTRIBUTING.md, "Submit a pull request"):');
    for (const problem of problems) console.error(`  ${problem}`);
    return 1;
  }
  console.log('check-pr-metadata: ok');
  return 0;
}

if (import.meta.main) process.exit(await main());
