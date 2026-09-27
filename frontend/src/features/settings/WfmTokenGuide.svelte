<script lang="ts">
  // Steps for copying the warframe.market `JWT` cookie out of the user's own
  // browser - the fallback sign-in when the in-app window cannot load WFM.
  // Text steps rather than screenshots or drawings: browser devtools are
  // redesigned often, and a picture of the wrong layout misleads more than a
  // step that names the tab.
  type Browser = 'firefox' | 'chromium';
  let browser = $state<Browser>(
    typeof navigator !== 'undefined' && /Windows/.test(navigator.userAgent) ? 'chromium' : 'firefox',
  );
</script>

<div class="token-guide" data-testid="wfm-token-guide">
  <div class="ui-segmented xs" role="group" aria-label="Your browser">
    <button type="button" aria-pressed={browser === 'firefox'} onclick={() => (browser = 'firefox')}>Firefox</button>
    <button type="button" aria-pressed={browser === 'chromium'} onclick={() => (browser = 'chromium')}>Chrome / Edge</button>
  </div>
  <ol>
    <li>
      <a href="https://warframe.market/auth/signin" target="_blank" rel="noopener noreferrer">Open warframe.market</a>
      in your browser and sign in.
    </li>
    {#if browser === 'firefox'}
      <li>Press <kbd>Shift</kbd>+<kbd>F9</kbd> to open the Storage panel.</li>
      <li>Expand <b>Cookies</b> and select <b>https://warframe.market</b>.</li>
      <li>Double-click the <b>Value</b> of the <b>JWT</b> row, then press <kbd>Ctrl</kbd>+<kbd>A</kbd> and <kbd>Ctrl</kbd>+<kbd>C</kbd>.</li>
    {:else}
      <li>Press <kbd>F12</kbd> and open the <b>Application</b> tab (behind <b>»</b> if it is hidden).</li>
      <li>Under <b>Storage</b> → <b>Cookies</b>, select <b>https://warframe.market</b>.</li>
      <li>Double-click the <b>Value</b> of the <b>JWT</b> row, then press <kbd>Ctrl</kbd>+<kbd>C</kbd>.</li>
    {/if}
    <li>Paste it into <b>Session token</b> above.</li>
  </ol>
  <p class="muted">
    TennoWorth checks the token with warframe.market before saving it,
    encrypted. It signs in as you, so don't share it. Close the tab rather than
    logging out of warframe.market there - logging out may end this token too.
  </p>
</div>

<style>
  .token-guide { display: flex; flex-direction: column; gap: var(--s2); }
  .token-guide .ui-segmented { align-self: flex-start; }
  .token-guide ol {
    margin: 0;
    padding-left: var(--s5);
    font-family: var(--font-body);
    font-size: var(--text-control);
    line-height: var(--leading-body);
  }
  .token-guide li + li { margin-top: var(--s1); }
  .token-guide p { margin: 0; }
  kbd {
    font-family: var(--font-mono);
    font-size: var(--text-caption);
    padding: 0 var(--s1);
    border: 1px solid var(--border);
    border-radius: var(--radius-input);
    background: var(--panel-2);
    color: var(--fg);
  }
</style>
