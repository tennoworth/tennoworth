<script lang="ts">
  import { onMount } from 'svelte';
  import type { BoxApi } from './api';
  import type { JobStatus, StatusState, UnitStatus } from '../contracts/generated/publisher';

  let { api, active }: { api: BoxApi; active: boolean } = $props();
  let current = $state<StatusState | null>(null);
  let error = $state('');
  let busy = $state(false);
  let confirm = $state<string | null>(null);
  let receipt = $state('');
  let now = $state(Date.now());

  const box = $derived(current?.status ?? null);
  const readiness = $derived(box?.readiness ?? null);
  const sweep = $derived(box?.services.find(service => service.service.unit === 'wfm-scrape.service') ?? null);
  const sweeping = $derived(!!sweep && running(sweep.service));
  // systemd clears the exit time when a new sweep starts, so while one runs the
  // last finished sweep comes from the readiness report instead.
  const lastSweepAt = $derived(sweep?.service.finished_at && !sweeping ? sweep.service.finished_at
    : readiness?.newest_sweep_age_seconds != null
      ? new Date(Date.parse(readiness.checked_at) - readiness.newest_sweep_age_seconds * 1000).toISOString()
      : null);
  const confirming = $derived(box?.jobs.find(job => job.id === confirm) ?? null);
  // Poll quickly only while something the page started could still change.
  const settling = $derived(!!box?.jobs.some(job => running(job.unit)
    || (job.last_request && now - Date.parse(job.last_request.at) < 60_000 && !finishedSince(job))));

  async function refresh() {
    if (busy) return;
    busy = true; error = '';
    try { current = await api.load(); now = Date.now(); }
    catch (cause) { error = cause instanceof Error ? cause.message : 'Could not load box status.'; }
    finally { busy = false; }
  }
  async function run(job: JobStatus) {
    if (busy) return;
    busy = true; error = ''; receipt = '';
    try {
      current = await api.run(job.id);
      now = Date.now();
      receipt = `${job.label} requested. The box starts it within a few seconds; this page follows it until it finishes.`;
    } catch (cause) { error = cause instanceof Error ? cause.message : `${job.label} was not started.`; }
    finally { busy = false; confirm = null; }
  }

  onMount(() => {
    void refresh();
    let timer = 0;
    const tick = () => {
      now = Date.now();
      if (active && document.visibilityState === 'visible') void refresh();
      timer = window.setTimeout(tick, settling ? 5_000 : 60_000);
    };
    timer = window.setTimeout(tick, 5_000);
    return () => window.clearTimeout(timer);
  });

  function running(unit: UnitStatus) {
    return ['active', 'activating', 'reloading', 'deactivating'].includes(unit.active_state) && unit.sub_state !== 'exited';
  }
  function finishedSince(job: JobStatus) {
    return !!job.last_request && !!job.unit.finished_at && Date.parse(job.unit.finished_at) >= Date.parse(job.last_request.at);
  }
  type Tone = 'good' | 'warn' | 'bad' | 'plain';
  function unitState(unit: UnitStatus, longRunning: boolean): [string, Tone] {
    if (!unit.installed) return ['Not installed', 'plain'];
    if (unit.active_state === 'failed' || (unit.result && unit.result !== 'success')) return [`Failed (${unit.result})`, 'bad'];
    if (running(unit)) return longRunning ? ['Running', 'good'] : ['Running now', 'warn'];
    if (longRunning) return ['Stopped', 'bad'];
    return ['Idle', 'plain'];
  }
  function ago(value: string | null | undefined) {
    if (!value) return 'Never';
    const seconds = Math.round((now - Date.parse(value)) / 1000);
    if (!Number.isFinite(seconds)) return 'Unknown';
    if (seconds < -60) return `in ${span(-seconds)}`;
    if (seconds < 60) return 'just now';
    return `${span(seconds)} ago`;
  }
  function span(seconds: number) {
    const minutes = Math.round(seconds / 60);
    if (minutes < 60) return `${minutes} min`;
    const hours = Math.floor(minutes / 60);
    if (hours < 48) return `${hours} h ${minutes % 60} min`;
    return `${Math.round(hours / 24)} days`;
  }
  function local(value: string | null | undefined) { return value ? new Date(value).toLocaleString() : ''; }
  function count(n: number, noun: string) { return `${n} ${noun}${n === 1 ? '' : 's'}`; }
  function gib(bytes: number | null) { return bytes === null ? 'unknown' : `${(bytes / 2 ** 30).toFixed(1)} GiB`; }
  function lag(): [string, Tone] {
    switch (readiness?.deploy_lag) {
      case 'current': return ['Scraper current', 'good'];
      case 'behind': return [`Scraper ${readiness.pending_scraper_commits ?? '?'} commits behind`, 'warn'];
      case 'ahead': return ['Scraper ahead of main', 'plain'];
      case 'diverged': return ['Scraper diverged from main', 'bad'];
      default: return ['Unknown', 'plain'];
    }
  }
  const lagState = $derived(lag());
  const backupTone = $derived<Tone>(!box?.backup.latest_at ? 'bad' : now - Date.parse(box.backup.latest_at) > 36 * 3600_000 ? 'warn' : 'good');
