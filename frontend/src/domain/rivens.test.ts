// @ts-nocheck - vitest runs these as JS-style fixtures; full TS shapes here would be busy-work without catching real bugs.
import { describe, it, expect } from 'vitest';
import {
  bandForRiven,
  dispoChangeFor,
  extractRivens,
  formatAuctionStat,
  isSplicedStat,
  readRivenRows,
  resolveRivens,
  rivenReport,
  rivenSimilarities,
} from './rivens.js';
import fixture from '../../../tests/fixtures/rivens/rivens.json';

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

function row(riven, attrs, rivenType) {
  return readRivenRows([{ riven: { veiled: false, ...riven }, rivenType }], attrs)[0];
}

function similarity(riven, auction, attrs) {
  return rivenSimilarities(riven, [auction], attrs)[0];
}

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

  // DE adds riven state (locks, splices) before its keys are known; the raw
  // string is what lets a user report it.
  it('keeps the fingerprint exactly as scanned, unknown keys included', () => {
    const withLock = JSON.stringify({ ...JSON.parse(REVEALED_FP), someFutureLockKey: 1 });
    const [r] = extractRivens(inventory([
      { ItemType: '/Lotus/Upgrades/Mods/Randomized/LotusShotgunRandomModRare', UpgradeFingerprint: withLock, ItemId: { $oid: 'abc' } },
    ]));
    expect(r.raw).toBe(withLock);
    const report = JSON.parse(rivenReport(r));
    expect(report).toEqual({
      ItemType: '/Lotus/Upgrades/Mods/Randomized/LotusShotgunRandomModRare',
      UpgradeFingerprint: JSON.parse(withLock),
    });
    expect(rivenReport(r)).not.toContain('abc');
    expect(row(r, ATTRS).unread).toEqual(['someFutureLockKey']);
  });

  it('names fingerprint keys the app does not read, and none for a known shape', () => {
    expect(row({ buffs: [], curses: [], raw: REVEALED_FP }, ATTRS).unread).toEqual([]);
    expect(row({ buffs: [], curses: [], raw: VEILED_FP }, ATTRS).unread).toEqual([]);
    const spliced = JSON.stringify({ ...JSON.parse(REVEALED_FP), advancedTrait: { Tag: 'X', Value: 1 } });
    expect(row({ buffs: [], curses: [], raw: spliced }, ATTRS).unread).toEqual(['advancedTrait']);
    expect(row({ buffs: [], curses: [], raw: '{broken' }, ATTRS).unread).toEqual([]);
    expect(row({ buffs: [], curses: [] }, ATTRS).unread).toEqual([]);
  });

  it('reports a malformed fingerprint verbatim, and nothing for a riven saved without one', () => {
    expect(JSON.parse(rivenReport({ path: '/p', raw: '{broken' })).UpgradeFingerprint).toBe('{broken');
    expect(rivenReport({ path: '/p' })).toBeNull();
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

describe('stat labels', () => {
  it('renders signed stat names without inventing values from roll seeds', () => {
    const r = row({ buffs: [{ tag: 'WeaponCritDamageMod', value: 1 }], curses: [{ tag: 'WeaponProcTimeMod', value: 1 }] }, ATTRS);
    expect(r.stats.map((s) => s.label)).toEqual(['+Critical Damage', '-Status Duration']);
  });

  it('keeps an unknown tag visible and says it is unrecognised', () => {
    const r = row({ buffs: [{ tag: 'WeaponMysteryMod', value: 1 }], curses: [] }, ATTRS);
    expect(r.stats[0]).toEqual({ tag: 'WeaponMysteryMod', slug: null, positive: true, label: '+WeaponMysteryMod (unrecognised)', spliced: false });
  });

  it('writes faction damage as the multiplier WFM sends, never as a signed number', () => {
    const attrs = [
      { game_ref: 'WeaponFactionDamageInfested', slug: 'damage_vs_infested', name: 'Damage to Infested', unit: 'multiply' },
      { game_ref: 'ComboDurationMod', slug: 'combo_duration', name: 'Combo Duration', unit: 'seconds' },
    ];
    expect(formatAuctionStat('damage_vs_infested', 1.43, true, attrs)).toBe('×1.43 Damage to Infested');
    expect(formatAuctionStat('damage_vs_infested', 0.7, false, attrs)).toBe('×0.70 Damage to Infested');
    expect(formatAuctionStat('combo_duration', 8.2, false, attrs)).toBe('-8.2 s Combo Duration');
  });

  it('formats auction values by url_name with the same unit rule', () => {
    expect(formatAuctionStat('critical_damage', 280, true, ATTRS)).toBe('+280.0% Critical Damage');
    expect(formatAuctionStat('punch_through', 1.5, false, ATTRS)).toBe('-1.50 Punch Through');
    expect(formatAuctionStat('status_duration', 90, false, ATTRS)).toBe('-90.0% Status Duration');
  });

  it('writes WFM-quoted negative values with one sign', () => {
    const attrs = [{ game_ref: '/Lotus/x/FireRate', slug: 'fire_rate', name: 'Fire Rate / Attack Speed', unit: 'percent' }];
    expect(formatAuctionStat('fire_rate', -27.9, false, attrs)).toBe('-27.9% Fire Rate / Attack Speed');
  });
});

describe('resolveRivens', () => {
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
    expect(resolved[1].slug).toBeNull(); // veiled - nothing to resolve
  });

  it('joins only weapons that carry a game_ref', () => {
    const [r] = extractRivens(inventory([{ ItemType: '/Lotus/Upgrades/Mods/Randomized/x', UpgradeFingerprint: JSON.stringify({ buffs: [{ Tag: 'x', Value: 1 }] }) }]));
    expect(resolveRivens([{ ...r, compat: '/Lotus/Weapons/Grineer/Pistols/GrnTorpedoPistol/GrnTorpedoPistol' }], market)[0].slug).toBe('kulstar');
    expect(resolveRivens([{ ...r, compat: 'undefined' }], market)[0].slug).toBeNull();
  });

  it('unresolvable rivens stay unresolved instead of crashing', () => {
    const raw = extractRivens(inventory([
      { ItemType: '/Lotus/Upgrades/Mods/Randomized/z', UpgradeFingerprint: JSON.stringify({ compat: '/Lotus/Brand/New', buffs: [{ Tag: 'x', Value: 1 }] }) },
    ]));
    const resolved = resolveRivens(raw, market);
    expect(resolved[0]).toBe(raw[0]);
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

describe('dispoChangeFor', () => {
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
});

describe('rivenSimilarities', () => {
  const owned = {
    buffs: [
      { tag: 'WeaponCritDamageMod', value: Math.round(0.9 * 1073741824) },
      { tag: 'WeaponPunctureDepthMod', value: Math.round(1.5 * 1073741824) },
    ],
    curses: [
      { tag: 'WeaponProcTimeMod', value: Math.round(-0.4 * 1073741824) },
    ],
  };
  const full = [
    { url_name: 'critical_damage', value: 90, positive: true },
    { url_name: 'punch_through', value: 1.5, positive: true },
    { url_name: 'status_duration', value: 40, positive: false },
  ];

  it('scores identical signed stat sets at 100%', () => {
    expect(similarity(owned, full, ATTRS)).toBe(100);
  });

  it('penalizes missing and opposite-sign stats without comparing roll strength', () => {
    expect(similarity(owned, [
      { url_name: 'critical_damage', value: 45, positive: true },
      { url_name: 'punch_through', value: 1.5, positive: false },
    ], ATTRS)).toBe(25);
  });

  it('returns null when no stats can be compared', () => {
    expect(similarity({ buffs: [], curses: [] }, [], ATTRS)).toBeNull();
  });

  it('counts a stat the manifest cannot name instead of shrinking the riven', () => {
    const withUnknown = { ...owned, buffs: [...owned.buffs, { tag: 'WeaponBrandNewMod', value: 1 }] };
    expect(similarity(withUnknown, full, ATTRS)).toBe(75);
  });

  it('scores a whole comps sample in order', () => {
    expect(rivenSimilarities(owned, [full, [], full.slice(0, 1)], ATTRS)).toEqual([100, null, 33]);
    expect(rivenSimilarities(owned, [], ATTRS)).toEqual([]);
  });
});

const SPLICE_ATTRS = [
  ['WeaponDamageAmountMod', 'base_damage_/_melee_damage', 'Damage'],
  ['WeaponZoomFovMod', 'zoom', 'Zoom'],
  ['WeaponFireIterationsMod', 'multishot', 'Multishot'],
  ['WeaponCritChanceMod', 'critical_chance', 'Critical Chance'],
  ['WeaponFireDamageMod', 'heat_damage', 'Heat'],
  ['WeaponFreezeDamageMod', 'cold_damage', 'Cold'],
  ['WeaponToxinDamageMod', 'toxin_damage', 'Toxin'],
  ['WeaponFireRateMod', 'fire_rate_/_attack_speed', 'Attack Speed'],
  ['WeaponMeleeRangeIncMod', 'range', 'Range'],
  ['WeaponFactionDamageCorpus', 'damage_vs_corpus', 'Damage to Corpus'],
  ['WeaponFactionDamageGrineer', 'damage_vs_grineer', 'Damage to Grineer'],
  ['WeaponBlastDamageMod', 'blast', 'Blast'],
  ['WeaponWeakpointDamage', 'weak_point_damage', 'Weak Point Damage'],
].map(([game_ref, slug, name]) => ({ game_ref, slug, name, unit: 'percent' }));

function riven(buffs, curses = []) {
  return { buffs: buffs.map((tag) => ({ tag, value: 1 })), curses: curses.map((tag) => ({ tag, value: 1 })) };
}

function splices(r, rivenType) {
  return row(r, SPLICE_ATTRS, rivenType).splices;
}

function results(options) {
  return options.map((o) => o.recipe.result).sort();
}

describe('splicing', () => {
  it('marks spliced traits on a riven and in a listing', () => {
    expect(row(riven(['WeaponBlastDamageMod', 'WeaponCritChanceMod']), SPLICE_ATTRS).stats.map((s) => s.spliced)).toEqual([true, false]);
    expect(isSplicedStat('viral')).toBe(true);
    expect(isSplicedStat('cold_damage')).toBe(false);
  });

  it('offers the elemental and ranged splices a rifle riven qualifies for', () => {
    const r = row(riven(['WeaponFireDamageMod', 'WeaponFreezeDamageMod', 'WeaponDamageAmountMod'], ['WeaponZoomFovMod']), SPLICE_ATTRS, 'rifle');
    expect(results(r.splices)).toEqual(['blast', 'weak_point_damage']);
    const wpd = r.splices.find((o) => o.recipe.result === 'weak_point_damage');
    expect(wpd.randomIsNegative).toBe(true);
    // The consumed stats are the row's own, in recipe order.
    expect(wpd.consumed).toEqual([r.stats[2], r.stats[3]]);
    expect(wpd.consumed[0]).toBe(r.stats[2]);
    expect(r.splices.find((o) => o.recipe.result === 'blast').randomIsNegative).toBe(false);
  });

  it('keeps ranged recipes off melee and melee recipes off guns', () => {
    expect(results(splices(riven(['WeaponFireRateMod', 'WeaponDamageAmountMod', 'WeaponFireIterationsMod']), 'pistol'))).toEqual(['weak_point_damage']);
    const sword = riven(['WeaponFireRateMod', 'WeaponDamageAmountMod', 'WeaponMeleeRangeIncMod']);
    expect(results(splices(sword, 'melee'))).toEqual(['parry_angle', 'slam_attack_damage']);
    expect(results(splices(sword, 'zaw'))).toEqual(['parry_angle', 'slam_attack_damage']);
  });

  it('limits a riven of unknown class to the all-class recipes', () => {
    expect(results(splices(riven(['WeaponFireDamageMod', 'WeaponFreezeDamageMod', 'WeaponDamageAmountMod', 'WeaponZoomFovMod']), undefined))).toEqual(['blast']);
  });

  it('accepts a faction curse beside a faction buff, never two curses', () => {
    expect(results(splices(riven(['WeaponFactionDamageGrineer'], ['WeaponFactionDamageCorpus']), 'rifle'))).toEqual(['damage_to_orokin']);
    expect(splices(riven(['WeaponCritChanceMod'], ['WeaponFactionDamageGrineer', 'WeaponFactionDamageCorpus']), 'rifle')).toEqual([]);
  });

  it('offers nothing once a riven carries its one spliced trait', () => {
    expect(splices(riven(['WeaponWeakpointDamage', 'WeaponFireDamageMod', 'WeaponFreezeDamageMod']), 'rifle')).toEqual([]);
  });

  it('offers nothing on a veiled riven', () => {
    expect(row({ ...riven(['WeaponFireDamageMod', 'WeaponFreezeDamageMod']), veiled: true }, SPLICE_ATTRS, 'rifle').splices).toEqual([]);
  });

  it('ignores stats the manifest cannot name', () => {
    expect(splices(riven(['WeaponFireDamageMod', 'WeaponUnknownMod']), 'rifle')).toEqual([]);
  });
});

describe('comps filter', () => {
  it('filters on the resolved positive stats only', () => {
    expect(row(riven(['WeaponCritChanceMod', 'WeaponFireIterationsMod'], ['WeaponZoomFovMod']), SPLICE_ATTRS).filter).toEqual({
      filter: { positive: ['critical_chance', 'multishot'], negative: [] },
      complete: true,
    });
  });

  it('says when an unrecognised positive was left out', () => {
    expect(row(riven(['WeaponCritChanceMod', 'WeaponUnknownMod']), SPLICE_ATTRS).filter).toEqual({ filter: { positive: ['critical_chance'], negative: [] }, complete: false });
  });

  it('is null when no positive stat resolves', () => {
    expect(row(riven(['WeaponUnknownMod'], ['WeaponZoomFovMod']), SPLICE_ATTRS).filter).toBeNull();
    expect(row(riven([]), SPLICE_ATTRS).filter).toBeNull();
  });
});

// The outputs the TypeScript rules gave before they moved native.
describe('shared fixture', () => {
  const attrsOf = (key) => fixture.attribute_sets[key] ?? undefined;

  it('extracts rivens as before', () => {
    for (const c of fixture.extract) expect(extractRivens(c.inventory), c.name).toEqual(c.expected);
  });

  it('joins weapons as before', () => {
    for (const c of fixture.resolve) {
      const market = c.weapons === undefined ? undefined : { rivens: c.weapons === null ? null : { weapons: c.weapons } };
      const got = resolveRivens(c.rivens, market);
      c.expected.forEach((want, i) => {
        if (want === null) expect(got[i], c.name).toBe(c.rivens[i]);
        else expect(got[i], c.name).toEqual({ ...c.rivens[i], ...want });
      });
    }
  });

  it('picks bands as before', () => {
    for (const c of fixture.band) expect(bandForRiven(c.riven, fixture.band_stats[c.stats]), c.name).toEqual(c.expected);
  });

  it('reads rows as before', () => {
    const recipes = fixture.recipes.map((r) => JSON.stringify(r));
    for (const c of fixture.rows) {
      const [got] = readRivenRows([{ riven: c.riven, rivenType: c.riven_type }], attrsOf(c.attributes));
      expect({
        stats: got.stats,
        splices: got.splices.map((o) => ({ recipe: recipes.indexOf(JSON.stringify(o.recipe)), consumed: o.consumed.map((s) => got.stats.indexOf(s)), randomIsNegative: o.randomIsNegative })),
        filter: got.filter,
        unread: got.unread,
      }, c.name).toEqual(c.expected);
    }
  });

  it('scores similarity as before', () => {
    for (const c of fixture.similarity) expect(similarity(c.riven, c.auction, attrsOf(c.attributes)), c.name).toBe(c.expected);
  });

  it('marks spliced traits as before', () => {
    for (const c of fixture.spliced) expect(isSplicedStat(c.slug), c.slug).toBe(c.expected);
  });
});

describe('roll verdicts', () => {
  const CD = { tag: 'WeaponCritDamageMod', value: 1 };
  const DMG = { tag: 'WeaponDamageAmountMod', value: 1 };

  it('judges the stats against the guide entry for the riven\'s weapon', () => {
    expect(row({ slug: 'acceltra', buffs: [CD, DMG], curses: [] }, ATTRS, 'rifle').roll)
      .toMatchObject({ kind: 'read', verdict: 'sellable', source: '44bananas', checked: '44.1.2' });
  });

  it('has nothing to judge without a weapon, and says when the guide has no entry', () => {
    expect(row({ buffs: [CD, DMG], curses: [] }, ATTRS).roll).toBeNull();
    expect(row({ slug: 'acceltra', veiled: true, buffs: [], curses: [] }, ATTRS).roll).toBeNull();
    expect(row({ slug: 'not_a_weapon', buffs: [CD, DMG], curses: [] }, ATTRS).roll).toEqual({ kind: 'no_guide' });
  });
});
