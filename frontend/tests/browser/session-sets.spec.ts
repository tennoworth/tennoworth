import { expect, test } from '@playwright/test';

// The batch this test edits depends on how the set and its parts rank, so their
// prices are pinned: each release refreshes public/market.json, and the
// 2026-10-30 snapshot made a different, equally valid batch.
const AKBOLTO = {
  akbolto_prime_set: { avg: 65.3, buys: 5, donch_bot_90d: 64, donch_top_90d: 105, ducats: 350, low5_avg: 63.8, low_sell: 60.0, median_90d: 80.0, median_now: 65.0, medians_7d: [65.0, 65.0, 65.0, 64.0, 64.0, 65.0, 65.0], price_basis: 'unit', ratio: 0.08, sells: 61, tags: ['weapon', 'prime', 'secondary', 'set'], top_buy: 50.0, vol: 37 },
  akbolto_prime_receiver: { avg: 26.3, buys: 9, donch_bot_90d: 23, donch_top_90d: 35, ducats: 100, low5_avg: 28.8, low_sell: 28.0, median_90d: 27.0, median_now: 27.5, medians_7d: [25.0, 26.0, 25.0, 25.0, 25.0, 25.0, 27.5], price_basis: 'unit', ratio: 0.45, sells: 20, tags: ['component', 'weapon', 'prime'], top_buy: 25.0, vol: 51 },
  akbolto_prime_link: { avg: 4.2, buys: 3, donch_bot_90d: 4, donch_top_90d: 6, ducats: 45, low5_avg: 7.6, low_sell: 5.0, median_90d: 5.0, median_now: 5.0, medians_7d: [5.0, 4.0, 4.0, 5.0, 4.0, 5.0, 5.0], price_basis: 'unit', ratio: 0.23, sells: 13, tags: ['component', 'weapon', 'prime'], top_buy: 2.0, vol: 19 },
  akbolto_prime_barrel: { avg: 7.1, buys: 5, donch_bot_90d: 6, donch_top_90d: 9, ducats: 45, low5_avg: 6.8, low_sell: 5.0, median_90d: 7.0, median_now: 8.0, medians_7d: [7.0, 7.0, 7.0, 7.0, 7.0, 8.0, 8.0], price_basis: 'unit', ratio: 0.26, sells: 19, tags: ['component', 'weapon', 'prime'], top_buy: 5.0, vol: 27 },
  akbolto_prime_blueprint: { avg: 3.6, buys: 1, donch_bot_90d: 3, donch_top_90d: 5, ducats: 15, low5_avg: 4.8, low_sell: 4.0, median_90d: 4.0, median_now: 4.0, medians_7d: [3.0, 5.0, 3.0, 3.5, 4.0, 3.0, 4.0], price_basis: 'unit', ratio: 0.09, sells: 11, tags: ['weapon', 'prime', 'secondary', 'blueprint'], top_buy: 2.0, vol: 7 },
};

test('complete sets compete with their parts and edited quantities cannot reuse components', async ({ page }) => {
  await page.route('**/market.json', async route => {
    const market = await (await route.fetch()).json();
    Object.assign(market.items, AKBOLTO);
    await route.fulfill({ json: market });
  });
  await page.goto('/?preview-desktop&sample=session-sets');
  await page.getByRole('button', { name: /^Plat per Trade/ }).click();
  await page.getByLabel('Trade budget').fill('2');
  const table = page.locator('.session-table');
  await expect(table).toContainText('Akbolto Prime Set');
  await expect(table).toContainText('Akbolto Prime Barrel ×2');
  await page.getByText('Target and complete sets', { exact: true }).click();
  await page.getByLabel('Include complete owned sets').uncheck();
  await expect(table).not.toContainText('Akbolto Prime Set');
  await page.getByLabel('Include complete owned sets').check();
  await page.getByRole('button', { name: 'Review batch', exact: true }).click();
  const dialog = page.getByRole('dialog');
  const set = dialog.locator('tbody tr').filter({ hasText: 'Akbolto Prime Set' });
  await expect(set).toBeVisible();
  await set.locator('input[type="number"]').first().fill('2');
  // The part selected beside this set consumes one of the same components;
  // two sets need the complete pool, so the edited batch must be rejected.
  await expect(dialog).toContainText('reuse components');
  await expect(dialog.getByRole('button', { name: /^Send/ })).toBeDisabled();
});
