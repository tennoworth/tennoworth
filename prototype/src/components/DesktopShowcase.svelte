<script lang="ts">
  import CopyBtn from './CopyBtn.svelte';
  import { plat, wfmItemUrl } from '../lib/format';
  import { sparklinePoints } from '../lib/sparkline';
  import type { HandoffRow } from '../lib/market-browse';

  // The hand-off panel on the informational landing (hosted site only —
  // {#if !isDesktop} in App.svelte; a desktop user is already in the app).
  // Flow v2: instead of a prose showcase, the SAME rows the visitor just read
  // in the search table, now with Own · Score · Potential filled (sample owned
  // counts, everything else from the snapshot), beside a site-vs-app table and
  // the two install buttons. The install commands, first-run notes and the
  // verify/signature information live under one disclosure — they carry
  // information but they aren't the pitch. Anchor: <section id="desktop">.
  let { rows = [] }: { rows?: HandoffRow[] } = $props();

  // Two platforms, two tabs. The per-distro tabs (Debian/Fedora/Arch) went
  // when the apt, dnf and AUR channels were retired — Linux is the AppImage.
  type Os = 'windows' | 'linux';
  // Default the active tab to the visitor's OS so a Windows user lands on the
  // Windows block and a Linux user on the AppImage block without a click.
  let activeOs = $state<Os>(detectOs());
  const osOrder: Os[] = ['windows', 'linux'];
  let installOpen = $state(false);

  function detectOs(): Os {
    try {
      const p = (navigator.platform || '').toLowerCase();
      return p.includes('win') ? 'windows' : 'linux';
    } catch {
      return 'windows';
    }
  }
  function openInstall(os: Os): void {
    activeOs = os;
    installOpen = true;
  }

  const RELEASES = 'https://github.com/tennoworth/tennoworth/releases';

  // Install blocks — keep the commands byte-identical to README.md's, so the
  // two can't drift (the Windows row points at the releases page instead of a
  // one-liner; the app auto-updates from there).
  const install = {
    linux: {
      title: 'Linux',
      note: 'One file, any distro, self-updating. First run: make it executable, then launch it like any app.',
      cmd: 'curl -LO https://github.com/tennoworth/tennoworth/releases/latest/download/TennoWorth-x86_64.AppImage\nchmod +x TennoWorth-x86_64.AppImage\n./TennoWorth-x86_64.AppImage',
      copiable: true,
    },
    windows: {
      title: 'Windows',
      note: 'Download the installer (.exe or .msi) from the latest release. Unsigned, so SmartScreen warns — click More info → Run anyway. The app updates itself from there.',
      cmd: 'Download from the latest release — https://github.com/tennoworth/tennoworth/releases',
      copiable: false,
    },
  } as const;

  const firstRun = [
    { n: '01', title: 'Install and launch', body: 'Open Warframe and get past the login screen — the credentials the scan needs are in memory by then.' },
    { n: '02', title: 'Scan inventory', body: 'Click Scan inventory in the app. It reads the running game’s memory and loads your items.' },
    { n: '03', title: 'See what to sell', body: 'Your ranked sell list is ready — filter by vaulted, ducats, presets, and your own price and quantity edits.' },
    { n: '04', title: 'List on warframe.market (optional)', body: 'Log in once in-app (token encrypted on your machine) to post listings and manage orders.' },
  ] as const;

  // The Linux ptrace grant lives on the scan step — it's the one thing that
  // trips a first-time Linux user up. Kept separate from the step body so the
  // code block renders as copyable, not inline prose. Only shown on the Linux
  // tab — it doesn't apply to Windows.
  //
  // This used to be `setcap cap_sys_ptrace=eip /usr/bin/tennoworth-desktop`,
  // which was a deb/rpm/AUR install path: there is no /usr/bin binary any
  // more, and file capabilities are ignored on the AppImage's nosuid FUSE
  // mount anyway. What is left is the yama scope, which is what actually
  // refuses the read on a default Debian/Ubuntu/Arch box (scope 1 allows
  // tracing descendants only, and the game is Steam's child, not ours). The
  // app itself prints the precise diagnosis — including the scope it found —
  // when a scan is refused; this is the common fix, stated once.
  const PTRACE_NOTE =
    'Linux: if the scan says “Permission denied”, reading the game’s memory needs ptrace. Allow it for this session (resets on reboot):';
  const PTRACE_CMD = 'sudo sysctl kernel.yama.ptrace_scope=0';
  const isLinux = $derived(activeOs === 'linux');

  const compare = [
    { what: 'Prices, trends, vault status', site: 'ok', app: 'ok' },
    { what: 'Your inventory, ranked', site: '—', app: 'ok scan' },
    { what: 'Top picks · sets · relics · spares', site: '—', app: 'ok' },
    { what: 'List & manage WFM orders', site: '—', app: 'ok' },
    { what: 'Listing health, live top-of-book', site: '—', app: 'ok' },
    { what: 'Login', site: 'none', app: 'only to list' },
    { what: 'Where your data lives', site: 'nowhere', app: 'your machine' },
    { what: 'Overwolf', site: '—', app: 'never' },
  ] as const;
