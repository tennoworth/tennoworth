import { extractRivens, type OwnedRiven } from '../../domain/rivens';
import { type Catalogs } from '../../domain/resolver';
import { diffOwned, type SaveSnapshotInput } from '../../domain/snapshot';
import { humanError } from '../../contracts/errors';
import type { StateStore } from '../../contracts/state-store';
import type { DesktopCapabilities } from '../../contracts/desktop';
import type { Market, OwnedRecord, Inventory } from '../../contracts/data';

type Phase = 'idle' | 'loading' | 'done' | 'error';

export class InventoryController {
  constructor(private store: StateStore, private transport: Pick<DesktopCapabilities, 'loadCachedMarket' | 'refreshMarket' | 'fetchInventory' | 'reportScanIssue'>, private sources: { loadMarket(): Promise<Market>; loadCatalogs(): Promise<Catalogs>; normalizeInventory(data: Inventory, catalogs: Catalogs, market: Market): Promise<{ owned: Map<string, OwnedRecord>; unresolved: Record<string, number>; flatCount: number }> }) { }
  phase = $state<Phase>('idle');
  error = $state<string | null>(null);
  marketLoadError = $state<string | null>(null);
  inventoryName = $state<string | null>(null);
  nativeSnapshotId = $state<number | null>(null);
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

  private generation = 0;
  private persistence = Promise.resolve();

  cancelPending(): void {
    this.generation += 1;
  }

  private persist(write: () => Promise<void>): Promise<void> {
    const pending = this.persistence.then(write);
    this.persistence = pending.catch(() => {});
    return pending;
  }

  private saveSnapshot(generation: number, input: SaveSnapshotInput): Promise<void> {
    return this.persist(async () => {
      if (generation === this.generation) await this.store.saveSnapshot(input);
    });
  }

  clear(): Promise<void> {
    this.cancelPending();
    this.nativeSnapshotId = null;
    this.inventoryName = null;
    this.lastUpdated = null;
    this.resolved = { owned: new Map(), unresolved: {} };
    this.ownedRivens = [];
    this.deltas = new Map();
    this.previousOwned = null;
    this.error = null;
    this.pullError = null;
    this.phase = 'idle';
    // A native write already dispatched cannot be cancelled. Clear must run
    // after it so a late completion cannot restore the forgotten snapshot.
    return this.persist(() => this.store.clearSnapshot());
  }

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

  async handleInventory({ name, data, snapshotId = null }: { name: string; data: Inventory; snapshotId?: number | null }) {
    const generation = ++this.generation;
    this.phase = 'loading';
    this.error = null;
    this.pullError = null;
    try {
      const [catalogs, market] = await Promise.all([
        this.catalogs ?? this.sources.loadCatalogs(),
        this.market ?? this.loadBestMarket(),
      ]);
      if (generation !== this.generation) return;
      this.catalogs = catalogs;
      this.market = market;
      const { owned, unresolved, flatCount } = await this.sources.normalizeInventory(data, catalogs, market);
      if (generation !== this.generation) return;
      const rivens = extractRivens(data);
      if (owned.size === 0) {
        this.pullError = flatCount === 0
          ? "The scan didn't find a recognizable inventory in the game's memory. Make sure Warframe is running and you're past the login screen, then try again."
          : 'The scan found items, but nothing in them is tradeable on warframe.market (quest items, resources, and brand-new content have no listings).';
        this.phase = 'idle';
        return;
      }
      const previous = await this.store.loadSnapshot();
      if (generation !== this.generation) return;
      await this.saveSnapshot(generation, { invName: name, owned, rivens, nativeSnapshotId: snapshotId });
      if (generation !== this.generation) return;
      this.deltas = diffOwned(previous?.owned, owned);
      this.previousOwned = previous?.owned ?? null;
      this.nativeSnapshotId = snapshotId;
      this.inventoryName = name;
      this.resolved = { owned, unresolved };
      this.ownedRivens = rivens;
      this.lastUpdated = Date.now();
      this.phase = 'done';
    } catch (e) {
      if (generation !== this.generation) return;
      console.error(e);
      this.error = humanError(e);
      this.phase = 'error';
    }
  }

  async handleImported({ invName, ts, ownedMap }: { invName: string; ts: number; ownedMap: Map<string, OwnedRecord> }) {
    const generation = ++this.generation;
    this.phase = 'loading';
    this.error = null;
    this.pullError = null;
    try {
      const previous = await this.store.loadSnapshot();
      if (generation !== this.generation) return;
      const market = this.market ?? await this.loadBestMarket();
      if (generation !== this.generation) return;
      await this.saveSnapshot(generation, { invName, owned: ownedMap });
      if (generation !== this.generation) return;
      this.nativeSnapshotId = null;
      this.inventoryName = invName;
      this.lastUpdated = ts;
      this.deltas = diffOwned(previous?.owned, ownedMap);
      this.previousOwned = previous?.owned ?? null;
      this.resolved = { owned: ownedMap, unresolved: {} };
      this.market = market;
      this.error = null;
      this.pullError = null;
      this.phase = 'done';
    } catch (error) {
      if (generation !== this.generation) return;
      this.error = humanError(error);
      this.phase = 'error';
      throw error;
    }
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
    const generation = ++this.generation;
    this.pullingInventory = true;
    this.pullError = null;
    try {
      const { data, snapshotId } = await this.transport.fetchInventory();
      if (generation !== this.generation) return;
      await this.handleInventory({
        name: 'inventory (from game)',
        data, snapshotId,
      });
    } catch (e) {
      if (generation === this.generation) this.pullError = humanError(e);
    } finally {
      this.pullingInventory = false;
    }
  }

  async restore() {
    // Startup can reach this after a user acts while the health check awaits IPC.
    if (this.generation !== 0) return;
    const generation = ++this.generation;
    try {
      const snap = await this.store.loadSnapshot();
      if (generation !== this.generation) return;
      if (snap) {
        this.inventoryName = snap.invName;
        this.lastUpdated = snap.ts;
        this.nativeSnapshotId = snap.nativeSnapshotId;
        this.resolved = { owned: snap.owned, unresolved: {} };
        this.ownedRivens = snap.rivens ?? [];
        if (!this.market) {
          try {
            const market = await this.loadBestMarket();
            if (generation !== this.generation) return;
            this.market = market;
          } catch (e) {
            if (generation !== this.generation) return;
            // We already have the restored inventory in hand - a failed price
            // refresh must NOT throw us back to cold-start and hide the user's
            // data. Render from the snapshot; flag prices unavailable.
            console.error(e);
            this.marketLoadError = 'Couldn’t load the price snapshot - you may be offline. Your saved inventory is shown below; prices and rankings will be unavailable until it loads. Reload to retry.';
          }
        }
        // No explicit recompute: the results $effect tracks resolved/market and
        // flushes before paint - the old call here just computed everything twice.
        if (generation !== this.generation) return;
        this.phase = 'done';
      }
    } catch (e) {
      if (generation !== this.generation) return;
      console.error(e);
      this.error = humanError(e);
      this.phase = 'error';
    }
  }
}
