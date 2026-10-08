export interface PriceReportPreferences {
  enabled: boolean;
  available: boolean;
  /** Sales the service accepted from this install during the current ISO week. */
  sent_this_week: number;
}
