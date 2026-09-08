import { flattenInventory, extractKeptLvls } from '../../domain/inventory';
import { extractRivens, type OwnedRiven } from '../../domain/rivens';
import { resolvePath, type Catalogs } from '../../domain/resolver';
import { diffOwned } from '../../domain/snapshot';
import { humanError } from '../../contracts/errors';
import type { StateStore } from '../../contracts/state-store';
import type { DesktopCapabilities } from '../../contracts/desktop';
import type { Market, OwnedRecord, Inventory } from '../../contracts/data';

type Phase = 'idle' | 'loading' | 'done' | 'error';

export class InventoryController {
  constructor(private store: StateStore, private transport: Pick<DesktopCapabilities, 'loadCachedMarket' | 'refreshMarket' | 'fetchInventory' | 'reportScanIssue'>, private sources: { loadMarket(): Promise<Market>; loadCatalogs(): Promise<Catalogs> }) { }
  phase = $state<Phase>('idle');
  error = $state<string | null>(null);
  marketLoadError = $state<string | null>(null);
  inventoryName = $state<string | null>(null);
  lastUpdated = $state<number | null>(null);
  catalogs = $state<Catalogs | null>(null);
  market = $state<Market | null>(null);
  resolved = $state<{ owned: Map<string, OwnedRecord>; unresolved: Record<string, number> }>({
    owned: new Map(),
    unresolved: {},
  });
  ownedRivens = $state<OwnedRiven[]>([]);
  deltas = $state<Map<string, number>>(new Map());
  previousOwned = $state<Map<string, OwnedRecord> | null>(null);
  pullingInventory = $state(false);
  pullError = $state<string | null>(null);
  reportUrl = $state<string | null>(null);
  reportingScan = $state(false);

  async loadBestMarket(): Promise<Market> {
    const cached = await this.transport.loadCachedMarket();
    return cached ?? (await this.sources.loadMarket());
  }

  async refreshMarketInBackground(): Promise<void> {
    try {
      const res = await this.transport.refreshMarket();
      if (!res.updated || !res.market) return;
      const cur = this.market?.updated_at ? Date.parse(this.market.updated_at) : NaN;
      const next = res.market.updated_at ? Date.parse(res.market.updated_at) : NaN;
      // Swap when we have nothing yet, the current stamp is unusable, or the
      // fetched snapshot is strictly newer (guards a server rollback serving an
      // older snapshot than the cache we already show).
      if (!this.market || !Number.isFinite(cur) || (Number.isFinite(next) && next > cur)) {
        this.market = res.market;
        this.marketLoadError = null;
      }
    } catch (e) {
      console.error('market refresh failed', e);
    }
  }

