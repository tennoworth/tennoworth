import type { DemandRead } from '../domain/demand';
export interface SellRow {
  key?: string;
  slug: string;
  subtype: string | null;
  name: string;
  owned: number;
  sellable: number;
  leveled: number;
  type: string;
  kept_lvl: number | null;
  ducats: number | null;
  plat_per_100d: number | null;
  avg_price: number;
  low_sell: number;
  clearing_price?: number;
  low5_avg: number;
  top_buy: number;
  volume_48h: number;
  ratio: number;
  potential_plat: number;
  raw_value: number;
  sell_score: number;
  /** Advisor verdict for calendar-dated primes; null elsewhere. */
  advice?: 'sell_now' | 'hold' | 'neutral' | null;
  advice_reasons?: string[];
  patience: boolean;
  timing: 'hold' | 'peak' | 'neutral';
  medians_7d: number[];
  median_90d: number | null;
  delta_90d_pct: number | null;
  tags: string[];
  is_augment: boolean;
  vault_status: 'vaulted' | 'vaulting-soon' | 'available' | null;
  /** DE usage telemetry fused with the live market signal. Absent on
   *  snapshots built before the usage surface landed. */
  demand?: DemandRead;
}
