// @ts-nocheck
import { describe, it, expect } from 'vitest';
import { advise, adviseOwned, buildPartToSet, slope30, preVaultMedian } from './advisor.js';

const NOW = Date.parse('2026-08-20T00:00:00Z');
const day = 86400000;

/** A history series: `spec` is [count, value][] segments, oldest first. */
function series(spec) {
  const median = [];
  for (const [n, v] of spec) for (let i = 0; i < n; i++) median.push(v);
  return { median, volume: median.map((m) => (m == null ? 0 : 3)) };
}

function marketWith({ primes = {}, resurgence_current = null, items = {}, set_to_parts = {} } = {}) {
  return {
    items,
    set_to_parts,
    calendar: { primes, resurgence_current, resurgence: [] },
  };
}

function historyWith(items, daysAgoStart = 365) {
  const start = new Date(NOW - daysAgoStart * day).toISOString().slice(0, 10);
  return { generated_at: '', start, days: daysAgoStart, items };
}

const gauss = {
  gauss_prime_set: {
    name: 'Gauss Prime',
    released: '2024-05-01',
    vaulted: true,
    vault_date: '2026-06-10',
    est_vault_date: '2026-06-10',
  },
};
const p2s = { gauss_prime_set: { name: 'Gauss Prime', parts: [{ slug: 'gauss_prime_chassis', component_name: 'Chassis' }] } };

