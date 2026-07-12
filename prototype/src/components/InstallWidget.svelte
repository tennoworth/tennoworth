<script lang="ts">
  import { onMount } from 'svelte';

  type OsName = 'linux' | 'windows' | 'mac';

  // navigator.userAgentData is missing from lib.dom.d.ts in older TS
  // libs; cast through a narrow shape rather than `any`.
  type UAData = {
    getHighEntropyValues(hints: string[]): Promise<{ platform: string }>;
  };
  const uaData: UAData | undefined = (navigator as unknown as { userAgentData?: UAData }).userAgentData;

  // OS detection. UA-Client-Hints is the modern API; UA string is the
  // fallback. We never auto-redirect — just preselect the right tab.
  let detectedOs = $state<OsName>('linux');
  let activeOs = $state<OsName>('linux');
  let copied = $state<OsName | null>(null);

  onMount(() => {
    const setFrom = (plat: string): void => {
      const p = (plat || '').toLowerCase();
      if (p.includes('win'))   { detectedOs = 'windows'; }
      else if (p.includes('mac') || p.includes('darwin')) { detectedOs = 'mac'; }
      else                     { detectedOs = 'linux'; }
      activeOs = detectedOs;
    };
    if (uaData?.getHighEntropyValues) {
      uaData.getHighEntropyValues(['platform'])
        .then((d) => setFrom(d.platform))
        .catch(() => setFrom(navigator.platform || navigator.userAgent));
    } else {
      setFrom(navigator.platform || navigator.userAgent);
    }
  });

  let origin = $derived(typeof location !== 'undefined' ? location.origin : '');

  // macOS doesn't have a release binary yet but the curl-install lives at
  // the linux path for it (Mac users can rebuild from source). Mapping
  // mac → linux command keeps the UA-detection path safe; if/when we
  // ship a Mac binary it gets its own entry.
  let commands: Record<OsName, string> = $derived({
    linux:   `curl -fsSL ${origin}/install.sh | sh`,
    windows: `iwr ${origin}/install.ps1 | iex`,
    mac:     `curl -fsSL ${origin}/install.sh | sh`,
  });

  async function copy(os: OsName): Promise<void> {
    try {
      await navigator.clipboard.writeText(commands[os]);
      copied = os;
      setTimeout(() => { if (copied === os) copied = null; }, 1400);
    } catch {
      copied = null;
    }
  }
</script>

