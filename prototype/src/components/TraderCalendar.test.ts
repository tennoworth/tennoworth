import { afterEach, describe, expect, it } from 'vitest';
import { cleanup, render, screen } from '@testing-library/svelte';

import type { Market, OwnedRecord } from '../lib/types';
import TraderCalendar from './TraderCalendar.svelte';

afterEach(cleanup);

const NOW = Date.parse('2026-08-22T00:00:00Z');

function market(completeness: 'complete' | 'partial' | 'unknown', fetched = '2026-08-22T00:00:00Z') {
  return {
    event_rewards: {
      goals: {
        goal: {
          id: 'goal', source: 'goal', title: 'Community Goal',
          starts_at: '2026-08-24T00:00:00Z', ends_at: '2026-08-30T00:00:00Z',
          completeness,
          groups: completeness === 'unknown' ? [] : [{ kind: 'final', credits: 50000, rewards: [
            { unique: '/Lotus/PrimedFury', name: 'Primed Fury', slug: 'primed_fury', quantity: 1 },
            ...(completeness === 'partial'
              ? [{ unique: '/Lotus/Unknown', name: 'Unknown Reward', quantity: 1 }]
              : []),
          ] }],
        },
      },
    },
    surface_provenance: {
      'world.goals': { disposition: 'used_current', attempted_at: fetched, data_fetched_at: fetched },
    },
  } as unknown as Market;
}

function owned(slugs: string[]): Map<string, OwnedRecord> {
  return new Map(slugs.map((slug) => [`${slug}|`, { slug, count: 1 } as OwnedRecord]));
}

describe('TraderCalendar event rewards', () => {
  it('asks for a scan instead of claiming unknown inventory is unaffected', () => {
    render(TraderCalendar, { props: { market: market('complete'), owned: null, now: NOW } });
    expect(screen.getByText('scan to check')).toBeTruthy();
  });

  it('labels complete misses and partial hits precisely', () => {
    const first = render(TraderCalendar, {
      props: { market: market('complete'), owned: owned([]), now: NOW },
    });
    expect(screen.getByText('none you hold')).toBeTruthy();
    first.unmount();

    render(TraderCalendar, {
      props: { market: market('partial'), owned: owned(['primed_fury']), now: NOW },
    });
    expect(screen.getByText('affects at least 1 you hold')).toBeTruthy();
    expect(screen.getByText(/partial coverage/)).toBeTruthy();
    expect(screen.getByText(/50,000 credits/)).toBeTruthy();
  });

  it('never presents an unsupported reward shape as none held', () => {
    render(TraderCalendar, {
      props: { market: market('unknown'), owned: owned([]), now: NOW },
    });
    expect(screen.getByText('reach unknown')).toBeTruthy();
    expect(screen.queryByText('none you hold')).toBeNull();
  });

  it('shows stale reward data without hiding the event', () => {
    render(TraderCalendar, {
      props: { market: market('complete', '2026-08-01T00:00:00Z'), owned: owned([]), now: NOW },
    });
    expect(screen.getByText('Community Goal')).toBeTruthy();
    expect(screen.getByText(/reward data 21d old/)).toBeTruthy();
  });
});
