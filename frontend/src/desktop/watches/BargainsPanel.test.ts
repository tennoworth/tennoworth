import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, screen, waitFor } from '@testing-library/svelte';
import { renderDesktop as render } from '../../dev/preview/render-desktop';
import BargainsPanel from './BargainsPanel.svelte';
import { installTauri, removeTauri } from '../../dev/testing/test-utils';
import type { Market } from '../../contracts/data';

afterEach(() => { cleanup(); removeTauri(); vi.restoreAllMocks(); });

const market = {
  catalog: { 'wolf sledge set': 'wolf_sledge_set', 'strun prime blueprint': 'strun_prime_blueprint' },
  items: {
    wolf_sledge_set: { avg: 45, low_sell: 30, top_buy: 40, vol: 40, median_90d: 49, low5_avg: 44 },
    strun_prime_blueprint: { avg: 24, low_sell: 14, top_buy: 12, vol: 222, median_90d: 24, low5_avg: 23.6 },
  },
} as unknown as Market;

describe('BargainsPanel', () => {
  it('lists the leads and checks each one live', async () => {
    const invoke = vi.fn(async (cmd: string) => {
      if (cmd !== 'live_top_prices') throw new Error(`unexpected ${cmd}`);
      return [
        { slug: 'wolf_sledge_set', sells: [31], buys: [38], low_sell: 31, top_buy: 38, low_sell_by: 'Seller' },
        { slug: 'strun_prime_blueprint', sells: [22], buys: [], low_sell: 22, top_buy: null },
      ];
    });
    installTauri(invoke, vi.fn(async () => () => {}));
    render(BargainsPanel, { props: { market } });

    expect(screen.getByRole('link', { name: 'Wolf Sledge Set' })).toBeTruthy();
    expect(screen.getByText('flip')).toBeTruthy();
    expect(screen.getByText('cheap')).toBeTruthy();
    expect(screen.getAllByText('not checked')).toHaveLength(2);

    await fireEvent.click(screen.getByRole('button', { name: 'Check live' }));
    await waitFor(() => expect(screen.getByText('still 31p · Seller')).toBeTruthy());
    expect(screen.getByText('gone · now 22p')).toBeTruthy();
    const call = invoke.mock.calls.find(([cmd]) => cmd === 'live_top_prices') as unknown as [string, { queries: unknown }];
    // Equal margins: the busier item leads.
    expect(call[1].queries).toEqual([{ slug: 'strun_prime_blueprint' }, { slug: 'wolf_sledge_set' }]);
  });

  it('says so when nothing is mispriced', () => {
    installTauri(vi.fn(), undefined);
    render(BargainsPanel, { props: { market: { catalog: {}, items: {} } as unknown as Market } });
    expect(screen.getByText(/Nothing in this snapshot is priced far from its book/)).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Check live' })).toBeNull();
  });
});
