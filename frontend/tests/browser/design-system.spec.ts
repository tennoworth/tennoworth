import { expect, test, type Page } from '@playwright/test';
import rewardFixture from '../../../tests/fixtures/relic-ocr/result.json' with { type: 'json' };

// Text placed on a filled title rail must take the rail's own pair; a rule
// written for --panel (a count, a glyph) vanishes at ~1.2:1 on the bar.
async function unreadableRailText(page: Page) {
  return page.evaluate(() => {
    const rgb = (value: string) => value.match(/[\d.]+/g)!.map(Number);
    const luminance = ([r, g, b]: number[]) => [r, g, b].map(v => v / 255)
      .map(v => v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4)
      .reduce((sum, v, i) => sum + v * [.2126, .7152, .0722][i], 0);
    const background = (element: Element | null) => {
      for (; element; element = element.parentElement) {
        const colour = rgb(getComputedStyle(element).backgroundColor);
        if ((colour[3] ?? 1) > .5) return colour;
      }
      return [255, 255, 255];
    };
    return [...document.querySelectorAll('.wrap.tw > .rail, .wrap.tw > .rail *')].filter(element => {
      const text = [...element.childNodes].some(node => node.nodeType === Node.TEXT_NODE && node.textContent!.trim());
      if (!text || !element.getClientRects().length) return false;
      const a = luminance(rgb(getComputedStyle(element).color)), b = luminance(background(element));
      return (Math.max(a, b) + .05) / (Math.min(a, b) + .05) < 4.5;
    }).map(element => element.textContent!.trim());
  });
}

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
          const pickCount = Number.parseInt(await page.locator('.picks-title .picks-count').innerText(), 10);
          expect(pickCount, 'sample inventory keeps a bounded Top Picks set').toBeGreaterThan(0);
          expect(pickCount, 'sample inventory keeps a bounded Top Picks set').toBeLessThanOrEqual(5);
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
        if (width === 1200) expect(await unreadableRailText(page), `${view} rail text`).toEqual([]);
        await page.screenshot({ path: testInfo.outputPath(`${view.replaceAll(' ', '-')}-${theme}-${width}.png`) });
      }
    }
  });
}

test('hosted rails keep readable text in both themes', async ({ page }) => {
  for (const theme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: theme });
    await page.goto('/');
    await expect(page.locator('#community-usage')).toBeVisible();
    expect(await unreadableRailText(page), `hosted ${theme}`).toEqual([]);
    const rail = await page.locator('#community-usage > .rail').boundingBox();
    const reference = await page.locator('.wrap.tw > .rail').first().boundingBox();
    expect(rail!.height, 'an h2 rail matches the h3 rails').toBeLessThanOrEqual(reference!.height + 1);
  }
});

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
      // Appearance, WFM account, Usage, Updates, Automatic scan, Relic reward
      // overlay, Notifications. The count guards against a section silently
      // disappearing; the shared bounds below are what this test is about.
      expect(bounds).toHaveLength(7);
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
  // The banner above the results appears once the snapshot is stale, so the
  // table's distance from the top is not fixed. Bring it into view before
  // asserting the horizontal scroll reveals the last column.
  const scroll = page.locator('.results > .scroll');
  await scroll.scrollIntoViewIfNeeded();
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

test('narrow order filters stay reachable and routines remain an actionable checklist', async ({ page }) => {
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
      const checklist = page.locator('.routine-checklist');
      const tribute = checklist.getByRole('checkbox', { name: /Claim the Daily Tribute/ });
      await expect(tribute).toBeVisible();
      if (await tribute.isChecked()) await tribute.uncheck();
      await expect(checklist.getByText('0 of 5 complete')).toBeVisible();
      await tribute.check();
      await expect(checklist.getByText('1 of 5 complete')).toBeVisible();
      await page.getByRole('button', { name: 'Monthly' }).click();
      await expect(page.getByText(/not a Warframe reset schedule/)).toBeVisible();
      const goal = page.getByLabel('Add a monthly goal');
      await goal.fill('Prepare an intentionally unfinished monthly goal draft');
      await page.locator('.sidebar').getByRole('button', { name: /^Sell/ }).click();
      await page.locator('.sidebar').getByRole('button', { name: /^Routines/ }).click();
      await page.getByRole('button', { name: 'Monthly' }).click();
      await expect(goal).toHaveValue('Prepare an intentionally unfinished monthly goal draft');
      await page.getByRole('button', { name: 'Daily' }).click();
      await expect(tribute).toBeChecked();
    }
  }
});

