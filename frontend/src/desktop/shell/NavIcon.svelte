<script lang="ts">
  // One line-drawn mark per sidebar entry, so the collapsed rail still says
  // where each button goes. Square caps and joins, like the rest of the ink.
  const PATHS = {
    sell: 'M3 12V4h8l10 10-8 8L3 12Z M7.5 7.5h.01',
    session: 'M4 8h15l-4-4 M20 16H5l4 4',
    sets: 'M12 3 3 8l9 5 9-5-9-5Z M3 13l9 5 9-5 M3 17.5l9 5 9-5',
    relics: 'M12 2.5 20 7v10l-8 4.5L4 17V7l8-4.5Z M12 7.5v9 M8 9.5l8 5 M16 9.5l-8 5',
    rivens: 'M12 3l6 9-6 9-6-9 6-9Z M12 3v18',
    baro: 'M6 3h12 M6 21h12 M7 3v3l5 6-5 6v3 M17 3v3l-5 6 5 6v3',
    routines: 'M4 6h10 M4 12h7 M4 18h5 M14 16l2.5 2.5L21 14',
    calendar: 'M4 5h16v15H4Z M4 10h16 M8 3v4 M16 3v4',
    meta: 'M3 17l6-6 4 4 8-8 M15 7h6v6',
    orders: 'M9 6h11 M9 12h11 M9 18h11 M4 6h1 M4 12h1 M4 18h1',
    watches: 'M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12Z M12 9.5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5Z',
    notifications: 'M6 16v-5a6 6 0 0 1 12 0v5l2 2H4l2-2Z M10 21h4',
    ledger: 'M5 4h11a3 3 0 0 1 3 3v14H8a3 3 0 0 1-3-3V4Z M5 18a3 3 0 0 1 3-3h11 M9 8h6',
    faq: 'M4 4h16v16H4Z M9.5 9.5a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .9-1 1.5v.7 M12 17h.01',
    settings: 'M4 6h9 M17 6h3 M4 12h3 M11 12h9 M4 18h11 M19 18h1 M15 4v4 M9 10v4 M17 16v4',
    collapse: 'M4 4v16 M15 6l-6 6 6 6',
    expand: 'M4 4v16 M10 6l6 6-6 6',
  } as const;
  let { name }: { name: keyof typeof PATHS } = $props();
</script>

<svg class="nav-icon" viewBox="0 0 24 24" aria-hidden="true"><path d={PATHS[name]} /></svg>

<style>
  .nav-icon { width: 1.125rem; height: 1.125rem; flex: none; fill: none; stroke: currentColor; stroke-width: 1.5; stroke-linecap: square; stroke-linejoin: miter; }
</style>
