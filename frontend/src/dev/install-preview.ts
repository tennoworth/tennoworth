import defaults from '../../../tests/fixtures/pacing.json';
import { evaluateDomainPreview } from './domain-preview';
import type { DomainRequest } from '../contracts/generated/domain';
import type { UpdateStatus } from '../contracts/update';
import { sampleAllocation } from './protection-preview';
import type { ProtectionPlan } from '../contracts/protection';
import type { OwnedRecord } from '../contracts/data';
export async function installPreview() {
  const scenario = new URLSearchParams(location.search).get('sample');
  const preview = scenario !== null
    ? (await import('./preview-data')).createPreview(scenario || 'populated')
    : null;
  const noUpdate: UpdateStatus = {
    checked: true,
    available: false,
    support: 'disabled_test_build',
    current_version: 'preview',
    version: null,
    notes: null,
  };
  const empties: Record<string, unknown> = {
    wfm_access_status: { revision: 0, reason: '', cooldown_until_ms: 0, queue_count: 0, outstanding: 0, requests: 0, throttles: 0, cache_hits: 0, cache_misses: 0, queue_rejections: 0, restrictions: defaults },
    wfm_auth_status: { logged_in: false, unlocked: false },
    tray_state: { labels: [], last_notification: null },
    update_status: scenario === 'feedback-update-error' ? { ...noUpdate, available: true, support: 'supported', version: 'next-preview' } : noUpdate,
    check_update: noUpdate,
    refresh_history: { updated: false, body: null },
    refresh_market: { updated: false, status: 'offline' },
    top_sellables: [], list_watches: [], list_listing_log: [], list_snapshots: [],
    ledger_rows: [], list_trades: [], list_notifications: [],
    get_notification_preferences: { popups: true, categories: Object.fromEntries(['trades', 'watches', 'scans', 'baro', 'calendar', 'digest'].map(k => [k, { enabled: true, native: true }])) },
    try_silent_unlock: false,
    get_overlay_settings: { enabled: false, autoDetect: true, shortcut: 'Ctrl+Shift+O', scale: 1, livePrices: true, showOwned: true, diagnostics: false },
    overlay_status: { state: 'disabled', backend: 'x11-window', presentationBackend: 'tauri-window', placement: 'anchored', ocrReady: true },
    setup_overlay_capture: { state: 'watching', backend: 'x11-window', presentationBackend: 'tauri-window', placement: 'anchored', ocrReady: true },
  };
  // The desktop store keeps settings + the reload-restore snapshot in SQLite
  // via get_setting/set_setting; back those onto localStorage so a seeded
  // browser snapshot round-trips exactly like the real thing.
  let protectionPlan: ProtectionPlan = { reserves: {}, goal: null };
  const invoke = (cmd: string, args?: Record<string, unknown>) => {
    if (cmd === 'evaluate_domain') return Promise.resolve().then(() => evaluateDomainPreview(args?.request as DomainRequest));
    if (preview && ['protection_state', 'save_protection_plan', 'get_setting', 'set_setting', 'delete_setting', 'fetch_orders', 'list_watches', 'list_trades', 'eelog_status', 'riven_comps', 'wfm_auth_status', 'live_top_prices', 'trade_session_state', 'submit_plan', 'list_notifications', 'mark_notifications_read', 'clear_notifications', 'get_notification_preferences', 'set_notification_preferences', 'test_notification'].includes(cmd)) return preview(cmd, args);
    if (cmd === 'save_protection_plan') { protectionPlan = JSON.parse(JSON.stringify(args?.plan)) as ProtectionPlan; return Promise.resolve(null); }
    if (cmd === 'protection_state') {
      const snapshot = JSON.parse(localStorage.getItem('last-owned') ?? '{"owned":[]}') as { owned: Array<[string, OwnedRecord]> };
      return Promise.resolve({ plan: protectionPlan, snapshot_id: snapshot.owned.length ? 1 : null,
        items: Object.fromEntries(snapshot.owned.map(([, row]) => [row.slug, sampleAllocation(row.count, row.leveled ?? 0,
          Number(localStorage.getItem('reserve-copies') ?? 0), protectionPlan.reserves[row.slug] ?? 0, protectionPlan.goal ? null : 0)])),
        issues: protectionPlan.goal ? ['Open the protected-plan sample to preview a pinned goal.'] : [] });
    }
    if (cmd === 'get_setting') return Promise.resolve(localStorage.getItem(String(args?.key)));
    if (cmd === 'set_setting') { localStorage.setItem(String(args?.key), String(args?.value)); return Promise.resolve(null); }
    if (cmd === 'delete_setting') { localStorage.removeItem(String(args?.key)); return Promise.resolve(null); }
    if (cmd === 'set_notification_preferences') return Promise.resolve(args?.preferences);
    if (cmd === 'test_notification') return Promise.resolve('Test sent (preview).');
    if (cmd === 'update_overlay_settings') return Promise.resolve(args?.settings ?? null);
    return Promise.resolve(cmd in empties ? empties[cmd] : null);
  };
  const w = globalThis as Record<string, unknown>;
  w.__TAURI_INTERNALS__ = { invoke };
  w.__TAURI__ = { core: { invoke }, event: { listen: () => Promise.resolve(() => { }) } };
}
