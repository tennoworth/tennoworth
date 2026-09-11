<script lang="ts">
  import { onMount } from 'svelte';
  import { parseUsageDaily, type UsageDaily } from '../../contracts/usage';
  let { sample }: { sample?: UsageDaily } = $props();
  let data = $state<UsageDaily | null>(null);
  let error = $state('');
  const maximum = $derived(Math.max(1, ...(data?.days.map(day => day.count) ?? [])));
  const stale = $derived(data ? Date.now() - Date.parse(data.updated_at) > 2 * 3600_000 : false);
  onMount(() => {
    if (sample) { data=sample; return; }
    const controller=new AbortController();
    const timeout=setTimeout(() => controller.abort(), 10_000);
    void fetch('/api/usage/daily', { credentials:'omit', signal:controller.signal })
      .then(response => { if (!response.ok) throw new Error(); return response.json(); })
      .then(value => { data=parseUsageDaily(value); })
      .catch(() => { error='Usage counts are currently unavailable.'; })
      .finally(() => clearTimeout(timeout));
    return () => { clearTimeout(timeout); controller.abort(); };
  });
</script>
<section id="community-usage" class="wrap tw" aria-labelledby="community-title">
  <div class="rail"><h2 id="community-title">Community usage</h2></div>
  <div class="ui-stack usage-content">
    <h3>Daily active installations sharing usage counts</h3>
    <p>Reported counts from people who explicitly opt in. Two computers count twice. Running in the tray counts; downloads and website visits do not. Offline installations and people who decline are absent. These figures do not estimate total people and submissions are not independently verified.</p>
    {#if error}<p role="status">{error}</p>
    {:else if !data}<p role="status">Loading daily counts…</p>
    {:else if data.days.length === 0}<p>No completed days have been recorded yet.</p>
    {:else}
      <p>Latest {data.days.length} completed UTC days. Updated {data.updated_at}. {#if stale}<strong>Updates are delayed; these counts may be stale.</strong>{/if}</p>
      <div class="bars" role="img" aria-label="Daily installation counts. Exact values and incomplete days are listed in the table below.">
        {#each data.days as day}
          <div class="bar-slot" title={`${day.date}: ${day.count}${day.complete ? '' : ' (incomplete)'}`}>
            <div class="bar" class:incomplete={!day.complete} style:height={`${day.count / maximum * 100}%`}></div>
          </div>
        {/each}
      </div>
      <div class="date-range" aria-hidden="true"><span>{data.days[0].date}</span><span>{data.days[data.days.length - 1].date}</span></div>
      <p>Incomplete days include launch, restarts, or known collection outages. Their counts may be low; a recorded zero is different from unavailable data.</p>
      <details><summary>Daily counts table</summary><div class="table-scroll"><table class="tw"><thead><tr><th scope="col">UTC date</th><th scope="col">Installations</th><th scope="col">Coverage</th></tr></thead><tbody>{#each data.days as day}<tr><th scope="row">{day.date}</th><td>{day.count}</td><td>{day.complete ? 'Complete' : 'Incomplete'}</td></tr>{/each}</tbody></table></div></details>
    {/if}
  </div>
</section>
<style>
  .usage-content { padding: var(--inset); }
  .bars { height: 12rem; display: flex; align-items: stretch; gap: 1px; border-bottom: 1px solid var(--border); }
  .bar-slot { flex: 1; min-width: 0; display: flex; align-items: end; }
  .bar { width: 100%; background: var(--accent); }
  .bar.incomplete { background: var(--muted); border-top: 2px dotted var(--border); }
  .date-range { display: flex; justify-content: space-between; gap: var(--s2); font-family: var(--font-mono); font-size: var(--text-caption); }
  .table-scroll { overflow-x: auto; }
  p { overflow-wrap: anywhere; }
</style>
