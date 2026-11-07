// Component-level tests for the Sell view: the pick strip (snooze), the
// tag-chip toggle, the "List N on WFM" staging label, and the empty-state
// quick-fix buttons. The scoring itself is covered by sell-priority.test.ts
// + the parity fixture; this file drives the component's event wiring.
import { describe, it, expect, vi, afterEach } from 'vitest';
import { screen, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import { renderDesktop as render } from '../../dev/preview/render-desktop';
import SellPane from './SellPane.svelte';
import { createRawSnippet, type ComponentProps } from 'svelte';
import type { SellRow } from '../../contracts/selling';

// Full ResultsTable row shape (SellPane passes results through to the table).
function row(overrides: Partial<SellRow> = {}): SellRow {
  return {
    key: 'k1', slug: 'accelerated_blast', subtype: null, name: 'Accelerated Blast',
    owned: 3, sellable: 2, leveled: 0, type: 'RawUpgrades', kept_lvl: null,
    ducats: null, plat_per_100d: null, avg_price: 20, low_sell: 15, low5_avg: 14,
    top_buy: 10, volume_48h: 40, ratio: 1.4, potential_plat: 100, raw_value: 120,
    sell_score: 45, patience: false, timing: 'peak', medians_7d: [15, 16, 15],
    median_90d: 15, delta_90d_pct: 12, tags: ['peak'], is_augment: false,
    vault_status: 'available',
    ...overrides,
  };
}

function renderPane(props: Partial<ComponentProps<typeof SellPane>> = {}) {
  const base: ComponentProps<typeof SellPane> = {
    minPrice: 10, minOwned: 2, typeFilter: 'all', hideAtLvl: 11,
    activeTags: new Set(), tableView: { rows: [], active: false },
    resolved: { owned: new Map(), unresolved: {} },
    results: [row()],
    deltas: new Map(),
    totalPotential: 100,
    marketFreshness: 'fresh', marketStaleness: null, marketLoadError: null,
    listableRows: [row({ slug: 'a', key: 'a' }), row({ slug: 'b', key: 'b' }), row({ slug: 'c', key: 'c' })],
    availableTags: [['prime', 1]],
    availableTypes: ['RawUpgrades', 'MiscItems'],
    visibleColumns: null, presetSort: null, emptyReason: null,
    activePreset: null, reserveCopies: 0, filtersOpen: false,
    sellOnboardingDismissed: true, keepCopiesNudgeDismissed: true,

    applyPreset: vi.fn(), setReserveCopies: vi.fn(), toggleFiltersOpen: vi.fn(),
    dismissSellOnboarding: vi.fn(), dismissKeepCopiesNudge: vi.fn(),
    openListingFlow: vi.fn(), pendingBanner: createRawSnippet(() => ({ render: () => '<span></span>' })),
    ...props,
  };
  return render(SellPane, { props: base });
}

afterEach(cleanup);

describe('SellPane', () => {
  it('labels the bulk listing action without a batch size and states the staged count in its title', () => {
    renderPane();
    expect(screen.getByRole('heading', { name: 'Sell' })).toBeTruthy();
    const stage = screen.getByRole('button', { name: 'List on WFM' });
    expect(stage.getAttribute('title')).toContain('Stage the top 3 rows');
  });

  it('toggles a tag chip through the bindable activeTags', async () => {
    renderPane();
    const chip = screen.getByRole('button', { name: 'prime 1' }) as HTMLButtonElement;
    expect(chip.getAttribute('aria-pressed')).toBe('false');
    await fireEvent.click(chip);
    await waitFor(() => expect(chip.getAttribute('aria-pressed')).toBe('true'));
    // clearing the selection renders the explicit clear button
    await waitFor(() => expect(screen.getByRole('button', { name: /clear/ })).toBeTruthy());
  });

  it('keeps the main tags on one line and the rest in a grouped list that narrows as you type', async () => {
    const availableTags: [string, number][] = [['mod', 30], ['rare', 25], ['weapon', 12], ['melee', 9], ['nyx', 4], ['jat_kittag', 2], ['axi', 1]];
    renderPane({ availableTags });
    const line = screen.getByRole('group', { name: 'Item types' });
    const onLine = () => [...line.querySelectorAll('button.chip .chip-label')].map((label) => label.firstChild?.textContent?.trim());
    expect(onLine()).toEqual(['mod', 'melee']);
    const more = screen.getByRole('button', { name: '+ 5 more' });
    await fireEvent.click(more);
    expect(more.getAttribute('aria-expanded')).toBe('true');
    expect(screen.getByRole('group', { name: 'Rarity' })).toBeTruthy();
    expect(screen.getByRole('group', { name: 'Relic era' })).toBeTruthy();
    const names = screen.getByRole('group', { name: 'Mods for a Warframe, weapon or companion' });
    expect(names.textContent).toContain('Jat Kittag');
    const narrow = screen.getByRole('searchbox', { name: 'Narrow tags' });
    await fireEvent.input(narrow, { target: { value: 'jat k' } });
    await waitFor(() => expect(screen.queryByRole('group', { name: 'Rarity' })).toBeNull());
    expect(screen.getByRole('button', { name: 'Jat Kittag 2' })).toBeTruthy();
    // A tag picked from the list stays put while the list is open, so nothing
    // moves under the pointer, and joins the main line once it closes.
    const jat = screen.getByRole('button', { name: 'Jat Kittag 2' });
    await fireEvent.click(jat);
    await waitFor(() => expect(jat.getAttribute('aria-pressed')).toBe('true'));
    expect(onLine()).toEqual(['mod', 'melee']);
    await fireEvent.input(narrow, { target: { value: 'oberon' } });
    await waitFor(() => expect(screen.getByText('No tag matches “oberon”.')).toBeTruthy());
    await fireEvent.keyDown(narrow, { key: 'Escape' });
    await waitFor(() => expect(screen.queryByRole('searchbox', { name: 'Narrow tags' })).toBeNull());
    expect(onLine()).toEqual(['mod', 'melee', 'Jat Kittag']);
  });

  it('splits the picks where today\'s trades run out, and not when the allowance is unknown', async () => {
    const allowance = { remaining: 3, mastery_rank: 12, observed_at: 1, utc_day: 1, snapshot_id: 1, confidence: 'scanned' as const, reason: null, monitoring: true };
    // Two non-bulk copies take 2 trades; nine bulk copies take 2 more, past the 3 left.
    const results = [row(), row({ key: 'k2', slug: 'arcane_energize', name: 'Arcane Energize', sellable: 9, sell_score: 40 })];
    renderPane({ results, tradeAllowance: allowance, bulkSlugs: new Set(['arcane_energize']) });
    expect(screen.getByText(/Uses 2 of 3 trades today · 1 after the reset/)).toBeTruthy();
    expect(screen.getByText(/After the reset/)).toBeTruthy();
    expect(screen.getAllByText(/^2 trades · \d+p per trade$/)).toHaveLength(2);
    // Hiding the first pick frees its trades, so the second now fits today.
    await fireEvent.click(screen.getByRole('button', { name: 'Hide Accelerated Blast for this session' }));
    await waitFor(() => expect(screen.queryByText(/After the reset/)).toBeNull());
    cleanup();
    renderPane({ results, tradeAllowance: { ...allowance, remaining: null }, bulkSlugs: new Set(['arcane_energize']) });
    expect(screen.queryByText(/After the reset/)).toBeNull();
    expect(screen.queryByText(/trades left today/)).toBeNull();
    cleanup();
    // Bulk status unknown (warframe.market locked): ranges, and no guessed divider.
    renderPane({ results, tradeAllowance: allowance, bulkSlugs: null });
    expect(screen.getByText('2–9 trades')).toBeTruthy();
    expect(screen.queryByText(/After the reset/)).toBeNull();
    expect(screen.getByText('3 trades left today').closest('[title]')?.getAttribute('title')).toMatch(/count each pick’s trades exactly/);
  });

  it('lists the picks after the reset in one batch and draws the day as a gauge', async () => {
    const allowance = { remaining: 3, mastery_rank: 12, observed_at: 1, utc_day: 1, snapshot_id: 1, confidence: 'scanned' as const, reason: null, monitoring: true };
    const later = [row({ key: 'k2', slug: 'arcane_energize', name: 'Arcane Energize', sellable: 9, sell_score: 40 }), row({ key: 'k3', slug: 'galvanized_chamber', name: 'Galvanized Chamber', sellable: 1, sell_score: 30 })];
    const openListingFlow = vi.fn();
    const view = renderPane({ results: [row(), ...later], tradeAllowance: allowance, bulkSlugs: new Set(['arcane_energize']), openListingFlow });
    await fireEvent.click(screen.getByRole('button', { name: 'List these 2 now' }));
    expect(openListingFlow).toHaveBeenCalledWith(later);
    // 12 trades a day: the picks use 2, 1 stays free, 9 are already spent.
    const count = (kind: string) => view.container.querySelectorAll(`.trade-gauge i.${kind}`).length;
    expect([count('plan'), count('free'), count('spent')]).toEqual([2, 1, 9]);
    cleanup();
    // Without the rank the day's size is unknown, so no gauge.
    const unranked = renderPane({ results: [row(), ...later], tradeAllowance: { ...allowance, mastery_rank: null }, bulkSlugs: new Set(['arcane_energize']) });
    expect(unranked.container.querySelector('.trade-gauge')).toBeNull();
  });

  it('hides a pick behind an Undo receipt that keeps its slot', async () => {
    renderPane();
    expect(screen.getByRole('button', { name: 'List Accelerated Blast on WFM' })).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Hide Accelerated Blast for this session' }));
    // The row stays where it was, as a receipt, so nothing moves under the pointer.
    await waitFor(() => expect(screen.getByText('Hidden for this session.')).toBeTruthy());
    expect(screen.queryByRole('button', { name: 'List Accelerated Blast on WFM' })).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: 'Undo hiding Accelerated Blast' }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'List Accelerated Blast on WFM' })).toBeTruthy());
    expect(screen.queryByText('Hidden for this session.')).toBeNull();
  });

  it('words a stale snapshot age as a time, not "3 d ago old"', () => {
    renderPane({ marketFreshness: 'stale', marketStaleness: '3 d ago' });
    expect(screen.getByText(/market snapshot was updated 3 d ago\. Rankings/)).toBeTruthy();
  });

  it('relaxFilters drops min price to 1 from the empty state', async () => {
    renderPane({ results: [], emptyReason: { kind: 'price', excluded: 3, candidates: 3 } });
    await fireEvent.click(screen.getByRole('button', { name: 'Drop min price to 1p' }));
    await waitFor(() => expect(screen.getByDisplayValue('1')).toBeTruthy());
  });
});

