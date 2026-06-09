export const meta = {
  name: 'release-readiness',
  description: 'Pre-tag gate: build + test all three domains, check WFM API shapes, run the security audit, emit a go/no-go verdict',
  whenToUse: 'Before tagging a release. Blocks on a failed build/test, a Breaking WFM drift, or a confirmed Critical/High security finding.',
  phases: [
    { title: 'Build & Test', detail: 'rust / svelte / python in parallel' },
    { title: 'API shapes', detail: 'wfm-api-shape-check against live endpoints' },
    { title: 'Security', detail: 'nested security-audit workflow' },
    { title: 'Verdict', detail: 'synthesize go / no-go' },
  ],
}

const ROOT = '/home/prowly/Desktop/Warframe market check'

const CHECKS_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['domain', 'allPassed', 'checks'],
  properties: {
    domain: { type: 'string' },
    allPassed: { type: 'boolean' },
    checks: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['name', 'passed', 'detail'],
        properties: {
          name: { type: 'string' },
          passed: { type: 'boolean' },
          detail: { type: 'string', description: 'count on pass; the failing block on failure' },
        },
      },
    },
  },
}

const API_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['breaking', 'latent', 'endpoints', 'summary'],
  properties: {
    breaking: { type: 'integer' },
    latent: { type: 'integer' },
    summary: { type: 'string' },
    endpoints: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['endpoint', 'result', 'note'],
        properties: {
          endpoint: { type: 'string' },
          result: { type: 'string', enum: ['PASS', 'FAIL', 'UNTESTED'] },
          note: { type: 'string' },
        },
      },
    },
  },
}

phase('Build & Test')
const domains = [
  {
    domain: 'rust',
    prompt: `Run, in order, from "${ROOT}/companion":
  cargo test --release --quiet
  cargo build --release --quiet
Report each as a check (passed + a short detail: test count on pass, the tail of the error block on failure). Do not fix anything.`,
  },
  {
    domain: 'svelte',
    prompt: `Run, in order, from "${ROOT}/prototype":
  bun run test
  npx svelte-check
  bun run build
Report each as a check (passed + a short detail: vitest count on pass, the failing block on failure). Do not fix anything.`,
  },
  {
    domain: 'python',
    prompt: `Run:
  /home/prowly/.local/bin/pytest "${ROOT}/tests/" -q
Report it as a check (passed + a short detail: test count on pass, the failing block on failure). Do not fix anything.`,
  },
]

const buildResults = await parallel(
  domains.map((d) => () =>
    agent(d.prompt, { label: `test:${d.domain}`, phase: 'Build & Test', schema: CHECKS_SCHEMA, agentType: 'general-purpose' }),
  ),
)

phase('API shapes')
const api = await agent(
  `Audit the WFM endpoints this project depends on. Sample 3-5 items from /v2/items and confirm the fields companion/src/main.rs fetch_wfm_catalog() reads (id, slug, i18n.en.name). Then sample prototype/public/market.json and confirm its shape matches what prototype/src/lib/market.js parses. Read-only only — no POST/PATCH/DELETE. Cap at 8 WFM requests total, 1 req/sec. Sort drift by Breaking / Latent.`,
  { label: 'wfm-shapes', phase: 'API shapes', schema: API_SCHEMA, agentType: 'wfm-api-shape-check' },
)

phase('Security')
let security = null
try {
  security = await workflow('security-audit')
} catch (e) {
  log(`security-audit workflow failed to run: ${e.message}`)
}

phase('Verdict')
const buildFailures = []
for (let i = 0; i < buildResults.length; i++) {
  const r = buildResults[i]
  if (!r) { buildFailures.push(`${domains[i].domain}: agent did not return`); continue }
  if (!r.allPassed) {
    for (const c of r.checks || []) if (!c.passed) buildFailures.push(`${r.domain}/${c.name}: ${c.detail}`)
  }
}

const secBlockers = security
  ? (security.findings || []).filter((f) => !f.unverified && (f.severity === 'Critical' || f.severity === 'High'))
  : []

const blockers = []
if (buildFailures.length) blockers.push(...buildFailures.map((b) => `BUILD/TEST — ${b}`))
if (api && api.breaking > 0) blockers.push(`WFM API — ${api.breaking} breaking drift(s): ${api.summary}`)
if (secBlockers.length) blockers.push(...secBlockers.map((f) => `SECURITY — ${f.severity} ${f.file}: ${f.issue}`))
if (!security) blockers.push('SECURITY — audit could not run; treat as NO-GO until checked manually')

const go = blockers.length === 0

return {
  verdict: go ? 'GO' : 'NO-GO',
  blockers,
  buildResults: buildResults.map((r, i) => (r ? { domain: r.domain, allPassed: r.allPassed } : { domain: domains[i].domain, allPassed: false, error: 'no result' })),
  api: api ? { breaking: api.breaking, latent: api.latent, summary: api.summary } : null,
  security: security ? { blockers: secBlockers.length, latent: (security.findings || []).length } : 'did not run',
}
