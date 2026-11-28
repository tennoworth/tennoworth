import { describe, expect, test } from 'bun:test';
import { appliesTo, findMissingMetadata } from './check-pr-metadata';

const labels = (...names: string[]) => names.map((name) => ({ name }));
const repo = (full_name: string) => ({ repo: { full_name } });
const complete = {
  head: repo('tennoworth/tennoworth'),
  base: repo('tennoworth/tennoworth'),
  assignees: [{ login: 'PedroAmorimP' }],
  labels: labels('bug', 'desktop'),
  milestone: { title: 'Desktop 0.10.108' },
};

describe('findMissingMetadata', () => {
  test('accepts a complete pull request', () => {
    expect(findMissingMetadata(complete)).toEqual([]);
  });

  test('reports every missing field of a bare pull request', () => {
    const problems = findMissingMetadata({ labels: [], assignees: [], milestone: null });
    expect(problems).toHaveLength(4);
    expect(problems.join('\n')).toContain('no assignee');
    expect(problems.join('\n')).toContain('type label');
    expect(problems.join('\n')).toContain('area label');
    expect(problems.join('\n')).toContain('no milestone');
  });

  test('refuses two type labels', () => {
    expect(findMissingMetadata({ ...complete, labels: labels('bug', 'maintenance', 'desktop') }))
      .toEqual(['needs exactly one type label (bug, enhancement, documentation, maintenance), has bug, maintenance']);
  });

  test('refuses an area label standing in for the type', () => {
    expect(findMissingMetadata({ ...complete, labels: labels('release', 'desktop') })[0]).toContain('has none');
  });

  test('lets a documentation change ship to no area', () => {
    expect(findMissingMetadata({ ...complete, labels: labels('documentation') })).toEqual([]);
  });

  test('requires an area for other types', () => {
    expect(findMissingMetadata({ ...complete, labels: labels('maintenance') })[0]).toContain('area label');
  });

  test('refuses a milestone that is not a desktop release train', () => {
    expect(findMissingMetadata({ ...complete, milestone: { title: 'Later' } }))
      .toEqual(['milestone "Later" is not "Desktop X.Y.Z"']);
  });

  test('tolerates absent arrays in a sparse payload', () => {
    expect(findMissingMetadata({ milestone: { title: 'Desktop 1.0.0' } })).toHaveLength(3);
  });
});

describe('appliesTo', () => {
  test('applies to a branch of this repository', () => {
    expect(appliesTo(complete)).toBe(true);
  });

  test('skips a fork, whose author cannot set labels or milestones', () => {
    expect(appliesTo({ ...complete, head: repo('someone/tennoworth') })).toBe(false);
  });

  test('skips a pull request whose fork was deleted', () => {
    expect(appliesTo({ ...complete, head: { repo: null } })).toBe(false);
    expect(appliesTo({})).toBe(false);
  });
});
