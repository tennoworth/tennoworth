// Feature rail: the hosted landing's breadth reveal. The rail is the only
// place a visitor learns the app is more than a price lookup, so the tab set
// itself is the contract — plus the WAI-ARIA tabs wiring, which is easy to
// half-implement (a roving tabindex without arrow keys renders a tab list
// nothing but a mouse can drive).
import { describe, it, expect, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/svelte';
import FeatureRail from './FeatureRail.svelte';

afterEach(() => { cleanup(); });

const TITLES = [
  'Sell', 'Set picks', 'Relics', 'Rivens',
  'Price watches', 'Ledger', 'My orders', 'Advisor',
];

/** The one tab with aria-selected="true". */
function selected(): HTMLElement {
  const tabs = screen.getAllByRole('tab');
  const on = tabs.filter((t) => t.getAttribute('aria-selected') === 'true');
  expect(on).toHaveLength(1);
  return on[0];
}

describe('FeatureRail', () => {
  it('renders one tab per surface, in the app\'s sidebar order', () => {
    render(FeatureRail);
    expect(screen.getAllByRole('tab').map((t) => t.textContent?.trim())).toEqual(TITLES);
  });

  it('shows the panel for the selected tab, and only that one', async () => {
    render(FeatureRail);
    // Sell leads.
    expect(selected().textContent?.trim()).toBe('Sell');
    expect(screen.getAllByRole('tabpanel')).toHaveLength(1);
    expect(screen.getByRole('tabpanel').textContent).toContain('prioritization score');
    // The Sell miniature's own columns, not another tab's.
    expect(screen.getByRole('columnheader', { name: 'Score' })).toBeDefined();

    await fireEvent.click(screen.getByRole('tab', { name: 'Ledger' }));
    expect(selected().textContent?.trim()).toBe('Ledger');
    const panel = screen.getByRole('tabpanel');
    expect(screen.getAllByRole('tabpanel')).toHaveLength(1);
    expect(panel.textContent).toContain('EE.log');
    // The panel is the one this tab points at.
    expect(panel.id).toBe(screen.getByRole('tab', { name: 'Ledger' }).getAttribute('aria-controls'));
    expect(panel.getAttribute('aria-labelledby')).toBe(screen.getByRole('tab', { name: 'Ledger' }).id);
  });

  it('moves selection with the arrow keys on both axes, and wraps', async () => {
    render(FeatureRail);
    const tab = (name: string) => screen.getByRole('tab', { name });

    await fireEvent.keyDown(tab('Sell'), { key: 'ArrowDown' });
    expect(selected().textContent?.trim()).toBe('Set picks');

    // The rail stacks to a horizontal strip under 48rem, so Right must work too.
    await fireEvent.keyDown(selected(), { key: 'ArrowRight' });
    expect(selected().textContent?.trim()).toBe('Relics');

    await fireEvent.keyDown(selected(), { key: 'ArrowUp' });
    expect(selected().textContent?.trim()).toBe('Set picks');
    await fireEvent.keyDown(selected(), { key: 'ArrowLeft' });
    expect(selected().textContent?.trim()).toBe('Sell');

    // Wrap backwards off the first tab.
    await fireEvent.keyDown(selected(), { key: 'ArrowLeft' });
    expect(selected().textContent?.trim()).toBe('Advisor');
    // ...and forwards off the last.
    await fireEvent.keyDown(selected(), { key: 'ArrowRight' });
    expect(selected().textContent?.trim()).toBe('Sell');
  });

  it('jumps to the ends with Home and End', async () => {
    render(FeatureRail);
    await fireEvent.keyDown(screen.getByRole('tab', { name: 'Sell' }), { key: 'End' });
    expect(selected().textContent?.trim()).toBe('Advisor');
    expect(screen.getByRole('tabpanel').textContent).toContain('prime calendar');

    await fireEvent.keyDown(selected(), { key: 'Home' });
    expect(selected().textContent?.trim()).toBe('Sell');
  });

  it('keeps a roving tabindex so the rail is one tab stop', async () => {
    render(FeatureRail);
    const tabIndexes = () => screen.getAllByRole('tab').map((t) => t.getAttribute('tabindex'));
    expect(tabIndexes().filter((v) => v === '0')).toHaveLength(1);
    expect(screen.getByRole('tab', { name: 'Sell' }).getAttribute('tabindex')).toBe('0');

    await fireEvent.click(screen.getByRole('tab', { name: 'Rivens' }));
    expect(tabIndexes().filter((v) => v === '0')).toHaveLength(1);
    expect(screen.getByRole('tab', { name: 'Rivens' }).getAttribute('tabindex')).toBe('0');
    expect(screen.getByRole('tab', { name: 'Sell' }).getAttribute('tabindex')).toBe('-1');
  });

  it('gives every tab a panel with its own miniature', async () => {
    render(FeatureRail);
    for (const title of TITLES) {
      await fireEvent.click(screen.getByRole('tab', { name: title }));
      const panel = screen.getByRole('tabpanel');
      expect(panel.getAttribute('aria-labelledby')).toBe(screen.getByRole('tab', { name: title }).id);
      // Each miniature is a real table of sample rows, not a picture.
      expect(panel.querySelectorAll('tbody tr').length).toBeGreaterThanOrEqual(2);
      expect(panel.querySelectorAll('tbody tr').length).toBeLessThanOrEqual(6);
    }
  });
});
