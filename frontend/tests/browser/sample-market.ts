import type { Page } from '@playwright/test';

// The session sample's suggested batch follows the day's market, and the tests
// that use it need two listings in it. A refreshed snapshot priced the Ivara
// Prime Neuroptics Blueprint below the minimum listing price, the planner
// rightly left it out, and every test that sends "listings" lost its second
// row. The sample's market entries are fixed here; everything else stays the
// snapshot's.
const PINNED = {
  arcane_energize: { avg: 7.1, low_sell: 7, low5_avg: 7.4, top_buy: 5, price_basis: 'unit', vol: 379, ratio: 0.05, buys: 1, sells: 22, tags: ['legendary', 'arcane_enhancement'], median_now: 7, median_90d: 8, medians_7d: [7, 7, 7, 7, 8, 8, 7], donch_top_90d: 9, donch_bot_90d: 7 },
  ivara_prime_neuroptics_blueprint: { avg: 6.6, low_sell: 5, low5_avg: 5.2, top_buy: 0, price_basis: 'unit', vol: 74, ratio: 0, buys: 0, sells: 26, tags: ['component', 'prime', 'warframe', 'blueprint'], ducats: 15, median_now: 6, median_90d: 7, medians_7d: [7, 6, 6, 7, 7, 7, 6], donch_top_90d: 9, donch_bot_90d: 5 },
  primed_flow: { avg: 36.7, low_sell: 40, low5_avg: 40, top_buy: 35, price_basis: 'unit', vol: 172, ratio: 0.17, buys: 5, sells: 29, tags: ['mod', 'legendary', 'warframe'], median_now: 35, median_90d: 30, medians_7d: [35, 40, 38.5, 39, 40, 35, 35], donch_top_90d: 40, donch_bot_90d: 20 },
};

export async function pinSampleMarket(page: Page) {
  await page.route('**/market.json', async (route) => {
    const response = await route.fetch();
    const market = await response.json();
    Object.assign(market.items, PINNED);
    await route.fulfill({ response, json: market });
  });
}
