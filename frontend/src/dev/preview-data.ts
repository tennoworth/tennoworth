import type { OwnedRiven } from '../domain/rivens';
import { serializeSnapshot } from '../domain/snapshot';

// This module is dynamically imported only by the development preview.
export function createPreview(scenario: string) {
  const now = Math.floor(Date.now() / 1000);
  const owned = new Map([
    ['pyrana_prime_set', { count: 30, name: 'Pyrana Prime Set', type: 'Weapon', slug: 'pyrana_prime_set', subtype: null, kept_lvl: null, leveled: 0 }],
    ['ivara_prime_neuroptics_blueprint', { count: 4, name: 'Ivara Prime Neuroptics Blueprint', type: 'Warframe', slug: 'ivara_prime_neuroptics_blueprint', subtype: null, kept_lvl: null, leveled: 0 }],
    ['neo_n8_relic', { count: 7, name: 'Neo N8 Relic', type: 'Relic', slug: 'neo_n8_relic', subtype: 'intact', kept_lvl: null, leveled: 0 }],
  ]);
  const sessionSample = scenario === 'session' || scenario.endsWith('-trades');
  if (sessionSample) {
    owned.set('arcane_energize', { count: 12, name: 'Arcane Energize', type: 'Arcane', slug: 'arcane_energize', subtype: null, kept_lvl: null, leveled: 0 });
    owned.set('primed_flow', { count: 6, name: 'Primed Flow', type: 'Mod', slug: 'primed_flow', subtype: null, kept_lvl: null, leveled: 0 });
  }
  const rivens: OwnedRiven[] = [{
    path: '/Lotus/Upgrades/Mods/Randomized/LotusRifleRandomModRare',
    compat: '/Lotus/Weapons/Tenno/LongGuns/SapientPrimary/SapientPrimaryWeapon',
    slug: null, weaponName: null, rerolls: 2, lvl: 0, pol: 'AP_ATTACK',
    buffs: [{ tag: 'WeaponCritDamageMod', value: 952698242 }],
    curses: [{ tag: 'WeaponProcTimeMod', value: 472179622 }], veiled: false,
  }];
  const settings = new Map<string, string>([
    ['last-owned', serializeSnapshot({ invName: 'Sample inventory with a deliberately long name', owned, rivens }, Date.now())],
    ['sell-onboarding-dismissed', '1'], ['keep-copies-nudge-dismissed', '1'],
    ['score-explainer-dismissed', '1'],
  ]);
  const empty = scenario === 'empty';
  if (sessionSample) settings.set('view', 'session');
  let notifications = empty ? [] : [{ id: 1, category: 'trades', title: 'Sold Pyrana Prime Set for 90p', body: 'Pyrana Prime Set ×1 · Listing update failed; review My Orders. Your completed trade is saved in the Ledger.', target: 'orders', created_at: Math.floor(Date.now() / 1000), read: false, delivery: 'failed' }];
  let notificationPreferences = { popups: true, categories: Object.fromEntries(['trades', 'watches', 'scans', 'baro', 'calendar', 'digest'].map(k => [k, { enabled: true, native: true }])) };
  const responses: Record<string, unknown> = {
    fetch_orders: { data: { sell: empty ? [] : [
      { id: 'preview-order', platinum: 90, visible: true, quantity: 2, item: { name: 'Pyrana Prime Set', slug: 'pyrana_prime_set' } },
      ...(sessionSample ? [{ id: 'preview-flow', platinum: 26, visible: true, quantity: 5, rank: 0, item: { name: 'Primed Flow', slug: 'primed_flow' } }] : []),
      ...(sessionSample ? [{ id: 'preview-arcane', platinum: 48, perTrade: 6, visible: false, quantity: 12, rank: 0, item: { name: 'Arcane Energize', slug: 'arcane_energize' } }] : []),
    ], buy: [] } },
    list_watches: empty ? [] : [{
      id: 1, slug: 'pyrana_prime_set', name: 'Pyrana Prime Set', side: 'sell', threshold: 70,
      rank: 0, subtype: null, created_at: now, last_price: 76, last_checked_at: now, last_fired_at: null,
    }],
    list_trades: empty ? [] : [{
      id: 1, at: now, partner: 'Sample trading partner with a long name', kind: 'sale', plat: 90,
      items: [{ name: 'Pyrana Prime Set', qty: 1, direction: 'given' }], log_stamp: null, wfm_closed: true,
    }],
    eelog_status: { path: '/sample/Warframe/EE.log', auto_close: false },
    trade_session_state: {
      allowance: { remaining: scenario === 'zero-trades' ? 0 : scenario === 'unknown-trades' ? null : 24,
        mastery_rank: scenario === 'unknown-trades' ? null : 24, observed_at: now - 120, utc_day: Math.floor(now / 86400), snapshot_id: 1,
        confidence: scenario === 'estimated-trades' ? 'estimated' : scenario === 'unknown-trades' ? 'unknown' : 'scanned',
        reason: null, monitoring: true },
      quantities: { pyrana_prime_set: 30, ivara_prime_neuroptics_blueprint: 4, arcane_energize: 12, primed_flow: 6 },
      bulk_slugs: scenario === 'logged-out' ? [] : ['arcane_energize'],
    },
    riven_comps: [{
      id: 'sample-comp', price: 60, buyout_price: 60, starting_price: 60, top_bid: null,
      is_direct_sell: true, owner: 'Sample seller', owner_status: 'online', mod_rank: 0,
      mastery_level: 12, re_rolls: 2, polarity: 'madurai', name: 'Sample riven', platform: 'pc',
      attributes: [{ url_name: 'critical_damage', value: 88, positive: true }],
    }],
    wfm_auth_status: { logged_in: scenario !== 'logged-out', unlocked: scenario !== 'logged-out' },
  };
  return async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
    if (command === 'list_notifications') {
      if (scenario === 'loading') await new Promise(resolve => setTimeout(resolve, 1500));
      if (scenario === 'error') throw new Error('Could not load notifications. Retry when storage is available.');
      return structuredClone(notifications);
    }
    if (command === 'mark_notifications_read') { notifications = notifications.map(n => args?.id == null || n.id === args.id ? { ...n, read: true } : n); return null; }
    if (command === 'clear_notifications') { notifications = []; return null; }
    if (command === 'get_notification_preferences') return structuredClone(notificationPreferences);
    if (command === 'set_notification_preferences') { if (scenario === 'preferences-error') throw new Error('Could not save notification preferences.'); notificationPreferences = JSON.parse(JSON.stringify(args?.preferences)) as typeof notificationPreferences; return structuredClone(notificationPreferences); }
    if (command === 'test_notification') return 'Test sent (preview).';
    if (command === 'get_setting') return settings.get(String(args?.key)) ?? null;
    if (command === 'set_setting') { settings.set(String(args?.key), String(args?.value)); return null; }
    if (command === 'delete_setting') { settings.delete(String(args?.key)); return null; }
    if (command in responses) {
      if (scenario === 'loading') await new Promise(resolve => setTimeout(resolve, 1500));
      if (scenario === 'error' && ['fetch_orders', 'list_watches', 'list_trades', 'riven_comps', 'trade_session_state'].includes(command)) {
        throw { code: 'internal', message: 'Sample connection failure. Retry when the service is available.' };
      }
      return structuredClone(responses[command]);
    }
    if (command === 'live_top_prices') return [];
    if (command === 'submit_plan') return { plan_id: 'preview-plan', results: (args?.items as Array<{slug: string}>).map(i => ({
      slug: i.slug, status: 'ok', action: i.slug === 'primed_flow' ? 'updated' : 'created', order_id: `preview-${i.slug}`,
    })) };
    throw new Error('Unsupported sample action: ' + command);
  };
}
