<script lang="ts">
  // The breadth reveal on the hosted landing ({#if !isDesktop} in App.svelte,
  // after DesktopShowcase, before the FAQ).
  //
  // Everything above this point is search → movers → vaulted → hand-off,
  // which reads as a price-lookup tool. It is one; it is also seven other
  // things once the scan has run, and a visitor had no way to find that out.
  // This is a vertical tab rail: one tab per surface, each with a sentence
  // and a MINIATURE of that surface rendered from hardcoded sample rows.
  //
  // Miniatures, not screenshots, on purpose — same reasoning as
  // DesktopShowcase's sample table: a PNG freezes one mode, goes stale the
  // first time a column moves, and costs bundle weight. A token-driven table
  // re-skins with the theme and can only drift if someone edits it.
  //
  // The column heads, verdict words and reason sentences below are the app's
  // OWN strings (ResultsTable's pills, advisor.ts's verdicts, listing-health's
  // "above the lowest other ask", the watch conditions, the relic planner's
  // "p / crack"). Keep them that way: the point of the rail is that the
  // visitor recognises the real thing when they open it. Every NUMBER is
  // invented — nothing here reads the snapshot.

  type TabId =
    | 'sell' | 'sets' | 'relics' | 'rivens'
    | 'watches' | 'ledger' | 'orders' | 'advisor';

  type Tab = { id: TabId; title: string; blurb: string };

  // Order = the app's own sidebar order (the Trade group, then the account
  // group), so someone who installs finds the nav where the rail said.
  const TABS: Tab[] = [
    {
      id: 'sell',
      title: 'Sell',
      blurb:
        'Your inventory ranked by expected plat per day rather than sticker price — a 300p item nobody buys sits below a 20p one that clears twice a day.',
    },
    {
      id: 'sets',
      title: 'Set picks',
      blurb:
        'Prime sets you nearly hold: what the missing parts cost, what the assembled set clears at, and whether that beats selling the parts one at a time.',
    },
    {
      id: 'relics',
      title: 'Relics',
      blurb:
        'The relics you own ranked by expected plat per crack, priced from what is actually in the drop table — and told when selling one intact beats running it.',
    },
    {
      id: 'rivens',
      title: 'Rivens',
      blurb:
        "Your scanned rivens against DE's weekly price bands, with the disposition move that shifted the floor under them since.",
    },
    {
      id: 'watches',
      title: 'Price watches',
      blurb:
        'Name a price on any item and the app checks warframe.market every ten minutes in the background, then notifies you when it crosses. Nothing left open in a tab.',
    },
    {
      id: 'ledger',
      title: 'Ledger',
      blurb:
        "Trades read from the game's own EE.log as they complete, so realised plat is a record rather than a guess — and a sale can shrink the matching listing for you.",
    },
    {
      id: 'orders',
      title: 'My orders',
      blurb:
        'Your live warframe.market listings checked against the current top of book: which asks have been walked past, and what to reprice them to.',
    },
    {
      id: 'advisor',
      title: 'Advisor',
      blurb:
        'Hold or sell, argued from the prime calendar and a year of price history — release decay, Resurgence reprints, post-vault ramps. Advice only; nothing is automated.',
    },
  ];

  let active = $state<TabId>('sell');

  function focusTab(id: TabId): void {
    active = id;
    document.getElementById(`frt-${id}`)?.focus();
  }

  // WAI-ARIA tabs pattern. The rail is vertical at width but stacks to a
  // horizontal strip under 48rem, so BOTH axes move selection — a reader on
  // the stacked layout reaches for Left/Right, one on the rail for Up/Down,
  // and neither should find a dead key.
  function onkeydown(e: KeyboardEvent): void {
    const i = TABS.findIndex((t) => t.id === active);
    let next: TabId | null = null;
    if (e.key === 'ArrowDown' || e.key === 'ArrowRight') next = TABS[(i + 1) % TABS.length].id;
    else if (e.key === 'ArrowUp' || e.key === 'ArrowLeft') next = TABS[(i - 1 + TABS.length) % TABS.length].id;
    else if (e.key === 'Home') next = TABS[0].id;
    else if (e.key === 'End') next = TABS[TABS.length - 1].id;
    if (next) {
      e.preventDefault();
      focusTab(next);
    }
  }

  // ---- sample rows, one shape per surface -------------------------------
  // Score = min(own, vol 48h ÷ 2) × clearing price, the definition the
  // hand-off panel and the Sell view both state.
  const sell = [
    { item: 'Ash Prime Systems', tag: 'vaulted', own: 2, score: 76, avg: 38 },
    { item: 'Primed Continuity', tag: 'peak', own: 1, score: 61, avg: 61 },
    { item: 'Nekros Prime Blueprint', tag: '', own: 4, score: 54, avg: 27 },
    { item: 'Braton Prime Receiver', tag: '', own: 6, score: 33, avg: 11 },
    { item: 'Arcane Nullifier', tag: 'patience', own: 3, score: 0, avg: 24 },
  ];

  // One set: three parts held, one missing. The verdict below compares
  // completing it against listing the three parts separately.
  const setParts = [
    { part: 'Blueprint', have: true, price: 22 },
    { part: 'Neuroptics', have: true, price: 14 },
    { part: 'Systems', have: true, price: 16 },
    { part: 'Chassis', have: false, price: 11 },
  ];
  const held = setParts.filter((p) => p.have);
  const heldTotal = held.reduce((n, p) => n + p.price, 0); // 52
  const missing = setParts.find((p) => !p.have)!;
  const setPrice = 88;
  const setNet = setPrice - missing.price - heldTotal; // +25

  const relic = [
    { r: 'R', drop: 'Garuda Prime Blueprint', chance: '20%', value: 45 },
    { r: 'U', drop: 'Baruuk Prime Systems', chance: '17%', value: 18 },
    { r: 'C', drop: 'Braton Prime Barrel', chance: '21%', value: 8 },
    { r: 'C', drop: 'Forma Blueprint', chance: '21%', value: 0 },
  ];

  const rivens = [
    { weapon: 'Kuva Bramma', stats: '+182% Multishot / −53% Zoom', rolls: 3, dispo: '1.05', move: '▲ 12%', band: '90 – 210p', note: 'rolled · n=42' },
    { weapon: 'Rubico Prime', stats: '+CC / +CD / −Status', rolls: 0, dispo: '0.85', move: '▼ 8%', band: '340 – 700p', note: 'unrolled · n=17' },
    { weapon: 'Torid', stats: 'challenge to reveal', rolls: 0, dispo: '1.25', move: '', band: '—', note: 'veiled' },
  ];

  const watches = [
    { item: 'Primed Flow', cond: 'ask ≤ 45p', seen: '38p · 12 min ago', hit: true },
    { item: 'Ash Prime Set', cond: 'bid ≥ 120p', seen: '96p · 9 min ago', hit: false },
    { item: 'Arcane Energize', cond: 'ask ≤ 210p', seen: 'not checked yet', hit: false },
  ];

  const ledger = [
    { when: 'Aug 19, 21:04', kind: 'Sold', item: 'Primed Flow', who: 'Tenno_Kaz', plat: 45 },
    { when: 'Aug 19, 19:37', kind: 'Sold', item: 'Ash Prime Systems ×2', who: 'Ordis_Fan', plat: 76 },
    { when: 'Aug 18, 22:10', kind: 'Bought', item: 'Saryn Prime Chassis', who: 'Vor_Prime', plat: -11 },
  ];
  const ledgerNet = ledger.reduce((n, t) => n + t.plat, 0);

  const orders = [
    { item: 'Arcane Grace', listed: 96, live: 96, fix: '', why: 'top of book', bad: false },
    { item: 'Primed Flow', listed: 52, live: 44, fix: '52p → 44p', why: 'above the lowest other ask', bad: true },
    { item: 'Saryn Prime Set', listed: 88, live: 88, fix: '', why: 'top of book', bad: false },
  ];

  const advisor = [
    {
      item: 'Ash Prime Set',
      verdict: 'hold',
      hold: true,
      reason: 'vaulted 96 d ago — the post-vault ramp typically runs for months · now ×1.3 its pre-vault price · 30 d move +7%',
    },
    {
      item: 'Nidus Prime Blueprint',
      verdict: 'sell now',
      hold: false,
      reason: 'Prime Resurgence is reprinting it until 2026-09-02 · price falling: −11% over 30 d',
    },
  ];