test('monthly goals add, rename and remove individually', async ({ page }) => {
  await page.goto('/?preview-desktop&sample');
  for (const theme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: theme });
    await page.evaluate(() => localStorage.removeItem('routine-checklist'));
    await page.reload();
    await page.locator('.sidebar').getByRole('button', { name: /^Routines/ }).click();
    await page.getByRole('button', { name: 'Monthly' }).click();
    await expect(page.getByText(/Add a monthly goal to build/)).toBeVisible();

    const add = page.getByLabel('Add a monthly goal');
    for (const text of ['List ranked mods', 'Prepare Prime sets']) {
      await add.fill(text);
      await page.getByRole('button', { name: 'Add goal' }).click();
    }
    await expect(page.getByRole('checkbox', { name: /List ranked mods/ })).toBeVisible();
    await expect(page.getByRole('checkbox', { name: /Prepare Prime sets/ })).toBeVisible();
    await expect(add).toHaveValue('');

    await page.getByRole('checkbox', { name: /List ranked mods/ }).check();
    await expect(page.getByText('1 of 2 complete')).toBeVisible();

    await page.getByRole('button', { name: 'Edit List ranked mods' }).click();
    const editor = page.getByLabel('Goal text');
    await expect(editor).toHaveValue('List ranked mods');
    expect((await editor.boundingBox())!.width).toBeGreaterThan(200);
    await editor.fill('List four ranked mods');
    await page.getByRole('button', { name: 'Save goal' }).click();
    await expect(page.getByRole('checkbox', { name: /List four ranked mods/ })).toBeVisible();
    await expect(page.getByText('0 of 2 complete')).toBeVisible();

    await page.getByRole('button', { name: 'Remove Prepare Prime sets' }).click();
    await expect(page.getByRole('checkbox', { name: /Prepare Prime sets/ })).toHaveCount(0);
    await expect(page.getByRole('checkbox', { name: /List four ranked mods/ })).toBeVisible();
  }

  await page.reload();
  await page.locator('.sidebar').getByRole('button', { name: /^Routines/ }).click();
  await page.getByRole('button', { name: 'Monthly' }).click();
  await expect(page.getByRole('checkbox', { name: /List four ranked mods/ })).toBeVisible();
  await expect(page.getByRole('checkbox', { name: /Prepare Prime sets/ })).toHaveCount(0);
});

test('monthly goals reorder and keep keyboard focus in the list', async ({ page }) => {
  await page.goto('/?preview-desktop&sample');
  for (const theme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: theme });
    await page.evaluate(() => localStorage.removeItem('routine-checklist'));
    await page.reload();
    await page.locator('.sidebar').getByRole('button', { name: /^Routines/ }).click();
    await page.getByRole('button', { name: 'Monthly' }).click();
    const add = page.getByLabel('Add a monthly goal');
    for (const text of ['First goal', 'Second goal', 'Third goal']) {
      await add.fill(text);
      await page.getByRole('button', { name: 'Add goal' }).click();
    }
    const titles = page.locator('.checklist-items .task-copy strong');
    await expect(page.getByRole('button', { name: 'Move First goal up' })).toBeDisabled();
    await expect(page.getByRole('button', { name: 'Move Third goal down' })).toBeDisabled();

    await page.getByRole('button', { name: 'Move Third goal up' }).click();
    await expect(titles).toHaveText(['First goal', 'Third goal', 'Second goal']);
    await expect(page.getByRole('button', { name: 'Move Third goal up' })).toBeFocused();

    await page.getByRole('button', { name: 'Move Third goal up' }).click();
    await expect(titles).toHaveText(['Third goal', 'First goal', 'Second goal']);
    await expect(page.getByRole('button', { name: 'Move Third goal down' })).toBeFocused();

    await page.getByRole('button', { name: 'Remove First goal' }).click();
    await expect(page.getByRole('button', { name: 'Remove Second goal' })).toBeFocused();
  }

  await page.reload();
  await page.locator('.sidebar').getByRole('button', { name: /^Routines/ }).click();
  await page.getByRole('button', { name: 'Monthly' }).click();
  await expect(page.locator('.checklist-items .task-copy strong')).toHaveText(['Third goal', 'Second goal']);
});

async function openMonthlyGoals(page: Page, goals: string[]) {
  await page.goto('/?preview-desktop&sample');
  await page.evaluate(() => localStorage.removeItem('routine-checklist'));
  await page.reload();
  await page.locator('.sidebar').getByRole('button', { name: /^Routines/ }).click();
  await page.getByRole('button', { name: 'Monthly' }).click();
  const add = page.getByLabel('Add a monthly goal');
  for (const text of goals) {
    await add.fill(text);
    await page.getByRole('button', { name: 'Add goal' }).click();
  }
}

