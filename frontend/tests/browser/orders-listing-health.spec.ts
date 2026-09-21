import { expect, test } from '@playwright/test';

// A prime set is assembled from parts, so a scan of individual items can never
// report the set itself. Reading that absence from the owned map as "you own
// none" flagged a buildable, live listing and offered a one-click delete for it.
//
// The `orders-unowned` sample lists one item the inventory really does not own
// (Vitality) beside one composed set it cannot report (Akbolto Prime Set), so
// both the surviving true positive and the silenced false one are on the real
// styled surface.
for (const theme of ['light', 'dark'] as const) {
  for (const width of [1200, 320]) {
    test(`${theme} ${width}px: composed sets are not called unowned, and deleting asks first`, async ({ page }, testInfo) => {
      await page.emulateMedia({ colorScheme: theme });
      await page.setViewportSize({ width, height: 700 });
      await page.goto('/?preview-desktop&sample=orders-unowned');
      await expect(page.locator('.shell')).toBeVisible();
      await page.locator('.sidebar').getByRole('button', { name: /^My orders/ }).click();

      const health = page.getByRole('region', { name: 'Listing health' });
      await expect(health).toBeVisible();

      // Exactly one ownership verdict - the mod the scan really cannot find.
      await expect(health.getByText('1 not owned')).toBeVisible();
      const queue = health.locator('table');
      await expect(queue.getByText('not in your inventory')).toHaveCount(1);

      // The set is in the queue for its snapshot drift, never as unowned.
      const setRow = queue.locator('tr', { hasText: 'Akbolto Prime Set' });
      await expect(setRow).toHaveCount(1);
      await expect(setRow.getByText('not in your inventory')).toHaveCount(0);

      // Removing a live listing arms first and acts on a second, deliberate click.
      const remove = queue.getByRole('button', { name: 'Delete', exact: true });
      await expect(remove).toHaveCount(1);
      await remove.click();
      await expect(queue.locator('tr.confirming')).toHaveCount(1);
      await expect(queue.getByRole('button', { name: 'Confirm' })).toHaveCount(1);
      await expect(queue.getByRole('button', { name: 'Delete', exact: true })).toHaveCount(0);

      // Cancelling leaves the listing alone and still fixable.
      await queue.getByRole('button', { name: 'Cancel delete' }).click();
      await expect(queue.locator('tr.confirming')).toHaveCount(0);
      await expect(queue.getByRole('button', { name: 'Delete', exact: true })).toHaveCount(1);

      // The extra control must not push the panel into horizontal scrolling.
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1)).toBe(true);

      await page.screenshot({ path: testInfo.outputPath(`orders-${theme}-${width}.png`) });
    });
  }
}
