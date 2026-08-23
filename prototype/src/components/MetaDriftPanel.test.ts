import { afterEach, describe, expect, it } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import MetaDriftPanel from './MetaDriftPanel.svelte';

afterEach(cleanup);

const row = (name: string, category: string, share: number) => ({ name, category, share });
const market = {
  items: {
    gain: { low_sell: 18, vol: 44 }, loss: { low_sell: 7, vol: 3 },
    new_item: { low_sell: 25, vol: 11 }, old_item: { low_sell: 2, vol: 1 },
  },
  usage_history: {
    years: [2023, 2025],
    by_year: {
      2023: { gain: row('Gain Gear', 'Primary', 1), loss: row('Loss Gear', 'Secondary', 4), old_item: row('Old Gear', 'Primary', 1) },
      2025: { gain: row('Gain Gear', 'Primary', 2.5), loss: row('Loss Gear', 'Secondary', 1), new_item: row('New Gear', 'Primary', 3) },
    },
  },
} as any;

describe('MetaDriftPanel', () => {
  it('renders real year gap, percentage points, price and volume', () => {
    render(MetaDriftPanel, { props: { market } });
    expect(screen.getByText('DE equip share · 2023 → 2025 (2024 unavailable)')).toBeTruthy();
    expect(screen.getByText('+1.50 pp')).toBeTruthy();
    expect(screen.getByText('18p')).toBeTruthy();
    expect(screen.getByText('44')).toBeTruthy();
  });

  it('filters categories/search and exposes loss and only-in-year tabs', async () => {
    render(MetaDriftPanel, { props: { market } });
    await fireEvent.click(screen.getByRole('button', { name: 'Losses' }));
    expect(screen.getByText('Loss Gear')).toBeTruthy();
    expect(screen.getByText('-3.00 pp')).toBeTruthy();
    await fireEvent.change(screen.getByLabelText('Filter meta drift category'), { target: { value: 'Primary' } });
    expect(screen.queryByText('Loss Gear')).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: 'Only in year data' }));
    expect(screen.getByText('Only in 2025 data')).toBeTruthy();
    expect(screen.getByText('Only in 2023 data')).toBeTruthy();
    await fireEvent.input(screen.getByLabelText('Search meta drift'), { target: { value: 'new' } });
    expect(screen.getByText('New Gear')).toBeTruthy();
    expect(screen.queryByText('Old Gear')).toBeNull();
  });

  it('quietly renders nothing for old one-year snapshots', () => {
    const { container } = render(MetaDriftPanel, { props: { market: { items: {}, usage_history: { years: [2025], by_year: { 2025: { one: row('One', 'Primary', 1) } } } } as any } });
    expect(container.querySelector('[data-testid="meta-drift"]')).toBeNull();
  });
});
