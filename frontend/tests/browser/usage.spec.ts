import { test, expect } from '@playwright/test';
import { readFileSync } from 'node:fs';
const fixture=JSON.parse(readFileSync(new URL('../../../tests/fixtures/usage/daily.json',import.meta.url),'utf8'));
for (const mode of ['light','dark']) {
  test(`public usage chart preserves counts in ${mode}`,async ({page}) => {
    const requests:string[]=[];
    await page.route('**/api/usage/daily',route => { requests.push(route.request().method()); return route.fulfill({json:fixture}); });
    await page.addInitScript(mode => localStorage.setItem('wfminv:theme-mode-v1',mode),mode);
    await page.goto('/');
    const chart=page.locator('#community-usage');
    await expect(chart.getByRole('heading',{name:'Daily active installations sharing usage counts'})).toBeVisible();
    await chart.getByText('Daily counts table').click();
    await expect(chart.getByRole('row',{name:'2026-09-09 0 Complete'})).toBeVisible();
    await expect(chart.getByRole('row',{name:'2026-09-10 3 Incomplete'})).toBeVisible();
    for (const width of [320,760,1440]) {
      await page.setViewportSize({width,height:480});
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    }
    expect(requests).toEqual(['GET']);
  });
}
test('unavailable usage is not displayed as zero',async ({page}) => {
  await page.route('**/api/usage/daily',route => route.fulfill({status:503,body:''}));
  await page.goto('/');
  await expect(page.locator('#community-usage').getByText('Usage counts are currently unavailable.')).toBeVisible();
  await expect(page.locator('#community-usage table')).toHaveCount(0);
});

for (const mode of ['light', 'dark'] as const) {
  test(`ninety days and settings fit narrow and short windows in ${mode}`, async ({ page }, testInfo) => {
    const today = new Date();
    const days = Array.from({ length: 90 }, (_, i) => ({
      date: new Date(Date.UTC(today.getUTCFullYear(), today.getUTCMonth(), today.getUTCDate() - 90 + i)).toISOString().slice(0, 10),
      count: i % 11, complete: i % 9 !== 0,
    }));
    await page.route('**/api/usage/daily', route => route.fulfill({ json: { updated_at: today.toISOString(), days } }));
    await page.emulateMedia({ colorScheme: mode });
    await page.goto('/');
    const chart = page.locator('#community-usage');
    await expect(chart.locator('.bar-slot')).toHaveCount(90);
    for (const width of [320, 760, 1440]) {
      await page.setViewportSize({ width, height: 480 });
      await chart.scrollIntoViewIfNeeded();
      expect(await chart.locator('.bars').evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
      await chart.screenshot({ path: testInfo.outputPath(`chart-${mode}-${width}.png`) });
    }
    await page.goto('/?preview-desktop&sample');
    await page.locator('.sidebar').getByRole('button', { name: /^Settings/ }).click();
    const checkbox = page.getByRole('checkbox', { name: /Share a daily usage count/ });
    await expect(checkbox).toBeDisabled();
    await expect(checkbox).not.toBeChecked();
    for (const width of [320, 760, 1440]) {
      await page.setViewportSize({ width, height: 480 });
      await checkbox.scrollIntoViewIfNeeded();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
      await page.locator('section', { has: checkbox }).screenshot({ path: testInfo.outputPath(`settings-${mode}-${width}.png`) });
    }
  });
}
