import { afterEach, describe, expect, it } from 'vitest';
import { cleanup, render, screen } from '@testing-library/svelte';
import CalendarLanes from './CalendarLanes.svelte';
import type { CalendarEvent } from '../../domain/calendar-events';

afterEach(cleanup);

const NOW = Date.parse('2026-11-22T18:00:00Z');
const alert = (id: string, item: string, from: string, to: string): CalendarEvent => ({
  id, kind: 'event', from: Date.parse(from), to: Date.parse(to), estimate: false, source: 'worldstate',
  text: { kind: 'alert', rewards: [{ item, count: 1 }], credits: null },
});

describe('CalendarLanes', () => {
  it('gives overlapping events of one kind their own rows and keeps the next kind below them', () => {
    render(CalendarLanes, { props: { now: NOW, surface: 'hosted', kinds: ['event', 'baro'], events: [
      alert('a', 'Kuva', '2026-11-20T19:00:00Z', '2026-11-23T19:00:00Z'),
      alert('b', 'Exilus Warframe Adapter', '2026-11-16T19:00:00Z', '2026-11-30T19:00:00Z'),
      alert('c', 'Forma', '2026-11-25T19:00:00Z', '2026-11-27T19:00:00Z'),
    ] } });
    const row = (name: RegExp) => screen.getByRole('button', { name }).style.gridRow;
    // b starts first and takes the top row; a overlaps it; c fits after a ends.
    expect([row(/^Alert: Exilus/), row(/^Alert: Kuva/), row(/^Alert: Forma/)]).toEqual(['3', '4', '4']);
    expect(screen.getByText('Baro Ki’Teer').closest('.lab')!.getAttribute('style')).toContain('grid-row: 5');
  });
});
