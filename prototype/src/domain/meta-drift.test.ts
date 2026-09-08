import { describe, expect, it } from 'vitest';
import { buildMetaDrift, formatDeltaPp } from './meta-drift';

function market(by_year: Record<number, Record<string, unknown>>, years = [2023, 2025]) {
  return {
    items: {
      gain_b: { low_sell: 20, vol: 8 }, gain_a: { low_sell: 10, vol: 4 },
      loss: { low_sell: 5, vol: 2 }, only_new: { low_sell: 30, vol: 12 },
    },
    usage_history: { years, by_year },
  } as any;
}

const row = (name: string, category: string, share: number) => ({ name, category, share });

describe('meta drift model', () => {
  it('uses the latest two available years, labels gaps, and sorts percentage points stably', () => {
    const model = buildMetaDrift(market({
      2023: { gain_b: row('Gain B', 'Primary', 1), gain_a: row('Gain A', 'Primary', 1), loss: row('Loss', 'Primary', 3) },
      2025: { gain_b: row('Gain B', 'Primary', 2), gain_a: row('Gain A', 'Primary', 2), loss: row('Loss', 'Primary', 1) },
    }))!;
    expect(model.label).toBe('DE equip share · 2023 → 2025 (2024 unavailable)');
    expect(model.gains.map((r) => r.slug)).toEqual(['gain_a', 'gain_b']);
    expect(model.gains[0].deltaPp).toBe(1);
    expect(model.losses.map((r) => r.slug)).toEqual(['loss']);
    expect(model.losses[0].deltaPp).toBe(-2);
    expect(model.gains[0]).toMatchObject({ lowSell: 10, volume48h: 4 });
  });

  it('never zero-fills missing items and excludes category changes', () => {
    const model = buildMetaDrift(market({
      2023: { only_old: row('Only Old', 'Primary', 2), changed: row('Changed', 'Primary', 1), no_market: row('No Market', 'Primary', 1) },
      2025: { only_new: row('Only New', 'Primary', 3), changed: row('Changed', 'Secondary', 2), no_market: row('No Market', 'Primary', 2) },
    }))!;
    expect(model.gains).toHaveLength(1);
    expect(model.gains[0]).toMatchObject({ slug: 'no_market', lowSell: null, volume48h: null });
    expect(model.losses).toEqual([]);
    expect(model.onlyCurrent.map((r) => r.slug)).toEqual(['only_new']);
    expect(model.onlyPrior.map((r) => r.slug)).toEqual(['only_old']);
    expect(model.onlyPrior[0]).toMatchObject({ lowSell: null, volume48h: null });
    expect(model.categoryChanges).toBe(1);
  });

  it('formats every nonzero round4 delta visibly and switches precision at 0.01 pp', () => {
    expect(formatDeltaPp(0.0001)).toBe('+0.0001 pp');
    expect(formatDeltaPp(-0.0099)).toBe('-0.0099 pp');
    expect(formatDeltaPp(0.01)).toBe('+0.01 pp');
    expect(formatDeltaPp(-0.01)).toBe('-0.01 pp');
  });

  it('rejects nonfinite and negative rows and is quiet with fewer than two valid years', () => {
    expect(buildMetaDrift(market({ 2023: { bad: row('Bad', 'Primary', -1) }, 2025: { bad: row('Bad', 'Primary', 2) } }))).toBeNull();
    expect(buildMetaDrift(market({ 2023: { one: row('One', 'Primary', 1) } }, [2023]))).toBeNull();
    expect(buildMetaDrift({ items: {} } as any)).toBeNull();
  });

  it('selects the latest two actually valid maps rather than adjacent declared years', () => {
    const model = buildMetaDrift(market({
      2021: { old: row('Old', 'Primary', 1) },
      2024: {},
      2025: { old: row('Old', 'Primary', 2) },
    }, [2021, 2024, 2025]))!;
    expect([model.priorYear, model.currentYear]).toEqual([2021, 2025]);
    expect(model.label).toBe('DE equip share · 2021 → 2025 (2022, 2023, 2024 unavailable)');
  });
});