describe('advise', () => {
  it('returns null for items with no calendar entry — no advice beats a guess', () => {
    const market = marketWith({ primes: gauss, set_to_parts: p2s });
    const partToSet = buildPartToSet(market);
    expect(advise({ slug: 'ash_prime_blueprint', market, history: null, partToSet, nowMs: NOW })).toBeNull();
  });

  it('parts resolve through their set for the calendar lookup', () => {
    const market = marketWith({ primes: gauss, set_to_parts: p2s });
    const v = advise({ slug: 'gauss_prime_chassis', market, history: null, partToSet: buildPartToSet(market), nowMs: NOW });
    expect(v).not.toBeNull();
    expect(v.advice).toBe('hold'); // vaulted 71 d ago, no price data → calendar call stands
    expect(v.reasons[0]).toContain('vaulted 71 d ago');
  });

  it('fresh release → sell_now into the launch demand', () => {
    const market = marketWith({
      primes: { styanax_prime_set: { name: 'Styanax Prime', released: '2026-08-01', vaulted: false } },
      set_to_parts: { styanax_prime_set: { name: 'Styanax Prime', parts: [] } },
    });
    const v = advise({ slug: 'styanax_prime_set', market, history: null, partToSet: buildPartToSet(market), nowMs: NOW });
    expect(v.advice).toBe('sell_now');
    expect(v.reasons[0]).toContain('released 19 d ago');
  });

  it('active resurgence + falling price → sell into the flood; stable price → neutral', () => {
    const rc = { from: '2026-08-06T18:00:00Z', to: '2026-09-03T18:00:00Z', frames: ['revenant_prime_set'] };
    const primes = { revenant_prime_set: { name: 'Revenant Prime', released: '2023-10-18', vaulted: true, vault_date: '2025-02-01' } };
    const s2p = { revenant_prime_set: { name: 'Revenant Prime', parts: [] } };
    const falling = historyWith({ revenant_prime_set: series([[335, 100], [15, 100], [15, 80]]) });
    const market = marketWith({ primes, resurgence_current: rc, set_to_parts: s2p });
    const v1 = advise({ slug: 'revenant_prime_set', market, history: falling, partToSet: buildPartToSet(market), nowMs: NOW });
    expect(v1.advice).toBe('sell_now');
    expect(v1.reasons.join(' ')).toContain('Resurgence');
    expect(v1.reasons.join(' ')).toContain('-20%');
    const flat = historyWith({ revenant_prime_set: series([[365, 100]]) });
    const v2 = advise({ slug: 'revenant_prime_set', market, history: flat, partToSet: buildPartToSet(market), nowMs: NOW });
    expect(v2.advice).toBe('neutral');
  });

  it('at the 1-year high (with a real range) → sell_now', () => {
    const primes = { loki_prime_set: { name: 'Loki Prime', released: '2014-06-11', vaulted: true, vault_date: '2024-01-01' } };
    const market = marketWith({
      primes,
      set_to_parts: { loki_prime_set: { name: 'Loki Prime', parts: [] } },
      items: { loki_prime_set: { median_now: 190, median_90d: 180 } },
    });
    const h = historyWith({ loki_prime_set: series([[300, 100], [65, 195]]) });
    const v = advise({ slug: 'loki_prime_set', market, history: h, partToSet: buildPartToSet(market), nowMs: NOW });
    expect(v.advice).toBe('sell_now');
    expect(v.reasons[0]).toContain('1-year high');
  });

  it('a flat year never triggers the near-high rule', () => {
    const primes = { frost_prime_set: { name: 'Frost Prime', released: '2013-03-05', vaulted: true, vault_date: '2020-01-01' } };
    const market = marketWith({
      primes,
      set_to_parts: { frost_prime_set: { name: 'Frost Prime', parts: [] } },
      items: { frost_prime_set: { median_now: 101, median_90d: 100 } },
    });
    const h = historyWith({ frost_prime_set: series([[365, 100]]) });
    const v = advise({ slug: 'frost_prime_set', market, history: h, partToSet: buildPartToSet(market), nowMs: NOW });
    expect(v.advice).toBe('neutral');
  });

  it('recently vaulted and below the ramp → hold, with the ramp multiple', () => {
    // Vaulted 2026-06-10 (71 d before NOW); pre-vault ~40p, now 52p → ×1.3 < 1.6.
    const market = marketWith({
      primes: gauss,
      set_to_parts: p2s,
      items: { gauss_prime_set: { median_now: 52, median_90d: 50 } },
    });
    const h = historyWith({ gauss_prime_set: series([[294 - 30, 38], [30, 40], [71, 52]]) });
    const v = advise({ slug: 'gauss_prime_set', market, history: h, partToSet: buildPartToSet(market), nowMs: NOW });
    expect(v.advice).toBe('hold');
    expect(v.reasons.join(' ')).toContain('×1.3');
  });

  it('vault expected within 90 days → hold for the post-vault ramp', () => {
    const primes = { wisp_prime_set: { name: 'Wisp Prime', released: '2023-07-27', vaulted: false, est_vault_date: '2026-10-01' } };
    const market = marketWith({ primes, set_to_parts: { wisp_prime_set: { name: 'Wisp Prime', parts: [] } } });
    const v = advise({ slug: 'wisp_prime_set', market, history: null, partToSet: buildPartToSet(market), nowMs: NOW });
    expect(v.advice).toBe('hold');
    expect(v.reasons[0]).toContain('2026-10-01');
  });
});

describe('helpers', () => {
  it('slope30 compares the last 15 days to the 15 before', () => {
    expect(slope30(series([[335, 100], [15, 100], [15, 80]]).median)).toBeCloseTo(-0.2);
    expect(slope30(series([[365, 100]]).median)).toBeCloseTo(0);
    expect(slope30(series([[10, 100]]).median)).toBeNull();
  });

  it('preVaultMedian reads the 30 days before the vault date', () => {
    const h = { start: '2025-08-20' };
    const median = series([[264, 38], [30, 40], [71, 52]]).median; // vault at index 294
    expect(preVaultMedian(h, median, '2026-06-10')).toBe(40);
    expect(preVaultMedian(h, median, '2025-08-01')).toBeNull(); // before the series
  });

  it('adviseOwned skips subtyped/unknown slugs and keys by slug', () => {
    const market = marketWith({ primes: gauss, set_to_parts: p2s });
    const m = adviseOwned(['gauss_prime_chassis', 'lith_c5_relic', 'gauss_prime_chassis'], market, null, NOW);
    expect([...m.keys()]).toEqual(['gauss_prime_chassis']);
  });
});
