# Desktop app showcase page — plan

The site is now informational-only and the desktop app is the product, but the
landing page has **no real mention of it** — the upsell pitch links to GitHub
releases and the FAQ answers listing questions, but there's no dedicated page
that says "this is the app, here's what it does, here's how to get started."
This plan adds one.

## Goal

A **Desktop app** showcase section on the landing page that a first-time
visitor can't miss: what the app is, what it does, how to install it, and a
rough first-run setup. Browser visitors learn that the market data they're
looking at is the free tier, and that the full "what should I sell" experience
is one install away.

## Why a section, not a route

The SPA is a single static page with **no router** — it's a landing
(`phase !== 'done'`) and a shell (`phase === 'done`), switched by an in-app
`view` state that only matters once an inventory is loaded. A standalone route
would mean inventing routing for one page. Instead, the showcase lives as a
dedicated `<section id="desktop">` on the landing, between the market browser
and the drop-zone, with the upsell pitch linking straight to it
(`<a href="#desktop">`). Same pattern as the existing `#trust` anchor.

## Content

### 1. Hero ("TennoWorth Desktop")
- Headline: **Your sell list, without a terminal.**
- Sub: one sentence — scan the running game, see what to sell ranked by plat,
  list on warframe.market in one place. Windows + Linux.
- Two buttons: **Download for Windows** (primary) and **Linux — apt / dnf /
  AUR** (secondary, opens the distro section below).
- The existing `docs/img/market-browser.png` screenshot as a framed preview.

### 2. Why the app (feature strip, 3–4 cards)
- **Scan the game directly** — no file, no terminal, one click.
- **Ranked sell list** — the same algorithm as the site, but for *your* items.
- **List on WFM in one place** — review, price, post; orders managed in-app.
- **No Overwolf, no accounts** — reads game memory locally, nothing uploaded.

### 3. Install (per platform, tabbed or stacked)
- **Windows** — `.exe` / `.msi` from the latest release; SmartScreen warns
  (unsigned) — click *More info → Run anyway*.
- **Debian/Ubuntu** — the signed apt repo one-liner (copy-paste block).
- **Fedora** — the signed dnf repo one-liner.
- **Arch** — `paru -S tennoworth` (or `tennoworth-bin`).
- Each with the SHA-256 verification note and a link to SECURITY.md.

### 4. First run (numbered, ~4 steps)
1. Install, launch, open Warframe past the login screen.
2. Click **Scan inventory** — reads the running game's memory.
3. Browse your ranked sell list; adjust filters/presets.
4. Optional: **Log in to warframe.market** (in-app dialog) to list and manage
   orders. Linux: grant ptrace once so the scan needs no sudo:
   `sudo setcap cap_sys_ptrace=eip /usr/bin/tennoworth-desktop`.

### 5. Site vs app (small comparison strip)
| | Site | Desktop app |
|---|---|---|
| Market data, trends, vault | ✅ | ✅ |
| Your inventory ranked | via file drop | ✅ scan |
| List on WFM / orders | — | ✅ |
| Login | no accounts | in-app |

### 6. Link the existing copy
- The upsell pitch ("use the desktop app") → `#desktop` instead of the raw
  GitHub releases URL.
- The FAQ "How do I list items on WFM?" → mention the showcase section.
- The landing footer keeps the GitHub release link.

## Implementation notes

- New component `prototype/src/components/DesktopShowcase.svelte`, rendered in
  the landing's `{#if phase === 'idle'}` block before `<DropZone>`, guarded
  `{#if !isDesktop}` (a desktop user doesn't need a pitch for the app they're
  already in).
- Reuses existing CSS variables + `.snippet-row` / `.card` / `.faq` styles
  already in `app.css` / `App.svelte`; the mockup below is the visual
  reference for the new classes.
- Copy blocks: reuse the exact repo one-liners from README.md so install docs
  don't drift.
- No new deps, no routing change, no network calls.

## Mockup

`docs/mockups/desktop-showcase.html` — a standalone, self-contained HTML file
using the site's real design tokens (`--bg`, `--panel`, `--accent`, etc.),
openable in any browser to visualize the section before building it in Svelte.
