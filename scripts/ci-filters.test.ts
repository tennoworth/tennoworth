import { expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

test('a README-only diff runs instruction and public-surface checks', () => {
  const workflow = Bun.YAML.parse(readFileSync(new URL('../.github/workflows/audit.yml', import.meta.url), 'utf8')) as any;
  const filterStep = workflow.jobs.changes.steps.find((step: any) => step.id === 'filter');
  const filters = Bun.YAML.parse(filterStep.with.filters) as Record<string, string[]>;
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
