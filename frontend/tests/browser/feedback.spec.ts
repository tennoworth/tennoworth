import { expect, test } from '@playwright/test';

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} feedback is available before a scan and preserves review while resizing`, async ({ page }, info) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.goto('/?preview-desktop');
    // Before a scan, feedback sits in the header's More menu; closing the
    // dialog returns focus to the menu button, since the menu item is gone.
    const trigger = page.getByRole('button', { name: 'More ▾', exact: true });
    await trigger.click();
    await page.getByRole('button', { name: 'Send feedback', exact: true }).click();
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
  await page.getByTestId('desktop-scan').waitFor();
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

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} a failed scan is reported by category and named in the dialog`, async ({ page }, info) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.goto('/?preview-desktop');
    await page.getByTestId('desktop-scan').waitFor();
    await page.evaluate(() => {
      const w = window as any;
      const original = w.__TAURI__.core.invoke;
      // The scan's real wording: its help text mentions "network", which once
      // made this read as a connection failure.
      w.__TAURI__.core.invoke = (command: string, args: unknown) => command === 'scan_inventory'
        ? Promise.reject("memory scan failed: No accountId/nonce pair found in WF memory.\nMake sure you're past the login screen and a recent network\ncall has fired (opening the trade or profile screen is reliable).")
        : original(command, args);
    });
    await page.getByTestId('desktop-scan').click();
    await page.getByRole('button', { name: 'Report a bug', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'Send feedback' });
    const problems = dialog.getByTestId('feedback-problems');
    await expect(problems).toContainText('Scan: the game session was not found in memory.');
    const bug = dialog.getByRole('link', { name: /Report a bug/ });
    await expect(bug).toHaveAttribute('aria-disabled', 'false');
    const url = new URL((await bug.getAttribute('href'))!);
    expect(url.searchParams.get('surface')).toBe('Desktop app');
    const snapshot = JSON.parse((await dialog.locator('pre').textContent())!);
    expect(url.searchParams.get('version')).toBe(`preview (build ${snapshot.app.build})`);
    const attachments = url.searchParams.get('attachments')!;
    expect(attachments).toContain('"error": "credentials_not_found"');
    expect(attachments).not.toContain('connection_error');
    for (const [width, height] of [[1440, 900], [1440, 480], [320, 480]]) {
      await page.setViewportSize({ width, height });
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
      expect(await dialog.evaluate(node => node.scrollWidth <= node.clientWidth + 1)).toBe(true);
      await page.screenshot({ path: info.outputPath(`${theme}-problems-${width}-${height}.png`) });
    }
    await dialog.getByRole('checkbox').uncheck();
    await expect(problems).toHaveCount(0);
  });
}

test('a sign-in failure offers a bug report that carries its code, not its text', async ({ page }) => {
  await page.goto('/?preview-desktop&sample=logged-out');
  await expect(page.locator('.shell')).toBeVisible();
  await page.evaluate(() => {
    const w = window as any;
    const original = w.__TAURI__.core.invoke;
    w.__TAURI__.core.invoke = (command: string, args: unknown) => command === 'wfm_login'
      ? Promise.reject({ code: 'wfm', message: '/v2/me returned 403 Forbidden: private-token' })
      : original(command, args);
  });
  await page.getByRole('button', { name: 'logged out', exact: true }).click();
  const login = page.getByTestId('wfm-login-dialog');
  const passphrase = login.locator('input[type="password"]').nth(0);
  const confirm = login.locator('input[type="password"]').nth(1);
  const submit = login.getByRole('button', { name: 'Continue to warframe.market' });
  await passphrase.fill('a-long-enough-passphrase');
  await confirm.fill('a-different-passphrase!');
  await submit.click();
  await expect(login.getByTestId('wfm-auth-error')).toContainText('don\'t match');
  await expect(login.getByRole('button', { name: 'Report a bug' })).toHaveCount(0);

  await passphrase.fill('a-long-enough-passphrase');
  await confirm.fill('a-long-enough-passphrase');
  await submit.click();
  await expect(login.getByTestId('wfm-auth-error')).toContainText('/v2/me returned 403');
  await login.getByRole('button', { name: 'Report a bug' }).click();

  const dialog = page.getByRole('dialog', { name: 'Send feedback' });
  await expect(dialog.getByTestId('feedback-problems')).toContainText('warframe.market sign-in: warframe.market returned an error (HTTP 403).');
  const snapshot = JSON.parse((await dialog.locator('pre').textContent())!);
  expect(snapshot.wfm).toEqual({ session: 'logged_out', error: 'wfm', httpStatus: 403 });
  const bug = dialog.getByRole('link', { name: /Report a bug/ });
  await expect(bug).toHaveAttribute('aria-disabled', 'false');
  expect(await bug.getAttribute('href')).not.toContain('private-token');
  await dialog.getByRole('button', { name: 'Close', exact: true }).click();
  await expect(login).toBeVisible();
});

test('the hosted site links to a bug report for the website', async ({ page }) => {
  await page.goto('/');
  const footer = page.locator('footer.sitefoot');
  const report = new URL((await footer.getByRole('link', { name: 'Report a problem' }).getAttribute('href'))!);
  expect(report.searchParams.get('template')).toBe('bug-report.yml');
  expect(report.searchParams.get('surface')).toBe('Website');
  expect(report.searchParams.get('version')).toMatch(/^build \S+$/);
  expect(await footer.getByRole('link', { name: 'Suggest an improvement' }).getAttribute('href')).toContain('template=improvement.yml');
});
