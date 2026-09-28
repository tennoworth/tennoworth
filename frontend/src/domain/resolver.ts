// The resolver catalog (wfstat-catalog.json, baked by wfm-scrape build) as the
// desktop hands it to native inventory normalization, which owns resolving
// /Lotus/... paths to warframe.market slugs (market_domain::inventory).
import type { SlimItemInfo } from '../contracts/data';

export interface Catalogs {
  uniqueToInfo: Map<string, SlimItemInfo>;
}