</script>

<div class="status-page">
  <header class="view-header">
    <h1>Box status</h1>
    <p class="lede">Is the box healthy, current and backed up? Run a scheduled job early from here; everything else stays on SSH.</p>
    <div class="vh-end">
      {#if box}<span class="muted" title={local(box.collected_at)}>Collected {ago(box.collected_at)}</span>{/if}
      <button class="btn" onclick={refresh} disabled={busy}>Refresh</button>
    </div>
  </header>

  {#if error}<div class="ui-notice" data-tone="bad" role="alert">{error}</div>{/if}
  {#if receipt}<div class="ui-notice" data-tone="good" role="status">{receipt}</div>{/if}

  {#if !box}
    {#if !error}<p class="muted">Loading box status…</p>{/if}
  {:else}
    <section class="ui-summary-strip summary" aria-label="Box summary">
      <div>
        <span class="k">Scrape readiness</span>
        {#if readiness}
          <strong class="word" data-tone={readiness.ready ? (readiness.warnings.length ? 'warn' : 'good') : 'bad'}>{readiness.ready ? 'Ready' : 'Not ready'}</strong>
          <small>{count(readiness.errors.length, 'error')} · {count(readiness.warnings.length, 'warning')} · checked {ago(readiness.checked_at)}</small>
        {:else}<strong class="word">Unknown</strong><small>No readable report</small>{/if}
      </div>
      <div>
        <span class="k">Last sweep</span>
        <strong class="word" title={local(lastSweepAt)}>{lastSweepAt ? ago(lastSweepAt) : 'Unknown'}</strong>
        <small>{#if sweeping}Another sweep is running, started {ago(sweep?.service.started_at)}{:else}{readiness?.csv_rows != null ? `${readiness.csv_rows.toLocaleString()} rows` : 'Row count unknown'}{readiness?.sweep_wall_seconds != null ? ` · took ${span(readiness.sweep_wall_seconds)}` : ''}{/if}</small>
      </div>
      <div>
        <span class="k">Deploy lag</span>
        <strong class="word" data-tone={lagState[1]}>{lagState[0]}</strong>
        <small class="mono">{readiness?.deploy_revision ? `running ${readiness.deploy_revision.slice(0, 7)}` : 'Deployed revision unknown'}</small>
      </div>
      <div>
        <span class="k">Announcement snapshot</span>
        <strong class="word" data-tone={backupTone}>{box.backup.latest_at ? ago(box.backup.latest_at) : 'Never'}</strong>
        <small>{box.backup.error ?? `${box.backup.count} kept`}</small>
      </div>
    </section>

    {#if box.failed_units.length}
      <div class="ui-notice" data-tone="bad" role="alert"><strong>Failing now:</strong> <span class="mono">{box.failed_units.join(', ')}</span>. Check its journal on the box.</div>
    {/if}
    {#if readiness && !readiness.ready}
      <div class="ui-notice ui-stack" data-tone="bad"><strong>The box is not ready.</strong><ul>{#each readiness.errors as reason}<li>{reason}</li>{/each}</ul></div>
    {/if}
    {#if box.problems.length}
      <div class="ui-notice ui-stack" data-tone="warn"><strong>Some sources could not be read; the rest of this page is current.</strong><ul>{#each box.problems as problem}<li>{problem}</li>{/each}</ul></div>
    {/if}

    <section class="ui-panel" aria-labelledby="jobs-title">
      <header><h2 id="jobs-title">Run now</h2><span>Starts the same service its timer runs</span></header>
      <div class="table-scroll">
        <table class="tw status-table">
          <thead><tr><th>Job</th><th><span class="visually-hidden">Action</span></th><th>Last run</th><th>State</th></tr></thead>
          <tbody>
            {#each box.jobs as job (job.id)}
              {@const [label, tone] = unitState(job.unit, false)}
              <tr>
                <td class="text"><strong>{job.label}</strong><small>{job.description}</small></td>
                <td class="text action">
                  <button class="btn" disabled={busy || !!job.blocked} onclick={() => { confirm = job.id; receipt = ''; }} aria-label={`Run ${job.label}`}>Run…</button>
                  {#if job.blocked}<small>{job.blocked}</small>{/if}
                </td>
                <td class="text">
                  <span title={local(job.unit.finished_at ?? job.unit.started_at)}>{job.unit.finished_at ? `Finished ${ago(job.unit.finished_at)}` : job.unit.started_at ? `Started ${ago(job.unit.started_at)}` : 'Never'}</span>
                  {#if job.last_request}<small>Requested {ago(job.last_request.at)} by {job.last_request.actor}</small>{/if}
                </td>
                <td class="text"><span class="word" data-tone={tone}>{label}</span></td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      {#if confirming}
        <div class="ui-notice ui-stack" data-tone="warn" role="group" aria-label="Confirm job">
          <p><strong>Start {confirming.label.toLowerCase()} now?</strong> {confirming.description}</p>
          <div class="ui-toolbar">
            <button class="btn primary" disabled={busy} onclick={() => run(confirming)}>Start now</button>
            <button class="btn" disabled={busy} onclick={() => { confirm = null; }}>Cancel</button>
          </div>
        </div>
      {/if}
    </section>

    <div class="two-col">
      <section class="ui-panel" aria-labelledby="services-title">
        <header><h2 id="services-title">Services</h2><span>{box.web_bundle ? `Website build ${local(box.web_bundle)}` : 'Website build unknown'}</span></header>
        <div class="table-scroll">
          <table class="tw status-table">
            <thead><tr><th>Service</th><th>State</th><th class="num">Last run</th><th class="num">Next run</th></tr></thead>
            <tbody>
              {#each box.services as service (service.service.unit)}
                {@const longRunning = !service.scheduled}
                {@const [label, tone] = unitState(service.service, longRunning)}
                <tr>
                  <td class="text"><strong>{service.label}</strong><small class="mono">{service.service.unit}</small></td>
                  <td class="text">
                    <span class="word" data-tone={tone}>{label}</span>
                    {#if service.health}<small>Health check <span class="word" data-tone={service.health === 'ok' ? 'good' : 'bad'}>{service.health === 'ok' ? 'passing' : 'failing'}</span></small>{/if}
                  </td>
                  <td title={local(service.service.finished_at ?? service.service.started_at)}>{longRunning ? (service.service.started_at ? `since ${ago(service.service.started_at).replace(' ago', '')}` : '–') : ago(service.service.finished_at ?? service.service.started_at)}</td>
                  <td title={local(service.next_run)}>{service.next_run ? ago(service.next_run) : '–'}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </section>

      <section class="ui-panel" aria-labelledby="readiness-title">
        <header><h2 id="readiness-title">Readiness report</h2><span>{readiness ? `Checked ${local(readiness.checked_at)}` : 'No report'}</span></header>
        {#if readiness}
          <dl class="facts">
            <dt>CSV rows</dt><dd class="mono">{readiness.csv_rows?.toLocaleString() ?? 'unknown'}</dd>
            <dt>Newest sweep at check</dt><dd class="mono">{readiness.newest_sweep_age_seconds != null ? `${span(readiness.newest_sweep_age_seconds)} old` : 'unknown'}</dd>
            <dt>Free disk</dt><dd class="mono">{gib(readiness.disk_free_bytes)} <span class="muted">of {gib(readiness.disk_floor_bytes)} floor</span></dd>
            <dt>Scraper deployed</dt><dd class="mono">{readiness.deploy_revision?.slice(0, 7) ?? 'unknown'}</dd>
            <dt>Main on the box</dt><dd class="mono">{readiness.checkout_head?.slice(0, 7) ?? 'unknown'}</dd>
          </dl>
          {#if readiness.warnings.length}<div class="ui-stack"><strong>Warnings</strong><ul>{#each readiness.warnings as warning}<li>{warning}</li>{/each}</ul></div>{/if}
        {:else}<p class="muted">The readiness check has not left a readable report. Run it from Run now.</p>{/if}
      </section>
    </div>

    <section class="ui-panel" aria-labelledby="alerts-title">
      <header><h2 id="alerts-title">Recent alerts</h2><span>Failed units, last 7 days</span></header>
      {#if !box.alerts.length}<p class="muted">No alerts in the last 7 days.</p>{/if}
      {#each box.alerts as alert, index (index)}
        <details class="alert">
          <summary><span class="mono">{alert.unit}</span> · {alert.result} · <span title={local(alert.at)}>{ago(alert.at)}</span></summary>
          {#if alert.context.length}<pre>{alert.context.join('\n')}</pre>{:else}<p class="muted">No log lines were captured.</p>{/if}
        </details>
      {/each}
    </section>
  {/if}
</div>

<style>
  .status-page { display: grid; gap: var(--s4); min-width: 0; }
  h1 { margin: 0; font-size: var(--text-heading); }
  .summary { grid-template-columns: repeat(4, minmax(0, 1fr)); }
  .summary small { color: var(--muted); font-size: var(--text-caption); }
  .summary strong.word { font-family: var(--font-ui); }
  .word[data-tone="good"] { color: var(--good); }
  .word[data-tone="warn"] { color: var(--warn); }
  .word[data-tone="bad"] { color: var(--bad); }
  .table-scroll { overflow-x: auto; margin-inline: calc(-1 * var(--inset)); }
  .status-table th, .status-table td.text { text-align: left; }
  .status-table th.num { text-align: right; }
  .status-table td { vertical-align: top; padding-block: var(--s1); }
  .status-table td.text { font-family: var(--font-body); color: var(--fg); white-space: normal; overflow-wrap: anywhere; min-width: 7rem; }
  .status-table td.text small { display: block; color: var(--muted); font-size: var(--text-caption); }
  .status-table td.text small.mono, .mono { font-family: var(--font-mono); }
  .status-table td.action { min-width: 6rem; width: 8rem; }
  .two-col { display: grid; grid-template-columns: minmax(0, 3fr) minmax(0, 2fr); gap: var(--s4); align-items: start; }
  .facts { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--s1) var(--s4); margin: 0; }
  .facts dt { color: var(--muted); }
  .facts dd { margin: 0; overflow-wrap: anywhere; }
  ul { margin: 0; padding-left: var(--s5); }
  .alert { border-bottom: 1px var(--rule) var(--border); padding-bottom: var(--s2); }
  .alert pre { margin: var(--s2) 0 0; font: var(--text-caption)/1.5 var(--font-mono); white-space: pre-wrap; overflow-wrap: anywhere; color: var(--muted); }
  .visually-hidden { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  @media (max-width: 1050px) { .summary { grid-template-columns: repeat(2, minmax(0, 1fr)); } .summary > div:nth-child(3) { border-left: 0; } .summary > div:nth-child(n + 3) { border-top: 1px var(--rule) var(--border); } }
  @media (max-width: 860px) { .two-col { grid-template-columns: minmax(0, 1fr); } }
  @media (max-width: 480px) { .summary { grid-template-columns: minmax(0, 1fr); } .summary > div + div { border-left: 0; border-top: 1px var(--rule) var(--border); } }
</style>
