import { expect, test } from '@playwright/test';

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} Trade Session preserves review edits at wide, narrow and short sizes`, async ({ page }, info) => {
    const errors: string[] = [];
    page.on('pageerror', e => errors.push(e.message));
    await page.emulateMedia({ colorScheme: theme });
    await page.goto('/?preview-desktop&sample=session');
    await page.locator('.sidebar').getByRole('button', { name: 'Trade Session', exact: true }).click();
    await expect(page.getByLabel('Trade budget')).toHaveValue('8');
    for (const intent of ['Fast Cash', 'Plat per Trade', 'Clear Inventory', 'Max Value']) {
      await page.getByRole('button', { name: new RegExp('^' + intent) }).click();
      await expect(page.getByRole('button', { name: new RegExp('^' + intent) })).toHaveAttribute('aria-pressed', 'true');
      await expect(page.getByRole('button', { name: 'Review batch', exact: true })).toBeEnabled();
    }
    await page.getByRole('button', { name: /^Fast Cash/ }).click();
    await page.getByRole('button', { name: 'Review batch', exact: true }).click();
    const dialog = page.getByRole('dialog');
    await expect(dialog.getByText('Update visible order')).toBeVisible();
    const flow = dialog.locator('tbody tr').filter({ hasText: 'Primed Flow' });
    const price = flow.locator('input[type=number]').nth(2);
    await price.fill('99');
    await dialog.getByRole('button', { name: 'Refresh existing orders' }).click();
    await expect(price).toHaveValue('99');
    for (const [width, height] of [[1440, 900], [768, 600], [320, 480]]) {
      await page.setViewportSize({ width, height });
      await expect(price).toHaveValue('99');
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1)).toBe(true);
      await dialog.locator('.scroll').evaluate(el => { el.scrollLeft = el.scrollWidth; });
      await page.screenshot({ path: info.outputPath(`review-${theme}-${width}.png`) });
    }
    await page.keyboard.press('Escape');
    await expect(dialog).not.toBeVisible();
    for (const [width, height] of [[1440, 900], [768, 600], [320, 480]]) {
      await page.setViewportSize({ width, height });
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1)).toBe(true);
      const region = page.getByRole('region', { name: /^Suggested listings/ });
      await region.focus();
      await region.evaluate(el => el.scrollIntoView({ block: 'start' }));
      const header = await page.locator('.statusbar').boundingBox();
      const table = await region.boundingBox();
      expect(table!.y).toBeGreaterThanOrEqual(header!.y + header!.height - 1);
      await region.evaluate(el => { el.scrollLeft = el.scrollWidth; });
      await page.screenshot({ path: info.outputPath(`session-${theme}-${width}.png`) });
    }
    expect(errors).toEqual([]);
  });
}

test('changed existing orders require another confirmation without losing edits', async ({ page }) => {
  await page.goto('/?preview-desktop&sample=session');
  await page.locator('.sidebar').getByRole('button', { name: 'Trade Session', exact: true }).click();
  await page.getByRole('button', { name: 'Review batch', exact: true }).click();
  const dialog = page.getByRole('dialog');
  await expect(dialog.getByText('Update visible order')).toBeVisible();
  await page.evaluate(() => {
    const runtime = (window as unknown as { __TAURI__: { core: { invoke: (cmd: string, args?: unknown) => Promise<any> } } }).__TAURI__;
    const original = runtime.core.invoke;
    (window as any).submittedSessionPlans = [];
    runtime.core.invoke = async (cmd, args) => {
      if (cmd === 'submit_plan') (window as any).submittedSessionPlans.push(args);
      const response = await original(cmd, args);
      if (cmd === 'fetch_orders') response.data.sell.find((o: any) => o.id === 'preview-flow').platinum = 27;
      return response;
    };
  });
  const price = dialog.locator('tbody tr').filter({ hasText: 'Primed Flow' }).locator('input[type=number]').nth(2);
  await price.fill('99');
  await dialog.getByRole('button', { name: /^Send \d+ listings/ }).click();
  await expect(dialog.getByRole('alert')).toContainText('Existing orders changed');
  await expect(price).toHaveValue('99');
  expect(await page.evaluate(() => (window as any).submittedSessionPlans.length)).toBe(0);
  await dialog.getByRole('button', { name: /^Send \d+ listings/ }).click();
  await expect.poll(() => page.evaluate(() => (window as any).submittedSessionPlans.length)).toBe(1);
  const items = await page.evaluate(() => (window as any).submittedSessionPlans[0].items);
  expect(items.every((i: any) => i.session.budget === 8 && i.per_trade === 1 && i.visible === false)).toBe(true);
  expect(items.find((i: any) => i.slug === 'primed_flow').reviewed_order).toMatchObject({ state: 'existing', visible: true, platinum: 27 });
});

test('a smaller safe quantity blocks sending and retains the entered price', async ({ page }) => {
  await page.goto('/?preview-desktop&sample=session');
  await page.locator('.sidebar').getByRole('button', { name: 'Trade Session', exact: true }).click();
  await page.getByRole('button', { name: 'Review batch', exact: true }).click();
  const dialog = page.getByRole('dialog');
  await expect(dialog.getByText('Update visible order')).toBeVisible();
  await page.evaluate(() => {
    const runtime = (window as any).__TAURI__;
    const original = runtime.core.invoke;
    runtime.core.invoke = async (cmd: string, args: unknown) => {
      const response = await original(cmd, args);
      if (cmd === 'trade_session_state') response.quantities.primed_flow = 0;
      return response;
    };
  });
  const price = dialog.locator('tbody tr').filter({ hasText: 'Primed Flow' }).locator('input[type=number]').nth(2);
  await price.fill('99');
  await dialog.getByRole('button', { name: 'Refresh existing orders' }).click();
  await expect(dialog.getByText(/A selected quantity exceeds/)).toBeVisible();
  await expect(price).toHaveValue('99');
  await expect(dialog.getByRole('button', { name: /^Send \d+ listings/ })).toBeDisabled();
});

test('bulk review separates unit asks from existing and proposed lot totals', async ({ page }) => {
  await page.goto('/?preview-desktop&sample=session');
  await page.getByRole('button', { name: /^Plat per Trade/ }).click();
  await page.getByRole('button', { name: 'Review batch', exact: true }).click();
  const dialog = page.getByRole('dialog');
  const arcane = dialog.locator('tbody tr').filter({ hasText: 'Arcane Energize' });
  await expect(arcane).toContainText('Update hidden order');
  await expect(arcane).toContainText('8p');
  await expect(arcane).toContainText('48p');
  await arcane.locator('input[type=number]').nth(2).fill('9');
  await expect(arcane).toContainText('54p');
  await page.evaluate(() => {
    const runtime = (window as any).__TAURI__;
    const original = runtime.core.invoke;
    (window as any).submittedSessionPlans = [];
    runtime.core.invoke = async (cmd: string, args: unknown) => {
      if (cmd === 'submit_plan') (window as any).submittedSessionPlans.push(args);
      return original(cmd, args);
    };
  });
  await dialog.getByRole('button', { name: /^Send \d+ listings/ }).click();
  await expect.poll(() => page.evaluate(() => (window as any).submittedSessionPlans.length)).toBe(1);
  const items = await page.evaluate(() => (window as any).submittedSessionPlans[0].items);
  expect(items.find((i: any) => i.slug === 'arcane_energize')).toMatchObject({
    platinum: 9, per_trade: 6,
    reviewed_order: { state: 'existing', platinum: 48, per_trade: 6, visible: false },
  });
});

for (const scenario of ['zero-trades', 'unknown-trades']) {
  test(`${scenario} cannot hand off a current-day batch`, async ({ page }) => {
    await page.goto(`/?preview-desktop&sample=${scenario}`);
    await page.locator('.sidebar').getByRole('button', { name: 'Trade Session', exact: true }).click();
    await expect(page.getByRole('button', { name: 'Review batch', exact: true })).toBeDisabled();
    await expect(page.getByText(scenario === 'zero-trades' ? /No trades remaining/ : 'Trade allowance unknown', { exact: scenario !== 'zero-trades' })).toBeVisible();
  });
}
