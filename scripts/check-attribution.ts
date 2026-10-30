#!/usr/bin/env bun
// Refuses tool attribution in commit messages and the pull request body.
// AGENTS.md "Automated assistance is invisible in project artifacts" is the
// rule; tools that write commits add these trailers by default, so a rule that
// lives only in instructions is one forgotten reminder away from a merged
// trailer.
//
// Usage: bun scripts/check-attribution.ts [<base>..<head>]
// Without a range it reads GITHUB_BASE_SHA/GITHUB_HEAD_SHA (set by the audit
// workflow) and the pull request body from GITHUB_EVENT_PATH.

const PATTERNS: Array<[RegExp, string]> = [
  [/^co-authored-by:.*\b(claude|anthropic|openai|chatgpt|gpt-?\d|copilot|codex|gemini|deepseek|cursor|devin|noreply@anthropic\.com)\b/im, 'tool co-author trailer'],
  [/generated (with|by) .*\b(claude|chatgpt|copilot|codex|gemini|ai)\b/i, 'generated-with line'],
  [/\bclaude\.(ai|com)\b/i, 'assistant link'],
  [/🤖/u, 'assistant marker'],
];

export function findAttribution(text: string): string[] {
  return PATTERNS.filter(([pattern]) => pattern.test(text)).map(([, label]) => label);
}

async function git(...args: string[]): Promise<string> {
  const proc = Bun.spawn(['git', ...args], { stdout: 'pipe', stderr: 'inherit' });
  const out = await new Response(proc.stdout).text();
  if ((await proc.exited) !== 0) throw new Error(`git ${args.join(' ')} failed`);
  return out;
}

async function main(): Promise<number> {
  const range = process.argv[2]
    ?? (process.env.GITHUB_BASE_SHA && process.env.GITHUB_HEAD_SHA
      ? `${process.env.GITHUB_BASE_SHA}..${process.env.GITHUB_HEAD_SHA}`
      : undefined);
  if (!range) {
    console.error('usage: check-attribution.ts <base>..<head> (or set GITHUB_BASE_SHA/GITHUB_HEAD_SHA)');
    return 2;
  }
  const problems: string[] = [];
  const log = await git('log', '--format=%H%x00%B%x01', range);
  for (const entry of log.split('\x01')) {
    const [sha, body] = entry.trim().split('\x00');
    if (!sha) continue;
    for (const label of findAttribution(body ?? '')) problems.push(`${sha.slice(0, 10)}: ${label}`);
  }
  const eventPath = process.env.GITHUB_EVENT_PATH;
  if (eventPath) {
    const event = await Bun.file(eventPath).json().catch(() => ({}));
    const body: string = event?.pull_request?.body ?? '';
    for (const label of findAttribution(body)) problems.push(`pull request body: ${label}`);
  }
  if (problems.length) {
    console.error('check-attribution: remove tool attribution (AGENTS.md, "Automated assistance is invisible"):');
    for (const problem of problems) console.error(`  ${problem}`);
    return 1;
  }
  console.log(`check-attribution: ok (${range})`);
  return 0;
}

if (import.meta.main) process.exit(await main());
