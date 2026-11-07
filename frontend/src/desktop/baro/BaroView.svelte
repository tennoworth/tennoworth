<script lang="ts">
import type { Snippet } from 'svelte';
import type { Market, OwnedRecord } from '../../contracts/data';
import type { InventoryController } from '../inventory/controller.svelte';
import { baroPhase } from '../../domain/baro-board';
import { humanWindow } from '../../ui/format';
import { formatLocale, t } from '../../i18n/i18n.svelte';
import Rich from '../../ui/Rich.svelte';
import BaroBoard from './BaroBoard.svelte';
import TraderCalendar from '../../features/calendar/TraderCalendar.svelte';
  let { inventory, baroSurfaceAge, keepSection, guidanceUnavailable, voidTrader, ducatStats, baroState, guidanceOwned, guidanceAvailability, unknownSlugs, onducats }: {
    inventory: InventoryController;
    baroSurfaceAge: string | null;
    keepSection: Snippet;
    guidanceUnavailable: boolean;
    voidTrader: (NonNullable<Market['baro']>) | null;
    ducatStats: {count: number; total: number} | null;
    baroState: ReturnType<typeof baroPhase> | null;
    guidanceOwned: Map<string, OwnedRecord>;
    guidanceAvailability: Map<string, number>;
    unknownSlugs: Set<string>;
    onducats: () => void;
  } = $props();

</script>

      <section data-shell class="view-header">
        <h2 data-shell>{t('baro.title')}</h2>
        <p data-shell class="lede">
          {#if baroState?.phase === 'here'}
            {t('baro.here', { location: voidTrader?.location ?? '', time: humanWindow(baroState.windowMs) })}
          {:else if baroState?.phase === 'incoming'}
            {t('baro.incoming', { location: voidTrader?.location ?? '', time: humanWindow(baroState.windowMs) })}
          {:else}
            {t('baro.next', { location: voidTrader?.location ?? '' })}
          {/if}
          {#if baroSurfaceAge}
            <span data-shell class="warn">· {t('baro.scheduleAge', { age: baroSurfaceAge })}</span>
          {/if}
        </p>
      </section>
      {@render keepSection()}
      <section data-shell class="card ui-panel baro-card" class:here={baroState?.phase === 'here'}>
        <div data-shell class="row">
          <div data-shell class="src">
            <span data-shell class="baro-icon" aria-hidden="true">⌬</span>
            <div data-shell class="baro-body">
              <p data-shell class="baro-detail">
                {#if ducatStats}
                <Rich text={t('baro.youHold', { count: ducatStats.count, ducats: ducatStats.total.toLocaleString(formatLocale()) })} />
                <Rich text={t(baroState?.phase === 'here' ? 'baro.spend' : 'baro.earmark')} />
                {:else}
                  {t('baro.yieldUnavailable')}
                {/if}
              </p>
            </div>
          </div>
          <div data-shell class="row gap-sm">
            <button data-shell onclick={onducats}>{t('baro.openPreset')}</button>
          </div>
        </div>
      </section>

      <!-- His actual stock, priced. Only rendered when the snapshot carries a
           manifest: worldState publishes one from announcement, but a snapshot
           built before that switch (or carried through a DE outage) may not
           have it, and an empty table would read as "he is selling nothing". -->
      {#if voidTrader?.inventory?.length}
        <section data-shell class="baro-stock">
          <BaroBoard market={inventory.market} baro={voidTrader} owned={guidanceOwned} availability={guidanceAvailability} unavailableItems={unknownSlugs.size} quantitiesUnavailable={guidanceUnavailable} />
        </section>
      {/if}

      <!-- Vault rotations and Darvo, from the same worldState poll. An
           unvaulting is the most expensive surprise in prime trading and it is
           announced days ahead. -->
      <section data-shell class="baro-calendar">
        <TraderCalendar market={inventory.market} owned={inventory.resolved.owned} />
      </section>

