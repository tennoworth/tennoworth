import { expect, test } from '@playwright/test';

/**
 * Trade recording can stop without stopping the app: the ledger refuses a
 * completed trade, the tailer holds it and retries, and everything behind it
 * waits. The user has to be able to learn that from the Ledger surface, at the
 * sizes the app actually runs at, in either theme.
 */

const PAUSED = /Trade recording paused/;

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} ledger names a paused recording and its cause at every required size`, async ({ page }, testInfo) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.goto('/?preview-desktop&sample=ledger-paused');
    await expect(page.locator('.shell')).toBeVisible();
    await page.locator('.sidebar').getByRole('button', { name: /^Ledger/ }).click();

    const notice = page.locator('.ui-notice[role="status"]').filter({ hasText: PAUSED });
    await expect(notice).toBeVisible();

    // Narrow, short, and wide. 480 CSS px is the short-height contract, not a
    // desktop-only layout.
    for (const [width, height] of [[1200, 700], [320, 700], [1200, 480], [320, 480]] as const) {
      await page.setViewportSize({ width, height });
      await expect(notice, `visible at ${width}x${height}`).toBeVisible();
      // A warning the user must scroll horizontally to read is not a warning.
      expect(
        await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1),
        `no horizontal overflow at ${width}x${height}`,
      ).toBe(true);
      const box = await notice.boundingBox();
      expect(box, `notice is laid out at ${width}x${height}`).not.toBeNull();
      expect(box!.width, `notice keeps a usable width at ${width}x${height}`).toBeGreaterThan(80);
      // At 320 the notice sits below the shell's wrapped header, so the capture
      // has to scroll to it - a screenshot of the top of the page proves nothing
      // about a warning further down.
      await notice.scrollIntoViewIfNeeded();
      await page.screenshot({ path: testInfo.outputPath(`ledger-paused-${theme}-${width}x${height}.png`) });
    }

    // The two causes need different next steps, so they must not share copy.
    await expect(notice).toContainText('database is locked');
    await expect(notice).toContainText('not in the history below yet');
  });
}

test('an unreadable log is described as the log, not as the ledger', async ({ page }) => {
  await page.goto('/?preview-desktop&sample=log-unreadable');
  await expect(page.locator('.shell')).toBeVisible();
  await page.locator('.sidebar').getByRole('button', { name: /^Ledger/ }).click();

  const notice = page.locator('.ui-notice[role="status"]').filter({ hasText: PAUSED });
  await expect(notice).toBeVisible();
  await expect(notice).toContainText('game log could not be read');
  await expect(notice).not.toContainText('not in the history below yet');
});

test('a healthy ledger shows no pause notice', async ({ page }) => {
  await page.goto('/?preview-desktop&sample');
  await expect(page.locator('.shell')).toBeVisible();
  await page.locator('.sidebar').getByRole('button', { name: /^Ledger/ }).click();
  await expect(page.locator('.ledger tbody tr').first()).toBeVisible();
  await expect(page.locator('.ui-notice[role="status"]').filter({ hasText: PAUSED })).toHaveCount(0);
});
