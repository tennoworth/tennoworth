import { expect, test } from '@playwright/test';
import { readFileSync } from 'node:fs';

const baseMarket = JSON.parse(readFileSync(new URL('../../public/market.json', import.meta.url), 'utf8'));

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} published surface ages reach shell warnings`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme });
    const market = structuredClone(baseMarket);
    const old = new Date(Date.now() - 10 * 86_400_000).toISOString();
    market.surface_provenance = {
      ...market.surface_provenance,
      set_to_parts: { disposition: 'merged_partial', attempted_at: new Date().toISOString(), data_fetched_at: old },
      relic_rewards: { disposition: 'empty_unavailable', attempted_at: new Date().toISOString(), data_fetched_at: new Date().toISOString() },
      baro: { disposition: 'preserved_unchanged', attempted_at: new Date().toISOString(), data_fetched_at: old },
    };
    await page.route('**/market.json', route => route.fulfill({ json: market }));
    await page.goto('/?preview-desktop&sample');
    for (const viewport of [{ width: 1200, height: 800 }, { width: 320, height: 640 }, { width: 800, height: 400 }]) {
      await page.setViewportSize(viewport);
      await page.locator('.sidebar').getByRole('button', { name: /^Set picks/ }).click();
      await expect(page.getByText(/set\/vault data .* ago/)).toBeVisible();
      await page.locator('.sidebar').getByRole('button', { name: /^Relics/ }).click();
      await expect(page.getByText('· ⚠ drop-table data age unknown')).toBeVisible();
      await page.locator('.sidebar').getByRole('button', { name: /^Baro/ }).click();
      await expect(page.getByText(/schedule data .* ago/)).toHaveCount(0);
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1)).toBe(true);
    }
  });
}
