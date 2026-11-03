import { afterEach, describe, expect, it } from 'vitest';
import { cleanup, render, screen, within } from '@testing-library/svelte';
import RelicPlannerView from './RelicPlannerView.svelte';
import { DomainResult } from '../selling/domain-result.svelte';
import type { RelicPlanEntry, RelicVerdict } from '../../contracts/generated/domain';

afterEach(cleanup);

function entry(name: string, owned: number, epp: number, sellNow: number, verdict: RelicVerdict): RelicPlanEntry {
  return {
    relic_slug: name.toLowerCase().replace(/ /g, '_'), relic_name: name, owned, epp, epp_owned: epp * owned,
    sell_now: sellNow, moving_count: 6, total_rewards: 6, rewards: [],
    decision: { ladder: [], best: null, sellNow, movingCount: 6, totalRewards: 6, verdict },
  };
}

describe('RelicPlannerView', () => {
  it('values each relic by its own decision, so Sell intact counts the sale', () => {
    const plan = [entry('Meso O2 Relic', 9, 6.3, 15, 'sell-intact'), entry('Meso T7 Relic', 17, 5.3, 0, 'crack')];
    const relicResult = new DomainResult<RelicPlanEntry[]>(() => []);
    relicResult.value = plan;
    relicResult.phase = 'done';
    render(RelicPlannerView, { props: { relicResult, relicPlan: plan, relicSurfaceAge: null, onretry: () => {} } });
    // 9 sold intact at 15p, not 9 cracked at 6.3p; the crack row keeps its EV.
    const rows = screen.getAllByRole('row').slice(1);
    expect(within(rows[0]).getByTitle('Selling all 9 intact at 15p each.').textContent).toMatch(/≈135\s*p/);
    expect(within(rows[1]).getByText(/≈90/)).toBeTruthy();
    expect(within(screen.getByRole('group', { name: 'Relics summary' })).getByText(/≈225/)).toBeTruthy();
  });
});
