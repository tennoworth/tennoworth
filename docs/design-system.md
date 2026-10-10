# TennoWorth design system

Version 1: the agreed visual and interaction contract for the desktop app and
hosted informational site. Preserve this identity when extending or fixing UI.
This document defines the standard; it does not certify that every existing
screen already meets every accessibility target. Shared foundations, migrated
screen patterns, and a development-only living styleguide are backed by source
and browser checks. Native Windows display verification remains a separate gate.

## Start here

Before changing markup, styles, layout, or interaction states:

1. Read this reference and inspect the affected surface in the running app.
2. Identify the existing tokens and patterns that apply. Reuse them before
   adding a new variant.
3. Preserve feature behavior, meaningful information, and unfinished edits.
4. Verify the changed surface in both themes and at narrow, short, and wide
   sizes. Include the affected failure and interaction states.
5. Update this reference when intentionally extending the system. Record the
   scope and reason for any exception; an existing inconsistency is not a
   precedent for another one.

The product-specific direction here takes precedence over generic aesthetic
suggestions. A feature specification owns business behavior; this reference
owns its presentation. Resolve conflicts explicitly rather than silently
changing either contract.

## Identity and hierarchy

TennoWorth is a precise, understated companion: glance, decide, return to
Warframe. Reduce decision time, not increase time spent managing the app.

- Preserve the warm paper-and-ink light theme and warm charcoal/brown-black
  dark theme. Both are first-class; neither is a cosmetic afterthought.
- Use compact analytical layouts, square edges, dotted secondary dividers,
  and restrained filled title rails. Avoid ornamental card grids, rounded
  pills, decorative gradients, glows, and entrance animations.
- Give each task surface a clear heading, relevant context, and an obvious
  primary action. Use filled emphasis selectively; secondary actions are
  quieter but remain legible and discoverable.
- Information outranks decoration. Names, prices, quantities, uncertainty,
  warnings, and consequential actions must not disappear to preserve a layout.
- The hosted site may use more introductory space, but shares the typography,
  palette, and component language. Do not expose desktop-only operations there.

## Tokens and typography

[app.css](../frontend/src/app.css) is the source of runtime token values and
theme overrides. [theme.ts](../frontend/src/ui/theme.ts) owns theme selection.
Do not maintain another palette or duplicate numeric token values in examples.
The active look is `yorha`; the resolved modes are `light` and `dark`.

| Role | Existing tokens | Usage |
|---|---|---|
| Surfaces | `--bg`, `--panel`, `--panel-2` | Page, panel, and secondary surface |
| Text | `--fg`, `--muted` | Primary and supporting readable content |
| Decoration | `--faint`, `--hairline`, `--grid`, `--hatch` | Nonessential ornament only; `--faint` never colours readable text, missing-data dashes included |
| Boundaries | `--border`, `--rule` | Solid outer outlines, dotted inner dividers |
| Emphasis | `--accent`, `--on-accent`, `--ink-bar`, `--on-ink`, `--on-ink-muted` | Use matching foreground/background pairs |
| Title rails | `--rail-bg`, `--rail-fg`, `--rail-muted`, `--rail-edge` | Panel, card, FAQ and dialog title rails. Light: the ink bar. Dark: a raised `--panel-2` band under a 2px rag rule, because the ink fill is reserved for selected state |
| Meaning | `--good`, `--warn`, `--bad`, `--ducat`, `--vault` | Success, caution, error, ducat data, and vaulted status; not decoration |
| Type | `--font-ui`, `--font-body`, `--font-mono` | Headings/labels, reading, numeric/technical data |
| Type scale | `--text-caption`, `--text-control`, `--text-body`, `--text-section`, `--text-heading`, `--text-dialog-title`, `--text-metric`, `--text-metric-lg`, `--text-statement`, `--leading-body`, `--leading-control` | Shared text roles and line heights; extend centrally rather than per panel |
| Rhythm | `--s1` through `--s6`, `--inset`, `--gutter`, `--stack`, `--cell` | Shared spacing and contextual insets |
| Density | `--ctl-xs`, `--ctl`, `--ctl-lg`, `--row`, `--row-cf`, `--head`, `--rail`, `--bar`, `--strip` | Baseline sizes, not clipping constraints |
| Shape and layering | `--radius-*`, `--shadow-pop`, `--scrim` | Square active-theme shapes and restrained overlay separation |
| Stacking | `--layer-chrome`, `--layer-menu`, `--layer-popover`, `--layer-modal`, `--layer-toast` | Header, menus, floating panels, review overlays, and notifications |