  async handleInventory({ name, data }: { name: string; data: Inventory }) {
    this.inventoryName = name;
    this.phase = 'loading';
    this.error = null;
    this.pullError = null;
    try {
      if (!this.catalogs || !this.market) {
        [this.catalogs, this.market] = await Promise.all([
          this.catalogs ?? this.sources.loadCatalogs(),
          this.market ?? this.loadBestMarket(),
        ]);
      }

      const keptLvls = extractKeptLvls(data);  // /Lotus/... → max lvl in Upgrades
      this.ownedRivens = extractRivens(data);
      const owned = new Map<string, OwnedRecord>();
      const unresolved: Record<string, number> = {};
      let flatCount = 0;
      for (const { category: invCat, path, count, xp } of flattenInventory(data)) {
        flatCount++;
        const { name: itemName, slug, category: itemType, subtype } =
          resolvePath(path, this.catalogs, this.market);
        if (!slug) {
          unresolved[invCat] = (unresolved[invCat] || 0) + 1;
          continue;
        }
        const type = itemType || invCat;
        const key = `${slug}|${subtype ?? ''}`;
        const keptLvl = keptLvls.get(path);
        const rec = owned.get(key) || {
          count: 0, name: itemName ?? slug, type, slug, subtype: subtype ?? null,
          kept_lvl: null, leveled: 0,
        };
        rec.count += count;
        // XP > 0 on an instance category entry means DE has flagged that
        // specific copy untradeable in-game (only unranked gear can trade).
        // Stack categories never carry XP, so this stays 0 for them.
        if (xp > 0) rec.leveled += count;
        // Carry forward the highest kept lvl across any inventory path
        // that resolved to the same slug+subtype (rare for mods - one path
        // per slug - but harmless for the relic refinement case).
        if (typeof keptLvl === 'number' && (rec.kept_lvl === null || keptLvl > rec.kept_lvl)) {
          rec.kept_lvl = keptLvl;
        }
        owned.set(key, rec);
      }
      // Nothing resolved to a tradeable item - the scan returned an inventory
      // with no recognizable tradeable content. Surface it as a pull error so
      // the user sees why the scan didn't produce a sell list, and don't
      // overwrite their snapshot.
      if (owned.size === 0) {
        this.pullError = flatCount === 0
          ? "The scan didn't find a recognizable inventory in the game's memory. Make sure Warframe is running and you're past the login screen, then try again."
          : 'The scan found items, but nothing in them is tradeable on warframe.market (quest items, resources, and brand-new content have no listings).';
        this.inventoryName = null;
        this.phase = 'idle';
        return;
      }
      // Diff against the previously-saved snapshot before overwriting it.
      const previous = await this.store.loadSnapshot();
      this.deltas = diffOwned(previous?.owned, owned);
      this.previousOwned = previous?.owned ?? null;
      this.resolved = { owned, unresolved };
      await this.store.saveSnapshot({ invName: name, owned, rivens: this.ownedRivens });
      this.lastUpdated = Date.now();

      // No explicit recompute: the results $effect below tracks resolved +
      // market and flushes before paint. The call that was here computed the
      // identical array a second time - the same fix the restore path above
      // already got. Its only distinct case, "market not loaded yet", is
      // covered by that effect's own `market` guard.
      this.phase = 'done';
    } catch (e) {
      console.error(e);
      this.error = humanError(e);
      this.phase = 'error';
    }
  }

  async handleImported({ invName, ts, ownedMap }: { invName: string; ts: number; ownedMap: Map<string, OwnedRecord> }) {
    this.inventoryName = invName;
    this.lastUpdated = ts;
    const previous = await this.store.loadSnapshot();
    this.deltas = diffOwned(previous?.owned, ownedMap);
    this.previousOwned = previous?.owned ?? null;
    this.resolved = { owned: ownedMap, unresolved: {} };
    if (!this.market) this.market = await this.loadBestMarket();
    await this.store.saveSnapshot({ invName: this.inventoryName, owned: ownedMap });
    this.phase = 'done';
  }

  async reportScanBroke(): Promise<void> {
    this.reportingScan = true;
    this.reportUrl = null;
    try {
      // A failed open is not a rejection - the command reports it so we can
      // fall back to a copyable link rather than a dead button.
      const r = await this.transport.reportScanIssue(this.pullError);
      if (!r.opened) this.reportUrl = r.url;
    } catch (e) {
      this.reportUrl = null;
      this.pullError = `${this.pullError}\n\nCouldn't build a report: ${humanError(e)}`;
    } finally {
      this.reportingScan = false;
    }
  }

  async pullInventory() {
    if (this.pullingInventory) return;
    this.pullingInventory = true;
    this.pullError = null;
    try {
      const data = await this.transport.fetchInventory() as Inventory;
      await this.handleInventory({
        name: 'inventory (from game)',
        data,
      });
    } catch (e) {
      this.pullError = humanError(e);
    } finally {
      this.pullingInventory = false;
    }
  }

  async restore() {
    const snap = await this.store.loadSnapshot();
    if (snap) {
      try {
        this.inventoryName = snap.invName;
        this.lastUpdated = snap.ts;
        this.resolved = { owned: snap.owned, unresolved: {} };
        this.ownedRivens = snap.rivens ?? [];
        if (!this.market) {
          try {
            this.market = await this.loadBestMarket();
          } catch (e) {
            // We already have the restored inventory in hand - a failed price
            // refresh must NOT throw us back to cold-start and hide the user's
            // data. Render from the snapshot; flag prices unavailable.
            console.error(e);
            this.marketLoadError = 'Couldn’t load the price snapshot - you may be offline. Your saved inventory is shown below; prices and rankings will be unavailable until it loads. Reload to retry.';
          }
        }
        // No explicit recompute: the results $effect tracks resolved/market and
        // flushes before paint - the old call here just computed everything twice.
        this.phase = 'done';
      } catch (e) {
        console.error(e);
        this.error = humanError(e);
        this.phase = 'error';
      }
    }

  }
}
