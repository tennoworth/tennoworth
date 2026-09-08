import { expect, test } from '@playwright/test';

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} buyer comparison covers only observed units and preserves selection`, async ({ page }, info) => {
    const errors: string[] = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.emulateMedia({ colorScheme: theme });
    await page.goto('/?preview-desktop&sample=buyers');
    await page.getByRole('button', { name: /^Max Value/ }).click();
    await page.getByLabel('Trade budget').fill('24');
    await page.locator('.session-table tbody tr').filter({ hasText: 'Primed Flow' }).getByRole('button', { name: 'Compare buyers' }).click();
    const panel = page.getByRole('region', { name: 'Buyer alternatives', exact: true });
    await expect(panel).toContainText('5 of 6 units covered');
    await expect(panel).toContainText('54p visible bid value');
    await expect(panel).toContainText('1 unit without observed coverage');
    await expect(panel.getByRole('link', { name: 'SampleBuyerA' })).toHaveAttribute('href', 'https://warframe.market/profile/sample_buyer_a');
    await page.getByLabel('Trade budget').fill('1');
    for (const [width, height] of [[1440, 900], [1200, 480], [320, 480]]) {
      await page.setViewportSize({ width, height });
      await expect(panel).toContainText('Primed Flow ×6');
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1)).toBe(true);
      await panel.scrollIntoViewIfNeeded();
      await page.screenshot({ path: info.outputPath(`buyers-${theme}-${width}.png`) });
      const table = panel.getByRole('region', { name: 'Compatible buyer quantities' });
      await table.evaluate(element => { element.scrollLeft = element.scrollWidth; });
      await page.screenshot({ path: info.outputPath(`buyer-columns-${theme}-${width}.png`) });
    }
    await panel.getByRole('button', { name: 'Refresh buyers' }).click();
    await expect(panel).toContainText('5 of 6 units covered');
    await panel.getByRole('button', { name: 'Close buyer comparison' }).click();
    await expect(panel).toHaveCount(0);
    expect(errors).toEqual([]);
  });
}

for (const [scenario, expected] of [
  ['buyers-empty', '0 of'], ['buyers-stale', 'stale'], ['buyers-locked', 'Unlock your WFM account'], ['buyers-error', 'Sample buyer lookup failed.'],
]) {
  test(`${scenario} stays explicit`, async ({ page }) => {
    await page.goto(`/?preview-desktop&sample=${scenario}`);
    await page.locator('.session-table').getByRole('button', { name: 'Compare buyers' }).first().click();
    const panel = page.getByRole('region', { name: 'Buyer alternatives', exact: true });
    await expect(panel).toContainText(expected);
    await expect(panel.getByRole('link', { name: 'Open item on WFM' })).toBeVisible();
  });
}