- Archivo Narrow (`--font-ui`) is for headings and short labels. Uppercase and
  tracking stay limited to short labels, not paragraphs or long item names.
- IBM Plex Sans (`--font-body`) is for names, explanations, instructions, and
  other reading. IBM Plex Mono (`--font-mono`) is for numerals, units attached
  to numerals, IDs and hashes, and keyboard shortcuts only; status words use
  `--font-ui` and sentences use `--font-body`, even inside a mono table cell.
  Align comparable numeric columns consistently and keep units clear.
- Use relative text sizes and readable line heights. Never shrink text as a
  responsive escape hatch. `--text-caption` is the floor for anything read,
  units after a number included. Plex Mono ships 400–600 only and synthetic
  bold is off, so request 600 rather than 700; code, kbd and samp use the mono
  face at the size of the surrounding words. Exact semantic type-size consolidation belongs in
  the shared stylesheet, not a new per-panel scale.
- Controls inherit the intended typography. Reserve sufficient height and
  padding for their labels; content may grow beyond the baseline density.
- The light-mode supporting-text token is dark enough for all three standard
  surfaces. Keep that coverage when adjusting its value, not just contrast on
  the lightest panel. The same applies to the status hues: tags and chips sit
  on `--panel-2` header, hover, and filter rows, so they must clear 4.5:1
  there too. Record measured ratios beside the token values in `app.css`.
- `--vault` is a cool slate, deliberately outside the warm status hues:
  vaulted is market fact, not success, caution, or error.
- Reuse spacing roles rather than selecting arbitrary gaps. Data cells may be
  denser than panel content, but dense rows must still accommodate wrapped names.
- Component styles must not introduce independent colors, fonts, or radii.
  Add a semantic token centrally when an existing one cannot express the role.
  Layout-specific dimensions are allowed when justified by the content; they
  must not become a second spacing or typography system.

## Shared patterns

Prefer shared CSS for repeated appearance and Svelte components for repeated
behavior. Do not create wrappers for every styled element or introduce a new
component library just to consolidate the existing UI. Account for Svelte style
scoping: a class defined inside one component is not a shared primitive.

Current shared selectors in `app.css`: `.btn` with `primary`, `ghost`, `bad`,
`xs`, and `lg` variants; `.wrap.tw` and `table.tw`; `dialog.cryptobox`;
`.ui-panel`, `.view-header` with `.vh-end`, `.ui-totals`, `.general-banner`, `.ui-stack`, `.ui-toolbar`,
`.ui-field`, `.ui-input`, `.ui-notice`, `.ui-segmented`, `.shimmer-field`, and `.tag`. Notices use
`data-tone="good|warn|bad"` for their semantic edge, with explicit readable text
for meaning. Keep feature-specific sizing local. These selectors are opt-in;
do not globally restyle similarly named legacy component classes by accident.
Inline status tags use the one `.tag` primitive in `app.css`; components may
position a tag but never recolour it. The border style carries the kind as well
as the hue: solid for advice (`peak`, `flip`, `hold`, `deal`, `stale`, advisor
verdicts, and neutral `patience`/`skip`), dotted for facts (`vaulted`, `soon`,
`aug`, `thin`, `unpriced`, `relic-tag`). Vaulted uses `--vault`; no tag uses
`--bad`. Filter chips that select by a tag reuse its class.
Use `table.tw.fixed` when declaring fixed column proportions; setting only
`table-layout: fixed` leaves the automatic item-column sizing rule active and
can collapse numeric columns. Verify every column, not just document overflow.

The hosted and desktop shells share `src/shells/shell.css`. Its `data-shell`
attribute limits those rules to shell-owned markup, preserving component style
isolation after extracting the shells and shared FAQ. Feature components do not
add this attribute. The reward surface does not consume those selectors.

Shared panel and banner rules replace formerly component-scoped copies in the
shell, Watches, Ledger, and update notices. Every task view shares one header
grammar: the title, a one-line lede beside it, and at the far edge (`.vh-end`)
either its comparable totals in a `.ui-totals` strip or its primary action
(Check live on My orders). The lede wraps before the end slot does. A view
does not hide its description in a hover-only info mark.

