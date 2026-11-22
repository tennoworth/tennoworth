import { t } from '../../i18n/i18n.svelte';
import type { SellRow } from '../../contracts/selling';

// The reason says only what the columns beside it cannot: where the price
// sits against its 90-day baseline. Low ask and Vol 48h are already on the
// row; the clearing price is named only when the troll-undercut clamp made it
// differ from the low ask the row shows.
export function pickReason(p: SellRow) {
  const price = Math.round(p.clearing_price ?? p.low_sell);
  const clamp = price !== Math.round(p.low_sell) ? t('sell.reasonClears', { price }) : '';
  const d = p.delta_90d_pct;
  if (p.timing === 'drop') return t('sell.reasonDrop') + clamp;
  if (p.timing === 'peak') return (d != null && d > 0 ? t('sell.reasonPeakUp', { pct: Math.round(d) }) : t('sell.reasonPeak')) + clamp;
  if (d != null && d >= 1) return t('sell.reasonUp', { pct: Math.round(d) }) + clamp;
  if (d != null && d <= -1) return t('sell.reasonDown', { pct: Math.abs(Math.round(d)) }) + clamp;
  return t('sell.reasonFlat') + clamp;
}
