import { expect, test } from '@playwright/test';

// A prime set is assembled from parts, so a scan of individual items can never
// report the set itself. Reading that absence from the owned map as "you own
// none" flagged a buildable, live listing and offered a one-click delete for it.
//
// The `orders-unowned` sample lists one item the inventory really does not own
// (Vitality) beside one composed set it cannot report (Akbolto Prime Set), so
// both the surviving true positive and the silenced false one are on the real
// styled surface.
const VIEWPORTS = [
  { width: 1200, height: 700 },
  { width: 320, height: 700 },
  { width: 1200, height: 480 },
  { width: 320, height: 480 },
] as const;

for (const theme of ['light', 'dark'] as const) {
  for (const { width, height } of VIEWPORTS) {
    test(`${theme} ${width}x${height}: composed sets are not called unowned, and deleting asks first`, async ({ page }, testInfo) => {
      await page.emulateMedia({ colorScheme: theme });
      await page.setViewportSize({ width, height });
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
      const confirm = queue.getByRole('button', { name: 'Confirm' });
      await expect(confirm).toHaveCount(1);
      await expect(queue.getByRole('button', { name: 'Delete', exact: true })).toHaveCount(0);
      // A short window must not put the confirmation or its escape out of reach.
      await expect(confirm).toBeInViewport();
      await expect(queue.getByRole('button', { name: 'Cancel delete' })).toBeInViewport();

      // Cancelling leaves the listing alone and still fixable.
      await queue.getByRole('button', { name: 'Cancel delete' }).click();
      await expect(queue.locator('tr.confirming')).toHaveCount(0);
      await expect(queue.getByRole('button', { name: 'Delete', exact: true })).toHaveCount(1);

      // The extra control must not push the panel into horizontal scrolling.
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1)).toBe(true);

      await page.screenshot({ path: testInfo.outputPath(`orders-${theme}-${width}x${height}.png`) });
    });
  }
}

// Which listings are composed sets comes from the market snapshot, so until it
// arrives the set's absence from the scan is no evidence either way. Reading it
// as zero owned briefly offered a delete for the live set listing.
test('a set listing is not called unowned while the market is still loading', async ({ page }) => {
  let release!: () => void;
  const held = new Promise<void>(resolve => { release = resolve; });
  await page.route('**/market.json', async route => { await held; await route.continue(); });
  await page.goto('/?preview-desktop&sample=orders-unowned');
  await expect(page.locator('.shell')).toBeVisible();
  await page.locator('.sidebar').getByRole('button', { name: /^My orders/ }).click();

  const health = page.getByRole('region', { name: 'Listing health' });
  const setRow = health.locator('tr', { hasText: 'Akbolto Prime Set' });
  await expect(health.getByText('not in your inventory')).toHaveCount(0);
  await expect(setRow.getByRole('button', { name: 'Delete', exact: true })).toHaveCount(0);

  release();
  await expect(health.getByText('1 not owned')).toBeVisible();
  await expect(health.locator('table').getByRole('button', { name: 'Delete', exact: true })).toHaveCount(1);
  await expect(setRow.getByText('not in your inventory')).toHaveCount(0);
});

// The keyboard path: arming replaces the very button the user activated, so
// focus has to follow it rather than dropping to the document body - and the
// focused confirmation has to be visibly focused, not just technically so.
test('keyboard: arming carries focus to the confirmation, and cancelling hands it back', async ({ page }) => {
  await page.goto('/?preview-desktop&sample=orders-unowned');
  await expect(page.locator('.shell')).toBeVisible();
  await page.locator('.sidebar').getByRole('button', { name: /^My orders/ }).click();

  const queue = page.getByRole('region', { name: 'Listing health' }).locator('table');
  const remove = queue.getByRole('button', { name: 'Delete', exact: true });
  await remove.focus();
  await page.keyboard.press('Enter');

  const confirm = queue.getByRole('button', { name: 'Confirm' });
  await expect(confirm).toBeFocused();
  await expect(confirm).toHaveCSS('outline-style', 'solid');

  await queue.getByRole('button', { name: 'Cancel delete' }).focus();
  await page.keyboard.press('Enter');
  await expect(remove).toBeFocused();
  // The listing survived the round trip.
  await expect(queue.getByText('not in your inventory')).toHaveCount(1);
});
