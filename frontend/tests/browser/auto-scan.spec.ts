// Automatic scanning, as the user meets it: the Settings section that turns it
// on, and the offer that stands in for a silent swap when adoption is off.
//
// The preview transport carries the same payloads the Rust loop does, so this
// exercises the real styled surface and the real event wiring between them.
import { expect, test } from '@playwright/test';

async function openSettings(page: import('@playwright/test').Page) {
  await page.goto('/?preview-desktop&sample');
  await page.locator('.sidebar').getByRole('button', { name: /^Settings/ }).click();
  return page.locator('.settings');
}

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} automatic scan settings follow the toggle and survive narrow windows`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme });
    const settings = await openSettings(page);
    const section = settings.locator('section[aria-labelledby="set-auto-scan"]');
    const toggle = section.getByRole('checkbox', { name: /Scan automatically while Warframe is running/ });

    await expect(toggle).not.toBeChecked();
    await expect(section.getByRole('combobox', { name: /Scan every/ })).toHaveCount(0);
    await expect(section).toContainText('Manual scans only.');

    await toggle.check();
    const cadence = section.getByRole('combobox', { name: /Scan every/ });
    await expect(cadence).toBeVisible();
    await expect(cadence).toHaveValue('30');
    // The loop republishes the stored preferences, so the status cannot keep
    // claiming "manual only" once scanning is on.
    await expect(section).toContainText('Waiting for Warframe');

    await cadence.selectOption('60');
    await expect(cadence).toHaveValue('60');

    for (const width of [1440, 760, 320]) {
      await page.setViewportSize({ width, height: 600 });
      await expect(toggle).toBeChecked();
      await expect(cadence).toHaveValue('60');
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    }
  });
}

test('a background scan is offered instead of replacing the inventory when adoption is off', async ({ page }) => {
  const settings = await openSettings(page);
  const section = settings.locator('section[aria-labelledby="set-auto-scan"]');
  await section.getByRole('checkbox', { name: /Update the open app automatically/ }).uncheck();

  await page.evaluate(() => (globalThis as unknown as { __TENNOWORTH_PREVIEW_EMIT__: (n: string, p: unknown) => void })
    .__TENNOWORTH_PREVIEW_EMIT__('inventory-scanned', { inventory: '{"Suits":[]}', snapshot_id: 7 }));

  const banner = page.locator('.general-banner', { hasText: 'An automatic scan finished' });
  await expect(banner).toBeVisible();
  await expect(banner.getByRole('button', { name: 'Load new scan' })).toBeVisible();

  await banner.getByRole('button', { name: 'Dismiss' }).click();
  await expect(banner).toHaveCount(0);
});
