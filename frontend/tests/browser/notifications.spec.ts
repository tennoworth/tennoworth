import { expect, test } from '@playwright/test';

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} notification inbox and preferences remain usable`, async ({ page }, testInfo) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.goto('/?preview-desktop&sample');
    await page.locator('.sidebar').getByRole('button', { name: /^Notifications/ }).click();
    await expect(page.getByRole('heading', { name: 'Sold Pyrana Prime Set for 90p' })).toBeVisible();
    for (const [width, height] of [[1440,900],[768,480],[320,600]]) {
      await page.setViewportSize({ width, height });
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
      await expect(page.getByRole('button', { name: 'Review My Orders' })).toBeVisible();
      await expect(page.getByText('Desktop popup could not be delivered.', { exact: false })).toBeVisible();
      await page.screenshot({ path: testInfo.outputPath(`inbox-${theme}-${width}.png`), fullPage: true });
    }
    await page.getByRole('button', { name: 'Mark all read', exact: true }).click();
    await page.getByLabel('Unread only').check();
    await expect(page.getByText('You’re all caught up.')).toBeVisible();
    await page.getByLabel('Unread only').uncheck();
    await page.getByRole('button', { name: 'Notification settings', exact: true }).click();
    await page.getByLabel('Desktop popups', { exact: true }).uncheck();
    await expect(page.getByText('Preferences saved.')).toBeVisible();
    await expect(page.getByLabel('Price watches popups', { exact: true })).toBeDisabled();
    await page.getByLabel('Baro arrival and departure enabled', { exact: true }).uncheck();
    await page.getByRole('button', { name: 'Send test notification', exact: true }).click();
    await expect(page.getByText('Test sent (preview).')).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
    await page.screenshot({ path: testInfo.outputPath(`settings-${theme}.png`), fullPage: true });
    await page.locator('.sidebar').getByRole('button', { name: /^Notifications/ }).click();
    await page.getByRole('button', { name: 'Review My Orders' }).click();
    await expect(page.getByRole('heading', { name: 'My orders', exact: true })).toBeVisible();
    await page.locator('.sidebar').getByRole('button', { name: /^Notifications/ }).click();
    await page.getByRole('button', { name: 'Clear history' }).click();
    await page.getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(page.getByRole('heading', { name: 'Sold Pyrana Prime Set for 90p' })).toBeVisible();
    await page.getByRole('button', { name: 'Clear history' }).click();
    await page.getByRole('button', { name: 'Clear notifications', exact: true }).click();
    await expect(page.getByText('No notifications yet.', { exact: false })).toBeVisible();
  });
}

test('notification errors, empty state and first-run entry are reachable', async ({ page }) => {
  await page.goto('/?preview-desktop&sample=error');
  await page.locator('.sidebar').getByRole('button', { name: /^Notifications/ }).click();
  await expect(page.getByRole('alert')).toContainText('Could not load notifications');
  await page.getByRole('button', { name: 'Retry', exact: true }).click();
  await expect(page.getByRole('alert')).toBeVisible();
  await page.goto('/?preview-desktop');
  await page.getByRole('button', { name: /^Notifications/ }).click();
  await expect(page.getByRole('heading', { name: 'Notifications', exact: true })).toBeVisible();
  await expect(page.getByText('No notifications yet.', { exact: false })).toBeVisible();
  await page.locator('.sidebar').getByRole('button', { name: /^Baro/ }).click();
  await expect(page.getByRole('heading', { name: "Baro Ki'Teer", exact: true })).toBeVisible();
});

test('failed preference save retains the persisted selection', async ({ page }) => {
  await page.goto('/?preview-desktop&sample=preferences-error');
  await page.locator('.sidebar').getByRole('button', { name: /^Settings/ }).click();
  const toggle = page.getByLabel('Baro arrival and departure enabled', { exact: true });
  await expect(toggle).toBeChecked();
  await toggle.click();
  await expect(page.getByRole('alert')).toContainText('Could not save notification preferences.');
  await expect(toggle).toBeChecked();
});

test('notification row reference uses both themes and preserves its read toggle', async ({ page }, testInfo) => {
  await page.goto('/?styleguide');
  await page.getByRole('combobox', { name: 'Data state' }).selectOption('notifications');
  const reference = page.getByRole('region', { name: 'Notification row reference' });
  for (const mode of ['Light', 'Dark']) {
    await page.getByRole('button', { name: mode, exact: true }).click();
    await page.setViewportSize({ width: 320, height: 480 });
    await reference.getByRole('button', { name: 'Mark read', exact: true }).click();
    await expect(reference.getByText('Read · Completed trade', { exact: true })).toBeVisible();
    await reference.getByRole('button', { name: 'Mark unread', exact: true }).click();
    await expect(reference.getByText('Unread · Completed trade', { exact: true })).toBeVisible();
    await reference.screenshot({ path: testInfo.outputPath(`reference-${mode}.png`) });
  }
});
