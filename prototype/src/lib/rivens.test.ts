// @ts-nocheck — vitest runs these as JS-style fixtures; full TS shapes here would be busy-work without catching real bugs.
import { describe, it, expect } from 'vitest';
import {
  attributeForTag,
  bandForRiven,
  buildWeaponIndex,
  dispoChangeFor,
  extractRivens,
  formatAuctionStat,
  formatRivenStat,
  rivenSimilarity,
  resolveRivens,
} from './rivens.js';

// Real fingerprint shapes from a DE inventory: a revealed shotgun riven, a
// veiled melee riven, and a non-riven Upgrade (railjack avionics) that must
// be ignored.
const REVEALED_FP = JSON.stringify({
  compat: '/Lotus/Weapons/Infested/LongGuns/InfArmCannon/InfArmCannon',
  lim: 727753084,
  lvlReq: 9,
  pol: 'AP_TACTIC',
  buffs: [
    { Tag: 'WeaponDamageAmountMod', Value: 847570554 },
    { Tag: 'WeaponCritDamageMod', Value: 952698242 },
    { Tag: 'WeaponFactionDamageGrineer', Value: 1035326012 },
  ],
  curses: [{ Tag: 'WeaponProcTimeMod', Value: 472179622 }],
  rerolls: 2,
});

const VEILED_FP = JSON.stringify({
  challenge: { Type: '/Lotus/Types/Challenges/PlainsTimedVariety', Progress: 0, Required: 1 },
});

function inventory(upgrades) {
  return { Upgrades: upgrades };
}

const ATTRS = [
  { game_ref: 'WeaponDamageAmountMod', slug: 'base_damage_/_melee_damage', name: 'Damage', unit: 'percent' },
  { game_ref: 'WeaponCritDamageMod', slug: 'critical_damage', name: 'Critical Damage', unit: 'percent' },
  { game_ref: 'WeaponProcTimeMod', slug: 'status_duration', name: 'Status Duration', unit: 'percent' },
  { game_ref: 'WeaponPunctureDepthMod', slug: 'punch_through', name: 'Punch Through' },
];

describe('extractRivens', () => {
  it('parses a revealed riven from its fingerprint', () => {
    const rivens = extractRivens(inventory([
      {
        ItemType: '/Lotus/Upgrades/Mods/Randomized/LotusShotgunRandomModRare',
        UpgradeFingerprint: REVEALED_FP,
      },
    ]));
    expect(rivens).toHaveLength(1);
    const r = rivens[0];
    expect(r.compat).toBe('/Lotus/Weapons/Infested/LongGuns/InfArmCannon/InfArmCannon');
    expect(r.rerolls).toBe(2);
    expect(r.lvl).toBe(0);
    expect(r.pol).toBe('AP_TACTIC');
    expect(r.buffs).toHaveLength(3);
    expect(r.buffs[0]).toEqual({ tag: 'WeaponDamageAmountMod', value: 847570554 });
    expect(r.curses).toHaveLength(1);
    expect(r.veiled).toBe(false);
    expect(r.slug).toBeNull();
    expect(r.weaponName).toBeNull();
  });

  it('marks a challenge-only fingerprint as veiled', () => {
    const rivens = extractRivens(inventory([
      {
        ItemType: '/Lotus/Upgrades/Mods/Randomized/PlayerMeleeWeaponRandomModRare',
        UpgradeFingerprint: VEILED_FP,
      },
    ]));
    expect(rivens).toHaveLength(1);
    expect(rivens[0].veiled).toBe(true);
    expect(rivens[0].compat).toBeNull();
    expect(rivens[0].buffs).toHaveLength(0);
  });

  it('ignores non-Randomized Upgrades (railjack avionics, regular mods)', () => {
    const rivens = extractRivens(inventory([
      { ItemType: '/Lotus/Upgrades/Skins/RailJack/EnginesVidarB', UpgradeFingerprint: '{}' },
      { ItemType: '/Lotus/Upgrades/Mods/Shotgun/DualStat/AcceleratedBlastMod', UpgradeFingerprint: '{}' },
      { ItemType: '/Lotus/Upgrades/Mods/Randomized/PlayerMeleeWeaponRandomModRare', UpgradeFingerprint: VEILED_FP },
    ]));
    expect(rivens).toHaveLength(1);
  });

  it('survives a malformed fingerprint without throwing', () => {
    const rivens = extractRivens(inventory([
      { ItemType: '/Lotus/Upgrades/Mods/Randomized/x', UpgradeFingerprint: '{not json' },
      { ItemType: '/Lotus/Upgrades/Mods/Randomized/y' }, // no fingerprint at all
    ]));
    expect(rivens).toHaveLength(2);
    expect(rivens[0].veiled).toBe(true);
  });

  it('returns [] for a null/empty inventory', () => {
    expect(extractRivens(null)).toEqual([]);
    expect(extractRivens({})).toEqual([]);
  });
});

