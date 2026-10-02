import { expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

function load(name: string) {
  const workflow = Bun.YAML.parse(readFileSync(new URL(`../.github/workflows/${name}`, import.meta.url), 'utf8')) as any;
  const filterStep = workflow.jobs.changes.steps.find((step: any) => step.id === 'filter');
  expect(filterStep.with['predicate-quantifier']).toBe('some-with-excludes');
  return { workflow, filters: Bun.YAML.parse(filterStep.with.filters) as Record<string, string[]> };
}

// paths-filter's `some-with-excludes`: any positive pattern matches and no `!` pattern does.
function matches(patterns: string[], path: string) {
  const glob = (pattern: string) => new Bun.Glob(pattern).match(path);
  return patterns.filter(p => !p.startsWith('!')).some(glob)
    && !patterns.filter(p => p.startsWith('!')).some(p => glob(p.slice(1)));
}

test('an instruction file under a code path does not start native jobs', () => {
  const native = { 'audit.yml': 'rust_code', 'ui-smoke.yml': 'desktop' };
  for (const [name, filter] of Object.entries(native)) {
    const { filters } = load(name);
    for (const path of ['rust/AGENTS.md', 'rust/wfm-core/AGENTS.md', 'rust/wfm-client/AGENTS.md', 'frontend/AGENTS.md']) {
      expect({ name, path, native: matches(filters[filter], path) }).toEqual({ name, path, native: false });
      expect({ name, path, docs: matches(filters.docs_only, path) }).toEqual({ name, path, docs: true });
    }
    expect(matches(filters[filter], 'rust/wfm-core/src/lib.rs')).toBe(true);
  }
});

test('a README-only diff runs instruction and public-surface checks', () => {
  const { workflow, filters } = load('audit.yml');
  expect(filters.instructions.some(pattern => new Bun.Glob(pattern).match('README.md'))).toBe(true);
  expect(filters.instructions.some(pattern => new Bun.Glob(pattern).match('scripts/ci-filters.test.ts'))).toBe(true);
  expect(filters.docs_only.some(pattern => new Bun.Glob(pattern).match('README.md'))).toBe(true);
  const job = workflow.jobs['instruction-check'];
  expect(job.steps.map((step: any) => step.run)).toContain('bun test scripts/ci-filters.test.ts');
  expect(job.if).toContain("needs.changes.outputs.instructions == 'true'");
  expect(job.steps.map((step: any) => step.run)).toContain('bun scripts/check-agent-instructions.ts');
  expect(job.steps.map((step: any) => step.run)).toContain('bun scripts/check-public-surface.ts');
  expect(workflow.jobs['audit-gate'].needs).toContain('instruction-check');
});
