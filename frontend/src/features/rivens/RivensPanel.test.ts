// Rivens view: renders owned rivens with their resolved weapons, DE weekly
// bands, disposition moves, splice options, and the per-riven comps drawer
// (riven_comps IPC).
import { describe, it, expect, vi, afterEach } from 'vitest';
import { screen, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import { renderDesktop as render } from '../../dev/render-desktop';
import RivensPanel from './RivensPanel.svelte';
import { installTauri, removeTauri } from '../../dev/test-utils';
import type { Market } from '../../contracts/data';
import { resolveRivens, type OwnedRiven } from '../../domain/rivens';

afterEach(() => { cleanup(); removeTauri(); });

const market = {
  updated_at: '2026-08-17T00:00:00Z', platform: 'pc', item_count: 1, catalog_count: 1,
  catalog: {}, items: {},
  rivens: {
    weapons: {
      acceltra: {
        name: 'Acceltra', disposition: 0.95, riven_type: 'rifle',
        game_ref: '/Lotus/Weapons/Grineer/LongGuns/GrnAcceltra/GrnAcceltra',
      },
    },
    attributes: [
      { game_ref: 'WeaponCritDamageMod', slug: 'critical_damage', name: 'Critical Damage', unit: 'percent' },
      { game_ref: 'WeaponProcTimeMod', slug: 'status_duration', name: 'Status Duration', unit: 'percent' },
      { game_ref: 'WeaponFireDamageMod', slug: 'heat_damage', name: 'Heat', unit: 'percent' },
      { game_ref: 'WeaponFreezeDamageMod', slug: 'cold_damage', name: 'Cold', unit: 'percent' },
      { game_ref: 'WeaponViralDamageMod', slug: 'viral', name: 'Viral', unit: 'percent' },
    ],
    changes: [
      { slug: 'acceltra', name: 'Acceltra', from: 0.9, to: 0.95, seen_at: '2026-08-01T00:00:00Z' },
    ],
  },
  riven_stats: {
    acceltra: {
      name: 'Acceltra',
      unrolled: { avg: 41.75, median: 35, min: 5, max: 400, stddev: 45.23, pop: 10 },
      rolled: { avg: 266.72, median: 100, min: 5, max: 4600, stddev: 580.64, pop: 3 },
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
    raw: '{"compat":"/Lotus/Weapons/Grineer/LongGuns/GrnAcceltra/GrnAcceltra","buffs":[{"Tag":"WeaponCritDamageMod","Value":952698242}],"curses":[{"Tag":"WeaponProcTimeMod","Value":472179622}],"rerolls":2}',
  },
  {
    path: '/Lotus/Upgrades/Mods/Randomized/LotusRifleRandomModRare',
    compat: '/Lotus/Weapons/Grineer/LongGuns/GrnAcceltra/GrnAcceltra',
    slug: null, weaponName: null,
    rerolls: 0, lvl: 0, pol: 'AP_ATTACK',
    buffs: [{ tag: 'WeaponFireDamageMod', value: 1 }, { tag: 'WeaponFreezeDamageMod', value: 1 }],
    curses: [],
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
    created: '2026-08-01T00:00:00.000+00:00', updated: '2026-08-02T00:00:00.000+00:00',
    attributes: [
      { url_name: 'critical_damage', value: 88, positive: true },
      { url_name: 'status_duration', value: 40, positive: false },
    ],
  },
  {
    id: 'a2', price: 60, buyout_price: 60, starting_price: 20, top_bid: 45,
    is_direct_sell: false, owner: 'Someone', owner_status: 'online',
    mod_rank: 0, mastery_level: 15, re_rolls: 5, polarity: 'vazarin',
    name: null, platform: 'pc', created: null, updated: null,
    attributes: [{ url_name: 'viral', value: 90, positive: true }],
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
    expect(await screen.findAllByText('Acceltra')).toHaveLength(2);
    expect(screen.getAllByText('V')).toHaveLength(2); // AP_ATTACK = Madurai
    // The fingerprint proves stat identity, not the final in-game value.
    expect(screen.getByText('+Critical Damage')).toBeTruthy();
    expect(screen.getByText('-Status Duration')).toBeTruthy();
    // DE weekly band: rerolled → rolled tier median 100p. pop is DE's 0-100
    // popularity, never a count of sales.
    expect(screen.getByText('100p')).toBeTruthy();
    expect(screen.getByText(/rolled · popularity 3\/100/)).toBeTruthy();
    expect(screen.queryByText(/sold n=/)).toBeNull();
    // disposition move from the change log, once per Acceltra riven
    expect(screen.getAllByText('▲ 5%')).toHaveLength(2);
    // veiled riven renders without a weapon
    expect(screen.getByText('Veiled')).toBeTruthy();
    expect(screen.getByText('challenge to reveal')).toBeTruthy();
  });

  it('lists the splices a riven qualifies for, and none for one that has no pair', async () => {
    installTauri(makeInvoke(), undefined);
    render(RivensPanel, { props: { market, rivens: resolveRivens(RAW_RIVENS, market) } });
    const options = await screen.findAllByTestId('splice-options');
    expect(options).toHaveLength(1);
    expect(options[0].textContent).toContain('Blast from +Heat and +Cold');
  });

  it('copies the raw riven data for a report, only where it was scanned', async () => {
    installTauri(makeInvoke(), undefined);
    const writeText = vi.fn(async (_text: string) => {});
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    render(RivensPanel, { props: { market, rivens: resolveRivens(RAW_RIVENS, market) } });
    const buttons = await screen.findAllByRole('button', { name: /Copy raw riven data/ });
    expect(buttons).toHaveLength(1);
    await fireEvent.click(buttons[0]);
    await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1));
    const copied = JSON.parse(writeText.mock.calls[0][0]);
    expect(copied.ItemType).toBe('/Lotus/Upgrades/Mods/Randomized/LotusRifleRandomModRare');
    expect(copied.UpgradeFingerprint.rerolls).toBe(2);
    await screen.findByText('Copied ✓');
  });

  it('warns when a riven carries fingerprint data the app does not read', async () => {
    installTauri(makeInvoke(), undefined);
    const raw = JSON.stringify({ compat: RAW_RIVENS[0].compat, buffs: [], curses: [], advancedTrait: { Tag: 'X', Value: 1 } });
    const rivens = resolveRivens([{ ...RAW_RIVENS[0], raw }, RAW_RIVENS[1]], market);
    render(RivensPanel, { props: { market, rivens } });
    const notes = await screen.findAllByTestId('unread-fingerprint');
    expect(notes).toHaveLength(1);
    expect(notes[0].textContent).toContain('advancedTrait');
  });

  it('shows a disposition decrease as a decrease', async () => {
    installTauri(makeInvoke(), undefined);
    const lowered = {
      ...market,
      rivens: { ...market.rivens, changes: [{ slug: 'acceltra', name: 'Acceltra', from: 1.0, to: 0.95, seen_at: '2026-08-01T00:00:00Z' }] },
    } as unknown as Market;
    render(RivensPanel, { props: { market: lowered, rivens: resolveRivens(RAW_RIVENS, lowered) } });
    expect(await screen.findAllByText('▼ 5%')).toHaveLength(2);
    expect(screen.queryByText(/▲/)).toBeNull();
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
    // Opens on rolls sharing the riven's positive stats.
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('riven_comps', {
      weapon: 'acceltra', stats: { positive: ['critical_damage'], negative: [] },
    }));
    // auction rows: price + converted attribute lines
    await screen.findByText('+88.0% Critical Damage');
    expect(screen.getByText('60p')).toBeTruthy();
    expect(screen.getByText(/top bid 45p/)).toBeTruthy();
    expect(screen.getByText('spliced')).toBeTruthy();
    expect(screen.getByText('-40.0% Status Duration')).toBeTruthy();
    expect(screen.getByText('100% stat match')).toBeTruthy();
    expect(screen.getByText('5 rerolls')).toBeTruthy();
    expect(screen.getByText(/Eleven041110/)).toBeTruthy();

    await fireEvent.click(screen.getByRole('button', { name: 'All on weapon' }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('riven_comps', { weapon: 'acceltra', stats: null }));
  });

  it('opens comps under the riven that asked, not every riven on the weapon', async () => {
    installTauri(makeInvoke(), undefined);
    render(RivensPanel, { props: { market, rivens: resolveRivens(RAW_RIVENS, market) } });
    // Two Acceltra rivens plus the veiled one, whose button is disabled.
    const buttons = await screen.findAllByRole('button', { name: 'Comps' });
    expect(buttons).toHaveLength(3);
    await fireEvent.click(buttons[0]);
    await screen.findByText('+88.0% Critical Damage');
    expect(screen.getAllByRole('group', { name: 'Compare with' })).toHaveLength(1);
    expect(screen.getAllByTestId('comps-sample')).toHaveLength(1);
    expect(screen.getAllByRole('button', { name: 'Hide comps' })).toHaveLength(1);
  });

  it('describes the comps sample as a sample, and can ask for a fresh one', async () => {
    const invoke = makeInvoke();
    installTauri(invoke, undefined);
    const rivens = resolveRivens(RAW_RIVENS, market);
    render(RivensPanel, { props: { market, rivens } });

    await fireEvent.click((await screen.findAllByRole('button', { name: 'Comps' }))[0]);
    const sample = await screen.findByTestId('comps-sample');
    expect(sample.textContent).toContain('2 cheapest buyouts');
    expect(sample.textContent).toContain('1 with a spliced trait');
    expect(sample.textContent).toContain('1 dated');
    expect(sample.textContent).toContain('1 online');
    expect(sample.textContent).toContain('1 offline');
    expect(sample.textContent).toContain('listing age is not time-to-sale');
    // The fixture's second auction carries no instant, so its age is unknown
    // rather than zero.
    expect(sample.textContent).not.toContain('no listing dates');

    await fireEvent.click(screen.getByRole('button', { name: 'Refresh sample' }));
    await waitFor(() => expect(
      invoke.mock.calls.filter((call) => call[0] === 'riven_comps').length,
    ).toBe(2));
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
