import { expect, test, type Page } from '@playwright/test';

async function recoveryCommands(page: Page) {
  await page.evaluate(() => {
    const w = window as any;
    const original = w.__TAURI__.core.invoke;
    w.recovery = { installs: 0, restarts: 0, scan: 'error', update: 'available' };
    w.__TAURI__.core.invoke = (command: string, args: any) => {
      const state = w.recovery;
      if (command === 'scan_inventory') return Promise.resolve('{}');
      if (command === 'evaluate_domain' && args.request.operation === 'normalize_inventory') {
        if (state.scan === 'error') return Promise.reject('The calculation contains an out-of-range number.');
        return Promise.resolve({ operation: 'normalize_inventory', result: { owned: [
          ['pyrana_prime_set', { slug: 'pyrana_prime_set', name: 'Pyrana Prime Set', type: 'Weapon', count: 3, leveled: 0, kept_lvl: null, subtype: null }],
        ], flat_count: 1, unresolved: {} } });
      }
      if (command === 'check_update') {
        if (state.update === 'error') return Promise.reject('Could not reach the update service');
        return Promise.resolve({ checked: true, available: state.update === 'available', support: state.update === 'unsupported' ? 'appimage_required' : 'supported', current_version: 'preview', version: 'next-preview', notes: null });
      }
      if (command === 'install_update') {
        state.installs++;
        return new Promise((resolve, reject) => { state.installResolve = resolve; state.installReject = reject; });
      }
      if (command === 'restart_app') { state.restarts++; return Promise.reject('Restart failed; close and reopen TennoWorth.'); }
      return original(command, args);
    };
  });
}

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} recovery keeps Settings, updates, feedback and market accessible without a successful scan`, async ({ page }, info) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.goto('/?preview-desktop');
    await page.getByTestId('market-browser').waitFor();
    await recoveryCommands(page);
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await expect(page.getByRole('heading', { name: 'Settings', exact: true })).toBeVisible();
    await expect.poll(() => page.evaluate(() => parseFloat(document.documentElement.style.getPropertyValue('--sticky-header-clearance')))).toBeGreaterThan(0);
    await page.locator('.sidebar').getByRole('button', { name: 'Inventory', exact: true }).click();
    await page.getByTestId('desktop-scan').click();
    const error = page.getByRole('alert', { name: 'Inventory unavailable' });
    await expect(error).toContainText('Settings and help are still available');
    await error.getByText('Technical details', { exact: true }).click();
    await expect(error).toContainText('out-of-range number');
    await expect(page.getByTestId('market-browser')).toBeVisible();
    for (const [width, height] of [[1440, 900], [1440, 480], [760, 600], [320, 480]]) {
      await page.setViewportSize({ width, height });
      await error.getByRole('button', { name: 'Retry scan' }).scrollIntoViewIfNeeded();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
      expect(await error.evaluate(node => node.scrollWidth <= node.clientWidth + 1)).toBe(true);
      await page.screenshot({ path: info.outputPath(`${theme}-error-${width}-${height}.png`) });
    }
    await error.getByRole('button', { name: 'Report a bug' }).click();
    const feedback = page.getByRole('dialog', { name: 'Send feedback' });
    await feedback.getByText('Review app-state snapshot', { exact: true }).click();
    await expect(feedback.locator('pre')).toContainText('"phase": "error"');
    await page.keyboard.press('Escape');
    await expect(error.getByRole('button', { name: 'Report a bug' })).toBeFocused();
    await error.getByRole('button', { name: 'Check for updates' }).click();
    const update = page.getByTestId('update-banner');
    await expect(update).toContainText('next-preview');
    await error.getByRole('button', { name: 'Settings', exact: true }).click();
    await expect(update).toContainText('next-preview');
    await page.locator('.sidebar').getByRole('button', { name: 'Inventory', exact: true }).click();
    await page.evaluate(() => { (window as any).recovery.scan = 'success'; });
    await error.getByRole('button', { name: 'Retry scan' }).click();
    await expect(page.locator('.shell')).toBeVisible();
    await expect(error).toHaveCount(0);
    await expect(page.locator('.statusbar')).toContainText('inventory (from game)');
    await expect(update).toContainText('next-preview');
  });

  test(`${theme} update installation and restart failures survive navigation before a scan`, async ({ page }, info) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.goto('/?preview-desktop');
    await page.getByTestId('desktop-scan').waitFor();
    await recoveryCommands(page);
    await page.getByRole('button', { name: 'Check for updates', exact: true }).click();
    const update = page.getByTestId('update-banner');
    await update.getByRole('button', { name: 'Install update', exact: true }).click();
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await expect(update).toContainText('Installing update');
    await page.evaluate(() => (window as any).recovery.installReject('Download failed. Try again.'));
    await expect(update).toContainText('Download failed');
    await page.locator('.sidebar').getByRole('button', { name: 'Inventory', exact: true }).click();
    await expect(update).toContainText('Download failed');
    await update.getByRole('button', { name: 'Install update', exact: true }).click();
    await page.evaluate(() => (window as any).recovery.installResolve());
    await expect(update).toContainText('Update installed');
    await update.getByRole('button', { name: 'Dismiss update notice' }).click();
    await expect(update).toHaveCount(0);
    await page.getByRole('button', { name: 'Check for updates', exact: true }).click();
    await expect(update).toContainText('Update installed');
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await update.getByRole('button', { name: 'Restart now' }).click();
    await expect(update).toContainText('Restart failed');
    await page.setViewportSize({ width: 320, height: 480 });
    await page.evaluate(() => { document.documentElement.style.zoom = '2'; });
    await update.getByRole('button', { name: 'Restart now' }).click();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
    await page.screenshot({ path: info.outputPath(`${theme}-update-zoom.png`) });
    expect(await page.evaluate(() => (window as any).recovery.installs)).toBe(2);
    expect(await page.evaluate(() => (window as any).recovery.restarts)).toBe(2);
  });
}

test('a failed replacement preserves the visible saved inventory, name and age', async ({ page }) => {
  await page.goto('/?preview-desktop&sample');
  await page.locator('.shell').waitFor();
  await expect(page.locator('.statusbar .file')).toContainText('Sample inventory');
  const identity = await page.locator('.statusbar .file').innerText();
  const timestamp = await page.locator('.statusbar .inv time').getAttribute('datetime');
  await recoveryCommands(page);
  await page.getByRole('button', { name: 'Refresh ▾', exact: true }).click();
  await page.getByTestId('desktop-scan').click();
  await expect(page.getByRole('alert', { name: 'Inventory unavailable' })).toContainText('Showing your last successful inventory');
  await expect(page.locator('.shell')).toBeVisible();
  expect(await page.locator('.statusbar .file').innerText()).toBe(identity);
  await expect(page.locator('.statusbar .inv time')).toHaveAttribute('datetime', timestamp!);
  await expect(page.locator('.statusbar .inv')).toContainText('Last refresh failed');
  await expect(page.locator('.workspace')).toContainText('Pyrana Prime Set');
  await page.locator('.sidebar').getByRole('button', { name: 'Settings', exact: true }).click();
  await page.getByRole('button', { name: 'Send feedback', exact: true }).click();
  const feedback = page.getByRole('dialog', { name: 'Send feedback' });
  await feedback.getByText('Review app-state snapshot', { exact: true }).click();
  await expect(feedback.locator('pre')).toContainText('"screen": "settings"');
  await expect(feedback.locator('pre')).toContainText('"phase": "error"');
});

test('manual checks expose failed and unsupported outcomes; Notifications never marks an inventory loaded', async ({ page }) => {
  await page.goto('/?preview-desktop');
  await page.getByTestId('desktop-scan').waitFor();
  await recoveryCommands(page);
  await page.evaluate(() => { (window as any).recovery.update = 'error'; });
  await page.getByRole('button', { name: 'Check for updates', exact: true }).click();
  const update = page.getByTestId('update-banner');
  await expect(update).toContainText('Could not reach the update service');
  await page.evaluate(() => { (window as any).recovery.update = 'unsupported'; });
  await update.getByRole('button', { name: 'Check again' }).click();
  await expect(update).toContainText('Download and run the TennoWorth AppImage');
  await page.getByRole('button', { name: 'Notifications', exact: true }).click();
  await page.getByRole('button', { name: 'Send feedback', exact: true }).click();
  const feedback = page.getByRole('dialog', { name: 'Send feedback' });
  await feedback.getByText('Review app-state snapshot', { exact: true }).click();
  await expect(feedback.locator('pre')).toContainText('"phase": "idle"');
  await page.keyboard.press('Escape');
  await page.locator('.sidebar').getByRole('button', { name: 'My orders', exact: true }).click();
  await expect(page.locator('.shell')).toBeVisible();
  await page.locator('.sidebar').getByRole('button', { name: 'Inventory', exact: true }).click();
  await expect(page.getByTestId('desktop-scan')).toBeVisible();
  await expect(update).toContainText('Download and run the TennoWorth AppImage');
});

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} snapshot age advances and failed refresh remains visible after dismissing details`, async ({ page }, info) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.clock.install({ time: new Date('2026-09-10T12:00:00Z') });
    await page.goto('/?preview-desktop&sample');
    const inventory = page.locator('.statusbar .inv');
    await expect(inventory.locator('time')).toContainText('just now');
    const timestamp = await inventory.locator('time').getAttribute('datetime');
    await page.clock.setSystemTime(new Date('2026-09-12T12:00:00Z'));
    await page.clock.runFor(60_000);
    await expect(inventory.locator('time')).toContainText('2 d ago');
    await page.clock.setSystemTime(new Date('2026-09-13T12:00:00Z'));
    await page.evaluate(() => window.dispatchEvent(new Event('focus')));
    await expect(inventory.locator('time')).toContainText('3 d ago');
    await recoveryCommands(page);
    await page.getByRole('button', { name: 'Refresh ▾', exact: true }).click();
    await page.getByTestId('desktop-scan').click();
    const error = page.getByRole('alert', { name: 'Inventory unavailable' });
    await expect(error).toBeVisible();
    await error.getByRole('button', { name: 'Dismiss scan error' }).click();
    await expect(error).toHaveCount(0);
    await expect(inventory).toContainText('Last refresh failed');
    await expect(inventory.locator('time')).toHaveAttribute('datetime', timestamp!);
    for (const [width, height] of [[1440,900],[1440,480],[760,600],[320,480]]) {
      await page.setViewportSize({ width, height });
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
      await page.getByRole('button', { name: 'Refresh ▾', exact: true }).scrollIntoViewIfNeeded();
      await page.screenshot({ path: info.outputPath(`${theme}-saved-${width}-${height}.png`) });
    }
    await page.evaluate(() => { (window as any).recovery.scan = 'success'; });
    await page.getByRole('button', { name: 'Refresh ▾', exact: true }).click();
    await page.getByTestId('desktop-scan').click();
    await expect(inventory).not.toContainText('Last refresh failed');
    await expect(inventory.locator('time')).toContainText('just now');
    await page.getByRole('button', { name: 'Refresh ▾', exact: true }).click();
    await page.getByRole('button', { name: 'Clear', exact: true }).click();
    await expect(inventory).toContainText('No inventory yet');
    await expect(inventory.locator('time')).toHaveCount(0);
  });
}
