export const meta = {
  name: 'feature-ship',
  description: 'Scope the remaining next-features.html features from the loop journal, then produce an adversarially-vetted implementation plan for each',
  whenToUse: 'Planning pass before running the /goal loop. Produces vetted, priority-ordered plans; it does NOT write feature code (the repo is not under git and all four features mutate the same files, so parallel implementation would conflict — execute plans sequentially via /goal).',
  phases: [
    { title: 'Scope', detail: 'read journal + mockup + CLAUDE.md, find unfinished features' },
    { title: 'Plan', detail: 'one implementation plan per remaining feature' },
    { title: 'Critique', detail: 'adversarial review of each plan' },
    { title: 'Revise', detail: 'fold the critique into a final plan' },
  ],
}

const ROOT = '/home/prowly/Desktop/Warframe market check'

const SCOPE_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['remaining', 'doneNote'],
  properties: {
    doneNote: { type: 'string', description: 'one line: what the journal says is already shipped' },
    remaining: {
      type: 'array',
      description: 'unfinished features, in the priority order from goal.md',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['key', 'title', 'status', 'why'],
        properties: {
          key: { type: 'string', description: 'short slug, e.g. tag-chips' },
          title: { type: 'string' },
          status: { type: 'string', enum: ['not-started', 'partial', 'blocked'] },
          why: { type: 'string', description: 'evidence from journal/code for this status' },
        },
      },
    },
  },
}

const PLAN_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['feature', 'dataLayer', 'ui', 'tests', 'files', 'risks'],
  properties: {
    feature: { type: 'string' },
    dataLayer: { type: 'string', description: 'market.json / csv_to_market_json.py changes, or "none"' },
    ui: { type: 'string', description: 'component changes — which files, what derived state, what renders' },
    tests: { type: 'string', description: 'vitest/pytest cases to add, and the Playwright probe to confirm' },
    files: { type: 'array', items: { type: 'string' }, description: 'exact files this plan touches' },
    risks: { type: 'array', items: { type: 'string' } },
  },
}

const CRITIQUE_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['soundEnough', 'objections'],
  properties: {
    soundEnough: { type: 'boolean', description: 'true if no objection would change the plan materially' },
    objections: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['concern', 'evidence', 'fix'],
        properties: {
          concern: { type: 'string' },
          evidence: { type: 'string', description: 'concrete file/line/edge-case/default that the plan gets wrong' },
          fix: { type: 'string' },
        },
      },
    },
  },
}

phase('Scope')
const scope = await agent(
  `Determine which of the four next-features for the wfminv project are still unfinished.
Read, in order:
  - ${ROOT}/.loop-journal.md  (source of truth on what's shipped and what was tried — read ALL entries)
  - ${ROOT}/.mockups/next-features.html  (the designs)
  - ${ROOT}/.claude/commands/goal.md  (the priority order and the agreed defaults)
  - ${ROOT}/prototype/src/App.svelte and components/ResultsTable.svelte (confirm by inspection what's actually wired up)
The four features in priority order are: (1) tag-chip filter row, (2) Δ-vs-90d badge + sparkline column, (3) set-completion card, (4) relic planner card.
Return only the ones NOT fully shipped, in priority order, each with evidence for its status.`,
  { label: 'scope', phase: 'Scope', schema: SCOPE_SCHEMA, agentType: 'Explore' },
)

log(`remaining: ${scope.remaining.map((r) => r.key).join(', ') || 'none'} — ${scope.doneNote}`)

if (!scope.remaining.length) {
  return { remaining: [], plans: [], note: `Nothing left to plan. ${scope.doneNote}` }
}

const plans = await pipeline(
  scope.remaining,
  (feat) =>
    agent(
      `Produce a tight, end-to-end implementation plan for this wfminv feature: "${feat.title}" (status: ${feat.status}; ${feat.why}).
Read CLAUDE.md, prototype/CLAUDE.md, the relevant mockup section in ${ROOT}/.mockups/next-features.html, App.svelte, ResultsTable.svelte, and scripts/csv_to_market_json.py before planning.
Honor the agreed defaults in .claude/commands/goal.md (Δ direction = up-is-good, set-card placement/cap, tag-chip coverage, relic data source, sparkline empty-data handling). Plan the SMALLEST slice: data layer first, then UI, then tests. Name exact files.`,
      { label: `plan:${feat.key}`, phase: 'Plan', schema: PLAN_SCHEMA, agentType: 'general-purpose' },
    ).then((p) => ({ feat, plan: p })),
  ({ feat, plan }) =>
    agent(
      `Adversarially critique this implementation plan for the wfminv feature "${feat.title}". Argue it is wrong. Find the missing edge case, the wrong default, the file it forgot, the market.json shape bump it skipped, the test that pins internals instead of contract.
PLAN:
${JSON.stringify(plan, null, 2)}
Be specific — cite files, lines, edge cases, failure modes. Set soundEnough=false if any objection would materially change the plan.`,
      { label: `critique:${feat.key}`, phase: 'Critique', schema: CRITIQUE_SCHEMA, agentType: 'Explore' },
    ).then((crit) => ({ feat, plan, crit })),
  ({ feat, plan, crit }) => {
    if (crit.soundEnough || !crit.objections.length) {
      return Promise.resolve({ feat, finalPlan: plan, critique: crit, revised: false })
    }
    return agent(
      `Revise this implementation plan for the wfminv feature "${feat.title}" to address the adversarial objections below. Keep the same plan shape; fold in the fixes that have merit and note any objection you consciously reject and why.
ORIGINAL PLAN:
${JSON.stringify(plan, null, 2)}
OBJECTIONS:
${JSON.stringify(crit.objections, null, 2)}`,
      { label: `revise:${feat.key}`, phase: 'Revise', schema: PLAN_SCHEMA, agentType: 'general-purpose' },
    ).then((finalPlan) => ({ feat, finalPlan, critique: crit, revised: true }))
  },
)

return {
  doneNote: scope.doneNote,
  remaining: scope.remaining.map((r) => ({ key: r.key, status: r.status })),
  plans: plans.filter(Boolean).map((p) => ({
    key: p.feat.key,
    title: p.feat.title,
    revised: p.revised,
    openObjections: (p.critique.objections || []).filter(() => !p.critique.soundEnough).length,
    plan: p.finalPlan,
  })),
}
