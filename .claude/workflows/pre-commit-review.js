export const meta = {
  name: 'pre-commit-review',
  description: 'Route changed files to the right reviewer, adversarially verify findings, consolidate one punch list',
  whenToUse: 'Everyday quality gate before committing. Pass changed file paths as args, or let it detect recent edits by mtime.',
  phases: [
    { title: 'Scope', detail: 'identify changed source files and bucket by domain' },
    { title: 'Review', detail: 'companion-reviewer / svelte5-reviewer per bucket' },
    { title: 'Verify', detail: 'adversarially confirm each P0/P1 finding' },
    { title: 'Consolidate', detail: 'merge surviving findings into one list' },
  ],
}

const ROOT = '/home/prowly/Desktop/Warframe market check'

const SCOPE_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['rust', 'svelte', 'python', 'note'],
  properties: {
    rust: { type: 'array', items: { type: 'string' }, description: 'changed .rs files under companion/' },
    svelte: { type: 'array', items: { type: 'string' }, description: 'changed .svelte / .js files under prototype/src' },
    python: { type: 'array', items: { type: 'string' }, description: 'changed .py files (scripts, root, tests)' },
    note: { type: 'string', description: 'one line: how the file list was determined' },
  },
}

const FINDINGS_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['summary', 'findings'],
  properties: {
    summary: { type: 'string' },
    findings: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['severity', 'file', 'line', 'rule', 'issue', 'fix'],
        properties: {
          severity: { type: 'string', enum: ['P0', 'P1', 'nit'] },
          file: { type: 'string' },
          line: { type: 'string', description: 'line number or range, as a string' },
          rule: { type: 'string', description: 'which checklist rule / invariant it violates' },
          issue: { type: 'string' },
          fix: { type: 'string' },
        },
      },
    },
  },
}

const VERDICT_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['stillStands', 'reasoning'],
  properties: {
    stillStands: { type: 'boolean', description: 'true only if reading the cited code confirms a real violation' },
    reasoning: { type: 'string' },
  },
}

phase('Scope')
const explicit = Array.isArray(args) ? args : (args && Array.isArray(args.files) ? args.files : null)
const scope = await agent(
  `Scope a pre-commit review for the wfminv project at "${ROOT}".
${explicit
    ? `Review exactly these files: ${JSON.stringify(explicit)}.`
    : `This is NOT a git repository, so detect recently-changed source by mtime. Run:
  find "${ROOT}/companion/src" "${ROOT}/prototype/src" "${ROOT}/scripts" -type f \\( -name '*.rs' -o -name '*.svelte' -o -name '*.js' -o -name '*.py' \\) -newermt '1 day ago' 2>/dev/null
  find "${ROOT}" -maxdepth 1 -name '*.py' -newermt '1 day ago' 2>/dev/null
Skip node_modules, target, dist, build, .pytest_cache, __pycache__. If nothing is recent, return empty arrays with a note saying so.`}
Bucket each path: rust = companion/**/*.rs ; svelte = prototype/src/**/*.{svelte,js} ; python = *.py anywhere else.`,
  { label: 'scope', phase: 'Scope', schema: SCOPE_SCHEMA, agentType: 'Explore' },
)

log(`scope → ${scope.rust.length} rust, ${scope.svelte.length} svelte, ${scope.python.length} python (${scope.note})`)

const buckets = [
  scope.rust.length ? { domain: 'rust', agentType: 'companion-reviewer', files: scope.rust } : null,
  scope.svelte.length ? { domain: 'svelte', agentType: 'svelte5-reviewer', files: scope.svelte } : null,
  scope.python.length ? { domain: 'python', agentType: 'general-purpose', files: scope.python } : null,
].filter(Boolean)

if (!buckets.length) {
  log('No changed source files found — nothing to review.')
  return { reviewedFiles: [], confirmed: [], note: scope.note }
}

const reviewed = await pipeline(
  buckets,
  (b) =>
    agent(
      `Review these ${b.domain} files for the wfminv project: ${JSON.stringify(b.files)}.
${b.domain === 'python'
        ? 'Read scripts/CLAUDE.md first. Check atomic writes, flush rules, UA requirements, and general correctness (error paths, empty input, malformed JSON, missing keys).'
        : 'Follow your standard checklist exactly. Cite file:line for every finding. Be concrete, not stylistic.'}
Return findings grouped by P0 / P1 / nit.`,
      { label: `review:${b.domain}`, phase: 'Review', schema: FINDINGS_SCHEMA, agentType: b.agentType },
    ).then((r) => ({ domain: b.domain, summary: r.summary, findings: r.findings || [] })),
  (rev) =>
    parallel(
      rev.findings
        .filter((f) => f.severity !== 'nit')
        .map((f) => () =>
          agent(
            `Adversarially verify this ${rev.domain} review finding. Read the cited code and decide if it is a REAL violation.
File: ${f.file}:${f.line}
Rule: ${f.rule}
Issue: ${f.issue}
Default stillStands=false if the cited line doesn't actually violate the rule, the file/line can't be found, or you can't confirm it by reading the code.`,
            { label: `verify:${f.file}`, phase: 'Verify', schema: VERDICT_SCHEMA, agentType: 'Explore' },
          ).then((v) => ({ ...f, domain: rev.domain, verdict: v })),
        ),
    ).then((verified) => ({
      domain: rev.domain,
      summary: rev.summary,
      verified: verified.filter(Boolean),
      nits: rev.findings.filter((f) => f.severity === 'nit').map((f) => ({ ...f, domain: rev.domain })),
    })),
)

phase('Consolidate')
const order = { P0: 0, P1: 1, nit: 2 }
const confirmed = []
const dropped = []
for (const r of reviewed.filter(Boolean)) {
  for (const f of r.verified) {
    if (f.verdict && f.verdict.stillStands) confirmed.push(f)
    else dropped.push(f)
  }
  for (const n of r.nits) confirmed.push(n)
}
confirmed.sort((a, b) => order[a.severity] - order[b.severity])

return {
  reviewedFiles: buckets.flatMap((b) => b.files),
  confirmed,
  droppedByVerifier: dropped.map((f) => ({ file: f.file, line: f.line, issue: f.issue, why: f.verdict && f.verdict.reasoning })),
  domainSummaries: reviewed.filter(Boolean).map((r) => ({ domain: r.domain, summary: r.summary })),
}
