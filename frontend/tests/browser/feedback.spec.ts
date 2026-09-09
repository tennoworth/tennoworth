import { expect, test } from '@playwright/test';

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} feedback is available before a scan and preserves review while resizing`, async ({ page }, info) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.goto('/?preview-desktop');
    const trigger = page.getByRole('button', { name: 'Send feedback', exact: true });
    await trigger.click();
    const dialog = page.getByRole('dialog', { name: 'Send feedback' });
    await expect(dialog.getByRole('status')).toHaveCount(0);
    await dialog.getByText('Review app-state snapshot', { exact: true }).click();
    for (const [width, height] of [[1440, 900], [1440, 480], [760, 600], [320, 480]]) {
      await page.setViewportSize({ width, height });
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
      expect(await dialog.evaluate(node => node.scrollWidth <= node.clientWidth + 1)).toBe(true);
      await expect(dialog.locator('details')).toHaveAttribute('open', '');
      await page.screenshot({ path: info.outputPath(`${theme}-${width}-${height}.png`) });
    }
    const snapshot = JSON.parse(await dialog.locator('pre').innerText());
    expect(snapshot.screen).toBe('landing');
    expect(snapshot.inventory.phase).toBe('idle');
    const url = new URL((await dialog.getByRole('link', { name: /Report a bug/ }).getAttribute('href'))!);
    expect(url.searchParams.get('attachments')).toContain(JSON.stringify(snapshot, null, 2));
    const downloadEvent = page.waitForEvent('download');
    await dialog.getByRole('button', { name: 'Download diagnostics' }).click();
    const download = await downloadEvent;
    const stream = await download.createReadStream();
    const chunks = []; for await (const chunk of stream!) chunks.push(chunk);
    expect(JSON.parse(Buffer.concat(chunks).toString())).toEqual(snapshot);
    await dialog.getByRole('checkbox').uncheck();
    const bare = new URL((await dialog.getByRole('link', { name: /Report a bug/ }).getAttribute('href'))!);
    expect(bare.searchParams.has('attachments')).toBe(false);
    await page.keyboard.press('Escape');
    await expect(dialog).not.toBeVisible();
    await expect(trigger).toBeFocused();
  });
}

test('failed scans use the same feedback form and GitHub handoff; unavailable metadata does not block it', async ({ page }) => {
  await page.goto('/?preview-desktop');
  await page.getByRole('button', { name: 'Send feedback', exact: true }).waitFor();
  await page.evaluate(() => {
    const w = window as any;
    const original = w.__TAURI__.core.invoke;
    w.__TAURI__.core.invoke = (command: string, args: unknown) => {
      if (command === 'scan_inventory') return Promise.reject('The calculation contains an out-of-range number. private-token');
      if (command === 'update_status') return new Promise(() => {});
      if (command === 'open_external_url') { w.feedbackOpenedUrl = (args as any).url; return Promise.resolve(true); }
      return original(command, args);
    };
  });
  await page.getByTestId('desktop-scan').click();
  await page.getByRole('button', { name: 'Report a bug', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: 'Send feedback' });
  const bug = dialog.getByRole('link', { name: /Report a bug/ });
  await expect(bug).toHaveAttribute('aria-disabled', 'false');
  const href = (await bug.getAttribute('href'))!;
  expect(href).not.toContain('private-token');
  expect(new URL(href).searchParams.get('attachments')).toContain('numeric_out_of_range');
  await bug.focus(); await page.keyboard.press('Enter');
  await expect.poll(() => page.evaluate(() => (window as any).feedbackOpenedUrl)).toBe(href);
  await dialog.getByRole('link', { name: /Suggest an improvement/ }).click();
  await expect.poll(() => page.evaluate(() => (window as any).feedbackOpenedUrl)).toContain('template=improvement.yml');
});

test('loaded inventory feedback retains update failures and offers a link if the browser cannot open', async ({ page }) => {
  await page.goto('/?preview-desktop&sample=feedback-update-error');
  await expect(page.locator('.shell')).toBeVisible();
  await page.evaluate(() => {
    const w = window as any;
    const original = w.__TAURI__.core.invoke;
    w.__TAURI__.core.invoke = (command: string, args: unknown) => {
      if (command === 'install_update') return Promise.reject('404 Not Found https://private-token');
      if (command === 'open_external_url') return Promise.resolve(false);
      return original(command, args);
    };
  });
  await page.getByRole('button', { name: 'Install update', exact: true }).click();
  await expect(page.getByTestId('update-banner')).toContainText('404 Not Found');
  await page.getByRole('button', { name: 'Send feedback', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: 'Send feedback' });
  const bug = dialog.getByRole('link', { name: /Report a bug/ });
  await expect(bug).toHaveAttribute('aria-disabled', 'false');
  const url = new URL((await bug.getAttribute('href'))!);
  expect(url.searchParams.get('attachments')).toContain('not_found');
  expect(url.searchParams.get('attachments')).toContain('"operation": "install"');
  expect(url.searchParams.get('attachments')).toContain('"screen": "sell"');
  expect(url.href).not.toContain('private-token');
  await bug.click();
  await expect(dialog.getByRole('alert')).toContainText('Couldn’t open your browser');
  await expect(dialog.getByRole('textbox', { name: 'GitHub issue link' })).toHaveValue(url.href);
  await page.evaluate(() => { document.documentElement.style.fontSize = '200%'; });
  expect(await dialog.evaluate(node => node.scrollWidth <= node.clientWidth + 1)).toBe(true);
  await dialog.getByRole('button', { name: 'Close', exact: true }).click();
  await expect(dialog).not.toBeVisible();
});