describe('formatRivenStat', () => {
  it('renders signed stat names without inventing values from roll seeds', () => {
    expect(formatRivenStat('WeaponCritDamageMod', true, ATTRS)).toBe('+Critical Damage');
    expect(formatRivenStat('WeaponProcTimeMod', false, ATTRS)).toBe('-Status Duration');
  });

  it('falls back to the tag when the attribute is unknown', () => {
    expect(formatRivenStat('WeaponMysteryMod', true, ATTRS)).toBe('+WeaponMysteryMod');
  });

  it('formats auction values by url_name with the same unit rule', () => {
    expect(formatAuctionStat('critical_damage', 280, true, ATTRS)).toBe('+280.0% Critical Damage');
    expect(formatAuctionStat('punch_through', 1.5, false, ATTRS)).toBe('-1.50 Punch Through');
    expect(formatAuctionStat('status_duration', 90, false, ATTRS)).toBe('-90.0% Status Duration');
  });
});

describe('resolveRivens / buildWeaponIndex', () => {
  const market = {
    rivens: {
      weapons: {
        inf_arm_cannon: { name: 'Infested Armor Cannon', disposition: 1.3, game_ref: '/Lotus/Weapons/Infested/LongGuns/InfArmCannon/InfArmCannon' },
        kulstar: { name: 'Kulstar', disposition: 1.3, game_ref: '/Lotus/Weapons/Grineer/Pistols/GrnTorpedoPistol/GrnTorpedoPistol' },
        no_game_ref: { name: 'No Path', disposition: 1.0 },
      },
    },
  };

  it('resolves compat paths through game_ref', () => {
    const raw = extractRivens(inventory([
      { ItemType: '/Lotus/Upgrades/Mods/Randomized/x', UpgradeFingerprint: REVEALED_FP },
      { ItemType: '/Lotus/Upgrades/Mods/Randomized/y', UpgradeFingerprint: VEILED_FP },
    ]));
    const resolved = resolveRivens(raw, market);
    expect(resolved[0].slug).toBe('inf_arm_cannon');
    expect(resolved[0].weaponName).toBe('Infested Armor Cannon');
    expect(resolved[1].slug).toBeNull(); // veiled — nothing to resolve
  });

  it('index skips weapons without a game_ref', () => {
    const idx = buildWeaponIndex(market.rivens);
    expect(idx.has('/Lotus/Weapons/Grineer/Pistols/GrnTorpedoPistol/GrnTorpedoPistol')).toBe(true);
    expect(idx.size).toBe(2);
  });

  it('unresolvable rivens stay unresolved instead of crashing', () => {
    const raw = extractRivens(inventory([
      { ItemType: '/Lotus/Upgrades/Mods/Randomized/z', UpgradeFingerprint: JSON.stringify({ compat: '/Lotus/Brand/New', buffs: [{ Tag: 'x', Value: 1 }] }) },
    ]));
    const resolved = resolveRivens(raw, market);
    expect(resolved[0].slug).toBeNull();
    expect(resolved[0].compat).toBe('/Lotus/Brand/New');
  });
});

