export interface PriceReportPreferences {
  enabled: boolean;
  available: boolean;
  /** Sales the service accepted from this install during the current ISO week. */
  sent_this_week: number;
  /** Riven sales are covered. False for an install that turned sharing on
   *  under the first terms, which left riven trades out. */
  rivens: boolean;
}

export type RollVerdictName = 'sellable' | 'almost' | 'weak' | 'unrated';

/** What a player says the roll guide gets wrong about one stat on a weapon:
 *  `good` is a wanted positive or a harmless negative. */
export interface RollSuggestion {
  stat: string;
  positive: boolean;
  good: boolean;
}

/** A rating of the verdict shown for one weapon's riven. The service keeps
 *  one per weapon a week; it is evidence for a guide change, never an edit. */
export interface RollVote {
  weapon: string;
  verdict: RollVerdictName;
  agree: boolean;
  suggest?: RollSuggestion | null;
}

/** Players' reported sale prices for one weapon's rivens under one verdict,
 *  from the newest week that published them. Display only: never advice. */
export interface CommunityRivenPrice {
  weapon: string;
  verdict: RollVerdictName;
  week: string;
  complete: boolean;
  contributors: number;
  sales: number;
  p25: number;
  median: number;
  p75: number;
}
