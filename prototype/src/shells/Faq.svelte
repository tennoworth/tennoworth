
  <section data-shell class="faq" id="faq">
    <h2 data-shell>FAQ</h2>

    <details data-shell>
      <summary data-shell>Is this safe? Can I get banned?</summary>
      <p data-shell>
        The desktop app reads the running game's process memory to find the
        <code data-shell>accountId</code> and <code data-shell>nonce</code> your client already
        obtained at login, then calls DE's own inventory endpoint with
        those - same call your game client makes. It writes to disk; it
        doesn't write to the game's memory or modify game state.
      </p>
      <p data-shell>
        <strong data-shell>We can't promise this is ban-safe</strong> - no third-party
        tool honestly can. What we <em data-shell>can</em> say: the app only ever
        reads memory. It never writes to the game, never injects code, and
        doesn't interact with anti-cheat. Other read-only inventory tools have
        run for years with no documented bans, but Digital Extremes has never
        formally blessed this category of tool. Use it at your own risk; we
        accept none. More detail in <a data-shell href="#trust">Trust &amp; safety</a>
        below.
      </p>
    </details>

    <details data-shell>
      <summary data-shell>Where does my inventory data actually go?</summary>
      <p data-shell>
        Nowhere we control. The desktop app scans the game and keeps your
        inventory locally (SQLite + your browser's storage). This site is
        informational - it never receives or stores your inventory. The market
        snapshot is the only thing we host, and it's the same for every visitor.
      </p>
      <p data-shell>
        No accounts, no telemetry, no analytics. Inspect the network tab
        if you don't trust us.
      </p>
    </details>

    <details data-shell>
      <summary data-shell>How current are the prices?</summary>
      <p data-shell>
        Our production box scrapes <a data-shell href="https://warframe.market">warframe.market</a>
        every 2 hours and serves the resulting <code data-shell>market.json</code>
        directly. The dot next to “Market data” is green if the snapshot is
        under 6 h old, amber under 24 h, red after.
      </p>
    </details>

    <details data-shell>
      <summary data-shell>How do I list items on WFM?</summary>
      <p data-shell>
        Listing is a <strong data-shell>desktop app</strong> feature. Install the desktop
        app (Windows + Linux), scan your account, and use <strong data-shell>List on
        WFM</strong> from the Sell view. The first time, the app asks you to
        log in to warframe.market once - the sign-in token is encrypted behind
        a passphrase you choose, and it never leaves your machine. After that,
        listing and your orders work for the rest of the session.
      </p>
      <p data-shell>
        This informational site can't list: it has no account, no token, and no
        way to reach warframe.market on your behalf.
      </p>
    </details>

    <details data-shell>
      <summary data-shell>I listed an item - what happens next?</summary>
      <p data-shell>
        Your listing sits on warframe.market (we create them
        <strong data-shell>hidden</strong> so you can review first - flip them visible
        in My orders or on WFM). When a buyer wants it, they message
        you <strong data-shell>in-game</strong>: <code data-shell>/w YourName Hi! I want to buy your
        Serration…</code>. Invite them to your squad, go to any dojo or relay,
        open a trade, and put in the item while they put in the platinum.
      </p>
      <p data-shell>
        Two gotchas: you must be <strong data-shell>in-game</strong> to trade (WFM marks
        you online automatically while the site is open), and trading requires
        a clan dojo or Maroo's Bazaar. Mark the listing sold on WFM afterwards
        - or just delete it from the <strong data-shell>My orders</strong> tab here.
      </p>
    </details>

    <details data-shell>
      <summary data-shell>Can I sync between desktop and laptop?</summary>
      <p data-shell>
        There are no accounts. In the desktop app, use <strong data-shell>Export</strong>
        to save an encrypted snapshot (passphrase-only, AES-256-GCM, PBKDF2
        600k), then <strong data-shell>Restore</strong> that file on any other device.
      </p>
    </details>

    <details data-shell>
      <summary data-shell>What about Rivens, Arcanes, frame mods?</summary>
      <p data-shell>
        Arcanes and frame mods (in <code data-shell>RawUpgrades</code>) are
        resolved and priced like anything else. Rivens are
        per-instance items with rolled stats - they don't have a single
        market price, they need a separate model (riven grader). Not
        supported here yet; semlar's tools do this better.
      </p>
    </details>

    <details data-shell>
      <summary data-shell>Want to support this?</summary>
      <p data-shell>
        Free, no ads, no accounts, no telemetry - always. If it saved
        you some plat and you want to chip in toward hosting, that's
        appreciated but never expected:
        <a data-shell href="https://ko-fi.com/prowly" target="_blank" rel="noopener">ko-fi.com/prowly</a>.
      </p>
    </details>
  </section>

  <section data-shell id="trust" class="faq trust">
    <h2 data-shell>Trust &amp; safety</h2>
    <p data-shell class="trust-lede">
      Straight answers about what this tool touches, what it can't, and how to
      check us for yourself - not legal boilerplate. The short version: the
      desktop app only ever <em data-shell>reads</em> your running game, the web app runs
      entirely in your browser, and we can't promise this is ban-safe - so use
      it at your own risk.
    </p>

    <details data-shell>
      <summary data-shell>What the desktop app reads</summary>
      <p data-shell>
        While Warframe is running, the desktop app scans the game's memory for
        two things your client already has: your <strong data-shell>account ID</strong>
        and the <strong data-shell>session nonce</strong> it uses to talk to Digital
        Extremes. It then asks DE's own inventory API for your items - the exact
        same request the game client makes. It <strong data-shell>never writes to the
        game's memory and never injects code</strong>; it only reads, then makes
        one HTTPS call. Your account ID and nonce are used for that single
        request and discarded - never printed, never saved, never sent anywhere
        else.
      </p>
      <p data-shell>
        For trade detection (the Ledger), the app also
        <strong data-shell>reads the game's own text log</strong> (<code data-shell>EE.log</code>,
        the file Warframe itself writes) - read-only tailing of a plain file,
        the same thing WFInfo and AlecaFrame have done for years. It starts at
        the end of the file, so nothing from before the app launched is ever
        read, and if the log isn't there, trade detection is simply off.
      </p>
      <p data-shell>
        If you explicitly enable the relic reward overlay, a reward log line or
        your retry shortcut captures the Warframe window and runs English OCR
        locally. The frame is cropped and held only in memory-never saved or
        uploaded. The overlay can use the existing cached snapshot offline;
        optional live pricing sends only the matched public item slug to
        warframe.market.
      </p>
    </details>

    <details data-shell>
      <summary data-shell>Why does the app read game memory?</summary>
      <p data-shell>
        The game does not expose a supported local inventory API. A read-only
        scan lets TennoWorth find the short-lived credentials the running game
        already uses, request your inventory once, and then discard them.
        Windows uses <code data-shell>ReadProcessMemory</code>; Linux reads
        <code data-shell>/proc/&lt;pid&gt;/mem</code>. Neither path modifies the game.
      </p>
    </details>

    <details data-shell>
      <summary data-shell>How it talks to warframe.market</summary>
      <p data-shell>
        Every request identifies itself as TennoWorth (name, version, and a
        contact link in the <code data-shell>User-Agent</code> - warframe.market's API
        rules require it, and we follow them). Traffic is polite by
        construction: live price checks run at most ~3 requests per second in
        short bursts of up to 100 items, price watches re-check every 10
        minutes, and riven comps are capped at warframe.market's 10 searches
        per minute. <strong data-shell>Order writes only ever happen when you click</strong>
        - the app never auto-bids, never auto-undercuts, and never reprices
        without you. The bots that instantly match every bid on
        warframe.market are exactly what this app refuses to be.
      </p>
    </details>

    <details data-shell>
      <summary data-shell>What never leaves your machine</summary>
      <p data-shell>
        The web app has no backend and no accounts - every item, price join, and
        ranking is computed in your browser tab. If you log in to
        warframe.market to post listings, that login token is
        <strong data-shell>encrypted on disk</strong> (AES-256-GCM) at
        <code data-shell>~/.config/wfminv/</code> (or the Windows equivalent), and the
        webview never sees it - it stays in the Rust process. The
        desktop app can optionally remember the unlock key in your OS
        keyring (KWallet, GNOME Keyring, Windows Credential Manager) -
        never the passphrase itself; details in SECURITY.md.
      </p>
      <p data-shell>
        No telemetry, no analytics - confirm it in your browser's network tab.
      </p>
    </details>

    <details data-shell>
      <summary data-shell>How you can verify all this yourself</summary>
      <p data-shell>
        Everything is open source - read the desktop app's memory-scan code and
        the web app's join logic. The desktop releases are
        <strong data-shell>reproducibly built in public CI</strong>: you can audit the
        workflow file, the source at the tagged commit, and the build logs, and
        every release ships checksums so you can confirm
        your download matches. The site
        loads <strong data-shell>zero third-party scripts</strong> - inspect the page's
        Content-Security-Policy. Full detail lives in
        <a data-shell href="https://github.com/tennoworth/tennoworth/blob/main/SECURITY.md" target="_blank" rel="noopener noreferrer">SECURITY.md</a>.
      </p>
    </details>

    <details data-shell>
      <summary data-shell>The honest risk - and what happens if DE's stance changes</summary>
      <p data-shell>
        We <strong data-shell>can't promise this is ban-safe</strong>; no third-party tool
        honestly can. What we can say: the desktop app only reads memory, never
        writes or injects, and doesn't interact with anti-cheat. Other
        read-only inventory tools have run for years with no documented bans,
        but Digital Extremes has never formally blessed this category of tool.
        Use it at your own risk.
      </p>
      <p data-shell>
        And if DE's stance ever changes, only the memory-scan path stops: the
        <strong data-shell>market browser on this site keeps working</strong>.
      </p>
    </details>
  </section>
