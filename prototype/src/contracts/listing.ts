import type { SessionConstraint, MarketItemEntry } from './data';
export interface ListingCandidate {
  key?: string;
  slug: string;
  subtype?: string | null;
  name: string;
  owned: number;
  sellable?: number;
  leveled?: number;
  low_sell: number;
  avg_price: number;
  clearing_price?: number;
  kept_lvl?: number | null;
  proposed_quantity?: number;
  per_trade?: number;
  bulk?: boolean;
  session?: SessionConstraint;
  market?: MarketItemEntry;
}
