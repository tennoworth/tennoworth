export const meta = {
  name: 'security-audit',
  description: 'Parallel companion + browser security audit, skeptic-verify Critical/High, synthesize one severity-sorted report',
  whenToUse: 'Whole-app security pass before a release, or after changes touching crypto, auth, storage, CSP, or network.',
  phases: [
    { title: 'Audit', detail: 'companion-security + browser-security in parallel' },
    { title: 'Verify', detail: 'independent skeptic pass on every Critical/High' },
    { title: 'Synthesize', detail: 'dedup + sort by severity' },
  ],
}

const SEC_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['posture', 'findings'],
  properties: {
    posture: { type: 'string', description: 'two-line overall posture summary' },
    findings: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['severity', 'file', 'issue', 'fix'],
        properties: {
          severity: { type: 'string', enum: ['Critical', 'High', 'Medium', 'Low'] },
          file: { type: 'string', description: 'file:line' },
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
  required: ['refuted', 'reasoning'],
  properties: {
    refuted: { type: 'boolean', description: 'true if the finding is a false positive or its severity is overstated' },
    adjustedSeverity: { type: 'string', enum: ['Critical', 'High', 'Medium', 'Low'] },
    reasoning: { type: 'string' },
  },
}

phase('Audit')
const audits = await parallel([
  () =>
    agent(
      `Full security audit of the Rust companion (companion/) for the wfminv project. Read SECURITY.md first and verify the code lives up to its commitments. Run \`cargo audit\` if it is available (best-effort). Cite file:line for every finding.`,
      { label: 'companion-security', phase: 'Audit', schema: SEC_SCHEMA, agentType: 'companion-security' },
    ).then((r) => ({ domain: 'companion', ...r })),
  () =>
    agent(
      `Full security audit of the browser app (prototype/) for the wfminv project. Cover CSP, crypto, storage hygiene, XSS/injection, network allowlist, and the install scripts. Run \`npm audit --audit-level=moderate\` if available. Cite file:line for every finding.`,
      { label: 'browser-security', phase: 'Audit', schema: SEC_SCHEMA, agentType: 'browser-security' },
    ).then((r) => ({ domain: 'browser', ...r })),
])

const findings = audits
  .filter(Boolean)
  .flatMap((a) => (a.findings || []).map((f) => ({ ...f, domain: a.domain })))

// dedup: same domain + file + first 60 chars of issue
const seen = new Set()
const unique = findings.filter((f) => {
  const key = `${f.domain}|${f.file}|${(f.issue || '').slice(0, 60).toLowerCase()}`
  if (seen.has(key)) return false
  seen.add(key)
  return true
})
log(`${findings.length} findings (${unique.length} after dedup) across ${audits.filter(Boolean).length} audits`)

phase('Verify')
const toVerify = unique.filter((f) => f.severity === 'Critical' || f.severity === 'High')
const verified = await parallel(
  toVerify.map((f) => () =>
    agent(
      `Adversarially verify this security finding in the wfminv ${f.domain}. Read the cited code and try to REFUTE it.
File: ${f.file}
Claimed severity: ${f.severity}
Issue: ${f.issue}
Proposed fix: ${f.fix}
Is this a real, correctly-rated finding, or a false positive / overstated severity? Set adjustedSeverity if the issue is real but mis-rated. Default refuted=true if you cannot confirm it by reading the code.`,
      { label: `verify:${f.file}`, phase: 'Verify', schema: VERDICT_SCHEMA, agentType: 'Explore' },
    ).then((v) => ({ ...f, verdict: v })),
  ),
)

phase('Synthesize')
const rank = { Critical: 0, High: 1, Medium: 2, Low: 3 }
const confirmed = []
const refuted = []
for (const f of verified.filter(Boolean)) {
  if (f.verdict && f.verdict.refuted) {
    refuted.push({ file: f.file, claimed: f.severity, why: f.verdict.reasoning })
  } else {
    const sev = (f.verdict && f.verdict.adjustedSeverity) || f.severity
    confirmed.push({ ...f, severity: sev })
  }
}
// Medium/Low were not skeptic-verified — pass through, marked unverified.
const lower = unique
  .filter((f) => f.severity === 'Medium' || f.severity === 'Low')
  .map((f) => ({ ...f, unverified: true }))

const report = [...confirmed, ...lower].sort((a, b) => rank[a.severity] - rank[b.severity])

return {
  posture: audits.filter(Boolean).map((a) => `${a.domain}: ${a.posture}`),
  findings: report,
  refutedByVerifier: refuted,
  auditsRun: audits.map((a, i) => (a ? a.domain : `audit ${i} FAILED`)),
}
