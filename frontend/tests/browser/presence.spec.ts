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
    const word = trigger.locator('.current');
    await expect(word).toHaveText(/Online in game/i);

    await trigger.click();
    const menu = page.locator('#presence-menu');
    await expect(menu).toBeVisible();
    await expect(menu).toContainText('Following the game · Warframe running');
    // Following keeps the status up itself, so its duration is not the user's to pick.
    await expect(menu.getByRole('button', { name: '1h' })).toBeDisabled();

    await menu.getByRole('radio', { name: /^Online Shown as online/ }).check();
    await expect(word).toHaveText(/^Online$/i);
    await expect(menu).toContainText('following waits for your next game session');
    await expect(menu.getByRole('button', { name: '1h' })).toBeEnabled();

    await menu.getByRole('button', { name: 'Follow the game now' }).click();
    await expect(word).toHaveText(/Online in game/i);

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

// The window opens at 1200 px. A status word of a different length rewrapped
// the strip there, and the whole window moved by a row on every change.
for (const width of [1200, 1440]) test(`changing status never changes the status strip’s height at ${width}`, async ({ page }) => {
  await page.setViewportSize({ width, height: 800 });
  await page.goto('/?preview-desktop&sample');
  const strip = page.locator('.statusbar');
  const trigger = strip.locator('.presence .trigger');
  await trigger.click();
  const cell = strip.locator('.cell.end');
  const heights = new Set<number>();
  // Height alone only fails when a neighbour sits at its wrap point, which
  // depends on the data's age; any width change can rewrap the strip.
  const widths = new Set<number>();
  for (const choice of ['online', 'invisible', 'ingame']) {
    await page.locator(`#presence-menu input[value="${choice}"]`).check();
    await expect(trigger).toHaveAttribute('aria-busy', 'false');
    heights.add(Math.round((await strip.boundingBox())!.height));
    widths.add(Math.round((await cell.boundingBox())!.width));
  }
  expect([...heights]).toHaveLength(1);
  expect([...widths]).toHaveLength(1);
});

test('presence settings open at the account panel and keep a refused account readable', async ({ page }) => {
  await page.goto('/?preview-desktop&sample&presence=unverified');
  const trigger = page.locator('.statusbar .presence .trigger');
  await expect(trigger.locator('.current')).toHaveText(/Verify account/i);
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
  await expect(page.locator('.glance')).toContainText('Verify account');
});

test('a signed-out session names the step instead of a status', async ({ page }) => {
  await page.goto('/?preview-desktop&sample=logged-out');
  const cell = page.locator('.statusbar .cell.end');
  await expect(page.locator('.statusbar .presence')).toHaveCount(0);
  await expect(cell).toContainText('Signed out');
  await cell.getByRole('button', { name: 'Sign in' }).click();
  await expect(page.getByTestId('wfm-login-dialog')).toBeVisible();
});

for (const theme of ['light', 'dark'] as const) {
  for (const session of ['locked', 'signed-out', 'unlocked'] as const) {
    test(`${theme} ${session} account stays on the right when the header wraps`, async ({ page }, info) => {
      await page.emulateMedia({ colorScheme: theme });
      if (session === 'locked') {
        await page.addInitScript(() => {
          let runtime: any;
          Object.defineProperty(window, '__TAURI__', {
            configurable: true,
            get: () => runtime,
            set(value) {
              runtime = value;
              const invoke = value.core.invoke;
              value.core.invoke = (command: string, args: unknown) => command === 'wfm_auth_status'
                ? Promise.resolve({ logged_in: true, unlocked: false })
                : invoke(command, args);
            },
          });
        });
      }
      await page.goto(`/?preview-desktop&sample${session === 'signed-out' ? '=logged-out' : ''}`);
      const strip = page.locator('.statusbar.shell-strip');
      const cell = strip.locator('.cell.end');
      const control = session === 'unlocked' ? cell.locator('.presence') : cell.getByRole('button', { name: session === 'locked' ? 'Unlock' : 'Sign in' });
      await expect(control).toBeVisible();
      await page.evaluate(() => document.fonts.ready);
      for (const width of [1502, 1233, 1100, 1000, 901, 900, 899, 768, 560, 559, 320, 1233, 1502]) {
        await page.setViewportSize({ width, height: width <= 768 ? 480 : 800 });
        const header = (await strip.boundingBox())!;
        const account = (await cell.boundingBox())!;
        const action = (await control.boundingBox())!;
        const padding = await cell.evaluate(node => parseFloat(getComputedStyle(node).paddingRight));
        expect(account.x + account.width, `account cell at ${width}px`).toBeCloseTo(header.x + header.width, 0);
        expect(action.x + action.width, `account controls at ${width}px`).toBeCloseTo(header.x + header.width - padding, 0);
        expect(action.x).toBeGreaterThanOrEqual(0);
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
        if ([1502, 1233, 320].includes(width)) {
          await page.screenshot({ path: info.outputPath(`account-${theme}-${session}-${width}.png`), clip: { x: header.x, y: header.y, width: header.width, height: header.height } });
        }
      }
    });
  }
}

// Invisible while Warframe runs is the costly case: most buyers filter for
// online sellers. Invisible after the game closes is what following does.
for (const theme of ['light', 'dark'] as const) test(`${theme} playing while Invisible offers one-click Go online; following stays calm`, async ({ page }) => {
  await page.emulateMedia({ colorScheme: theme });
  await page.goto('/?preview-desktop&sample');
  const strip = page.locator('.statusbar');
  const trigger = strip.locator('.presence .trigger');
  const go = strip.getByRole('button', { name: 'Go online' });
  await expect(strip.locator('.presence .tag.auto')).toBeVisible();
  await expect(go).toHaveCount(0);
  await expect(trigger).not.toHaveAttribute('data-tone', 'warn');

  await trigger.click();
  await page.locator('#presence-menu input[value="invisible"]').check();
  await expect(trigger).toHaveAttribute('aria-busy', 'false');
  await page.keyboard.press('Escape');
  await expect(trigger).toHaveAttribute('data-tone', 'warn');
  await expect(strip.locator('.presence .tag.auto')).toHaveCount(0);
  await page.screenshot({ path: `test-results/responsive/presence-invisible-playing-${theme}.png`, clip: { x: 0, y: 0, width: page.viewportSize()!.width, height: 120 } });

  await go.click();
  await expect(trigger.locator('.current')).toHaveText(/Online in game/i);
  await expect(go).toHaveCount(0);
  await expect(trigger).not.toHaveAttribute('data-tone', 'warn');
});

test('Invisible after the game closes is expected: no warning, no Go online', async ({ page }) => {
  await page.goto('/?preview-desktop&sample&presence=closed');
  const strip = page.locator('.statusbar');
  await expect(strip.locator('.presence .trigger .current')).toHaveText(/Invisible/i);
  await expect(strip.locator('.presence .trigger')).not.toHaveAttribute('data-tone', 'warn');
  await expect(strip.locator('.presence .tag.auto')).toBeVisible();
  await expect(strip.getByRole('button', { name: 'Go online' })).toHaveCount(0);
});