<section id="companion" class="install">
  <header>
    <h2>Install the companion</h2>
    <p class="muted">
      Tiny CLI (~3 MB, single file). Reads the running game's process
      memory to fetch <code>inventory.json</code> from DE's own endpoint.
      One command — paste and go.
    </p>
  </header>

  <div class="tabs" role="tablist">
    <button
      role="tab"
      class:active={activeOs === 'linux'}
      aria-selected={activeOs === 'linux'}
      onclick={() => (activeOs = 'linux')}
    >Linux{detectedOs === 'linux' ? ' · detected' : ''}</button>
    <button
      role="tab"
      class:active={activeOs === 'windows'}
      aria-selected={activeOs === 'windows'}
      onclick={() => (activeOs = 'windows')}
    >Windows{detectedOs === 'windows' ? ' · detected' : ''}</button>
  </div>

  <div class="cmd">
    <code>{commands[activeOs]}</code>
    <button class="copy" onclick={() => copy(activeOs)} aria-label="Copy command">
      {copied === activeOs ? 'Copied' : 'Copy'}
    </button>
  </div>

  {#if activeOs === 'linux'}
    <p class="muted small">
      Installs to <code>~/.local/bin</code>. Needs <code>curl</code>. To run
      without sudo each time, one-time (path-agnostic, also works for a
      from-source build on your PATH):
      <code>sudo setcap cap_sys_ptrace=eip "$(command -v wfm-fetch-inventory)"</code>.
      Re-run after every upgrade — replacing the binary wipes the capability.
    </p>
  {:else}
    <p class="muted small">
      PowerShell. Installs to <code>%LOCALAPPDATA%\wfminv</code> and adds it
      to your user PATH. No elevation needed as long as you run it as the
      same user that launched Warframe.
    </p>
  {/if}

  <p class="muted small">
    That's all you need to see <em>what to sell</em>. To also create/edit
    warframe.market listings from the app, run <code>wfm-fetch-inventory login</code>
    once, then <code>wfm-fetch-inventory serve</code> in a terminal, and paste
    the URL it prints into the Companion tab.
  </p>

  <details class="advanced">
    <summary>Prefer to download the binary manually?</summary>
    <p>
      Grab the latest release from
      <a href="https://github.com/OWNER/REPO/releases/latest" target="_blank" rel="noopener noreferrer">GitHub releases</a>
      (asset names: <code>wfm-fetch-inventory-linux-x86_64</code>,
      <code>wfm-fetch-inventory-windows-x86_64.exe</code>). The release also
      ships a <code>SHA256SUMS</code> file — verify with
      <code>sha256sum</code> (Linux) or <code>Get-FileHash</code> (Windows).
    </p>
    <p>
      Both installers are short, plain-text, and live in this repo at
      <code>prototype/public/install.sh</code> /
      <code>install.ps1</code>. Inspect before piping.
    </p>
  </details>
</section>

<style>
  .install {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 18px 20px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .install header { display: flex; flex-direction: column; gap: 4px; }
  .install header h2 { margin: 0; font-size: 14px; font-weight: 600; letter-spacing: 0.04em; text-transform: uppercase; color: var(--muted); }
  .install header p { margin: 0; font-size: 13px; color: var(--fg); max-width: 70ch; line-height: 1.5; }
  .install header p.muted { color: var(--fg); }
  .tabs {
    display: inline-flex;
    gap: 4px;
    padding: 3px;
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-radius: 8px;
    width: max-content;
  }
  .tabs button {
    appearance: none;
    border: none;
    background: transparent;
    color: var(--muted);
    font-size: 12.5px;
    padding: 5px 12px;
    border-radius: 5px;
    cursor: pointer;
    transition: color 120ms ease, background 120ms ease;
  }
  .tabs button:hover { color: var(--fg); }
  .tabs button.active {
    background: var(--panel);
    color: var(--fg);
    box-shadow: 0 0 0 1px var(--border);
  }
  .cmd {
    display: flex;
    align-items: stretch;
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
  }
  .cmd code {
    flex: 1;
    background: transparent;
    color: var(--fg);
    padding: 12px 14px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 13px;
    white-space: nowrap;
    overflow-x: auto;
    border-radius: 0;
  }
  .cmd .copy {
    background: transparent;
    border: none;
    border-left: 1px solid var(--border);
    color: var(--muted);
    font-size: 12px;
    padding: 0 16px;
    cursor: pointer;
    transition: color 120ms ease, background 120ms ease;
  }
  .cmd .copy:hover { color: var(--accent); background: var(--panel); }
  .small { font-size: 12px; line-height: 1.55; }
  .small code { font-size: 0.92em; }
  .advanced {
    margin-top: 2px;
    border-top: 1px solid var(--border);
    padding-top: 12px;
  }
  .advanced > summary {
    cursor: pointer;
    list-style: none;
    font-size: 12.5px;
    color: var(--muted);
    user-select: none;
  }
  .advanced > summary::-webkit-details-marker { display: none; }
  .advanced > summary::before {
    content: '+';
    display: inline-block;
    width: 12px;
    color: var(--muted);
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .advanced[open] > summary::before { content: '−'; }
  .advanced > summary:hover { color: var(--accent); }
  .advanced p { margin: 8px 0 0 0; font-size: 12.5px; color: var(--muted); line-height: 1.55; max-width: 72ch; }
</style>