In the workspace the status strip holds to one line on wide windows: the scan
cell takes the slack and a long scan name wraps inside its own box. Orders to
fix and Baro's countdown are sidebar badges beside the views they open; the
strip keeps them only on the first-run landing, which has no sidebar. The
strip's activity cell is the one place scans, listing sends, visibility
changes and live checks report: a caution square while busy, then the result,
which stays until something replaces it. An older result never replaces work
still in progress.
Existing analytical table variants retain their feature-specific column layouts.
Listing review uses the shared button patterns, readable deselected rows, and
keyboard focus containment/restoration; native authentication dialogs retain
their own browser-provided modal behavior.

Trade Session uses the existing mode-button, field, notice, and fixed analytical
table patterns. Its expanded listing review scrolls as a whole in short windows
while keeping a usable table region; before/after order details must not collapse
the editable rows. The keyboard boundary also handles focus moving to the document
when a focused refresh button temporarily disables itself.
Below 900px the sidebar foot (project links, feedback and the build label)
follows the workspace instead of preceding it, and at phone width the brand
heading is visually hidden, so a short window reaches its first decision.
The shell measures its wrapped status header into `--sticky-header-clearance`;
document scroll padding keeps focused/scrolled content below that header, including
narrow and enlarged layouts. This is a measured layout value, not a fixed spacer.