</script>

<section class="wrap tw rail-panel" aria-labelledby="feature-rail-h">
  <div class="rail">
    <h3 id="feature-rail-h">The rest of the app, in miniature</h3>
    <span class="exp">Eight surfaces behind the scan · every number below is a sample</span>
  </div>

  <div class="rbody">
    <div class="tabs" role="tablist" aria-orientation="vertical" aria-label="Desktop app surfaces">
      {#each TABS as t (t.id)}
        <button
          type="button"
          role="tab"
          id="frt-{t.id}"
          aria-controls="frp-{t.id}"
          aria-selected={active === t.id}
          tabindex={active === t.id ? 0 : -1}
          class:on={active === t.id}
          onclick={() => (active = t.id)}
          {onkeydown}
        >{t.title}</button>
      {/each}
    </div>

    {#each TABS as t (t.id)}
      {#if active === t.id}
        <div class="panel" role="tabpanel" id="frp-{t.id}" aria-labelledby="frt-{t.id}" tabindex="0">
          <p class="desc">{t.blurb}</p>

          <div class="mini scroll">
            {#if t.id === 'sell'}
              <table class="tw">
                <thead><tr>
                  <th class="l">Item</th>
                  <th>Own</th>
                  <th title="Expected plat per day: min(owned, vol 48h ÷ 2) × clearing price">Score</th>
                  <th>Avg</th>
                </tr></thead>
                <tbody>
                  {#each sell as r (r.item)}
                    <tr>
                      <td class="l">{r.item}{#if r.tag}<span class="tag {r.tag}">{r.tag}</span>{/if}</td>
                      <td>{r.own}</td>
                      <td class="score">{#if r.score}{r.score}{:else}<span class="faint">—</span>{/if}</td>
                      <td>{r.avg}<span class="unit">p</span></td>
                    </tr>
                  {/each}
                </tbody>
              </table>

            {:else if t.id === 'sets'}
              <table class="tw">
                <thead><tr>
                  <th class="l">Saryn Prime Set</th>
                  <th>Have</th>
                  <th>Part clears at</th>
                </tr></thead>
                <tbody>
                  {#each setParts as p (p.part)}
                    <tr class:dim={!p.have}>
                      <td class="l">{p.part}</td>
                      <td>{#if p.have}<span class="good">✓</span>{:else}<span class="faint">—</span>{/if}</td>
                      <td>{p.price}<span class="unit">p</span></td>
                    </tr>
                  {/each}
                </tbody>
              </table>

            {:else if t.id === 'relics'}
              <table class="tw">
                <thead><tr>
                  <th class="l">Lith G1 ×4 · Radiant</th>
                  <th>Chance</th>
                  <th>Worth</th>
                </tr></thead>
                <tbody>
                  {#each relic as d (d.drop)}
                    <tr class:dim={d.value === 0}>
                      <td class="l"><span class="rar {d.r}">{d.r}</span>{d.drop}</td>
                      <td>{d.chance}</td>
                      <td>{#if d.value}{d.value}<span class="unit">p</span>{:else}<span class="faint">—</span>{/if}</td>
                    </tr>
                  {/each}
                </tbody>
              </table>

            {:else if t.id === 'rivens'}
              <table class="tw">
                <thead><tr>
                  <th class="l">Weapon</th>
                  <th class="l">Stats</th>
                  <th>Rolls</th>
                  <th class="l">Dispo</th>
                  <th class="l">DE weekly</th>
                </tr></thead>
                <tbody>
                  {#each rivens as r (r.weapon)}
                    <tr class:dim={r.band === '—'}>
                      <td class="l">{r.weapon}</td>
                      <td class="l stats">{r.stats}</td>
                      <td>{r.rolls}</td>
                      <td class="l">
                        {r.dispo}
                        {#if r.move}<span class:up={r.move.startsWith('▲')} class:down={r.move.startsWith('▼')}>{r.move}</span>{/if}
                      </td>
                      <td class="l"><span class="fg">{r.band}</span> <span class="note">{r.note}</span></td>
                    </tr>
                  {/each}
                </tbody>
              </table>

            {:else if t.id === 'watches'}
              <table class="tw">
                <thead><tr>
                  <th class="l">Item</th>
                  <th class="l">Condition</th>
                  <th class="l">Last seen</th>
                  <th class="l">Status</th>
                </tr></thead>
                <tbody>
                  {#each watches as w (w.item)}
                    <tr>
                      <td class="l">{w.item}</td>
                      <td class="l">{w.cond}</td>
                      <td class="l">{w.seen}</td>
                      <td class="l">{#if w.hit}<span class="good">satisfied</span>{:else}<span class="faint">waiting</span>{/if}</td>
                    </tr>
                  {/each}
                </tbody>
              </table>

            {:else if t.id === 'ledger'}
              <table class="tw">
                <thead><tr>
                  <th class="l">When</th>
                  <th class="l">&nbsp;</th>
                  <th class="l">Items</th>
                  <th class="l">With</th>
                  <th>Plat</th>
                </tr></thead>
                <tbody>
                  {#each ledger as e (e.when)}
                    <tr>
                      <td class="l">{e.when}</td>
                      <td class="l"><span class="type" class:buy={e.plat < 0}>{e.kind}</span></td>
                      <td class="l">{e.item}</td>
                      <td class="l">{e.who}</td>
                      <td class:good={e.plat > 0} class:bad={e.plat < 0}>{e.plat > 0 ? '+' : '−'}{Math.abs(e.plat)}<span class="unit">p</span></td>
                    </tr>
                  {/each}
                  <tr class="sum">
                    <td class="l" colspan="4">Net, all time — confirmed from EE.log</td>
                    <td class="score">+{ledgerNet}<span class="unit">p</span></td>
                  </tr>
                </tbody>
              </table>

            {:else if t.id === 'orders'}
              <table class="tw">
                <thead><tr>
                  <th class="l">Item</th>
                  <th>Listed</th>
                  <th>Live ask</th>
                  <th class="l">Why</th>
                </tr></thead>
                <tbody>
                  {#each orders as o (o.item)}
                    <tr>
                      <td class="l">{o.item}</td>
                      <td class="fg">{o.listed}<span class="unit">p</span></td>
                      <td>{o.live}<span class="unit">p</span></td>
                      <td class="reason" class:bad={o.bad}>
                        {#if o.fix}<span class="fix">{o.fix}</span>{/if}{o.why}
                      </td>
                    </tr>
                  {/each}
                </tbody>
              </table>

            {:else}
              <table class="tw">
                <thead><tr>
                  <th class="l">Item</th>
                  <th class="l">Advice</th>
                  <th class="l">Because</th>
                </tr></thead>
                <tbody>
                  {#each advisor as a (a.item)}
                    <tr>
                      <td class="l">{a.item}</td>
                      <td class="l"><span class="tag" class:hold={a.hold} class:peak={!a.hold}>{a.verdict}</span></td>
                      <td class="reason">{a.reason}</td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            {/if}
          </div>

          {#if t.id === 'sets'}
            <p class="verdict">
              <!-- .lbl uppercases, so the plat figure stays outside it — "+25P"
                   is not a unit anyone writes. -->
              <span class="lbl good">Complete</span>
              +{setNet}p — buy {missing.part} for {missing.price}p and sell as a set for {setPrice}p, against
              {heldTotal}p for the three parts you hold sold one at a time.
            </p>
          {:else if t.id === 'relics'}
            <p class="verdict">
              <span class="lbl good">Crack</span>
              18.4p expected per crack at Radiant, 3 of 6 rewards moving. Selling this one intact
              clears 22p each — under the crack, so run it.
            </p>
          {/if}
        </div>
      {/if}
    {/each}
  </div>

  <div class="line">
    <span class="exp">All of it runs on your machine against the inventory the scan read. Nothing is uploaded; no warframe.market login until you list.</span>
    <span class="grow"></span>
    <a href="#desktop">↑ get the app</a>
  </div>
</section>

<style>
  /* Tab list left, panel right; the list collapses to a wrapping strip above
     the panel under 48rem. Both sit inside the shared .wrap.tw panel, so the
     border, radius, rail and line chrome all come from app.css. */
  /* 16rem = the eight 32px tabs. Pinning the body to it stops the section
     from jumping height as the reader moves between a 3-row and a 5-row
     miniature. */
  .rbody { display: grid; grid-template-columns: 11rem minmax(0, 1fr); align-items: stretch; min-height: 16rem; }

  .tabs {
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--border);
    background: var(--panel-2);
    min-width: 0;
  }
  .tabs > button {
    font: inherit;
    font-family: var(--font-ui);
    font-size: 13px;
    text-align: left;
    height: var(--row);
    padding: 0 var(--inset);
    background: transparent;
    border: none;
    border-bottom: 1px var(--rule) var(--hairline);
    border-radius: 0; /* UA reset, as .seg > button does — not a design radius */
    color: var(--muted);
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    transition: color 120ms ease;
  }
  .tabs > button:last-child { border-bottom: none; }
  .tabs > button:hover { color: var(--fg); background: var(--hover); }
  /* Active = inversion, not a hue — the theme's one filled surface. */
  .tabs > button.on,
  .tabs > button.on:hover { background: var(--ink-bar); color: var(--on-ink); font-weight: 600; }

  .panel { min-width: 0; display: flex; flex-direction: column; }
  .panel:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }
  .desc {
    margin: 0;
    padding: var(--s3) var(--inset);
    font-size: 13px;
    line-height: 1.25rem;
    color: var(--muted);
    max-width: 72ch;
  }
  /* The rule sits on the table, not on .desc — .desc is capped at 72ch, and a
     divider that stopped where the sentence stops reads as a broken border. */
  .mini { min-width: 0; border-top: 1px var(--rule) var(--hairline); }
  /* Deliberately the same anatomy as the search and hand-off tables above:
     name left, numbers pinned right. A miniature that laid its columns out
     differently would stop reading as the same app. */
  .mini table { min-width: 30rem; }
  .mini .stats, .mini .note { font-size: 11px; color: var(--muted); }
  .mini .note { font-family: var(--font-body); }
  .mini .fix { font-weight: 600; color: var(--fg); margin-right: var(--s2); }
  .mini tr.sum td { border-top: 1px solid var(--border); color: var(--muted); }
  .mini tr.sum td.score { color: var(--good); }
  /* Relic drop rarity, as the planner shows it: a single C / U / R initial in
     front of the name (its own column would get stretched by auto layout). */
  .mini .rar { display: inline-block; width: 1.25rem; font-family: var(--font-mono); font-weight: 600; color: var(--faint); }
  .mini .rar.U { color: var(--muted); }
  .mini .rar.R { color: var(--warn); }
  .mini .good { color: var(--good); }
  .mini .bad { color: var(--bad); }
  .mini .fg { color: var(--fg); }
  /* The shared tag rule carries a left margin for the "Item …tag" case; a tag
     that IS the cell shouldn't be indented by it. */
  .mini td.l > .tag:first-child { margin-left: 0; }

  .verdict {
    margin: 0;
    padding: var(--s2) var(--inset);
    border-top: 1px var(--rule) var(--hairline);
    font-size: 12px;
    line-height: 1rem;
    color: var(--muted);
  }
  .verdict .lbl { display: inline; margin-right: var(--s2); }

  .line a { color: var(--accent); }

  @media (max-width: 48rem) {
    .rbody { grid-template-columns: 1fr; min-height: 0; }
    .tabs {
      flex-direction: row;
      flex-wrap: wrap;
      border-right: none;
      border-bottom: 1px solid var(--border);
    }
    .tabs > button {
      flex: 1 1 auto;
      text-align: center;
      padding: 0 var(--s3);
      border-bottom: none;
      border-right: 1px var(--rule) var(--hairline);
    }
    .tabs > button:last-child { border-right: none; }
  }

  @media (prefers-reduced-motion: reduce) {
    .tabs > button { transition: none; }
  }
</style>
