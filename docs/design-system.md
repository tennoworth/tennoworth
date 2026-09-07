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

[app.css](../prototype/src/app.css) is the source of runtime token values and
theme overrides. [theme.ts](../prototype/src/lib/theme.ts) owns theme selection.
Do not maintain another palette or duplicate numeric token values in examples.
The active look is `yorha`; the resolved modes are `light` and `dark`.

| Role | Existing tokens | Usage |
|---|---|---|
| Surfaces | `--bg`, `--panel`, `--panel-2` | Page, panel, and secondary surface |
| Text | `--fg`, `--muted` | Primary and supporting readable content |
| Decoration | `--faint`, `--hairline`, `--grid`, `--hatch` | Nonessential ornament; not essential text |
| Boundaries | `--border`, `--rule` | Solid outer outlines, dotted inner dividers |
| Emphasis | `--accent`, `--on-accent`, `--ink-bar`, `--on-ink`, `--on-ink-muted` | Use matching foreground/background pairs |
| Meaning | `--good`, `--warn`, `--bad`, `--ducat` | Success, caution, error, and ducat data; not decoration |
| Type | `--font-ui`, `--font-body`, `--font-mono` | Headings/labels, reading, numeric/technical data |
| Type scale | `--text-caption`, `--text-control`, `--text-body`, `--text-section`, `--text-heading`, `--text-metric`, `--text-metric-lg`, `--leading-body`, `--leading-control` | Shared text roles and line heights; extend centrally rather than per panel |
| Rhythm | `--s1` through `--s6`, `--inset`, `--gutter`, `--stack`, `--cell` | Shared spacing and contextual insets |
| Density | `--ctl-xs`, `--ctl`, `--ctl-lg`, `--row`, `--row-cf`, `--head`, `--rail`, `--bar`, `--strip` | Baseline sizes, not clipping constraints |
| Shape and layering | `--radius-*`, `--shadow-pop`, `--scrim` | Square active-theme shapes and restrained overlay separation |
| Stacking | `--layer-chrome`, `--layer-menu`, `--layer-popover`, `--layer-modal`, `--layer-toast` | Header, menus, floating panels, review overlays, and notifications |

- Archivo Narrow (`--font-ui`) is for headings and short labels. Uppercase and
  tracking stay limited to short labels, not paragraphs or long item names.
- IBM Plex Sans (`--font-body`) is for names, explanations, instructions, and
  other reading. IBM Plex Mono (`--font-mono`) is for numeric and technical
  data; align comparable numeric columns consistently and keep units clear.
- Use relative text sizes and readable line heights. Never shrink text as a
  responsive escape hatch. Exact semantic type-size consolidation belongs in
  the shared stylesheet, not a new per-panel scale.
- Controls inherit the intended typography. Reserve sufficient height and
  padding for their labels; content may grow beyond the baseline density.
- The light-mode supporting-text token is dark enough for all three standard
  surfaces. Keep that coverage when adjusting its value, not just contrast on
  the lightest panel.
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
`.ui-panel`, `.view-header`, `.general-banner`, `.ui-stack`, `.ui-toolbar`,
`.ui-field`, `.ui-input`, and `.ui-notice`. Notices use
`data-tone="good|warn|bad"` for their semantic edge, with explicit readable text
for meaning. Keep feature-specific sizing local. These selectors are opt-in;
do not globally restyle similarly named legacy component classes by accident.
Use `table.tw.fixed` when declaring fixed column proportions; setting only
`table-layout: fixed` leaves the automatic item-column sizing rule active and
can collapse numeric columns. Verify every column, not just document overflow.

Shared panel and banner rules replace formerly component-scoped copies in the
shell, Watches, Ledger, and update notices. Sell and Settings share view headers.
Existing analytical table variants retain their feature-specific column layouts.
Listing review uses the shared button patterns, readable deselected rows, and
keyboard focus containment/restoration; native authentication dialogs retain
their own browser-provided modal behavior.

