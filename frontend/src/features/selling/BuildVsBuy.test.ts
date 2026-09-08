import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/svelte';
import { DESKTOP_CONTEXT } from '../../contracts/services';
import type { BuildPlan } from '../../domain/build-cost';
import BuildVsBuy from './BuildVsBuy.svelte';
afterEach(cleanup);
const props = { setSlug: 'a', setName: 'A', parts: [], market: null };
const result = (name: string) => ({ plan: { setSlug: name, setName: name, have: [{ slug: name, name, count: 1 }], missing: [], setPrice: null, paths: [], incomplete: false } satisfies BuildPlan, cheapest: null });
it('shows unavailable after native failure instead of calculating a browser recommendation', async () => {
  const buildPlan = vi.fn().mockRejectedValue(new Error('native unavailable'));
  render(BuildVsBuy, { props, context: new Map([[DESKTOP_CONTEXT, { buildPlan }]]) });
  expect((await screen.findByRole('alert')).textContent).toContain('Build comparison unavailable');
  expect(screen.queryByRole('table')).toBeNull();
});
it('ignores a late reply from the previously selected set', async () => {
  let finishFirst!: (value: ReturnType<typeof result>) => void;
  const buildPlan = vi.fn().mockImplementationOnce(() => new Promise((resolve) => { finishFirst = resolve; })).mockResolvedValue(result('New part'));
  const view = render(BuildVsBuy, { props, context: new Map([[DESKTOP_CONTEXT, { buildPlan }]]) });
  await waitFor(() => expect(buildPlan).toHaveBeenCalledTimes(1));
  await view.rerender({ ...props, setSlug: 'b', setName: 'B' });
  await screen.findByText(/You hold New part/);
  finishFirst(result('Old part'));
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(screen.queryByText(/You hold Old part/)).toBeNull();
  expect(screen.getByText(/You hold New part/)).toBeTruthy();
});