| Pattern | Contract |
|---|---|
| Panels and title rails | Clear heading, restrained inversion, dotted internal separation. Panel and dialog titles are always rails; controls may sit in a rail, status chips may not; titles and adjacent status copy may wrap. Anything placed on a rail uses the `--rail-*` pair, whatever its heading level; rail buttons are outlined in that pair rather than filled |
| Toolbars | Group related controls; allow wrapping without changing the logical or keyboard order |
| Buttons and links | Clear action labels, consistent emphasis, visible focus; navigation uses links and actions use buttons. Links inside table cells show their dotted rule only on row hover or focus |
| Fields | Persistent accessible labels, units and constraints nearby, actionable validation; placeholders are not labels. Checkboxes and radios use the shared square box in `app.css` (checked is the ink inversion); components may size it, not recolour it. Text-entry focus draws over the field's own border so it reads as one frame. Number fields hide spinner arrows; arrow keys still step. Option labels do not repeat the field label |
| Mode selectors | Explicit selected state and accessible semantics; descriptions remain readable when the group wraps. Single-choice groups use `.ui-segmented` (joined, dotted inner rules, wraps rather than scrolls); the selected option is always the ink inversion, whether it is marked with `aria-pressed`, `aria-checked` or `aria-selected`. Mode cards that carry descriptions keep the card shape but use the same selected fill, with a title that outranks the description |
| Prompt banners | `PromptBanner.svelte` offers something optional that is safe to ignore, such as an opt-in or a support link. It uses the neutral panel with no toned edge, because an invitation is not a warning, and says that ignoring it changes nothing. Its title names the benefit, the body states what happens, and the actions are the accept action, an optional link to the full control, and Not now. A launch shows at most one prompt, never while that launch's automatic “What’s new” is pending or open, and asks are at least a week apart across prompts. Each prompt sets its own delay, launch minimum, gap and ask limit, and may wait for an update launch for its first ask (`features/prompt-policies.ts`, highest priority first); showing it counts as an ask, so ignoring it equals Not now, and accepting retires it. Prompts stay out of first run and active trading, and step aside on the view that already holds the full control. Select Prompt banner in the living styleguide's Data state control |
| Status and notices | Explain what happened, its consequence, and any next action; never rely on color or a transient toast alone for critical information. A 3px left edge is reserved for meaning (toned notices, toned toasts, action-needed notes, Baro's visit window); never nest one edged surface in another. Stale market data is a caution: an outlined `--warn` chip in the header and a warn-toned notice, not a red dot |
| Tables | Preserve meaningful columns and item identity; align comparable numbers; contain horizontal scrolling within the table region. The price a row is about (`td.price`) is ink at 600 with a quiet unit; volume and other context stay muted. A sortable header is a button inside its `th`, which carries `aria-sort`, and its column is wide enough for the label plus the sort arrow. Column explanations live in one Column guide dialog beside the Columns menu, not in per-header popovers. A table's action column sits beside the item, so Top Picks keeps List and hide in view while price and reason scroll. Select Sell table in the living styleguide's Data state control for the production pattern |
| Sparklines | `Sparkline.svelte` draws recent medians in ink, a dotted `--faint` rule at the 90-day median, and an endpoint dot coloured by the move against that median. The baseline shares the line's scale, so its distance from the line is honest |
| Disclosures | `<details>` summaries share one glyph from `app.css`: ▸ closed, ▾ open, in the label face. Menus keep a trailing ▾; `+` marks add actions only. Rail section toggles pair the glyph with Show or Hide |
| Dialogs and popovers | Stay within the viewport; provide reachable dismissal and actions, visible focus, and appropriate keyboard behavior |
| Review surfaces | Make consequential changes explicit, preserve edits, and distinguish existing state from proposed state |

Loading, empty, error, disabled, selected, hover, and keyboard-focus states are
part of each applicable pattern, not finishing touches.

## Continuity and waiting

The app never shows the wrong screen while it waits. Until startup has looked
for the saved scan, "no inventory" is unknown, not empty: the workspace shows
the Sell layout with placeholder rows and the strip says the scan is loading;
the first-run page appears only once the saved scan is known to be absent.
Placeholders are static hairline bars at the final row height (no shimmer),
and Top Picks holds its place while sale values calculate, so the table below
does not drop when they arrive. Nav entries that depend on prices stay in
place while the market snapshot loads.

A visited view stays mounted while another is on screen, so its search, page,
scroll and drafts survive the trip; global shortcuts act only on the view on
screen. A view that fetches on arrival (My orders) refreshes when it returns
with the last rows in view, and a failed refresh keeps them readable with
changes paused until a refresh succeeds.

What a click did shows where it was made. A listed pick becomes a receipt in
its own row (listed, hidden or visible, Make visible, My orders), and Own
shows what this session listed. Hiding a pick leaves an Undo receipt in its
slot until the pick set changes. A scan that lands while the table is on
screen updates values in place and keeps the row order; the count bar offers
Re-sort, and sorting or filtering applies the new order. Nothing animates in. Empty inventory is not
a failed request. Missing data is not zero. Estimates must be labeled as such,
and background refresh must not silently replace user edits.

Retained inventory displays its original absolute timestamp alongside a live
relative age. Identify saved and imported snapshots explicitly. A failed refresh
keeps a textual status beside that timestamp even after error details are
dismissed; only a successful replacement or clearing inventory resets it.
A scan with no tradeable items explains that saved inventory remains visible,
in an informational notice, separately from an acquisition or processing
failure. Its heading must not claim that the refresh failed. The living styleguide’s
Error state demonstrates these retained-data notices.

For listing review, keep before/after price and quantity changes, visibility,
and relevant uncertainty readable. A design migration must not change trading
calculations or create a second execution flow. New planning surfaces use the
same patterns without treating illustrative mock data as production behavior.

## Keeping and quantity status

Use red (`--bad`) for invalid data and blocked calculations, amber (`--warn`)
for incomplete estimates, and green (`--good`) for successfully applied keep
rules. Green does not certify listing readiness. Staying logged out, keeping an
item, or having no crafting goal is neutral. Pair color with explicit text.
Error notices use a red outline and restrained tinted background in both themes.

Show one primary quantity failure and its recovery action. Suppress derived
failure notices and ordinary “no picks” messaging while calculations are blocked.
Keep settings appear as a one-line “What I’m keeping” strip under each view’s
title and stale-data notices (Sell, Trade Session, Set picks, Baro); failures
surface as notices above it rather than by expanding it. The editor is a
keyboard-contained dialog, and the complete quantity table is opt-in. Show actual
saved minimum-copy settings, automatic leveled exclusions, and crafting/item
rules. Never advertise a last-copy default that is not enabled. Missing data stays
unavailable; scan-only quantities are estimates before existing listings.

## Preferences and page rhythm

Before the first scan, the desktop page leads with the scan action beside its
pitch, with the privacy line directly under the button; Settings, Notifications
and a More menu (updates, feedback, project links) sit in the header rather than
the page body. Sections that are one part of a longer page (Meta Drift on the
hosted and first-run pages) show their top ten with an in-place Show all; a
search lifts the limit, and the dedicated desktop view shows everything.

The hosted landing opens on one statement in the reading face
(`--text-statement`; no hero, image or display numerals), then search, the
Rising and Falling movers as twin titled panels, and Vaulted beside Baro's top
five. One desktop panel follows: the app's surfaces as tabs whose Sell tab is
the visitor's own rows completed by the app, with the comparison and downloads
beside them. Meta Drift and Riven dispositions are tabs of one Market context
panel. FAQ and Trust & safety are one Questions panel with Trust answers in
their own column; `#faq` and `#trust` both land there. Community usage is one
line while counts are unavailable. A failed market load says so with Retry
rather than substituting the install pitch.

Settings and FAQ use a bounded 64rem reading width. Every other view, Set
picks, Relics and Routines included, uses the workspace width, so the right
edge does not move between views; prose inside a panel caps its own line
length. Ranked lists are tables: Set picks (a plain Do column, net plat and
set volume aligned) and Relics (one row per relic, the refinement ladder as
four cells on one scale with the chosen rung inverted, picked only for a crack
or refine verdict). My orders is one listings table: listing health is a
column and a row state with each fix beside the item, and Live ask and bid are
always reserved so a live check fills cells rather than moving controls.
Analytical tables scroll locally when needed. Settings includes notification preferences within the same column.
Settings is ordered by how often people return to it. A status strip leads,
linking the automatic-scan, overlay and account state to their sections,
followed by section links. Then come the everyday panels (Automatic scan,
Relic reward overlay, Notifications) and the set-once panels (warframe.market
account, then This app: appearance, updates, usage sharing and price sharing). Status comes
before the controls that change it. Troubleshooting detail and full privacy
terms sit in disclosures, and diagnostics open with their warning in view when
enabled. The inbox's settings link opens the page at Notifications. Category
preferences are a table with Inbox and Popup columns, and each category's
timing sits under its name. Settings → Updates and the update banner read one
shared update state: a check from Settings offers Install update and then
Restart now in that row, with install failures beside it, so nobody has to
scroll back to the banner.
`.ui-setting-row`, `.ui-setting-copy`, `.ui-setting-control`, and
`.ui-setting-check` align labels, help text, and controls; they stack below
760px without remounting the controls. `.ui-section-group` separates related
options, and `.ui-panel-footer` groups actions and supporting details.
`.ui-summary-strip` presents comparable totals without nested cards.
These patterns are demonstrated in the living reference’s Aligned preferences
section. Watches separates creation from monitoring; Ledger separates totals,
listing automation, and trade history. Top picks labels its decision facts
independently of the inventory’s optional analytical columns.

Scan-only guidance labels its heading and value as estimates and uses one
contextual notice explaining that current WFM listings are not accounted for.
Keep posting actions disabled until listing quantities are checked, with a
separate “Check WFM listings” action. Missing protection data is unavailable,
never a zero-valued estimate. With partial coverage, label totals as known
estimates and state how many items are excluded. Set picks and Baro must also
show unavailable quantity states. Imported backups can show protected estimates,
with a Scan game action before posting. A blocked review explains why and keeps
price/quantity edits while the user rechecks requirements. Failed allocation
offers Recheck protection; a review tied to an older scan offers Close review
so a new batch can be prepared. Hide prior listing
totals when current listings are unknown. The living styleguide includes an
Estimates state.

The schematic page background uses bounded repeating tiles. WebKit can stretch
a page-sized gradient raster on long surfaces even when computed CSS matches
Chromium. The grid repeats every 28px; the diagonal’s 9px period projects to a
12.7279220614px square tile. Keep the bounded sizes and verify the actual
rendering in both engines when changing the pattern.

## Notification history

Desktop history uses `.notification-entry` rows with a raised unread surface,
explicit Read/Unread text, evidence, and navigation to the next step. Rows lead
with the title and category/read state; the exact local time has its own aligned
column beside the title, with the day and short date below. At narrow widths the
time moves below the title rather than into the message. History groups by local
calendar day, including Today and Yesterday, with the absolute date on each
heading. Day labels update while the view remains open.

Structured Baro notices separate visit context, value picks, and a three-part
ducat yield strip. Held stock names and the published schedule use disclosures;
keep-rule guidance and resale uncertainty remain visible. Digest rows align
item names/quantities with estimated unit prices, retaining inventory and price
evidence and exact timestamps. Stored prose is shown whole when structured
details are absent or unreadable. The shared `NotificationRow.svelte` owns this
presentation for the inbox and living reference; do not parse popup text into
facts. Toolbars wrap, with the unread filter and history count at the far edge.
Read entries retain full text contrast. Native delivery failures remain visible
in the inbox; a transient popup is never the only record. Category controls and
popup preferences use labeled checkboxes, with disabled states kept readable.
Long trade descriptions wrap, and row actions wrap without hiding their labels.
Select Alerts in the living styleguide’s Data state control for the production
row pattern and read-state toggle.

## Resizing contract

- The page must not require horizontal scrolling to reach ordinary navigation,
  notices, or actions. Wide data tables may scroll locally; their last column
  must remain reachable by keyboard and pointer.
- Essential item identity wraps. Truncation is only acceptable for secondary
  content when the full value is available through an accessible interaction;
  a hover-only tooltip is not sufficient.
- Let toolbars, status strips, headings, and mode groups grow vertically.
  Fixed single-line heights must not clip wrapped content.
- Base layout changes on available space and actual content. Test immediately
  around breakpoints, not only a few named device sizes. Do not automatically
  turn every table into cards or hide columns on small screens. Choose columns
  by task instead: each Sell preset names its own column set, and the Columns
  menu lets people add or remove columns per preset, with a reset. When a
  column is the point of a table, place it next to the item name so narrow
  windows reach it without scrolling (Meta Drift's Δ share).
- Account for short windows as well as narrow ones. Flex/grid children must
  not collapse whole panels, and sticky controls must not obscure rows or focus.
- Bound floating surfaces to the viewport and give their bodies usable scroll
  space. Critical validation, dismissal, and confirmation stay reachable.
  Floating panels must also sit above the shell chrome: test clicking their
  actions, not just their bounding boxes. A viewport-fitting Filters panel once
  had its close button intercepted by the header.
- Resizing and theme changes preserve selections, entered values, open review
  drafts, and logical focus. Do not remount an editing surface to change layout.
- Verify zoom separately from viewport resizing. CSS zoom is a useful stress
  test, not evidence of native browser zoom or operating-system scaling support.

## Readability and interaction targets

These are implementation targets, not a claim of completed accessibility
certification:

- Aim for at least 4.5:1 contrast for readable text, including supporting text,
  on its actual rendered background. Aim for at least 3:1 for essential control
  boundaries and focus indicators. Measure both themes and interactive states;
  token names and old stylesheet comments do not prove contrast.
- Never dim an entire informative row with opacity. Preserve readable text and
  use explicit state labels. Color communicates meaning alongside text or icons.
- Keep pointer targets at least 24 by 24 CSS pixels; prefer larger targets for
  primary actions and touch use. Compact visuals do not require tiny hit areas.
- All interactive controls have accessible names, visible keyboard focus, and
  sensible tab order. Focus must not be clipped or covered by sticky content.
- Modal surfaces contain keyboard focus and restore it on closing. Nonmodal
  popovers do not trap focus unnecessarily. Use established accessible behavior
  rather than duplicating incomplete dialog handlers.
- Motion only clarifies state, never delays work or hides content. Respect
  reduced-motion preferences and avoid unnecessary movement by default.

## Surface variants and exceptions

The in-game reward overlay is a separate surface variant. It shares the visual
language, but has game-relative placement, transparency, and timing constraints.
Do not apply opaque app-shell backgrounds or normal page scrolling to it.
Verify readability against representative game imagery and ensure its global
CSS cannot leak into the hosted site or main desktop window.

The browser-rendered reward overlay uses the `--reward-*` palette, scoped to
`html.relic-overlay-surface` in `app.css`. Its dark translucent surface stays
stable in either desktop theme because game imagery determines the background.
Names and facts wrap; card width accounts for the user-selected scale before
transforming, so increasing scale does not widen a card beyond its reward slot.
The separately native-rendered overlay backend is not styled by CSS: changing
these tokens does not certify that renderer or live capture/game placement.

Keep native window behavior and Windows/Linux constraints explicit. If native
and frontend implementations must share a value or calculation, use the
repository's shared parity-fixture approach rather than copying constants.

### Field shimmer

`use:shimmer` in [shimmer.ts](../frontend/src/ui/shimmer.ts) draws a moving
light, 4px scanlines and lit edge rules in `--ink-bar` over a `.shimmer-field`
wrapper, adapted from the YoRHa terminal input shader. It is an exception to
the rule against glows and decorative gradients, scoped to the two fields that
start a task, and it shows their state:

- **Market lookup** (hosted site and desktop): a quiet idle sweep at rest marks
  where to start. While focused and empty it runs in full, with sparks in `--fg`
  in the dark theme only; once a query shows results it returns to the quiet
  sweep so the eye moves to the table.
- **Sell name filter**: the quiet sweep at rest, one light pass when
  <kbd>/</kbd> moves focus there so the jump is visible on a long table, and the
  full sweep while focused. A click gives no pass, and the filter never shows
  sparks.

The idle sweep runs at about half strength with dimmer edges, so the field
reads as calm beside data. Never add it to a second field on the same screen,
or to secrets, prices, quantities or writing fields. Reduced motion shows only
the lit edges. The canvas ignores the pointer, is hidden from assistive
technology and never replaces the focus outline; the animation stops when the
window is hidden or the field is off screen. Select Field shimmer in the living
styleguide's Data state control.

For any additional exception, document the surface, the rule being varied, why
the shared pattern is unsuitable, and the verification performed. Broad visual
direction changes require explicit approval, not an incidental component edit.

## Living reference and verification

Open `/?styleguide` on the development server for the living reference in
[Styleguide.svelte](../frontend/src/dev/Styleguide.svelte). It consumes
production tokens and shared patterns, not copied mock CSS, and is excluded
from production builds. It shows both themes, interaction and failure states,
long content, a locally scrolling table, and an editable native dialog. Theme
choices and sample edits do not persist or invoke account operations. It is an
reference for the approved visual direction, not a separate design system.

For UI changes, use the existing responsive suite in
[frontend/tests](../frontend/tests) and the checks described in
[contributor checks](../CONTRIBUTING.md#frontend-checks). Extend coverage for new behavior rather
than considering existing green tests sufficient.

Review affected screens at narrow, intermediate, and wide widths, including
320 CSS pixels, and short heights such as 480 CSS pixels. Exercise actual styled
routes with realistic data, both themes, keyboard navigation, local table
scrolling, and open dialogs. Check long names and loading/empty/error states.
Use 200% zoom as an additional stress case. Record which runtime and mechanism
were tested; do not generalize browser results to native Windows or Linux.

The styleguide browser tests check overflow, core text-token contrast pairs,
data states, modal editing and focus restoration. Linux Chromium/WebKit visual
baselines cover both themes at narrow and wide widths. Generate candidates with
`bunx playwright test --grep 'reference stays readable' --update-snapshots`,
inspect the images, then rerun without that flag. Use the browser version pinned
in the lockfile and a consistent Linux runtime. Never bulk-accept unexplained
differences to make a gate green.

`design-system.spec.ts` exercises the desktop views in both themes, shared
control targets, column widths, keyboard theme selection and listing review, and
the isolated browser reward surface. `design-system.test.ts` scans all
shell, feature, shared UI, and development component style blocks for literal
colors, pixel-based text sizes, and nonzero pixel radii. Inline styles and
arbitrary semantic misuse are not covered by that source check. These checks
run through the existing test/browser CI jobs; no optional manual command is
needed to include them. They are not a complete accessibility audit. Review
visual differences and actual behavior; automation does not replace design review.

`trade-session.spec.ts` adds mode selection, lot-aware review, preserved edits,
changed-order reconfirmation, decreased safe quantities, and unavailable/zero
allowance states. Open its fictional dataset through the living reference's
Open sample app link, which starts on Trade Session.

Playwright's WebKit is a newer build than the system WebKitGTK the Linux app
ships on, so a layout that passes there can still collapse in the app: the
Column guide once rendered as its bare title rail in WebKitGTK 2.52 only. The
desktop probe (`ui-smoke`) therefore records layout evidence for dialogs from
the real webview, and `check-probe-report.ts` gates on it. Add a dialog to that
evidence when it depends on flex or grid sizing that engines resolve differently.

Run the applicable repository gates for each implementation batch. Complete
cross-platform verification in actual desktop windows before claiming Windows
and Linux resize/scaling support. Documentation-only changes require link,
consistency, and whitespace checks, not a new visual certification.

## App icon

The mark is the hex lattice on a transparent ground. Classic blue is its own
colour and the default everywhere: the favicon, the installed icons, the
installer, shortcuts and the Linux launcher. Below 48px a compact drawing
replaces the detailed one, with heavier strokes and a solid centre.

The desktop App icon setting (This app → Appearance, under Colour mode) can
draw the window and tray icons in ink or rag, the light and dark `--fg`
tokens, or match the resolved colour mode. Its options are radio cards in a
two-column grid that stacks below 760px. Each card holds the shared square
radio, a preview of its mark on the light and the dark `--bg`, and a name with
a hint. The selected card is the ink inversion, like other mode cards; its
radio, hint and focus ring take the `--on-ink` pair so the check stays visible,
and the previews keep their own grounds.
On a GTK Wayland session the window and taskbar keep the launcher icon, and the
setting says so; the tray still follows it. `scripts/build-icons.ts` generates
every icon and preview from one drawing and the tokens; change the mark there,
never in an output file. Select App icon in the living styleguide's Data state
control for the production row.

## Community usage chart

The hosted community section uses a daily bar chart with an expandable exact-value
table. Bars share the accent token; incomplete days use muted bars and explicit
coverage text in the table. One-pixel bar gaps keep all 90 days within narrow
panels; endpoint dates orient the chart without crowding it. Zero has no filled bar and remains an explicit table
value. Loading, unavailable, empty, and stale states never invent observations.
Select Community usage in the living reference’s Data state control for the
production chart with fictional daily counts.

## Trade presence

The user's warframe.market status uses the website's own names and choices:
Online, Online in game and Invisible, and "keep status for" While running, 30m,
1h, 2h or 4h. Nothing is offered that the website cannot show, so there is no
activity or mission text. Following the game is the app's addition and is off
until the user turns it on.

Once the session is unlocked, the status strip's WFM cell shows the status word
and a 7px mark, opening a menu with the same shape as More ▾. The mark's shape
carries the state as well as its hue: filled `--good` for Online in game, a
`--good` outline for Online, a faint `--muted` outline for Invisible, and filled
`--warn` when the user has something to fix. `PresenceMark.svelte` is its only
definition, and the word always sits beside it. A locked or signed-out session
keeps the strip's existing link to the sign-in dialogs.

Settings shows the status first on the warframe.market summary card, with the
session on its second line. The account panel's Trade presence group holds the
status as a `.ui-segmented` choice, Follow the game with the status chosen for
when Warframe closes, and keep status for. Keep status for is disabled, with its
reason, while following manages the status. A status picked by hand pauses
following until the next game session: a warn-toned notice says so and offers
Follow the game now. A refusal from warframe.market, such as an unverified
account, is a bad-toned notice. Connection detail sits in a closed
troubleshooting disclosure. The tray mirrors the strip in a Status submenu.

Select Trade presence in the living styleguide's Data state control for the
production Settings group. The desktop preview takes
`&presence=website|closed|unverified|unreachable` for the other strip and
Settings states.

## Fallback WFM sign-in

When the in-app sign-in window cannot load warframe.market, the login dialog
asks for the browser's `JWT` cookie and shows `WfmTokenGuide.svelte`: a
`.ui-segmented` browser choice (Firefox, Chrome / Edge) above numbered text
steps, with keys in mono `kbd` caps. Steps name the devtools tab rather than
picture it, because browser devtools are redesigned often and an outdated
drawing misleads. Explanatory sentences stay outside the uppercase field label.
Select WFM token sign-in in the living styleguide's Data state control.

## Installed update notes

“What’s new” uses the native dialog pattern with a 47rem maximum width and a
`--text-dialog-title` heading. Its explanatory header deliberately uses the
normal panel surface instead of the inverted authentication-dialog rail, as in
the approved mock; scoped overrides prevent the authentication form’s negative
header margins from clipping the heading. A single summary combines skipped releases into
New & improved and Fixes; Action needed entries stay fully visible before
ordinary highlights. Long gaps offer Show all changes and a release-by-release
disclosure. Version range, partial history, and an unknown starting version are
explicit. Use green only for a confirmed installed upgrade; reopening notes or
an unknown earlier version uses neutral status text.

Got it, Close and Escape dismiss the same summary. Failed acknowledgement
closes the dialog and leaves a persistent corner notice with Retry saving and
Dismiss notice, rather than hiding the error below a long dashboard. The footer points to Settings
→ Updates → What’s new; short windows scroll the whole dialog so actions remain
reachable. Theme/resizing preserve expanded content. Automatic presentation waits
for the hydrated, visible, focused main window and never stacks over another
modal or pending recovery. Select Update notes in the living styleguide to open
the production dialog with a fictional upgrade state.
