import { encryptPayload } from '../../src/adapters/encrypted-snapshot';
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

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} imported estimates use the restored count and cannot inherit scan authorization`, async ({ page }, info) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.goto('/?preview-desktop&sample=logged-out');
    await page.getByText('Protected selling plan · No pinned goal', { exact: true }).click();
    await page.getByRole('button', { name: 'Edit protection', exact: true }).click();
    await page.getByLabel('Item to protect', { exact: true }).selectOption('primed_flow');
    await page.getByLabel('Manual copies', { exact: true }).fill('5');
    await page.getByRole('button', { name: 'Set quantity', exact: true }).click();
    await page.getByRole('button', { name: 'Save protection', exact: true }).click();
    const backup = await encryptPayload({ invName: 'One copy restored', ts: 1_780_000_000_000, owned: [['primed_flow', { slug: 'primed_flow', name: 'Primed Flow', count: 1, leveled: 0, type: 'RawUpgrades', kept_lvl: null, subtype: null }]], nativeSnapshotId: 1 }, 'review-test-passphrase');
    await page.getByRole('button', { name: 'Refresh ▾', exact: true }).click();
    const chooser = page.waitForEvent('filechooser');
    await page.getByRole('button', { name: 'Restore…', exact: true }).click();
    await (await chooser).setFiles({ name: 'inventory.json', mimeType: 'application/json', buffer: Buffer.from(JSON.stringify(backup)) });
    const dialog = page.getByRole('dialog').filter({ has: page.getByRole('heading', { name: 'Restore encrypted snapshot' }) });
    await dialog.getByLabel('Passphrase', { exact: true }).fill('review-test-passphrase');
    await dialog.getByRole('button', { name: 'Review restore', exact: true }).click();
    await dialog.getByRole('button', { name: 'Confirm restore', exact: true }).click();
    await expect(dialog).not.toBeVisible();
    const summary = page.getByRole('group', { name: 'Sell summary' });
    await expect(summary.locator('.cell').filter({ hasText: 'Opportunities' })).toContainText('0');
    await expect(summary.locator('.cell').filter({ hasText: 'Estimated value' })).toContainText('0');
    await expect(page.getByRole('region', { name: 'Inventory allocation' }).locator('tbody tr').filter({ hasText: 'Primed Flow' }).locator('td')).toHaveText(['Primed Flow', '1', '5', 'Unknown', 'Unknown']);
    await expect(page.getByRole('button', { name: /^List \d+ on WFM$/ })).toHaveCount(0);
    const stored = await page.evaluate(async () => JSON.parse(await (window as any).__TAURI__.core.invoke('get_setting', { key: 'last-owned-v2' })));
    expect(stored.nativeSnapshotId).toBeNull();
    await expect(page.getByRole('button', { name: 'Scan game', exact: true })).toBeVisible();
    for (const [width, height] of [[1440, 900], [1200, 480], [320, 480]]) {
      await page.setViewportSize({ width, height });
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
      await summary.scrollIntoViewIfNeeded();
      await page.screenshot({ path: info.outputPath(`import-protection-${theme}-${width}.png`) });
    }
  });
}

test('missing allocation keeps known estimates while unavailable sets and scrap stay explicit', async ({ page }) => {
  await page.goto('/?preview-desktop&sample=logged-out');
  await page.getByText('Protected selling plan · No pinned goal', { exact: true }).click();
  await page.evaluate(() => {
    const w = window as any;
    const original = w.__TAURI__.core.invoke;
    w.missingAll = false;
    w.__TAURI__.core.invoke = async (command: string, args: any) => {
      const result = await original(command, args);
      if (command === 'protection_state') {
        if (w.missingAll) result.items = {};
        else delete result.items.primed_flow;
      }
      return result;
    };
  });
  await page.getByRole('button', { name: 'Refresh allocation', exact: true }).click();
  await expect(page.getByText('Quantities unavailable for 1 item.', { exact: false })).toBeVisible();
  await expect(page.getByRole('group', { name: 'Sell summary' })).toContainText('Known estimated value');
  await expect(page.getByRole('group', { name: 'Sell summary' }).locator('.cell').filter({ hasText: 'Opportunities' })).not.toContainText('0');
  await page.evaluate(() => { (window as any).missingAll = true; });
  await page.getByRole('button', { name: 'Refresh allocation', exact: true }).click();
  await expect(page.getByRole('group', { name: 'Sell summary' })).toContainText('—');
  await page.getByRole('button', { name: /^Set picks/ }).click();
  await expect(page.getByText('Set quantities unavailable. Recheck inventory protection.')).toBeVisible();
  await page.getByRole('button', { name: 'Baro', exact: true }).click();
  await expect(page.getByText(/Scrap quantities unavailable/)).toBeVisible();
  await expect(page.getByText(/Scrapping every spare/)).toHaveCount(0);
});

test('a changed native scan blocks an open review and retains its edits after rescanning', async ({ page }) => {
  await page.clock.install();
  await page.goto('/?preview-desktop&sample');
  await page.getByRole('button', { name: /^List \d+ on WFM$/ }).click();
  const dialog = page.getByRole('dialog');
  const price = dialog.locator('tbody tr').first().locator('input[type=number]').nth(1);
  await price.fill('99');
  await page.evaluate(async () => {
    const w = window as any;
    const original = w.__TAURI__.core.invoke;
    const stored = JSON.parse(await original('get_setting', { key: 'last-owned-v2' }));
    w.rescanned = false;
    w.__TAURI__.core.invoke = async (command: string, args: any) => {
      if (command === 'scan_inventory') { w.rescanned = true; return { inventory: '{}', snapshot_id: 2 }; }
      if (command === 'evaluate_domain' && args.request.operation === 'normalize_inventory') return { operation: 'normalize_inventory', result: { owned: stored.owned, flat_count: stored.owned.length, unresolved: {} } };
      const result = await original(command, args);
      if (command === 'protection_state') {
        result.snapshot_id = w.rescanned ? 2 : null;
        if (w.rescanned) for (const row of Object.values(result.items) as any[]) { row.available = row.estimated; row.listed = 0; }
      }
      return result;
    };
  });
  await page.clock.runFor(30_001);
  await expect(dialog.getByRole('button', { name: /^Send \d+ listings$/ })).toBeDisabled();
  await expect(price).toHaveValue('99');
  await dialog.getByRole('button', { name: 'Scan game', exact: true }).click();
  await expect(dialog.getByText(/Inventory changed\. Close review/)).toBeVisible();
  await expect(dialog.getByRole('button', { name: /^Send \d+ listings$/ })).toBeDisabled();
  await expect(price).toHaveValue('99');
});

test('logout blocks Trade Session review until current listings are rechecked', async ({ page }) => {
  await page.goto('/?preview-desktop&sample=session');
  await expect(page.getByRole('button', { name: 'Review batch', exact: true })).toBeEnabled();
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await page.getByRole('button', { name: 'Log out', exact: true }).click();
  await page.getByRole('button', { name: 'Confirm log out', exact: true }).click();
  await page.getByRole('button', { name: 'Trade Session', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Review batch', exact: true })).toBeDisabled();
  await expect(page.getByText('Connect WFM to check current listings before posting.', { exact: false })).toBeVisible();
  await page.evaluate(() => {
    const w = window as any;
    const original = w.__TAURI__.core.invoke;
    w.__TAURI__.core.invoke = async (command: string, args: any) => {
      if (command === 'wfm_auth_status') return { logged_in: true, unlocked: true };
      const result = await original(command, args);
      if (command === 'protection_state') for (const row of Object.values(result.items) as any[]) { row.listed = 0; row.available = row.estimated; }
      return result;
    };
  });
  await page.getByRole('button', { name: 'Check WFM listings', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Review batch', exact: true })).toBeEnabled();
});
