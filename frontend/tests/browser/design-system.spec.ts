import { expect, test } from '@playwright/test';
import rewardFixture from '../../../tests/fixtures/relic-ocr/result.json' with { type: 'json' };

const views = ['Sell', 'Trade Session', 'Set picks', 'Relics', 'Rivens', 'Baro', 'Routines', 'Meta Drift', 'My orders', 'Price watches', 'Ledger', 'FAQ', 'Settings'];

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} migrated screens retain content and shared control targets`, async ({ page }, testInfo) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.goto('/?preview-desktop&sample');
    await expect(page.locator('.shell')).toBeVisible();
    for (const view of views) {
      await page.locator('.sidebar').getByRole('button', { name: new RegExp('^' + view) }).click();
      for (const width of [1200, 320]) {
        await page.setViewportSize({ width, height: 600 });
        if (view === 'Sell' && width === 1200) {
          const header = await page.locator('.picks-head').boundingBox();
          const toggle = await page.locator('.picks-head .rail-toggle').boundingBox();
          const inset = await page.evaluate(() => parseFloat(getComputedStyle(document.documentElement).fontSize) / 2);
          expect(header!.height, 'Top Picks stays a compact single-control rail').toBeLessThanOrEqual(toggle!.height + inset + 1);
          await expect(page.locator('.picks-title .picks-count')).toContainText('2 picks');
        }
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1)).toBe(true);
        const small = await page.locator('.btn:visible').evaluateAll(buttons => buttons.filter(button => {
          const box = button.getBoundingClientRect();
          return box.width < 23.5 || box.height < 23.5;
        }).map(button => button.textContent));
        expect(small, `${view} shared control targets at ${width}`).toEqual([]);
        const collapsed = await page.locator('main table:visible th').evaluateAll(headers => headers.filter(header => {
          return header.textContent?.trim() && header.getBoundingClientRect().width < 20;
        }).map(header => header.textContent));
        expect(collapsed, `${view} column headings at ${width}`).toEqual([]);
        await page.screenshot({ path: testInfo.outputPath(`${view.replaceAll(' ', '-')}-${theme}-${width}.png`) });
      }
    }
  });
}

test('theme radio navigation and listing-review focus stay keyboard accessible', async ({ page }) => {
  await page.goto('/?preview-desktop&sample');
  await page.locator('.sidebar').getByRole('button', { name: /^Settings/ }).click();
  const modes = page.getByRole('radiogroup', { name: 'Colour mode' });
  await modes.getByRole('radio', { name: 'Light', exact: true }).click();
  await page.keyboard.press('ArrowRight');
  await expect(modes.getByRole('radio', { name: 'Dark', exact: true })).toBeFocused();
  await expect(page.locator('html')).toHaveAttribute('data-mode', 'dark');
  await page.keyboard.press('Home');
  await expect(modes.getByRole('radio', { name: 'Light', exact: true })).toHaveAttribute('aria-checked', 'true');
  await page.locator('.sidebar').getByRole('button', { name: /^Sell/ }).click();
  const trigger = page.getByRole('button', { name: /^List \d+ on WFM$/ });
  await trigger.click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole('button', { name: 'Close', exact: true })).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  expect(await dialog.evaluate(node => node.contains(document.activeElement))).toBe(true);
  await page.keyboard.press('Tab');
  await expect(dialog.getByRole('button', { name: 'Close', exact: true })).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(dialog).not.toBeVisible();
  await expect(trigger).toBeFocused();
});

test('reward overlay keeps complete names and scaled cards inside their slots', async ({ page }, testInfo) => {
  await page.goto('/?preview-desktop&surface=relic-overlay');
  await page.waitForFunction(() => typeof window.__TENNOWORTH_RELIC_OVERLAY_UPDATE__ === 'function');
  for (const width of [1280, 1920]) {
    await page.setViewportSize({ width, height: 720 });
    for (const scale of [1, 1.5]) {
      await page.evaluate(({ fixture, scale }) => window.__TENNOWORTH_RELIC_OVERLAY_UPDATE__?.({ ...fixture, scale }), { fixture: rewardFixture, scale });
      await expect(page.locator('.reward')).toHaveCount(3);
      const failures = await page.locator('.reward').evaluateAll(cards => cards.filter(card => {
        const slot = card.getBoundingClientRect();
        const inner = card.querySelector('.inner')!.getBoundingClientRect();
        const name = card.querySelector('.name')!;
        return inner.left < slot.left - 1 || inner.right > slot.right + 1 || inner.bottom > innerHeight + 1
          || name.scrollWidth > name.clientWidth + 1 || name.scrollHeight > name.clientHeight + 1;
      }).map(card => card.textContent));
      expect(failures).toEqual([]);
      await expect(page.locator('.overlay')).toHaveCSS('pointer-events', 'none');
      await expect(page.locator('body')).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
      await page.screenshot({ path: testInfo.outputPath(`reward-${width}-${scale}.png`) });
    }
  }
  await page.goto('/');
  await expect(page.locator('html')).not.toHaveClass(/relic-overlay-surface/);
  await expect(page.locator('body')).not.toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
});

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} settings groups keep aligned controls and preserve edits on resize`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.goto('/?preview-desktop&sample');
    await page.locator('.sidebar').getByRole('button', { name: /^Settings/ }).click();
    const settings = page.locator('.settings');
    const recognition = settings.getByRole('checkbox', { name: /Enable local screen recognition/ });
    await recognition.check();
    await expect(page.locator('#overlay-shortcut')).toBeEnabled();
    await page.locator('#overlay-shortcut').fill('Ctrl+Shift+P');
    await page.locator('#overlay-shortcut').press('Tab');
    await settings.getByRole('checkbox', { name: /Save local recognition diagnostics/ }).check();
    await expect(settings.getByRole('button', { name: 'Open diagnostics', exact: true })).toBeVisible();
    for (const width of [1440, 761, 760, 320]) {
      await page.setViewportSize({ width, height: 480 });
      await expect(page.locator('#overlay-shortcut')).toHaveValue('Ctrl+Shift+P');
      const bounds = await settings.locator(':scope > section').evaluateAll(sections => sections.map(section => {
        const box = section.getBoundingClientRect();
        return { left: box.left, right: box.right };
      }));
      expect(bounds).toHaveLength(6);
      for (const box of bounds) {
        expect(box.left).toBeCloseTo(bounds[0].left, 0);
        expect(box.right).toBeCloseTo(bounds[0].right, 0);
      }
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    }
    await recognition.uncheck();
    await expect(page.locator('#overlay-shortcut')).toBeDisabled();
    await expect(page.locator('#overlay-scale')).toBeDisabled();
    await expect(settings.getByRole('button', { name: 'Preview overlay', exact: true })).toBeDisabled();
    await recognition.check();
    await expect(page.locator('#overlay-shortcut')).toHaveValue('Ctrl+Shift+P');
  });
}