it('keeps filter edits while native values load and disables stale listing actions', async () => {
  const openListingFlow = vi.fn();
  const view = renderPane({ calculationPending: true, openListingFlow });
  const stage = screen.getByRole('button', { name: 'List on WFM' }) as HTMLButtonElement;
  expect(stage.getAttribute('aria-disabled')).toBe('true');
  expect(screen.getByRole('status').textContent).toContain('Calculating sale values');
  expect(screen.getByRole('group', { name: 'Sell summary' }).textContent).not.toContain('100p');
  await fireEvent.click(stage);
  expect(openListingFlow).not.toHaveBeenCalled();
  const chip = screen.getByRole('button', { name: 'prime 1' });
  await fireEvent.click(chip);
  await waitFor(() => expect(chip.getAttribute('aria-pressed')).toBe('true'));
  expect(screen.queryByRole('button', { name: 'Hide Accelerated Blast for this session' })).toBeNull();
  await view.rerender({ calculationPending: false, activeTags: new Set(['prime']) });
  expect(screen.getByRole('button', { name: 'Hide Accelerated Blast for this session' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'prime 1' })).toBe(chip);
  expect(chip.getAttribute('aria-pressed')).toBe('true');
  expect(stage.getAttribute('aria-disabled')).toBeNull();
});

it('shows native failure as unavailable values with a retry action', async () => {
  const retry = vi.fn();
  renderPane({ results: [], calculationError: 'Snapshot invalid', onretryCalculation: retry });
  expect(screen.getByRole('alert').textContent).toContain('Snapshot invalid');
  const summary = screen.getByRole('group', { name: 'Sell summary' });
  expect(summary.textContent).not.toContain('100p');
  expect(summary.textContent).toContain('Unavailable');
  await fireEvent.click(screen.getByRole('button', { name: 'Retry calculations' }));
  expect(retry).toHaveBeenCalledOnce();
});

it('shows the filtered sellable count against the whole inventory', () => {
  renderPane({ allSellable: 480 });
  expect(screen.getByRole('group', { name: 'Sell summary' }).textContent).toMatch(/Sellable\s*\d+ of 480/);
});

it('hides the total when the filters show every sellable item', () => {
  renderPane({ allSellable: null });
  expect(screen.getByRole('group', { name: 'Sell summary' }).textContent).not.toMatch(/ of \d/);
});
