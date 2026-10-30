// The resolver catalog (wfstat-catalog.json, baked by tennoworth-pipeline build) as the
// desktop hands it to native inventory normalization, which owns resolving
// /Lotus/... paths to warframe.market slugs (market_domain::inventory).
import type { SlimItemInfo } from './data';

export interface Catalogs {
  uniqueToInfo: Map<string, SlimItemInfo>;
}