</script>

{#snippet mark(v: string)}
  {#if v === 'ok'}<span class="ok">✓</span>
  {:else if v === 'ok scan'}<span class="ok">✓</span> scan
  {:else if v === '—'}<span class="no">—</span>
  {:else}{v}{/if}
{/snippet}

<section id="desktop" class="wrap tw handoff" aria-label="TennoWorth Desktop">
  <div class="rail">
    <h3>Own any of this? The desktop app fills in the rest.</h3>
    <span class="exp">Same rows — plus what you own, what it's worth to <em>you</em>, and one-click listing on warframe.market.</span>
  </div>

  <div class="hbody">
    <div class="rows">
      {#if rows.length}
        <div class="scroll">
        <table class="tw fixed">
          <colgroup>
            <col />
            <col style="width:3.75rem" />
            <col style="width:4.75rem" />
            <col style="width:3.5rem" />
            <col style="width:4.25rem" />
            <col style="width:4rem" />
            <col style="width:3.5rem" />
            <col style="width:4rem" />
            <col style="width:5.25rem" />
            <col style="width:4rem" />
          </colgroup>
          <thead><tr>
            <th class="l">Item</th>
            <th>Δ 90d</th>
            <th>Trend</th>
            <th>Avg</th>
            <th>Low sell</th>
            <th>Vol 48h</th>
            <th class="you y1" title="How many you own — read from your inventory by the scan">Own</th>
            <th class="you" title="Prioritization score: price × likely sell-through × bounded DE usage; not expected plat/day">Score</th>
            <th class="you" title="Owned × Avg">Potential</th>
            <th class="you"></th>
          </tr></thead>
          <tbody>
            {#each rows as r (r.slug)}
              <tr>
                <td class="l">
                  <a href={wfmItemUrl(r.slug)} target="_blank" rel="noopener noreferrer">{r.name}</a>
                  {#if r.vault === 'vaulted'}<span class="tag vaulted">vaulted</span>{:else if r.vault === 'vaulting-soon'}<span class="tag soon">soon</span>{/if}
                </td>
                <td>
                  {#if r.deltaPct != null && Math.abs(r.deltaPct) >= 1}
                    {#if r.deltaPct > 0}<span class="up">▲{r.deltaPct.toFixed(0)}%</span>{:else}<span class="down">▼{Math.abs(r.deltaPct).toFixed(0)}%</span>{/if}
                  {:else}<span class="flat">·</span>{/if}
                </td>
                <td>
                  {#if sparklinePoints(r.medians_7d, 60, 18)}
                    <svg class="spark" viewBox="0 0 60 18" width="60" height="18" aria-hidden="true">
                      <polyline points={sparklinePoints(r.medians_7d, 60, 18)} fill="none" stroke="currentColor" stroke-width="1.25" />
                    </svg>
                  {:else}<span class="faint">—</span>{/if}
                </td>
                <td class="fg">{plat(r.avg)}</td>
                <td>{plat(r.lowSell)}</td>
                <td>{r.vol.toLocaleString()}</td>
                <td class="you y1">{r.owned}</td>
                <td class="you score">{r.score.toLocaleString()}</td>
                <td class="you">{r.potential.toLocaleString()}<span class="unit">p</span></td>
                <td class="you act"><span class="btn xs" aria-hidden="true">List</span></td>
              </tr>
            {/each}
          </tbody>
        </table>
        </div>
      {/if}
      <div class="line">
        <span class="exp">Score prioritizes what to list from price, likely sell-through, and a bounded DE usage weight — it is not expected plat/day. Potential remains the unweighted stack value. Owned counts here are sample values; the desktop app scans the running game and fills these. Nothing is uploaded; no WFM login until you list.</span>
      </div>
      <div class="cta">
        <a class="btn lg primary" href={RELEASES} target="_blank" rel="noopener noreferrer">Windows .msi</a>
        <button type="button" class="btn lg" onclick={() => openInstall(activeOs === 'windows' ? 'linux' : activeOs)} aria-expanded={installOpen} aria-controls="desktop-install">Linux · AppImage</button>
        <span class="fine">free · open source · reads memory only<br />unsigned Windows build — see Install &amp; verify</span>
      </div>
    </div>

    <table class="tw fixed cmp" aria-label="This site versus the desktop app">
      <colgroup><col /><col style="width:4.25rem" /><col style="width:6.25rem" /></colgroup>
      <thead><tr><th class="l">&nbsp;</th><th>This site</th><th>Desktop app</th></tr></thead>
      <tbody>
        {#each compare as c (c.what)}
          <tr><td class="l">{c.what}</td><td>{@render mark(c.site)}</td><td>{@render mark(c.app)}</td></tr>
        {/each}
      </tbody>
    </table>
  </div>

  <!-- The single biggest objection, answered where the decision is made.
       The FAQ entry below stays canonical and longer; this is the short,
       collapsed version so it can't be missed by someone who reads the CTA
       and leaves. Keep the two consistent — and with SECURITY.md's
       "What we cannot promise". -->
  <details class="disc safety">
    <summary>
      <span class="lbl">Is this safe?</span>
      <span class="exp">Can I get banned? — the honest answer</span>
    </summary>
    <div class="disc-body">
      <p class="note">
        The app only ever <strong>reads</strong> the running game's memory: the
        <code>accountId</code> and <code>nonce</code> your client already obtained at login, used
        for the same inventory call the game client makes. It never writes to the game, never
        injects code, and never touches anti-cheat.
      </p>
      <p class="note">
        We still <strong>can't promise it's ban-safe</strong> — no third-party tool honestly can.
        Equivalent tools have run for years with no documented bans, but Digital Extremes has
        never formally blessed the category. Use it at your own risk.
        <a href="#faq">Full answer in the FAQ ↓</a>
      </p>
    </div>
  </details>

  <details class="disc install" id="desktop-install" bind:open={installOpen}>
    <summary>
      <span class="lbl">Install &amp; verify</span>
      <span class="exp">Windows .msi · Linux AppImage · first run · how to check the build</span>
    </summary>
    <div class="disc-body">
      <div class="seg" role="tablist" aria-label="Operating system">
        {#each osOrder as key (key)}
          <button
            role="tab"
            id="tab-{key}"
            aria-controls="panel-install"
            aria-selected={activeOs === key}
            class:on={activeOs === key}
            tabindex={activeOs === key ? 0 : -1}
            onclick={() => (activeOs = key)}
            onkeydown={(e) => {
              // Arrow-key navigation between tabs (WAI-ARIA tabs pattern).
              const idx = osOrder.indexOf(activeOs);
              let next = null;
              if (e.key === 'ArrowRight') next = osOrder[(idx + 1) % osOrder.length];
              if (e.key === 'ArrowLeft') next = osOrder[(idx - 1 + osOrder.length) % osOrder.length];
              if (next) {
                e.preventDefault();
                activeOs = next;
                document.getElementById(`tab-${next}`)?.focus();
              }
            }}
          >{install[key].title}</button>
        {/each}
      </div>
      <div role="tabpanel" id="panel-install" aria-labelledby="tab-{activeOs}">
        <div class="snippet-row">
          <pre class="snippet"><code>{install[activeOs].cmd}</code></pre>
          {#if install[activeOs].copiable}
            <CopyBtn text={install[activeOs].cmd} />
          {:else}
            <a class="btn" href={RELEASES} target="_blank" rel="noopener noreferrer">Open releases ↗</a>
          {/if}
        </div>
        <p class="note">{install[activeOs].note}</p>
        {#if isLinux}
          <p class="note">
            Installed from apt, dnf or the AUR? Those channels are retired — the AppImage is the only
            one that updates itself. Nothing breaks: the repositories stay served and signed, frozen
            at their last published version. Take the AppImage above, then remove the old package.
          </p>
        {/if}
      </div>

      <h4>First run</h4>
      <ol class="steps">
        {#each firstRun as s (s.n)}
          <li>
            <span class="n">{s.n}</span>
            <div class="sbody">
              <strong>{s.title}</strong>
              <p>{s.body}</p>
              {#if s.n === '02' && isLinux}
                <p class="note">{PTRACE_NOTE}</p>
                <div class="snippet-row inline">
                  <pre class="snippet"><code>{PTRACE_CMD}</code></pre>
                  <CopyBtn text={PTRACE_CMD} />
                </div>
              {/if}
            </div>
          </li>
        {/each}
      </ol>

      <h4>Verify</h4>
      <p class="note">
        Every release is built in public CI from the tagged commit and ships a .sha256 file
        next to each download, so you can confirm what you got matches. Updates are signed: the app
        verifies each one against a key compiled into the running binary before installing it. The
        Windows build is <strong>unsigned</strong> — no paid certificate — so SmartScreen warns once;
        the app only ever reads the running game's memory and nothing leaves your machine. Full detail in
        <a href="https://github.com/tennoworth/tennoworth/blob/main/SECURITY.md" target="_blank" rel="noopener noreferrer">SECURITY.md</a>.
      </p>
    </div>
  </details>
</section>

<style>
  .handoff { scroll-margin-top: var(--s3); }
  /* Rows + CTA on the left, the comparison table in a 24rem right column. */
  .hbody { display: grid; grid-template-columns: minmax(0, 1fr) 24rem; }
  .rows { border-right: 1px var(--rule) var(--hairline); min-width: 0; display: flex; flex-direction: column; }
  .rows .scroll { overflow-x: auto; }
  .rows table { min-width: 44rem; }
  .rows .line {
    display: flex; align-items: center; gap: var(--s2);
    min-height: var(--rail); padding: var(--s2) var(--inset);
    border-top: 1px var(--rule) var(--hairline);
    font-size: 12px; line-height: 1rem; color: var(--muted);
  }
  .rows .line .exp { white-space: normal; }
  .cta {
    display: flex; align-items: center; gap: var(--s2);
    padding: var(--s3) var(--inset);
    border-top: 1px var(--rule) var(--hairline);
    margin-top: auto;
  }
  .cta .fine { color: var(--muted); /* informational — --faint is decorative-only */ font-size: 11px; line-height: 1rem; margin-left: auto; text-align: right; }
  .cmp { font-size: 12px; }
  .cmp td { height: 1.5rem; font-family: var(--font-body); color: var(--muted); }
  .cmp td.l { color: var(--fg); }
  .cmp .ok { color: var(--good); }
  .cmp .no { color: var(--faint); }
  @media (max-width: 900px) {
    .hbody { grid-template-columns: 1fr; }
    .rows { border-right: none; border-bottom: 1px var(--rule) var(--hairline); }
  }

  /* Two disclosures under the panel: the ban answer, then install & verify.
     Same rail chrome, so they read as one pair of quiet rows rather than
     two different widgets. */
  .disc { border-top: 1px solid var(--border); }
  .disc > summary {
    display: flex; align-items: center; gap: var(--s2);
    min-height: var(--rail); padding: 0 var(--inset);
    cursor: pointer; list-style: none; user-select: none;
    font-size: 12px; color: var(--muted); white-space: nowrap;
  }
  .disc > summary::-webkit-details-marker { display: none; }
  .disc > summary::after {
    content: '+'; margin-left: auto; font-family: var(--font-mono); color: var(--muted);
  }
  .disc[open] > summary::after { content: '−'; color: var(--accent); }
  .disc > summary:hover { color: var(--fg); }
  .disc > summary .lbl { width: auto; color: var(--fg); }
  .disc-body { padding: var(--s3) var(--inset) var(--s4); border-top: 1px var(--rule) var(--hairline); display: flex; flex-direction: column; gap: var(--s3); }
  .disc-body .seg { align-self: flex-start; }
  .disc-body h4 { margin: var(--s2) 0 0; font-size: 10px; letter-spacing: 0.12em; text-transform: uppercase; color: var(--muted); font-weight: 600; }
  .safety .note code { font-size: 11px; }
  /* The summary's one-line exp must be allowed to ellipsize on narrow
     viewports rather than push the +/- marker off the rail. */
  .disc > summary .exp { white-space: nowrap; }
  .snippet-row { display: flex; gap: var(--s2); align-items: stretch; }
  .snippet-row.inline { margin: var(--s2) 0 0; max-width: 34rem; }
  .snippet-row.inline .snippet { padding: var(--s1) var(--s2); font-size: 12px; white-space: nowrap; }
  .snippet {
    flex: 1; min-width: 0; overflow-x: auto; margin: 0;
    background: var(--panel-2); border: 1px solid var(--border); border-radius: var(--radius-input);
    padding: var(--s2) var(--s3);
    font-family: var(--font-mono); font-size: 12px; color: var(--fg); white-space: pre;
  }
  .snippet code { background: transparent; padding: 0; font-size: inherit; }
  .note { margin: 0; font-size: 12px; line-height: 1rem; color: var(--muted); max-width: 72ch; }
  .note a { color: var(--accent); }
  .note strong { color: var(--fg); }
  .steps { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: 1fr 1fr; gap: var(--s2) var(--s5); }
  .steps li { display: flex; gap: var(--s3); align-items: flex-start; }
  .steps .n { font-family: var(--font-mono); font-size: 12px; letter-spacing: 0.05em; color: var(--accent); font-weight: 600; min-width: 1.5rem; line-height: 1.25rem; }
  .sbody { min-width: 0; }
  .sbody strong { font-size: 13px; font-weight: 600; line-height: 1.25rem; }
  .sbody p { margin: 0; font-size: 12px; line-height: 1rem; color: var(--muted); }
  @media (max-width: 720px) { .steps { grid-template-columns: 1fr; } }
</style>