test('selling tables label pick facts and retain readable item identities', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/?preview-desktop&sample');
  await page.locator('.sidebar').getByRole('button', { name: /^Sell/ }).click();
  const picks = page.getByRole('region', { name: 'Top picks', exact: true });
  await expect(picks.getByRole('columnheader')).toHaveText(['Item', 'Low sell', 'Vol 48h', 'Why list now']);
  for (const selector of ['.picks-table td:first-child', '.results tbody td:first-child']) {
    const widths = await page.locator(selector).evaluateAll(cells => cells.map(cell => cell.getBoundingClientRect().width));
    expect(widths.length).toBeGreaterThan(0);
    for (const width of widths) expect(width).toBeGreaterThanOrEqual(319);
  }
  const scroll = page.locator('.results > .scroll');
  await scroll.evaluate(el => { el.scrollLeft = el.scrollWidth; });
  await expect(page.getByRole('columnheader', { name: /Potential/ })).toBeInViewport();
});

test('page background keeps fine repeating tiles on tall WebKit surfaces', async ({ browser }) => {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  try {
    // A tall standalone document reproduces WebKit’s page-sized raster while
    // loading the same production background styles.
    await page.route('**/background-probe', route => route.fulfill({
      contentType: 'text/html',
      body: `<!doctype html><html data-look="yorha" data-mode="dark"><head><link rel="stylesheet" href="/src/app.css"></head><body style="height:auto;min-height:100%"><main style="height:1700px;width:80%;margin:auto;background:var(--panel)"></main></body></html>`,
    }));
    await page.goto('/background-probe');
    const background = await page.screenshot();
    // The grid is faint enough that the default colour tolerance hides distortion.
    expect(background).toMatchSnapshot('background-Dark.png', { threshold: 0, maxDiffPixelRatio: 0.002 });
  } finally { await page.close(); }
});

test('narrow order filters stay reachable and routine rails fill their panel', async ({ page }) => {
  await page.goto('/?preview-desktop&sample');
  for (const theme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: theme });
    for (const width of [320, 761, 1440]) {
      await page.setViewportSize({ width, height: 480 });
      await page.locator('.sidebar').getByRole('button', { name: /^My orders/ }).click();
      const orderBounds = await page.locator('.orders').boundingBox();
      const filterBounds = await page.locator('.orders .seg button').evaluateAll(buttons => buttons.map(button => {
        const box = button.getBoundingClientRect();
        return { right: box.right, height: box.height, clipped: button.scrollHeight > button.clientHeight + 1 };
      }));
      for (const button of filterBounds) {
        expect(button.right).toBeLessThanOrEqual(orderBounds!.x + orderBounds!.width - 1);
        expect(button.height).toBeGreaterThanOrEqual(24);
        expect(button.clipped).toBe(false);
      }
      await page.locator('.sidebar').getByRole('button', { name: /^Routines/ }).click();
      const summary = page.locator('.routine-checklist > summary');
      for (let state = 0; state < 2; state++) {
        const bounds = await summary.evaluate(element => ({
          width: element.getBoundingClientRect().width,
          panelWidth: element.parentElement!.getBoundingClientRect().width,
        }));
        expect(bounds.width).toBeCloseTo(bounds.panelWidth - 2, 0);
        await summary.click();
      }
    }
  }
});