describe('bandForRiven', () => {
  const stats = {
    acceltra: {
      name: 'Acceltra',
      unrolled: { avg: 41.75, median: 35, min: 5, max: 400, stddev: 45.23, pop: 10 },
      rolled: { avg: 266.72, median: 100, min: 5, max: 4600, stddev: 580.64, pop: 12 },
    },
    ax_52: { name: 'AX-52', unrolled: { avg: 87.2, median: 30, min: 5, max: 3000, stddev: 300.77, pop: 10 } },
  };

  it('picks the rerolled tier for a rerolled riven and vice versa', () => {
    expect(bandForRiven({ slug: 'acceltra', rerolls: 0 }, stats).median).toBe(35);
    expect(bandForRiven({ slug: 'acceltra', rerolls: 3 }, stats).median).toBe(100);
  });

  it('falls back to the other tier when DE only published one', () => {
    expect(bandForRiven({ slug: 'ax_52', rerolls: 4 }, stats).median).toBe(30);
  });

  it('is null without a slug or stats', () => {
    expect(bandForRiven({ slug: null, rerolls: 0 }, stats)).toBeNull();
    expect(bandForRiven({ slug: 'acceltra', rerolls: 0 }, undefined)).toBeNull();
    expect(bandForRiven({ slug: 'nonexistent', rerolls: 0 }, stats)).toBeNull();
  });
});

describe('dispoChangeFor / attributeForTag', () => {
  it('finds the latest change for a slug', () => {
    const surface = {
      changes: [
        { slug: 'acceltra', name: 'Acceltra', from: 1.2, to: 1.35, seen_at: '2026-08-01T00:00:00Z' },
        { slug: 'braton', name: 'Braton', from: 1.0, to: 1.05, seen_at: '2026-07-01T00:00:00Z' },
      ],
    };
    expect(dispoChangeFor('acceltra', surface).to).toBe(1.35);
    expect(dispoChangeFor('lato', surface)).toBeUndefined();
    expect(dispoChangeFor(null, surface)).toBeUndefined();
  });

  it('looks attributes up by the fingerprint tag', () => {
    expect(attributeForTag('WeaponCritDamageMod', ATTRS).slug).toBe('critical_damage');
    expect(attributeForTag('WeaponPunctureDepthMod', ATTRS).unit).toBeUndefined();
    expect(attributeForTag('WeaponNopeMod', ATTRS)).toBeUndefined();
  });
});

describe('auction single-sign rule', () => {
  const attrs = [
    { game_ref: '/Lotus/x/FireRate', slug: 'fire_rate', name: 'Fire Rate / Attack Speed', unit: 'percent' },
  ];
  it('formatAuctionStat likewise for WFM-quoted negative values', () => {
    const out = formatAuctionStat('fire_rate', -27.9, false, attrs as never);
    expect(out).toBe('-27.9% Fire Rate / Attack Speed');
  });
});

describe('rivenSimilarity', () => {
  const owned = {
    buffs: [
      { tag: 'WeaponCritDamageMod', value: Math.round(0.9 * 1073741824) },
      { tag: 'WeaponPunctureDepthMod', value: Math.round(1.5 * 1073741824) },
    ],
    curses: [
      { tag: 'WeaponProcTimeMod', value: Math.round(-0.4 * 1073741824) },
    ],
  };

  it('scores identical signed stat sets at 100%', () => {
    expect(rivenSimilarity(owned, [
      { url_name: 'critical_damage', value: 90, positive: true },
      { url_name: 'punch_through', value: 1.5, positive: true },
      { url_name: 'status_duration', value: 40, positive: false },
    ], ATTRS)).toBe(100);
  });

  it('penalizes missing and opposite-sign stats without comparing roll strength', () => {
    const score = rivenSimilarity(owned, [
      { url_name: 'critical_damage', value: 45, positive: true },
      { url_name: 'punch_through', value: 1.5, positive: false },
    ], ATTRS);
    expect(score).toBe(25);
  });

  it('returns null when no stats can be compared', () => {
    expect(rivenSimilarity({ buffs: [], curses: [] }, [], ATTRS)).toBeNull();
  });
});
