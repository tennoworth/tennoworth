import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { DESKTOP_CONTEXT } from '../../contracts/services';
import type { InventoryController } from '../inventory/controller.svelte';
import type { DomainResult } from './domain-result.svelte';
import type { SetReco } from '../../contracts/generated/domain';
import SetPicksView from './SetPicksView.svelte';
afterEach(cleanup);

const reco = (slug: string): SetReco => ({ kind: 'near-complete', set_slug: slug, set_name: slug, parts: [{ slug: `${slug}_a`, name: 'A', count: 1, required: 1, low_sell: 5 }], missing: [{ slug: `${slug}_b`, name: 'B', quantity: 1, low_sell: 9 } as never], missing_cost: 9, net_plat: 10 });

it('compares building and buying only for the set whose section is opened', async () => {
  const buildPlan = vi.fn(() => new Promise(() => {}));
  const market = { items: {}, set_to_parts: { one_set: { name: 'One', parts: [] }, two_set: { name: 'Two', parts: [] } } };
  const inventory = { market, resolved: { owned: new Map() } } as unknown as InventoryController;
  const setRecos = [reco('one_set'), reco('two_set')];
  render(SetPicksView, {
    props: { inventory, setSurfaceAge: null, keepSection: createRawSnippet(() => ({ render: () => '<div></div>' })), guidanceUnavailable: false,
      setResult: { phase: 'done', error: null, value: setRecos } as unknown as DomainResult<SetReco[]>, setRecos, adviceMap: new Map(), onretry: () => {} },
    context: new Map([[DESKTOP_CONTEXT, { buildPlan }]]),
  });
  const sections = screen.getAllByText('build it or buy it');
  expect(sections).toHaveLength(2);
  expect(buildPlan).not.toHaveBeenCalled();
  const details = sections[1].closest('details')!;
  details.open = true;
  await fireEvent(details, new Event('toggle'));
  await vi.waitFor(() => expect(buildPlan).toHaveBeenCalledTimes(1));
  expect((buildPlan.mock.calls[0] as unknown[])[0]).toBe('two_set');
});
