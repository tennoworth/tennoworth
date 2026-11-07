import { afterEach, describe, expect, it } from 'vitest';
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { CrackAnyway } from './crack-anyway.svelte';
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

  it("cracks a relic anyway on the player's say-so, counts the crack and remembers it", async () => {
    const plan = [entry('Meso O2 Relic', 9, 6.3, 15, 'sell-intact'), entry('Meso T7 Relic', 17, 5.3, 0, 'crack')];
    const relicResult = new DomainResult<RelicPlanEntry[]>(() => []);
    relicResult.value = plan;
    relicResult.phase = 'done';
    const saved = new Map<string, string>();
    const store = { getSetting: (key: string) => saved.get(key) ?? null, setSetting: async (key: string, value: string) => { saved.set(key, value); } };
    render(RelicPlannerView, { props: { relicResult, relicPlan: plan, relicSurfaceAge: null, onretry: () => {}, crackAnyway: new CrackAnyway(store as never) } });
    const summary = () => within(screen.getByRole('group', { name: 'Relics summary' }));
    expect(summary().getByText(/≈225/)).toBeTruthy();
    // A plain crack verdict has nothing to overrule.
    expect(screen.getAllByRole('button', { name: /anyway/ })).toHaveLength(1);

    const overrule = screen.getByRole('button', { name: /^I.ll crack Meso O2 Relic anyway$/ });
    overrule.focus();
    await fireEvent.click(overrule);
    // The same button now offers the way back, and keeps focus.
    expect(document.activeElement).toBe(overrule);
    expect(overrule.textContent).toBe('Fine, sell intact');
    const row = screen.getAllByRole('row')[1];
    expect(within(row).getByText('Cracking anyway')).toBeTruthy();
    // 9 cracked at 6.3p (≈57) replaces 9 sold at 15p (135): 57 + 90.
    expect(summary().getByText(/≈147/)).toBeTruthy();
    expect(saved.get('relic-crack-anyway')).toBe('["meso_o2_relic"]');

    cleanup();
    render(RelicPlannerView, { props: { relicResult, relicPlan: plan, relicSurfaceAge: null, onretry: () => {}, crackAnyway: new CrackAnyway(store as never) } });
    expect(screen.getByText('Cracking anyway')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: /^Follow the advice for Meso O2 Relic/ }));
    expect(summary().getByText(/≈225/)).toBeTruthy();
    expect(saved.get('relic-crack-anyway')).toBe('[]');
  });
});