test('monthly goals expose a drag handle and keep it on the title line', async ({ page }) => {
  await openMonthlyGoals(page, ['First goal', 'Second goal', 'Third goal']);
  const handles = page.locator('[data-drag-handle]');
  const firstRow = page.locator('.checklist-items li').first();
  await expect(handles).toHaveCount(3);
  expect(await handles.first().getAttribute('draggable')).toBe('true');
  expect(await firstRow.getAttribute('draggable')).toBeNull();
  expect(await page.getByRole('checkbox', { name: /First goal/ }).getAttribute('draggable')).toBeNull();

  // Native dragging cannot be automated in WebKit (microsoft/playwright#31539),
  // so the wiring is asserted on every engine and the geometry reorder below
  // runs where the browser can actually be driven.
  await handles.first().evaluate(element => element.dispatchEvent(new Event('dragstart', { bubbles: true })));
  await expect(firstRow).toHaveClass(/dragging/);
  await handles.first().evaluate(element => element.dispatchEvent(new Event('dragend', { bubbles: true })));
  await expect(firstRow).not.toHaveClass(/dragging/);

  await page.setViewportSize({ width: 360, height: 760 });
  const handleBox = await handles.first().boundingBox();
  const titleBox = await page.locator('.checklist-items .task-copy strong').first().boundingBox();
  expect(handleBox!.y).toBeLessThan(titleBox!.y + titleBox!.height);
  expect(titleBox!.y).toBeLessThan(handleBox!.y + handleBox!.height);
});

test('monthly goals can be dragged by their handle to a new position', async ({ page, browserName }) => {
  test.skip(browserName !== 'chromium', 'Playwright cannot drive native HTML5 drag in WebKit (microsoft/playwright#31539)');
  await openMonthlyGoals(page, ['First goal', 'Second goal', 'Third goal']);
  const titles = page.locator('.checklist-items .task-copy strong');
  const handles = page.locator('[data-drag-handle]');
  const rows = page.locator('.checklist-items li');

  await handles.nth(2).dragTo(rows.nth(0), { targetPosition: { x: 40, y: 6 } });
  await expect(titles).toHaveText(['Third goal', 'First goal', 'Second goal']);

  await handles.nth(0).dragTo(rows.nth(2), { targetPosition: { x: 40, y: 60 } });
  await expect(titles).toHaveText(['First goal', 'Second goal', 'Third goal']);

  await page.reload();
  await page.locator('.sidebar').getByRole('button', { name: /^Routines/ }).click();
  await page.getByRole('button', { name: 'Monthly' }).click();
  await expect(titles).toHaveText(['First goal', 'Second goal', 'Third goal']);
});

test('the monthly goal field keeps a control height at narrow widths', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 640 });
  await page.goto('/?preview-desktop&sample');
  await page.locator('.sidebar').getByRole('button', { name: /^Routines/ }).click();
  await page.getByRole('button', { name: 'Monthly' }).click();
  const field = page.getByLabel('Add a monthly goal');
  await expect(field).toBeVisible();
  const box = await field.boundingBox();
  expect(box!.height).toBeLessThanOrEqual(48);
  expect(box!.x + box!.width).toBeLessThanOrEqual(360);
});

test('routine progress survives reload and failed persistence stays retryable', async ({ page }) => {
  await page.goto('/?preview-desktop&sample');
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.locator('.sidebar').getByRole('button', { name: /^Routines/ }).click();
  const tribute = page.getByRole('checkbox', { name: /Claim the Daily Tribute/ });
  await tribute.check();
  await expect.poll(() => page.evaluate(() => localStorage.getItem('routine-checklist'))).toContain('login-tribute');
  await page.reload();
  await page.locator('.sidebar').getByRole('button', { name: /^Routines/ }).click();
  await expect(tribute).toBeChecked();

  await page.evaluate(() => {
    (window as unknown as { originalStorageSetItem: typeof Storage.prototype.setItem }).originalStorageSetItem = Storage.prototype.setItem;
    Storage.prototype.setItem = () => { throw new DOMException('Storage disabled'); };
  });
  await tribute.uncheck();
  await expect(page.getByRole('alert')).toContainText('Progress is only saved in this open view');
  await page.evaluate(() => {
    Storage.prototype.setItem = (window as unknown as { originalStorageSetItem: typeof Storage.prototype.setItem }).originalStorageSetItem;
  });
  await page.getByRole('button', { name: 'Retry saving' }).click();
  await expect(page.getByRole('alert')).toHaveCount(0);
  await page.reload();
  await page.locator('.sidebar').getByRole('button', { name: /^Routines/ }).click();
  await expect(tribute).not.toBeChecked();
});
