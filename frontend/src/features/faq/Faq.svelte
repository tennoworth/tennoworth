
  <!-- One Questions panel: "Is this safe?" used to be asked in the hand-off,
       the FAQ and Trust & safety. Trust answers sit in their own column, with
       the data-location question beside them; #faq and #trust both land here. -->
  <section data-shell class="faq questions" id="faq">
    <h2 data-shell>Questions</h2>
    <p data-shell class="trust-lede">
      Straight answers about what this tool touches, what it can't, and how to
      check us for yourself - not legal boilerplate. The short version: the
      desktop app only ever <em data-shell>reads</em> your running game, the web app runs
      entirely in your browser, and we can't promise this is ban-safe - so use
      it at your own risk.
    </p>
    <div data-shell class="qcol" id="trust">
      <h3 data-shell>Trust &amp; safety</h3>
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
          construction: at most two requests start per second with no more than
          two in flight, price watches re-check every 10 minutes, and riven
          comps stay under warframe.market's 10 searches per minute.
          <strong data-shell>Prices and new listings only ever change when you click</strong>
          - the app never auto-bids, never auto-undercuts, and never reprices
          without you. The two automatic actions are both off until you enable
          them: lowering a listing's quantity after a sale the Ledger confirmed,
          and keeping your warframe.market status in step with the game. The bots that instantly match every bid on
          warframe.market are exactly what this app refuses to be.
        </p>
      </details>
      <details data-shell>
        <summary data-shell>What never leaves your machine</summary>
        <p data-shell>
          There are no TennoWorth accounts and no inventory upload: the site only
          downloads the public market snapshot, and the desktop app computes every
          item, price join, and ranking on your machine. If you log in to
          warframe.market to post listings, that login token is
          <strong data-shell>encrypted on disk</strong> (AES-256-GCM) at
          <code data-shell>~/.config/wfminv/</code> (or the Windows equivalent), and the
          webview never sees it - it stays in the Rust process. The
          desktop app can optionally remember the unlock key in your OS
          keyring (KWallet, GNOME Keyring, Windows Credential Manager) -
          never the passphrase itself; details in SECURITY.md.
        </p>
        <p data-shell>
          No website analytics. Optional desktop usage sharing is disabled until you enable it,
          and so is price sharing, which sends the item and price of trades you make for
          platinum and never who you traded with.
        </p>
      </details>
      <details data-shell>
        <summary data-shell>How you can verify all this yourself</summary>
        <p data-shell>
          Everything is open source - read the desktop app's memory-scan code and
          the code that ranks your items. The desktop releases are
          <strong data-shell>built in public CI</strong>: you can audit the
          workflow file, the source at the tagged commit, and the build logs, and
          every release ships checksums so you can confirm your download matches.
          That makes the builds auditable, not bit-for-bit reproducible. The site
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
      <details data-shell>
        <summary data-shell>Where does my inventory data actually go?</summary>
        <p data-shell>
          Nowhere we control. The desktop app scans the game and keeps your
          inventory locally (SQLite + your browser's storage). This site is
          informational - it never receives or stores your inventory. The market snapshot and public aggregate usage counts are the same for every visitor.
        </p>
        <p data-shell>
          No TennoWorth accounts or website analytics. Desktop usage counting is optional and off by default. Only a daily token is sent after you enable it in Settings; inventory stays local.
        </p>
      </details>
    </div>
    <div data-shell class="qcol">
      <h3 data-shell>Using TennoWorth</h3>
      <details data-shell>
        <summary data-shell>What does the community usage count measure?</summary>
        <p data-shell>Only opted-in desktop installations running that UTC day, including in the tray. Two computers count twice. Downloads and visits do not count, and we do not estimate total people. <a data-shell href="/#community-usage">View the public daily chart</a>.</p>
        <p data-shell>Tokens change daily and expire from the collector at the next UTC rollover. Only aggregate counts are retained and backed up. Turning sharing off stops future check-ins, but does not remove past aggregate contributions. IP addresses are visible to the delivery infrastructure; see <a data-shell href="https://github.com/tennoworth/tennoworth/security/policy">the security policy</a> for the hosting boundary.</p>
      </details>
      <details data-shell>
        <summary data-shell>How current are the prices?</summary>
        <p data-shell>
          Our production box scrapes <a data-shell href="https://warframe.market">warframe.market</a>
          every 2 hours and serves the resulting <code data-shell>market.json</code>
          directly. The dot next to “Market data” is green if the snapshot is
          under 3 h old, amber under 24 h, red after.
        </p>
      </details>
      <details data-shell>
        <summary data-shell>How do I list items on WFM?</summary>
        <p data-shell>
          Listing is a <strong data-shell>desktop app</strong> feature. Install the desktop
          app (Windows + Linux), scan your account, and use <strong data-shell>List on
          WFM</strong> from the Sell view. The first time, you sign in on
          warframe.market's own page in a separate window, so TennoWorth never
          sees your password. The resulting sign-in token is encrypted behind a
          passphrase you choose and never leaves your machine; the app can
          remember the unlock in your OS keyring so later launches don't ask
          again.
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
          Serration…</code>. Invite them to your squad, meet at your clan dojo's
          Trading Post or Maroo's Bazaar, open a trade, and put in the item while
          they put in the platinum.
        </p>
        <p data-shell>
          Buyers look for sellers shown as <strong data-shell>Online in game</strong>.
          Set your warframe.market status from the status strip, the tray or
          Settings, or let it follow the game: online in game while Warframe
          runs, invisible when it closes. After a sale, the
          <strong data-shell>Ledger</strong> can lower or remove the matching
          listing for you if you turn that on; otherwise remove it from
          <strong data-shell>My orders</strong> or mark it sold on WFM.
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
          per-instance items with rolled stats, so they have no single market
          price and the app does not invent one. The desktop app's Rivens view
          shows DE's weekly price band for the weapon, the disposition trend,
          the splices each riven qualifies for, and the cheapest live buyouts
          for rolls with the same positive stats.
        </p>
      </details>
      <details data-shell>
        <summary data-shell>Want to support this?</summary>
        <p data-shell>
          Free, no ads, no accounts. Daily desktop usage sharing is optional and off by default. If it saved
          you some plat and you want to chip in toward hosting, that's
          appreciated but never expected:
          <a data-shell href="https://ko-fi.com/prowly" target="_blank" rel="noopener">ko-fi.com/prowly</a>.
        </p>
      </details>
    </div>
  </section>
