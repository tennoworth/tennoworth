// Trade presence, as the user meets it: the status strip's menu and the
// Settings group behind "Presence settings". The preview transport pushes the
// same status payloads the Rust loop does, so this drives the real event wiring.
import { expect, test, type Locator, type Page } from '@playwright/test';

async function within(page: Page, element: Locator): Promise<void> {
  const box = await element.boundingBox();
  const width = page.viewportSize()!.width;
  expect(box, 'menu rendered').not.toBeNull();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(width + 1);
}

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} status menu picks a status, pauses following and stays inside narrow windows`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.goto('/?preview-desktop&sample');
    const trigger = page.locator('.statusbar .presence .trigger');
    await expect(trigger).toHaveText(/Online in game/i);

    await trigger.click();
    const menu = page.locator('#presence-menu');
    await expect(menu).toBeVisible();
    await expect(menu).toContainText('Following the game · Warframe running');
    // Following keeps the status up itself, so its duration is not the user's to pick.
    await expect(menu.getByRole('button', { name: '1h' })).toBeDisabled();

    await menu.getByRole('radio', { name: /^Online Shown as online/ }).check();
    await expect(trigger).toHaveText(/^Online ▾$/i);
    await expect(menu).toContainText('following waits for your next game session');
    await expect(menu.getByRole('button', { name: '1h' })).toBeEnabled();

    await menu.getByRole('button', { name: 'Follow the game now' }).click();
    await expect(trigger).toHaveText(/Online in game/i);

    for (const width of [1440, 760, 320]) {
      await page.setViewportSize({ width, height: 600 });
      await within(page, menu);
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    }

    await page.keyboard.press('Escape');
    await expect(menu).toHaveCount(0);
    await expect(trigger).toBeFocused();
  });
}

test('presence settings open at the account panel and keep a refused account readable', async ({ page }) => {
  await page.goto('/?preview-desktop&sample&presence=unverified');
  const trigger = page.locator('.statusbar .presence .trigger');
  await expect(trigger).toHaveText(/Status refused/i);
  await trigger.click();
  const menu = page.locator('#presence-menu');
  // A refused account cannot pick a status, and the menu says why.
  await expect(menu.getByRole('radio', { name: /^Online Shown as online/ })).toBeDisabled();
  await expect(menu).toContainText('this account is not verified');

  await menu.getByRole('button', { name: 'Presence settings →' }).click();
  const group = page.locator('#settings-account [aria-labelledby="set-presence"]');
  await expect(group).toBeVisible();
  await expect(group.locator('.ui-notice[data-tone="bad"]')).toContainText('not verified');
  await expect(group.getByRole('button', { name: 'Online', exact: true })).toBeDisabled();
  await expect(page.locator('.glance')).toContainText('Status refused');
});

test('a locked session keeps the strip’s sign-in link instead of a status', async ({ page }) => {
  await page.goto('/?preview-desktop&sample=logged-out');
  await expect(page.locator('.statusbar .presence')).toHaveCount(0);
});
