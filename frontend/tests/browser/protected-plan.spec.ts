import { expect, test } from '@playwright/test';

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} protection reserves recipe quantities and preserves unsaved edits`, async ({ page }, info) => {
    const errors: string[] = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.emulateMedia({ colorScheme: theme });
    await page.goto('/?preview-desktop&sample=protection');
    await page.getByText('Protected selling plan · No pinned goal', { exact: true }).click();
    await page.getByRole('button', { name: 'Edit protection', exact: true }).click();
    await page.getByLabel('Pinned set goal', { exact: true }).selectOption('akbolto_prime_set');
    await page.getByLabel('Item to protect', { exact: true }).selectOption('akbolto_prime_barrel');
    await page.getByLabel('Manual copies', { exact: true }).fill('1');
    await page.getByRole('button', { name: 'Set quantity', exact: true }).click();
    for (const [width, height] of [[1440, 900], [1200, 480], [320, 480]]) {
      await page.setViewportSize({ width, height });
      await expect(page.getByLabel('Pinned set goal', { exact: true })).toHaveValue('akbolto_prime_set');
      await expect(page.getByLabel('Manual copies', { exact: true })).toHaveValue('1');
      await page.getByRole('button', { name: 'Save protection', exact: true }).scrollIntoViewIfNeeded();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1)).toBe(true);
      await page.screenshot({ path: info.outputPath(`protection-${theme}-${width}.png`) });
    }
    await page.getByRole('button', { name: 'Save protection', exact: true }).click();
    const row = page.getByRole('region', { name: 'Inventory allocation' }).locator('tbody tr').filter({ hasText: 'Akbolto Prime Barrel' });
    await expect(row.locator('td')).toHaveText(['Akbolto Prime Barrel', '5', '3', '0', '2']);
    await page.getByRole('button', { name: 'Refresh allocation', exact: true }).click();
    await expect(row.locator('td')).toHaveText(['Akbolto Prime Barrel', '5', '3', '0', '2']);
    expect(errors).toEqual([]);
  });
}

test('a failed save retains the protection draft', async ({ page }) => {
  await page.goto('/?preview-desktop&sample=protection-save-error');
  await page.getByText('Protected selling plan · No pinned goal', { exact: true }).click();
  await page.getByRole('button', { name: 'Edit protection', exact: true }).click();
  await page.getByLabel('Pinned set goal', { exact: true }).selectOption('akbolto_prime_set');
  await page.getByRole('button', { name: 'Save protection', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('Sample protection save failed');
  await expect(page.getByLabel('Pinned set goal', { exact: true })).toHaveValue('akbolto_prime_set');
});

test('unavailable current orders never imply zero listed copies', async ({ page }) => {
  await page.goto('/?preview-desktop&sample=protection-error');
  await page.getByText('Protected selling plan · No pinned goal', { exact: true }).click();
  await expect(page.getByRole('region', { name: 'Inventory allocation' })).toContainText('Unknown');
  await expect(page.getByRole('button', { name: 'Review batch', exact: true })).toBeDisabled();
});

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} scan-only opportunities preserve unknown listing quantities`, async ({ page }, info) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.goto('/?preview-desktop&sample=logged-out');
    await page.getByRole('button', { name: /^Opportunities\s/ }).click();
    const summary = page.getByRole('group', { name: 'Sell summary' });
    await expect(summary).toContainText('Estimated value');
    await expect(summary.locator('.cell').filter({ hasText: 'Opportunities' })).not.toContainText('0');
    await expect(page.getByRole('status', { name: 'Estimated guidance' })).toBeVisible();
    await expect(page.getByRole('button', { name: /^List \d+ on WFM$/ })).toHaveCount(0);
    const listButtons = page.getByRole('button', { name: 'List', exact: true });
    for (const button of await listButtons.all()) await expect(button).toBeDisabled();
    for (const [width, height] of [[1440, 900], [1200, 480], [768, 600], [320, 480]]) {
      await page.setViewportSize({ width, height });
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1)).toBe(true);
      await page.screenshot({ path: info.outputPath(`scan-estimates-${theme}-${width}.png`) });
    }
    await page.getByText('Protected selling plan · No pinned goal', { exact: true }).click();
    await expect(page.getByRole('region', { name: 'Inventory allocation' })).toContainText('Unknown');
    await page.getByRole('button', { name: 'Check WFM listings', exact: true }).click();
    await expect(page.getByRole('dialog')).toBeVisible();

    await page.goto('/?preview-desktop&sample=protection');
    await page.getByRole('button', { name: /^Sell\s/ }).click();
    await expect(summary).toContainText('Sellable');
    await expect(page.getByRole('status', { name: 'Estimated guidance' })).toHaveCount(0);

    await page.goto('/?preview-desktop&sample=protection-error');
    await page.getByRole('button', { name: /^Opportunities\s/ }).click();
    await expect(summary).toContainText('Estimated value');
    await expect(page.getByRole('status', { name: 'Estimated guidance' })).toBeVisible();
  });
}

test('protection failures hide estimates and successful listing checks enable review', async ({ page }) => {
  await page.goto('/?preview-desktop&sample=logged-out');
  await page.getByRole('button', { name: /^Opportunities\s/ }).click();
  await page.getByText('Protected selling plan · No pinned goal', { exact: true }).click();
  await page.evaluate(() => {
    const w = window as any;
    const original = w.__TAURI__.core.invoke;
    w.allocationMode = 'failure';
    w.__TAURI__.core.invoke = async (command: string, args: any) => {
      if (command === 'wfm_auth_status' && w.allocationMode === 'checked') return { logged_in: true, unlocked: true };
      if (command === 'protection_state') {
        if (w.allocationMode === 'failure') throw new Error('Protection storage could not be read');
        const state = await original(command, args);
        if (w.allocationMode === 'invalid') {
          for (const row of Object.values(state.items) as any[]) row.estimated = null;
        }
        if (w.allocationMode === 'checked') {
          for (const row of Object.values(state.items) as any[]) { row.available = row.estimated; row.listed = 0; }
          state.issues = [];
        }
        return state;
      }
      return original(command, args);
    };
  });
  const refresh = page.getByRole('button', { name: 'Refresh allocation', exact: true });
  for (const mode of ['failure', 'invalid']) {
    await page.evaluate(mode => { (window as any).allocationMode = mode; }, mode);
    await refresh.click();
    await expect(page.getByRole('group', { name: 'Sell summary' })).toContainText('—');
    await expect(page.getByRole('status', { name: 'Estimated guidance' })).toHaveCount(0);
    await expect(page.getByRole('button', { name: /^List \d+ on WFM$/ })).toHaveCount(0);
  }
  await page.evaluate(() => { (window as any).allocationMode = 'offline'; });
  await refresh.click();
  await expect(page.getByRole('status', { name: 'Estimated guidance' })).toBeVisible();
  await page.getByRole('button', { name: 'Trade Session', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Review batch', exact: true })).toBeDisabled();
  await page.getByRole('button', { name: /^Opportunities\s/ }).click();
  await page.evaluate(() => { (window as any).allocationMode = 'checked'; });
  await page.getByRole('button', { name: 'Check WFM listings', exact: true }).click();
  await expect(page.getByRole('status', { name: 'Estimated guidance' })).toHaveCount(0);
  await expect(page.getByRole('group', { name: 'Sell summary' })).toContainText('Sellable');
  await expect(page.getByRole('button', { name: /^List \d+ on WFM$/ })).toBeEnabled();
});
