// @ts-nocheck - vitest runs these as JS-style fixtures; full TS shapes here would be busy-work without catching real bugs.
import { describe, it, expect } from 'vitest';
import {
  attributeForTag,
  bandForRiven,
  buildWeaponIndex,
  compsFilterFor,
  dispoChangeFor,
  extractRivens,
  formatAuctionStat,
  formatRivenStat,
  isSplicedStat,
  resolveRivenStats,
  rivenSimilarity,
  resolveRivens,
  rivenReport,
  SPLICE_RECIPES,
  unreadFingerprintKeys,
  spliceOptions,
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
  });

  it('names fingerprint keys the app does not read, and none for a known shape', () => {
    expect(unreadFingerprintKeys({ raw: REVEALED_FP })).toEqual([]);
    expect(unreadFingerprintKeys({ raw: VEILED_FP })).toEqual([]);
    const spliced = JSON.stringify({ ...JSON.parse(REVEALED_FP), advancedTrait: { Tag: 'X', Value: 1 } });
    expect(unreadFingerprintKeys({ raw: spliced })).toEqual(['advancedTrait']);
    expect(unreadFingerprintKeys({ raw: '{broken' })).toEqual([]);
    expect(unreadFingerprintKeys({})).toEqual([]);
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

describe('formatRivenStat', () => {
  it('renders signed stat names without inventing values from roll seeds', () => {
    expect(formatRivenStat('WeaponCritDamageMod', true, ATTRS)).toBe('+Critical Damage');
    expect(formatRivenStat('WeaponProcTimeMod', false, ATTRS)).toBe('-Status Duration');
  });

  it('keeps an unknown tag visible and says it is unrecognised', () => {
    expect(formatRivenStat('WeaponMysteryMod', true, ATTRS)).toBe('+WeaponMysteryMod (unrecognised)');
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
    expect(resolved[1].slug).toBeNull(); // veiled - nothing to resolve
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

  it('counts a stat the manifest cannot name instead of shrinking the riven', () => {
    const withUnknown = { ...owned, buffs: [...owned.buffs, { tag: 'WeaponBrandNewMod', value: 1 }] };
    expect(rivenSimilarity(withUnknown, [
      { url_name: 'critical_damage', value: 90, positive: true },
      { url_name: 'punch_through', value: 1.5, positive: true },
      { url_name: 'status_duration', value: 40, positive: false },
    ], ATTRS)).toBe(75);
  });
});

// WFM /v2/riven/attributes slugs as served on 2026-10-08, after the Update
// 44.1 traits were added. Guards the recipe table against a typo'd slug, which
// would otherwise silently never match.
const WFM_ATTRIBUTE_SLUGS = new Set(["ammo_efficiency", "ammo_maximum", "base_damage_/_melee_damage", "blast", "chance_to_gain_combo_count", "chance_to_gain_extra_combo_count", "channeling_damage", "channeling_efficiency", "cold_damage", "combo_duration", "corrosive", "critical_chance", "critical_chance_on_slide_attack", "critical_damage", "damage_to_orokin", "damage_to_scaldra", "damage_to_techrot", "damage_vs_corpus", "damage_vs_grineer", "damage_vs_infested", "electric_damage", "finisher_damage", "fire_rate_/_attack_speed", "gas", "heat_damage", "heavy_attack_wind_up_speed", "impact_damage", "magazine_capacity", "magazine_reloaded_s_when_holstered", "magnetic", "melee_damage_on_heavy_attack", "multishot", "parry_angle", "projectile_speed", "punch_through", "puncture_damage", "radiation", "range", "recoil", "reload_speed", "slam_attack_damage", "slash_damage", "status_chance", "status_damage", "status_duration", "toxin_damage", "viral", "weak_point_critical_chance", "weak_point_damage", "zoom"]);

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

function results(options) {
  return options.map((o) => o.recipe.result).sort();
}

describe('splicing', () => {
  it('names only attributes WFM actually lists', () => {
    for (const r of SPLICE_RECIPES) {
      expect(WFM_ATTRIBUTE_SLUGS.has(r.result), r.result).toBe(true);
      for (const slug of r.ingredients) expect(WFM_ATTRIBUTE_SLUGS.has(slug), slug).toBe(true);
    }
  });

  it('marks spliced traits on a riven and in a listing', () => {
    const stats = resolveRivenStats(riven(['WeaponBlastDamageMod', 'WeaponCritChanceMod']), SPLICE_ATTRS);
    expect(stats.map((s) => s.spliced)).toEqual([true, false]);
    expect(isSplicedStat('viral')).toBe(true);
    expect(isSplicedStat('cold_damage')).toBe(false);
  });

  it('offers the elemental and ranged splices a rifle riven qualifies for', () => {
    const stats = resolveRivenStats(riven(['WeaponFireDamageMod', 'WeaponFreezeDamageMod', 'WeaponDamageAmountMod'], ['WeaponZoomFovMod']), SPLICE_ATTRS);
    const options = spliceOptions(stats, 'rifle');
    expect(results(options)).toEqual(['blast', 'weak_point_damage']);
    const wpd = options.find((o) => o.recipe.result === 'weak_point_damage');
    expect(wpd.randomIsNegative).toBe(true);
    expect(options.find((o) => o.recipe.result === 'blast').randomIsNegative).toBe(false);
  });

  it('keeps ranged recipes off melee and melee recipes off guns', () => {
    const gun = resolveRivenStats(riven(['WeaponFireRateMod', 'WeaponDamageAmountMod', 'WeaponFireIterationsMod']), SPLICE_ATTRS);
    expect(results(spliceOptions(gun, 'pistol'))).toEqual(['weak_point_damage']);
    const sword = resolveRivenStats(riven(['WeaponFireRateMod', 'WeaponDamageAmountMod', 'WeaponMeleeRangeIncMod']), SPLICE_ATTRS);
    expect(results(spliceOptions(sword, 'melee'))).toEqual(['parry_angle', 'slam_attack_damage']);
    expect(results(spliceOptions(sword, 'zaw'))).toEqual(['parry_angle', 'slam_attack_damage']);
  });

  it('limits a riven of unknown class to the all-class recipes', () => {
    const stats = resolveRivenStats(riven(['WeaponFireDamageMod', 'WeaponFreezeDamageMod', 'WeaponDamageAmountMod', 'WeaponZoomFovMod']), SPLICE_ATTRS);
    expect(results(spliceOptions(stats, undefined))).toEqual(['blast']);
  });

  it('accepts a faction curse beside a faction buff, never two curses', () => {
    const mixed = resolveRivenStats(riven(['WeaponFactionDamageGrineer'], ['WeaponFactionDamageCorpus']), SPLICE_ATTRS);
    expect(results(spliceOptions(mixed, 'rifle'))).toEqual(['damage_to_orokin']);
    const cursed = resolveRivenStats(riven(['WeaponCritChanceMod'], ['WeaponFactionDamageGrineer', 'WeaponFactionDamageCorpus']), SPLICE_ATTRS);
    expect(spliceOptions(cursed, 'rifle')).toEqual([]);
  });

  it('offers nothing once a riven carries its one spliced trait', () => {
    const stats = resolveRivenStats(riven(['WeaponWeakpointDamage', 'WeaponFireDamageMod', 'WeaponFreezeDamageMod']), SPLICE_ATTRS);
    expect(spliceOptions(stats, 'rifle')).toEqual([]);
  });

  it('ignores stats the manifest cannot name', () => {
    const stats = resolveRivenStats(riven(['WeaponFireDamageMod', 'WeaponUnknownMod']), SPLICE_ATTRS);
    expect(spliceOptions(stats, 'rifle')).toEqual([]);
  });
});

describe('compsFilterFor', () => {
  it('filters on the resolved positive stats only', () => {
    const stats = resolveRivenStats(riven(['WeaponCritChanceMod', 'WeaponFireIterationsMod'], ['WeaponZoomFovMod']), SPLICE_ATTRS);
    expect(compsFilterFor(stats)).toEqual({
      filter: { positive: ['critical_chance', 'multishot'], negative: [] },
      complete: true,
    });
  });

  it('says when an unrecognised positive was left out', () => {
    const stats = resolveRivenStats(riven(['WeaponCritChanceMod', 'WeaponUnknownMod']), SPLICE_ATTRS);
    expect(compsFilterFor(stats)).toEqual({ filter: { positive: ['critical_chance'], negative: [] }, complete: false });
  });

  it('is null when no positive stat resolves', () => {
    expect(compsFilterFor(resolveRivenStats(riven(['WeaponUnknownMod'], ['WeaponZoomFovMod']), SPLICE_ATTRS))).toBeNull();
    expect(compsFilterFor([])).toBeNull();
  });
});
