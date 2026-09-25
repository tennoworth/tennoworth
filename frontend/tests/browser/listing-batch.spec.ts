import { expect, test } from '@playwright/test';

/**
 * An interrupted listing batch holds up to two kinds of unfinished work, and
 * they are not interchangeable. An item that was never sent can be resumed; an
 * item whose send outcome the market never confirmed may already be live, so
 * offering Resume would promise a re-send nobody can safely perform.
 *
 * This drives the real styled shell, because the banner is what tells the user
 * either of these exists - and a batch that renders nothing is a batch they
 * cannot see or clear.
 */

const PENDING = /Interrupted batch from/;

test('a never-sent item offers Resume', async ({ page }) => {
  await page.goto('/?preview-desktop&sample=batch-pending');
  await expect(page.locator('.shell')).toBeVisible();

  const banner = page.locator('section', { hasText: PENDING }).last();
  await expect(banner).toBeVisible();
  await expect(banner).toContainText('1 pending');
  await expect(banner.getByRole('button', { name: 'Resume' })).toBeVisible();
  await expect(banner.getByRole('button', { name: 'Discard' })).toBeVisible();
});

test('an unconfirmed outcome says so and does not offer Resume', async ({ page }) => {
  await page.goto('/?preview-desktop&sample=batch-uncertain');
  await expect(page.locator('.shell')).toBeVisible();

  const banner = page.locator('section', { hasText: PENDING }).last();
  await expect(banner).toBeVisible();
  await expect(banner).toContainText('1 with an unknown outcome');
  // The consequence and the next step, not just a label.
  await expect(banner).toContainText('may already be live');
  await expect(banner.getByRole('button', { name: 'Review in My Orders' })).toBeVisible();
  await expect(banner.getByRole('button', { name: 'Discard' })).toBeVisible();
  await expect(banner.getByRole('button', { name: 'Resume' })).toHaveCount(0);
});

test('the banner stays readable at every required size in both themes', async ({ page }, testInfo) => {
  for (const theme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: theme });
    await page.goto('/?preview-desktop&sample=batch-uncertain');
    await expect(page.locator('.shell')).toBeVisible();
    const banner = page.locator('section', { hasText: PENDING }).last();
    await expect(banner).toBeVisible();

    for (const [width, height] of [[1200, 700], [320, 700], [1200, 480], [320, 480]] as const) {
      await page.setViewportSize({ width, height });
      expect(
        await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1),
        `no horizontal overflow at ${width}x${height}`,
      ).toBe(true);
      await banner.scrollIntoViewIfNeeded();
      await expect(banner, `visible at ${width}x${height}`).toBeVisible();
      const box = await banner.boundingBox();
      expect(box, `laid out at ${width}x${height}`).not.toBeNull();
      await page.screenshot({ path: testInfo.outputPath(`batch-uncertain-${theme}-${width}x${height}.png`) });
    }
  }
});

test('a completed journal left on disk stays visible and removable', async ({ page }) => {
  for (const theme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: theme });
    await page.goto('/?preview-desktop&sample=batch-finished');
    const banner = page.locator('section', { hasText: 'Completed batch still saved.' }).last();
    await expect(banner).toBeVisible();
    await expect(banner.getByRole('button', { name: 'Discard record' })).toBeVisible();
    for (const [width, height] of [[1200, 700], [320, 480]] as const) {
      await page.setViewportSize({ width, height });
      await banner.scrollIntoViewIfNeeded();
      await expect(banner).toBeVisible();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
    }
  }
});
