<script lang="ts">
  import CopyBtn from './CopyBtn.svelte';

  // The desktop-app showcase on the informational landing. Rendered only on
  // the hosted site ({#if !isDesktop} in App.svelte) — a desktop user is
  // already in the app. No routing: this is an <section id="desktop"> the
  // upsell pitch links to with a same-page anchor.

  type Os = 'windows' | 'linux' | 'debian' | 'fedora' | 'arch';
  // Default the active tab to the visitor's OS so a Windows user lands on the
  // Windows block and a Linux user on the AppImage block without a click.
  // The distro repo tabs stay for the transition; the AppImage is the lead.
  let activeOs = $state<Os>(detectOs());
  const osOrder: Os[] = ['windows', 'linux', 'debian', 'fedora', 'arch'];

  function detectOs(): Os {
    try {
      const p = (navigator.platform || '').toLowerCase();
      return p.includes('win') ? 'windows' : 'linux';
    } catch {
      return 'windows';
    }
  }

  // Install blocks — keep the commands byte-identical to README.md's, so the
  // two can't drift (the Windows row points at the releases page instead of a
  // one-liner; the app auto-updates from there).
  const install = {
    linux: {
      title: 'Linux',
      note: 'One file, any distro, self-updating. First run: make it executable, then launch it like any app. (The apt/dnf/AUR packages below keep working during the transition.)',
      cmd: 'curl -LO https://github.com/tennoworth/tennoworth/releases/latest/download/TennoWorth-x86_64.AppImage\nchmod +x TennoWorth-x86_64.AppImage\n./TennoWorth-x86_64.AppImage',
      copiable: true,
    },
    windows: {
      title: 'Windows',
      note: 'Download the installer (.exe or .msi) from the latest release. Unsigned, so SmartScreen warns — click More info → Run anyway. The app updates itself from there.',
      cmd: 'Download from the latest release — https://github.com/tennoworth/tennoworth/releases',
      copiable: false,
    },
    debian: {
      title: 'Debian / Ubuntu',
      note: 'The signed apt repository. Updates arrive with the rest of your system.',
      cmd: 'curl -fsSL https://tennoworth.app/tennoworth-archive-keyring.asc | sudo tee /etc/apt/keyrings/tennoworth.asc > /dev/null\necho "deb [signed-by=/etc/apt/keyrings/tennoworth.asc] https://tennoworth.app/apt stable main" | sudo tee /etc/apt/sources.list.d/tennoworth.list > /dev/null\nsudo apt update && sudo apt install tennoworth',
      copiable: true,
    },
    fedora: {
      title: 'Fedora',
      note: 'The signed dnf repository. Updates arrive with the rest of your system.',
      cmd: 'sudo dnf config-manager --add-repo https://tennoworth.app/rpm/tennoworth.repo\nsudo dnf install tennoworth',
      copiable: true,
    },
    arch: {
      title: 'Arch',
      note: 'From the AUR. tennoworth builds from source; tennoworth-bin uses the prebuilt binary.',
      cmd: 'paru -S tennoworth',
      copiable: true,
    },
  } as const;

  const feature = [
    { title: 'Scan the game', body: 'One click reads the running game\u2019s memory \u2014 no file, no terminal, no copy-paste.' },
    { title: 'Ranked sell list', body: 'Your items scored by expected plat, not raw averages \u2014 preset filters for vaulted, ducats, and more.' },
    { title: 'List on WFM', body: 'Review and price a batch, post hidden, then manage your orders in-app.' },
    { title: 'No Overwolf, no accounts', body: 'Reads memory locally; logs into warframe.market only when you list. Nothing is uploaded to us.' },
  ] as const;

  const firstRun = [
    { n: '01', title: 'Install and launch', body: 'Open Warframe and get past the login screen \u2014 the credentials the scan needs are in memory by then.' },
    { n: '02', title: 'Scan inventory', body: 'Click Scan inventory in the app. It reads the running game\u2019s memory and loads your items.' },
    { n: '03', title: 'See what to sell', body: 'Your ranked sell list is ready \u2014 filter by vaulted, ducats, presets, and your own price and quantity edits.' },
    { n: '04', title: 'List on warframe.market (optional)', body: 'Log in once in-app (token encrypted on your machine) to post listings and manage orders.' },
  ] as const;

  // The Linux ptrace grant lives on the scan step — it's the one thing that
  // trips a first-time Linux user up. Kept separate from the step body so the
  // code block renders as copyable, not inline prose. Only shown on the Linux
  // install tabs — it doesn't apply to Windows.
  const PTRACE_NOTE = 'Linux: grant ptrace once so no sudo is needed per scan:';
  const PTRACE_CMD = 'sudo setcap cap_sys_ptrace=eip /usr/bin/tennoworth-desktop';
  const isLinux = $derived(activeOs !== 'windows');

  // Screenshot slot: the real asset is a human follow-up, so the <img> loads
  // it when it exists and the onerror drops back to the placeholder.
  let shotFailed = $state(false);
