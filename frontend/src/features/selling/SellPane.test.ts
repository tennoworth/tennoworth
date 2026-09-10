// Component-level tests for the Sell view: the pick strip (snooze), the
// tag-chip toggle, the "List N on WFM" staging label, and the empty-state
// quick-fix buttons. The scoring itself is covered by sell-priority.test.ts
// + the parity fixture; this file drives the component's event wiring.
import { describe, it, expect, vi, afterEach } from 'vitest';
import { screen, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import { renderDesktop as render } from '../../dev/render-desktop';
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
    scoreExplainerDismissed: true, sellOnboardingDismissed: true, keepCopiesNudgeDismissed: true,
    isDesktop: true,
    applyPreset: vi.fn(), setReserveCopies: vi.fn(), toggleFiltersOpen: vi.fn(),
    dismissSellOnboarding: vi.fn(), dismissKeepCopiesNudge: vi.fn(),
    openListingFlow: vi.fn(), pendingBanner: createRawSnippet(() => ({ render: () => '<span></span>' })),
    ...props,
  };
  return render(SellPane, { props: base });
}

afterEach(cleanup);

describe('SellPane', () => {
  it('renders the staged listing label from listableRows', () => {
    renderPane();
    expect(screen.getByRole('heading', { name: 'Sell' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'List 3 on WFM' })).toBeTruthy();
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

  it('snoozes a pick for the session', async () => {
    renderPane();
    expect(screen.getByRole('button', { name: 'List Accelerated Blast on WFM' })).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Hide Accelerated Blast for this session' }));
    await waitFor(() =>
      expect(screen.getByText('All picks snoozed for this session.')).toBeTruthy(),
    );
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
  const stage = screen.getByRole('button', { name: 'List 3 on WFM' }) as HTMLButtonElement;
  expect(stage.disabled).toBe(true);
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
  expect(stage.disabled).toBe(false);
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
