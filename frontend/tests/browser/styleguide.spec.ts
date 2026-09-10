import { expect, test } from '@playwright/test';

test('reference stays readable in both themes and narrow, short, wide windows', async ({ page }, testInfo) => {
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto('/?styleguide');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('One visual language. Room for the information.');
  for (const mode of ['Light', 'Dark']) {
    await page.getByRole('button', { name: mode, exact: true }).click();
    await expect(page.locator('html')).toHaveAttribute('data-mode', mode.toLowerCase());
    for (const width of [320, 560, 901, 1440]) {
      await page.setViewportSize({ width, height: 480 });
      const clipped = await page.evaluate(() => {
        const root = document.documentElement;
        const elements = [...document.querySelectorAll('.styleguide .rail, .styleguide .btn, .styleguide .ui-notice')];
        return {
          pageOverflow: root.scrollWidth > root.clientWidth + 1,
          contents: elements.filter(el => el.scrollWidth > el.clientWidth + 2 || el.scrollHeight > el.clientHeight + 2).map(el => el.textContent),
        };
      });
      expect(clipped).toEqual({ pageOverflow: false, contents: [] });
      const columns = await page.getByRole('columnheader').evaluateAll(headers => headers.map(header => ({
        text: header.textContent, width: header.getBoundingClientRect().width,
        clipped: header.scrollWidth > header.clientWidth + 1,
      })));
      expect(columns).toHaveLength(4);
      for (const column of columns) {
        expect(column.width, `${column.text} collapsed`).toBeGreaterThanOrEqual(64);
        expect(column.clipped, `${column.text} clipped`).toBe(false);
      }
      const region = page.getByRole('region', { name: 'Sample inventory table' });
      await region.evaluate(el => { el.scrollLeft = el.scrollWidth; });
      const bounds = await region.evaluate(el => {
        const outer = el.getBoundingClientRect();
        const last = el.querySelector('th:last-child')!.getBoundingClientRect();
        return { right: last.right, limit: outer.right };
      });
      expect(bounds.right).toBeLessThanOrEqual(bounds.limit + 1);
      await region.evaluate(el => { el.scrollLeft = 0; });
      if (width === 320 || width === 1440) {
        await page.evaluate(() => document.fonts.ready);
        if (process.platform === 'linux') {
          await expect(page.locator('.styleguide')).toHaveScreenshot(`styleguide-${mode}-${width}.png`, { animations: 'disabled', maxDiffPixelRatio: 0.002 });
        }
        const path = testInfo.outputPath(`styleguide-${mode}-${width}.png`);
        await page.screenshot({ path, fullPage: true });
        await testInfo.attach(`styleguide-${mode}-${width}`, { path, contentType: 'image/png' });
      }
    }
    const contrast = await page.evaluate(() => {
      const styles = getComputedStyle(document.documentElement);
      const luminance = (hex: string) => {
        const channels = hex.trim().replace('#', '').match(/../g)!.map(v => parseInt(v, 16) / 255)
          .map(v => v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4);
        return channels[0] * .2126 + channels[1] * .7152 + channels[2] * .0722;
      };
      const pairs = [
        ['--bad', '--panel'], ['--bad', '--panel-2'], ['--good', '--panel'], ['--warn', '--panel'],
        ['--fg', '--bg'], ['--fg', '--panel'], ['--muted', '--panel'], ['--muted', '--panel-2'],
        ['--on-accent', '--accent'], ['--on-ink', '--ink-bar'], ['--on-ink-muted', '--ink-bar'],
      ];
      return pairs.map(([fg, bg]) => {
        const a = luminance(styles.getPropertyValue(fg));
        const b = luminance(styles.getPropertyValue(bg));
        return { pair: `${fg}/${bg}`, ratio: (Math.max(a, b) + .05) / (Math.min(a, b) + .05) };
      });
    });
    for (const { pair, ratio } of contrast) expect(ratio, `${mode} ${pair}`).toBeGreaterThanOrEqual(4.5);
  }
  expect(errors).toEqual([]);
});

test('states, local filtering, modal editing, and keyboard focus are usable', async ({ page }) => {
  await page.goto('/?styleguide');
  const filter = page.getByRole('textbox', { name: 'Filter sample items' });
  await filter.fill('no-such-item');
  await expect(page.getByRole('heading', { name: 'No matching items' })).toBeVisible();
  await page.getByRole('button', { name: 'Show sample inventory' }).click();
  const state = page.getByRole('combobox', { name: 'Data state' });
  await state.selectOption('empty');
  await expect(page.getByRole('heading', { name: 'No inventory yet' })).toBeVisible();
  await state.selectOption('estimates');
  await expect(page.getByRole('heading', { name: 'Estimated opportunities' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'List on WFM', exact: true })).toBeDisabled();
  await page.getByRole('button', { name: 'Recheck protection', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('Recheck protection');
  await state.selectOption('loading');
  await expect(page.getByText('Loading sample inventory…')).toBeVisible();
  await state.selectOption('error');
  await expect(page.getByRole('alert')).toContainText('Saved items remain available');
  await expect(page.getByText('Last refresh failed', { exact: true })).toBeVisible();
  await expect(page.locator('time')).toHaveAttribute('datetime', '2026-09-08T12:00:00Z');
  await page.getByRole('button', { name: 'Retry sample load' }).click();
  await expect(page.getByRole('table')).toBeVisible();
  await page.getByRole('button', { name: 'Clear Inventory', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Clear Inventory', exact: true })).toHaveAttribute('aria-pressed', 'true');
  const open = page.getByRole('button', { name: 'Review sample listing' });
  await open.click();
  const dialog = page.getByRole('dialog');
  const quantity = dialog.getByRole('spinbutton', { name: 'Total quantity' });
  await quantity.fill('3');
  await page.setViewportSize({ width: 320, height: 480 });
  await expect(quantity).toHaveValue('3');
  const box = await dialog.boundingBox();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.y).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(320);
  expect(box!.y + box!.height).toBeLessThanOrEqual(480);
  await dialog.getByRole('button', { name: 'Confirm sample' }).focus();
  await page.keyboard.press('Tab');
  // Native dialog focus cycling may visit the document before its first field.
  await page.keyboard.press('Tab');
  expect(await page.evaluate(() => document.activeElement?.closest('dialog') !== null)).toBe(true);
  await quantity.fill('0');
  await expect(dialog.getByRole('button', { name: 'Confirm sample' })).toBeDisabled();
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await expect(dialog).not.toBeVisible();
  await expect(open).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(dialog).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(dialog).not.toBeVisible();
  await expect(open).toBeFocused();
});
