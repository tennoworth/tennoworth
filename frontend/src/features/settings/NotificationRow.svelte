<script lang="ts">
  import type { NotificationEntry } from '../../contracts/desktop';
  import { notificationDate, plat, baroLocation } from '../../ui/format';
  let { entry, now, busy = false, onopen, onread }: {
    entry: NotificationEntry; now: number; busy?: boolean;
    onopen: () => void; onread: () => void;
  } = $props();
  const categories = { trades: 'Completed trade', watches: 'Price watch', baro: 'Baro', calendar: 'Calendar', digest: 'Daily digest' };
  const targets = { sell: 'Open Sell', orders: 'Review My Orders', ledger: 'Open Ledger', watches: 'Open price watches', baro: 'Open Baro and ducat planner', routines: 'Open calendar' };
  let date = $derived(notificationDate(entry.created_at, now));
  let content = $derived(entry.content);
  function stamp(raw: string, options: Intl.DateTimeFormatOptions = { dateStyle: 'medium', timeStyle: 'short' }) {
    const parsed = new Date(raw);
    return Number.isFinite(parsed.getTime()) ? parsed.toLocaleString(undefined, options) : 'Unknown';
  }
</script>

<article class="notification-entry" class:unread={!entry.read} aria-label={entry.title}>
  <header class="entry-head">
    <div class="entry-title"><h3>{entry.title}</h3><div class="entry-meta"><span>{entry.read ? 'Read' : 'Unread'} · {categories[entry.category]}</span></div></div>
    <div class="entry-time"><time class="clock" datetime={date.iso}>{date.time}</time><span>{date.label}{date.shortDate ? ` · ${date.shortDate}` : ''}</span></div>
  </header>
  {#if content?.kind === 'baro'}
    <div class="entry-content">
      <p class="context"><span>{baroLocation(content.location)}</span>{#if content.stock_count !== null}<span>{content.stock_count} stock {content.stock_count === 1 ? 'item' : 'items'}</span>{/if}<span>{now < Date.parse(content.activation) ? 'Arrives' : 'Departure'} · {stamp(now < Date.parse(content.activation) ? content.activation : content.expiry, { weekday: 'long', hour: 'numeric', minute: '2-digit' })}</span></p>
      {#if content.stock_count === null}
        <p>Current stock is not yet verified.</p>
      {:else if content.value}
        {@const value = content.value}
        {#if value.best.length}
          <div class="ui-stack best-value">
            <p><strong>Best value</strong> · {value.prices_stale ? 'at last snapshot asking prices' : 'at snapshot asking prices'}</p>
            <ul class="digest-list">{#each value.best as pick}<li><span>{pick.name}</span><b>{pick.platPerDucat.toFixed(2)}p/ducat</b></li>{/each}</ul>
            {#if value.more_tradeable > 0}<p class="evidence">{value.more_tradeable} more tradeable</p>{/if}
            <p class="evidence">Prices · {stamp(value.price_at)}</p>
            <p class="keeping-note">Arrival can depress prices; resale is not guaranteed.</p>
          </div>
        {/if}
        {#if value.fodder_ducats !== null && value.fodder_items !== null && value.cheap_fodder !== null}
          <dl class="yield-strip">
            <div><dt>Ducat yield · all copies</dt><dd>{plat(value.fodder_ducats)}d</dd></div>
            <div><dt>Ducat-earning copies</dt><dd>{plat(value.fodder_items)}</dd></div>
            <div><dt>Under 4p each</dt><dd>{plat(value.cheap_fodder)}</dd><small>At {value.prices_stale ? 'last snapshot' : 'snapshot'} asking prices</small></div>
          </dl>
          <p class="keeping-note">Check keep rules in the planner before scrapping.</p>
        {:else}<p>Scan to estimate ducat yield from items you hold.</p>{/if}
      {/if}
      {#if content.held.length}
        <details class="holding-details"><summary>You hold {content.held.length} {content.held.length === 1 ? 'item' : 'items'} from this stock</summary><ul class="holding-list">{#each content.held as item}<li>{item}</li>{/each}</ul></details>
      {/if}
      <details class="schedule-details"><summary>Published schedule</summary><dl><dt>Arrival</dt><dd>{stamp(content.activation)}</dd><dt>Departure</dt><dd>{stamp(content.expiry)}</dd><dt>Source</dt><dd>{content.activation} → {content.expiry}</dd>{#if content.value}<dt>Prices</dt><dd>{content.value.price_at || 'Unknown'}</dd>{/if}</dl></details>
    </div>
  {:else if content?.kind === 'digest'}
    <div class="entry-content">
      <ul class="digest-list">{#each content.opportunities as item}<li><span>{item.name} ×{plat(item.quantity)}</span><b>~{plat(item.price)}p each</b></li>{/each}</ul>
      <p class="keeping-note">Estimated market values, not guaranteed sales.</p>
      <p class="evidence"><span>Inventory · {stamp(content.inventory_at)}</span><span>Prices · {stamp(content.price_at)}</span></p>
      <details class="schedule-details"><summary>Snapshot timestamps</summary><dl><dt>Inventory</dt><dd>{content.inventory_at || 'Unknown'}</dd><dt>Prices</dt><dd>{content.price_at || 'Unknown'}</dd></dl></details>
    </div>
  {:else}<p class="body">{entry.body}</p>{/if}
  {#if entry.delivery === 'failed'}<p class="ui-notice" data-tone="warn">Desktop popup could not be delivered. This notification is saved here.</p>{/if}
  {#if entry.delivery === 'pending'}<p class="evidence">Desktop delivery was not confirmed. This notification is saved here.</p>{/if}
  <div class="ui-toolbar entry-actions">
    <button class="btn" disabled={busy} onclick={onopen}>{targets[entry.target]}</button>
    {#if !entry.read}<button class="btn ghost" disabled={busy} onclick={onread}>Mark read</button>{/if}
  </div>
</article>

<style>
  h3, p { margin: 0; }
  .body { white-space: pre-wrap; overflow-wrap: anywhere; line-height: var(--leading-body); }
  .entry-head { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: start; gap: var(--s2) var(--s5); }
  .entry-title h3 { font: 600 var(--text-section)/var(--leading-body) var(--font-ui); overflow-wrap: anywhere; }
  .entry-meta { color: var(--muted); font-size: var(--text-caption); }
  .entry-time { text-align: right; color: var(--muted); font-size: var(--text-caption); display: flex; flex-direction: column; gap: var(--s1); }
  .clock { color: var(--fg); font-family: var(--font-mono); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .entry-content { display: flex; flex-direction: column; gap: var(--s2); }
  .entry-content p { line-height: var(--leading-body); }
  .context, .evidence, .keeping-note { color: var(--muted); font-size: var(--text-control); }
  .context, .evidence { display: flex; flex-wrap: wrap; gap: var(--s1) var(--s3); }
  .context span + span { padding-left: var(--s3); border-left: 1px var(--rule) var(--border); }
  .best-value { gap: var(--s2); }
  .yield-strip { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); border-block: 1px var(--rule) var(--border); margin: 0; }
  .yield-strip > div { padding: var(--s3) var(--s4); min-width: 0; }
  .yield-strip > div:first-child { padding-left: 0; }
  .yield-strip > div + div { border-left: 1px var(--rule) var(--border); }
  .yield-strip dt { color: var(--muted); font: 500 var(--text-caption)/var(--leading-body) var(--font-ui); letter-spacing: .08em; text-transform: uppercase; margin-bottom: var(--s1); }
  .yield-strip dd { margin: 0; font: 500 var(--text-metric)/var(--leading-body) var(--font-mono); }
  .yield-strip small { display: block; font: 400 var(--text-caption)/var(--leading-body) var(--font-body); color: var(--muted); }
  .holding-details, .schedule-details { font-size: var(--text-control); }
  summary { min-height: var(--ctl); display: flex; align-items: center; width: fit-content; }
  .holding-list { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: var(--s2) var(--s4); padding: var(--s2) 0 var(--s2) var(--s5); margin: 0; }
  .holding-list li { overflow-wrap: anywhere; }
  .schedule-details dl { display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: var(--s1) var(--s3); margin: var(--s2) 0; }
  .schedule-details dt { color: var(--muted); }
  .schedule-details dd { margin: 0; overflow-wrap: anywhere; }
  .entry-actions { margin-top: var(--s1); }
  .digest-list { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--s1) var(--s5); list-style: none; padding: 0; margin: 0; }
  .digest-list li { display: flex; justify-content: space-between; gap: var(--s3); min-width: 0; }
  .digest-list b { font-weight: 400; font-family: var(--font-mono); white-space: nowrap; font-size: var(--text-control); }
  @media (max-width: 900px) { .holding-list { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
  @media (max-width: 560px) {
    .entry-head { grid-template-columns: minmax(0, 1fr); }
    .entry-time { flex-direction: row; text-align: left; align-items: center; gap: var(--s3); flex-wrap: wrap; }
    .yield-strip { grid-template-columns: minmax(0, 1fr); }
    .yield-strip > div { padding: var(--s2) 0; display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: var(--s1) var(--s3); align-items: baseline; }
    .yield-strip > div + div { border-left: 0; border-top: 1px var(--rule) var(--hairline); }
    .yield-strip dt { margin: 0; }
    .yield-strip small { grid-column: 1 / -1; }
    .holding-list, .digest-list { grid-template-columns: minmax(0, 1fr); }
  }
</style>
