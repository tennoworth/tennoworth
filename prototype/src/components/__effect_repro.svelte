<script lang="ts">
  interface ColumnDef { key: string; align: 'left' | 'right'; noSort?: boolean; }
  interface Props { visibleColumns?: string[] | null; }
  let { visibleColumns = null }: Props = $props();

  let sortKey = $state<string>('sell_score');

  const ALL_COLUMNS: ColumnDef[] = [
    { key: 'name', align: 'left' },
    { key: 'owned', align: 'right' },
    { key: 'sell_score', align: 'right' },
    { key: 'avg_price', align: 'right' },
    { key: 'low_sell', align: 'right' },
    { key: 'top_buy', align: 'right' },
    { key: 'potential_plat', align: 'right' },
  ];

  let columns: ColumnDef[] = $derived.by(() => {
    let cols = ALL_COLUMNS;
    if (Array.isArray(visibleColumns) && visibleColumns.length > 0) {
      const byKey = new Map(cols.map((c) => [c.key, c]));
      cols = visibleColumns.map((k) => byKey.get(k)).filter((c): c is ColumnDef => Boolean(c));
    }
    return cols;
  });

  // EXACT replica of ResultsTable.svelte:130-135
  $effect(() => {
    if (!columns.find((c) => c.key === sortKey)) {
      const fallback = columns.find((c) => c.align === 'right' && !c.noSort);
      if (fallback) sortKey = fallback.key;
    }
  });

  export function getSortKey() { return sortKey; }
  export function setSort(k: string) { sortKey = k; }
</script>

<div data-testid="sortkey">{sortKey}</div>