</script>

<section id="desktop" class="desktop-showcase">

  <div class="hero">
    <div class="tag">TennoWorth Desktop</div>
    <h2>Your sell list, without a terminal.</h2>
    <p class="sub">
      Scan the running game, and TennoWorth ranks <em>your</em> inventory by what
      to sell right now — the same prices and vault data as this site, joined to
      what you actually own. Review, price, and post to warframe.market in one
      place. Windows + Linux, no Overwolf, no accounts.
    </p>
    <div class="ctas">
      <a
        class="btn primary"
        href="https://github.com/tennoworth/tennoworth/releases"
        target="_blank"
        rel="noopener noreferrer"
      >Download for Windows</a>
      <a class="btn ghost" href="#desktop-install">Linux — apt / dnf / AUR</a>
    </div>
    <p class="trust">Reads memory only · open source · nothing leaves your machine</p>
    <div class="hero-shot">
      {#if shotFailed}
        <div class="frame-fallback">screenshot: the app's ranked sell list</div>
      {:else}
        <img
          class="shot"
          src="screenshots/desktop-sell-list.png"
          alt="Screenshot of TennoWorth Desktop's ranked sell list"
          loading="lazy"
          onerror={() => (shotFailed = true)}
        />
      {/if}
      <div class="cap">The market browser you're using now, plus your inventory ranked by expected plat.</div>
    </div>
  </div>

  <div class="features">
    {#each feature as f (f.title)}
      <div class="feature">
        <h3>{f.title}</h3>
        <p>{f.body}</p>
      </div>
    {/each}
  </div>

  <div class="block" id="desktop-install">
    <h3>Install</h3>
    <div class="tabs" role="tablist" aria-label="Operating system">
      {#each osOrder as key (key)}
        <button
          role="tab"
          id="tab-{key}"
          aria-controls="panel-install"
          aria-selected={activeOs === key}
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
    {#if install[activeOs].copiable}
      <div class="snippet-row">
        <pre class="snippet"><code>{install[activeOs].cmd}</code></pre>
        <CopyBtn text={install[activeOs].cmd} />
      </div>
    {:else}
      <div class="snippet-row">
        <pre class="snippet"><code>{install[activeOs].cmd}</code></pre>
        <a
          class="btn small"
          href="https://github.com/tennoworth/tennoworth/releases"
          target="_blank"
          rel="noopener noreferrer"
        >Open releases ↗</a>
      </div>
    {/if}
    <p class="install-note">{install[activeOs].note}</p>
    </div>
  </div>

  <div class="block">
    <h3>First run</h3>
    <div class="steps">
      {#each firstRun as s (s.n)}
        <div class="step">
          <span class="n">{s.n}</span>
          <div class="body">
            <h4>{s.title}</h4>
            <p>{s.body}</p>
            {#if s.n === '02' && isLinux}
              <p class="muted">{PTRACE_NOTE}</p>
              <div class="snippet-row inline">
                <pre class="snippet"><code>{PTRACE_CMD}</code></pre>
                <CopyBtn text={PTRACE_CMD} />
              </div>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  </div>

  <div class="block">
    <h3>Site vs Desktop app</h3>
    <table class="cmp">
      <thead>
        <tr><th></th><th>This site</th><th>Desktop app</th></tr>
      </thead>
      <tbody>
        <tr><td>Market data, trends, vault status</td><td class="yes">✓</td><td class="yes">✓</td></tr>
        <tr><td>Your inventory ranked</td><td class="no">—</td><td class="yes">✓ scan</td></tr>
        <tr><td>List on WFM / manage orders</td><td class="no">—</td><td class="yes">✓</td></tr>
        <tr><td>Login</td><td>no accounts</td><td class="yes">in-app (warframe.market)</td></tr>
      </tbody>
    </table>
  </div>

</section>

<style>
  .desktop-showcase {
    margin-top: 24px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 12px;
    overflow: hidden;
  }

  .hero {
    padding: 36px 32px 28px;
    background:
      radial-gradient(600px 200px at 85% -40px, rgba(78, 158, 234, 0.18), transparent 70%),
      var(--panel-2);
    border-bottom: 1px solid var(--hairline);
  }
  .hero h2 { margin: 0 0 8px; font-size: 24px; font-weight: 700; letter-spacing: -0.015em; }
  .hero .tag {
    color: var(--accent);
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    font-weight: 600;
  }
  .hero .sub { color: var(--muted); max-width: 62ch; font-size: 14px; margin: 10px 0 0; }
  .hero .ctas { display: flex; gap: 12px; flex-wrap: wrap; margin-top: 20px; }
  .hero .trust { margin: 12px 0 0; color: var(--muted); font-size: 12px; }

  .btn {
    appearance: none;
    border: none;
    cursor: pointer;
    font: inherit;
    font-weight: 600;
    font-size: 13.5px;
    padding: 10px 18px;
    border-radius: 8px;
    text-decoration: none;
    display: inline-block;
  }
  .btn.primary { background: var(--accent); color: #fff; }
  .btn.ghost { background: transparent; color: var(--fg); border: 1px solid var(--border); }
  .btn.small { font-size: 11.5px; padding: 0 12px; border-radius: 6px; border: 1px solid var(--border); background: transparent; color: var(--muted); }
  .btn.small:hover { color: var(--accent); }

  .hero-shot { margin-top: 24px; border: 1px solid var(--hairline); border-radius: 8px; overflow: hidden; }
  .hero-shot .shot { display: block; width: 100%; }
  /* The screenshot asset is a human follow-up; until it exists (or on a load
     failure) this placeholder carries the same chrome so the slot never
     renders unstyled. */
  .hero-shot .frame-fallback {
    display: block;
    color: var(--faint);
    font-size: 12px;
    text-align: center;
    padding: 46px 0;
    font-family: ui-monospace, Menlo, monospace;
    background: var(--panel-2);
  }
  .hero-shot .cap { padding: 8px 12px; font-size: 12px; color: var(--muted); }

  .features {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: 1px;
    background: var(--hairline);
  }
  .feature { background: var(--panel); padding: 20px; }
  .feature h3 { margin: 0 0 6px; font-size: 13px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.04em; color: var(--muted); }
  .feature p { margin: 0; font-size: 13px; color: var(--fg); }

  .block { padding: 24px 32px; border-top: 1px solid var(--hairline); }
  .block h3 { margin: 0 0 12px; font-size: 14px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.04em; color: var(--muted); }

  .tabs {
    display: inline-flex;
    gap: 4px;
    padding: 3px;
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-radius: 8px;
    margin-bottom: 14px;
  }
  .tabs button {
    appearance: none;
    border: none;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: 12.5px;
    padding: 5px 12px;
    border-radius: 5px;
    cursor: pointer;
  }
  .tabs button:hover { color: var(--fg); }
  .tabs button[aria-selected="true"] { background: var(--panel); color: var(--fg); box-shadow: 0 0 0 1px var(--border); }

  .snippet-row { display: flex; gap: 6px; align-items: stretch; margin-bottom: 10px; }
  .snippet-row.inline { margin: 8px 0 0; max-width: 560px; }
  .snippet-row.inline .snippet { padding: 8px 10px; font-size: 12px; white-space: nowrap; }
  .snippet {
    flex: 1;
    min-width: 0;
    overflow-x: auto;
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 12px 14px;
    font-family: ui-monospace, Menlo, monospace;
    font-size: 12.5px;
    color: var(--fg);
    white-space: pre;
    margin: 0;
  }
  .snippet code { background: transparent; padding: 0; font-size: inherit; }
  .install-note { margin: 10px 0 0; font-size: 12.5px; color: var(--muted); }

  .steps { display: flex; flex-direction: column; gap: 12px; }
  .step { display: flex; gap: 14px; align-items: flex-start; }
  .step .n {
    font-family: ui-monospace, Menlo, monospace;
    font-size: 13px;
    letter-spacing: 0.05em;
    color: var(--accent);
    font-weight: 600;
    padding-top: 2px;
    min-width: 26px;
  }
  .step .body { min-width: 0; }
  .step h4 { margin: 0 0 4px; font-size: 13.5px; font-weight: 600; }
  .step p { margin: 0; font-size: 13px; color: var(--fg); }
  .step p.muted { color: var(--muted); }
  .step code { background: var(--panel-2); padding: 1px 5px; border-radius: 4px; font-size: 0.92em; }

  table.cmp { width: 100%; border-collapse: collapse; font-size: 13px; }
  table.cmp th, table.cmp td { text-align: left; padding: 8px 12px; border-bottom: 1px solid var(--hairline); }
  table.cmp th { color: var(--muted); font-weight: 600; text-transform: uppercase; letter-spacing: 0.04em; font-size: 11.5px; }
  table.cmp td.yes { color: var(--good); }
  table.cmp td.no { color: var(--faint); }
  table.cmp tr:last-child td { border-bottom: none; }
</style>
