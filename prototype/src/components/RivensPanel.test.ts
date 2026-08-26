// Rivens view: renders owned rivens with their resolved weapons, DE weekly
// bands, disposition moves, and the per-weapon comps drawer (riven_comps IPC).
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import RivensPanel from './RivensPanel.svelte';
import { installTauri, removeTauri } from '../lib/test-utils';
import type { Market } from '../lib/types';
import { resolveRivens, type OwnedRiven } from '../lib/rivens';

afterEach(() => { cleanup(); removeTauri(); });

const market = {
  updated_at: '2026-08-17T00:00:00Z', platform: 'pc', item_count: 1, catalog_count: 1,
  catalog: {}, items: {},
  rivens: {
    weapons: {
      acceltra: {
        name: 'Acceltra', disposition: 0.95,
        game_ref: '/Lotus/Weapons/Grineer/LongGuns/GrnAcceltra/GrnAcceltra',
      },
    },
    attributes: [
      { game_ref: 'WeaponCritDamageMod', slug: 'critical_damage', name: 'Critical Damage', unit: 'percent' },
      { game_ref: 'WeaponProcTimeMod', slug: 'status_duration', name: 'Status Duration', unit: 'percent' },
    ],
    changes: [
      { slug: 'acceltra', name: 'Acceltra', from: 0.9, to: 0.95, seen_at: '2026-08-01T00:00:00Z' },
    ],
  },
  riven_stats: {
    acceltra: {
      name: 'Acceltra',
      unrolled: { avg: 41.75, median: 35, min: 5, max: 400, stddev: 45.23, pop: 10 },
      rolled: { avg: 266.72, median: 100, min: 5, max: 4600, stddev: 580.64, pop: 12 },
    },
  },
  surface_fetched_at: { riven_stats: '2026-08-17T00:00:00Z' },
} as unknown as Market;

const RAW_RIVENS: OwnedRiven[] = [
  {
    path: '/Lotus/Upgrades/Mods/Randomized/LotusRifleRandomModRare',
    compat: '/Lotus/Weapons/Grineer/LongGuns/GrnAcceltra/GrnAcceltra',
    slug: null, weaponName: null,
    rerolls: 2, lvl: 0, pol: 'AP_ATTACK',
    buffs: [{ tag: 'WeaponCritDamageMod', value: 952698242 }],
    curses: [{ tag: 'WeaponProcTimeMod', value: 472179622 }],
    veiled: false,
  },
  {
    path: '/Lotus/Upgrades/Mods/Randomized/PlayerMeleeWeaponRandomModRare',
    compat: null, slug: null, weaponName: null,
    rerolls: 0, lvl: 0, pol: null,
    buffs: [], curses: [], veiled: true,
  },
];

const AUCTIONS = [
  {
    id: 'a1', price: 35, buyout_price: 35, starting_price: 35, top_bid: null,
    is_direct_sell: true, owner: 'Eleven041110', owner_status: 'offline',
    mod_rank: 0, mastery_level: 12, re_rolls: 0, polarity: 'madurai',
    name: 'arma-purado', platform: 'pc',
    attributes: [
      { url_name: 'critical_damage', value: 88, positive: true },
      { url_name: 'status_duration', value: 40, positive: false },
    ],
  },
  {
    id: 'a2', price: 60, buyout_price: null, starting_price: 60, top_bid: 45,
    is_direct_sell: false, owner: 'Someone', owner_status: 'online',
    mod_rank: 0, mastery_level: 15, re_rolls: 5, polarity: 'vazarin',
    name: null, platform: 'pc', attributes: [],
  },
];

function makeInvoke() {
  return vi.fn(async (cmd: string, _args?: Record<string, unknown>) => {
    if (cmd === 'riven_comps') return AUCTIONS;
    throw new Error(`unexpected ${cmd}`);
  });
}

describe('RivensPanel', () => {
  it('renders each riven with weapon, stats, band and disposition move', async () => {
    const invoke = makeInvoke();
    installTauri(invoke, undefined);
    const rivens = resolveRivens(RAW_RIVENS, market);
    render(RivensPanel, { props: { market, rivens } });

    // resolved weapon name + polarity glyph
    await screen.findByText('Acceltra');
    expect(screen.getByText('V')).toBeTruthy(); // AP_ATTACK = Madurai
    // stat lines (percent stats from the fingerprint, Q30 conversion)
    expect(screen.getByText('+88.7% Critical Damage')).toBeTruthy();
    expect(screen.getByText('-44.0% Status Duration')).toBeTruthy();
    // DE weekly band: rerolled → rolled tier median 100p, n=12
    expect(screen.getByText('100p')).toBeTruthy();
    expect(screen.getByText(/rolled · n=12/)).toBeTruthy();
    // disposition move from the change log
    expect(screen.getByText('▲ 5%')).toBeTruthy();
    // veiled riven renders without a weapon
    expect(screen.getByText('Veiled')).toBeTruthy();
    expect(screen.getByText('challenge to reveal')).toBeTruthy();
  });

  it('shows an empty state when nothing is owned', async () => {
    installTauri(makeInvoke(), undefined);
    render(RivensPanel, { props: { market, rivens: [] } });
    await screen.findByText(/No rivens in your scanned inventory/);
  });

  it('fetches and shows comps on demand', async () => {
    const invoke = makeInvoke();
    installTauri(invoke, undefined);
    const rivens = resolveRivens(RAW_RIVENS, market);
    render(RivensPanel, { props: { market, rivens } });

    const compsButton = (await screen.findAllByRole('button', { name: 'Comps' }))[0];
    await fireEvent.click(compsButton);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('riven_comps', { weapon: 'acceltra' }));
    // auction rows: price + converted attribute lines
    await screen.findByText('35p');
    expect(screen.getByText('+88.0% Critical Damage')).toBeTruthy();
    expect(screen.getByText('-40.0% Status Duration')).toBeTruthy();
    expect(screen.getByText('95% similar')).toBeTruthy();
    expect(screen.getByText('5 rerolls')).toBeTruthy();
    expect(screen.getByText(/Eleven041110/)).toBeTruthy();
  });

  it('surfaces a comps fetch failure inline without crashing', async () => {
    const invoke = vi.fn(async (cmd: string) => {
      if (cmd === 'riven_comps') throw new Error('HTTP 503');
      throw new Error(`unexpected ${cmd}`);
    });
    installTauri(invoke, undefined);
    const rivens = resolveRivens(RAW_RIVENS, market);
    render(RivensPanel, { props: { market, rivens } });
    const compsButton = (await screen.findAllByRole('button', { name: 'Comps' }))[0];
    await fireEvent.click(compsButton);
    await screen.findByText(/Couldn't load comps/);
  });
});