Trade Session uses the existing mode-button, field, notice, and fixed analytical
table patterns. Its expanded listing review scrolls as a whole in short windows
while keeping a usable table region; before/after order details must not collapse
the editable rows. The keyboard boundary also handles focus moving to the document
when a focused refresh button temporarily disables itself.
The shell measures its wrapped status header into `--sticky-header-clearance`;
document scroll padding keeps focused/scrolled content below that header, including
narrow and enlarged layouts. This is a measured layout value, not a fixed spacer.

| Pattern | Contract |
|---|---|
| Panels and title rails | Clear heading, restrained inversion, dotted internal separation; titles and adjacent status copy may wrap |
| Toolbars | Group related controls; allow wrapping without changing the logical or keyboard order |
| Buttons and links | Clear action labels, consistent emphasis, visible focus; navigation uses links and actions use buttons |
| Fields | Persistent accessible labels, units and constraints nearby, actionable validation; placeholders are not labels |
| Mode selectors | Explicit selected state and accessible semantics; descriptions remain readable when the group wraps |
| Status and notices | Explain what happened, its consequence, and any next action; never rely on color or a transient toast alone for critical information |
| Tables | Preserve meaningful columns and item identity; align comparable numbers; contain horizontal scrolling within the table region |
| Dialogs and popovers | Stay within the viewport; provide reachable dismissal and actions, visible focus, and appropriate keyboard behavior |
| Review surfaces | Make consequential changes explicit, preserve edits, and distinguish existing state from proposed state |

Loading, empty, error, disabled, selected, hover, and keyboard-focus states are
part of each applicable pattern, not finishing touches. Empty inventory is not
a failed request. Missing data is not zero. Estimates must be labeled as such,
and background refresh must not silently replace user edits.

For listing review, keep before/after price and quantity changes, visibility,
and relevant uncertainty readable. A design migration must not change trading
calculations or create a second execution flow. New planning surfaces use the
same patterns without treating illustrative mock data as production behavior.

## Notification history

Desktop history uses `.notification-entry` rows with a restrained unread edge,
explicit Read/Unread text, timestamp, evidence, and navigation to the next step.
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
  turn every table into cards or hide columns on small screens.
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

For any additional exception, document the surface, the rule being varied, why
the shared pattern is unsuitable, and the verification performed. Broad visual
direction changes require explicit approval, not an incidental component edit.

## Living reference and verification

Open `/?styleguide` on the development server for the living reference in
[Styleguide.svelte](../prototype/src/components/Styleguide.svelte). It consumes
production tokens and shared patterns, not copied mock CSS, and is excluded
from production builds. It shows both themes, interaction and failure states,
long content, a locally scrolling table, and an editable native dialog. Theme
choices and sample edits do not persist or invoke account operations. It is an
reference for the approved visual direction, not a separate design system.

For UI changes, use the existing responsive suite in
[prototype/tests](../prototype/tests) and the checks described in
[Development](../README.md#development). Extend coverage for new behavior rather
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
the isolated browser reward surface. `design-system.test.ts` scans App and all
component style blocks (including nested component directories) for literal
colors, pixel-based text sizes, and nonzero pixel radii. Inline styles and
arbitrary semantic misuse are not covered by that source check. These checks
run through the existing test/browser CI jobs; no optional manual command is
needed to include them. They are not a complete accessibility audit. Review
visual differences and actual behavior; automation does not replace design review.

`trade-session.spec.ts` adds mode selection, lot-aware review, preserved edits,
changed-order reconfirmation, decreased safe quantities, and unavailable/zero
allowance states. Open its fictional dataset through the living reference's
Open sample app link, which starts on Trade Session.

Run the applicable repository gates for each implementation batch. Complete
cross-platform verification in actual desktop windows before claiming Windows
and Linux resize/scaling support. Documentation-only changes require link,
consistency, and whitespace checks, not a new visual certification.
