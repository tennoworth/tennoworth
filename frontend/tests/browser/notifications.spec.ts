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
    // The inbox's link opens Settings at its notification section, not the top.
    await expect(page.locator('#settings-notifications')).toBeInViewport();
    await page.getByLabel('Desktop popups', { exact: true }).uncheck();
    await expect(page.getByText('Preferences saved.')).toBeVisible();
    await expect(page.getByRole('checkbox', { name: / in inbox$/ })).toHaveCount(5);
    await expect(page.getByLabel('Inventory scan summary in inbox', { exact: true })).toHaveCount(0);
    await expect(page.getByLabel('Price watches popup', { exact: true })).toBeDisabled();
    await page.getByLabel('Baro arrival and departure in inbox', { exact: true }).uncheck();
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
  const toggle = page.getByLabel('Baro arrival and departure in inbox', { exact: true });
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

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} organized history separates timestamps and preserves structured facts`, async ({ page }, testInfo) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.clock.setFixedTime(new Date('2026-09-04T19:00:00Z'));
    await page.goto('/?preview-desktop&sample=notifications');
    await page.locator('.sidebar').getByRole('button', { name: /^Notifications/ }).click();
    const baro = page.getByRole('article', { name: "Baro Ki'Teer is here", exact: true });
    const digest = page.getByRole('article', { name: "Today's sell opportunities", exact: true });
    await expect(page.getByRole('heading', { name: 'Today', exact: true })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Yesterday', exact: true })).toBeVisible();
    await expect(baro.locator('.yield-strip')).toContainText('180d');
    await expect(digest).toContainText('Arcane Energize ×12');
    await expect(digest).toContainText('~48p each');
    await expect(page.locator('.notification-entry .body').last()).toContainText('Full saved context from before structured notification details.');
    await baro.getByText('You hold 1 item from this stock', { exact: true }).click();
    await expect(baro.locator('.holding-list')).toContainText('Primed Flow');
    await baro.getByText('Published schedule', { exact: true }).click();
    await expect(baro.locator('.schedule-details')).toContainText('2026-09-04T13:00:00Z');
    for (const width of [1440, 768, 561, 560, 320]) {
      await page.setViewportSize({ width, height: 480 });
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
      const title = await baro.locator('.entry-title').boundingBox();
      const time = await baro.locator('.entry-time').boundingBox();
      expect(title).not.toBeNull(); expect(time).not.toBeNull();
      if (width > 560) expect(time!.x).toBeGreaterThanOrEqual(title!.x + title!.width);
      else expect(time!.y).toBeGreaterThanOrEqual(title!.y + title!.height);
      await expect(baro.locator('.holding-list')).toBeVisible();
      await baro.scrollIntoViewIfNeeded();
      await page.screenshot({ path: testInfo.outputPath(`organized-${theme}-${width}.png`), fullPage: true });
    }
    await page.setViewportSize({ width: 1440, height: 900 });
    await baro.getByRole('button', { name: 'Mark read', exact: true }).click();
    await expect(baro.getByText('Read · Baro', { exact: true })).toBeVisible();
    await expect(baro.locator('.holding-list')).toBeVisible();
    await expect(baro.locator('.schedule-details')).toHaveAttribute('open', '');
    await page.getByLabel('Unread only').check();
    await expect(baro).toHaveCount(0);
    await expect(page.getByRole('heading', { name: 'Yesterday', exact: true })).toHaveCount(0);
    await page.getByLabel('Unread only').uncheck();
    await page.getByRole('button', { name: 'Clear history', exact: true }).click();
    await page.getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(digest).toBeVisible();
    await page.getByRole('button', { name: 'Clear history', exact: true }).click();
    await page.getByRole('button', { name: 'Clear notifications', exact: true }).click();
    await expect(page.getByText('No notifications yet.', { exact: false })).toBeVisible();
  });
}
