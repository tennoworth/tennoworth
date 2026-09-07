import { expect, test, type Locator, type Page } from '@playwright/test';

const INVENTORY = {
  ts: 1_800_000_000_000,
  invName: 'responsive-audit-inventory-with-an-intentionally-long-filename.json',
  owned: [
    ['pyrana_prime_set', { count: 3, name: 'Pyrana Prime Set', type: 'Weapon', slug: 'pyrana_prime_set', subtype: null, kept_lvl: null, leveled: 0 }],
    ['ivara_prime_neuroptics_blueprint', { count: 4, name: 'Ivara Prime Neuroptics Blueprint', type: 'Warframe', slug: 'ivara_prime_neuroptics_blueprint', subtype: null, kept_lvl: null, leveled: 0 }],
    ['neo_n8_relic', { count: 7, name: 'Neo N8 Relic', type: 'Relic', slug: 'neo_n8_relic', subtype: 'Intact', kept_lvl: null, leveled: 0 }],
  ],
  rivens: [],
};

const SWEEP_WIDTHS = [320, 360, 480, 559, 560, 561, 719, 720, 721, 759, 760, 761, 899, 900, 901, 1200, 1600];
const DESKTOP_VIEWS = ['Sell', 'Set picks', 'Baro', 'Routines', 'Meta Drift', 'My orders', 'Price watches', 'Ledger', 'FAQ', 'Settings'];

async function assertDocumentFits(page: Page): Promise<void> {
  const dimensions = await page.evaluate(() => ({
    clientWidth: document.documentElement.clientWidth,
    scrollWidth: document.documentElement.scrollWidth,
  }));
  expect(dimensions.scrollWidth, `document overflowed at ${dimensions.clientWidth}px`).toBeLessThanOrEqual(dimensions.clientWidth + 1);
  const clippedStatus = await page.locator('.statusbar .cell').evaluateAll((cells) =>
    cells.filter(cell => {
      const rect = cell.getBoundingClientRect();
      return rect.left < -1 || rect.right > document.documentElement.clientWidth + 1;
    }).map(cell => cell.textContent?.trim()),
  );
  expect(clippedStatus, 'status information and actions must fit even when the shell hides overflow').toEqual([]);
  const clippedRails = await page.locator('.rail:visible, .summary:visible, .orders .seg:visible').evaluateAll(elements =>
    elements.filter(element => element.scrollWidth > element.clientWidth + 2 || element.scrollHeight > element.clientHeight + 2)
      .map(element => element.textContent?.trim()),
  );
  expect(clippedRails, 'information rails must not clip their contents').toEqual([]);
  for (const rail of await page.locator('.workspace .rail:visible').all()) {
    const parent = rail.locator('..');
    const railBox = await rail.boundingBox();
    const parentBox = await parent.boundingBox();
    expect(parentBox!.height, 'short windows must not collapse a panel around its heading').toBeGreaterThanOrEqual(railBox!.height);
  }
}

async function assertReadableTableLabels(page: Page): Promise<void> {
  const collapsed = await page.locator('table:visible th.l, table:visible td.l, table:visible td.col-name').evaluateAll((cells) =>
    cells
      .map((cell) => ({
        text: (cell.textContent ?? '').trim(),
        width: cell.getBoundingClientRect().width,
      }))
      .filter(({ text, width }) => text.length > 0 && width < 72),
  );
  expect(collapsed, 'a visible table collapsed its label column').toEqual([]);
}

async function assertControlReachable(control: Locator): Promise<void> {
  await control.scrollIntoViewIfNeeded();
  const box = await control.boundingBox();
  expect(box).not.toBeNull();
  expect(box!.width).toBeGreaterThan(0);
  expect(box!.height).toBeGreaterThan(0);
}

async function assertWithinViewport(page: Page, locator: Locator): Promise<void> {
  const box = await locator.boundingBox();
  const viewport = page.viewportSize();
  expect(box).not.toBeNull();
  expect(viewport).not.toBeNull();
  expect(box!.x).toBeGreaterThanOrEqual(-1);
  expect(box!.y).toBeGreaterThanOrEqual(-1);
  expect(box!.x + box!.width).toBeLessThanOrEqual(viewport!.width + 1);
  expect(box!.y + box!.height).toBeLessThanOrEqual(viewport!.height + 1);
}

async function openDesktop(page: Page): Promise<void> {
  await page.addInitScript((snapshot) => {
    localStorage.setItem('last-owned', JSON.stringify(snapshot));
    localStorage.setItem('sell-onboarding-dismissed', '1');
    localStorage.setItem('keep-copies-nudge-dismissed', '1');
  }, INVENTORY);
  await page.goto('/?preview-desktop');
  await expect(page.getByTestId('desktop-mode')).toHaveCount(0);
  await expect(page.locator('.shell')).toBeVisible();
}

test.describe('hosted responsive layout', () => {
  test('reflows continuously without losing table labels', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByTestId('market-browser')).toBeVisible();

    for (const width of SWEEP_WIDTHS) {
      await page.setViewportSize({ width, height: width <= 560 ? 480 : 800 });
      await assertDocumentFits(page);
      await assertReadableTableLabels(page);
    }
  });

  test('search results and horizontally dense reports keep local scroll', async ({ page }) => {
    await page.setViewportSize({ width: 320, height: 480 });
    await page.goto('/');
    await expect(page.getByTestId('market-browser')).toBeVisible();
    await page.getByRole('textbox', { name: 'Search items' }).fill('prime');
    await expect(page.locator('.lookup .results')).toBeVisible();
    await assertDocumentFits(page);
    await assertReadableTableLabels(page);

    const horizontalTables = page.locator('.scroll').filter({ has: page.locator('table') });
    await expect(horizontalTables.first()).toBeVisible();
    const scrollable = await horizontalTables.evaluateAll((regions) =>
      regions.some((region) => region.scrollWidth > region.clientWidth),
    );
    expect(scrollable).toBe(true);
  });
});

test('open editors and disclosures survive resizing in both directions', async ({ page }) => {
  await page.goto('/?preview-desktop&sample');
  await page.locator('.shell').waitFor();
  await page.getByRole('button', { name: /^List \d+ on WFM$/ }).click();
  const review = page.locator('.modal');
  const quantity = review.locator('tbody input[type="number"]').first();
  await quantity.fill('2');
  for (const [width, height] of [[320, 480], [768, 480], [1200, 480], [901, 700], [320, 480]]) {
    await page.setViewportSize({ width, height });
    await assertWithinViewport(page, review);
    await expect(quantity).toHaveValue('2');
    expect(await review.locator('.scroll').evaluate(element => element.clientHeight)).toBeGreaterThanOrEqual(80);
    await review.locator('.scroll').evaluate(element => { element.scrollLeft = element.scrollWidth; });
    await review.getByRole('button', { name: /^Send \d+ listings$/ }).click({ trial: true });
  }
  await review.getByRole('button', { name: 'Cancel' }).click();
  await page.locator('.filter-disclosure > summary').click();
  for (const width of [1200, 901, 561, 320, 768]) {
    await page.setViewportSize({ width, height: 480 });
    const filters = page.locator('.filters-panel');
    await filters.scrollIntoViewIfNeeded();
    await assertWithinViewport(page, filters);
  }
  await page.getByRole('button', { name: 'Close filters' }).click();
  const help = page.getByRole('button', { name: 'What does Score mean?' });
  await help.focus();
  await help.press('Enter');
  for (const width of [320, 901, 1440]) {
    await page.setViewportSize({ width, height: 480 });
    await assertWithinViewport(page, page.locator('.help-popover'));
  }
  await page.locator('.view-header h2').click();
  await page.locator('.sidebar').getByRole('button', { name: /^Rivens/ }).click();
  await page.getByRole('button', { name: 'Comps', exact: true }).first().click();
  await expect(page.getByText('Sample seller')).toBeVisible();
  for (const width of [320, 901, 1440]) {
    await page.setViewportSize({ width, height: 480 });
    await assertDocumentFits(page);
    await expect(page.getByText('Sample seller')).toBeVisible();
  }
});

test('populated workspaces keep their content through width and height changes', async ({ page }, testInfo) => {
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto('/?preview-desktop&sample');
  await expect(page.locator('.shell')).toBeVisible();
  const surfaces = [
    ['Sell', '.col-name'], ['Set picks', '.reco'], ['Relics', '.relic-card'],
    ['Rivens', '.weapon'], ['Baro', '.baro-card'], ['Routines', '.routine'],
    ['Meta Drift', '.meta-drift tbody tr'], ['My orders', '.orders tbody tr'],
    ['Price watches', '.watchlist tbody tr'], ['Ledger', '.ledger tbody tr'],
    ['FAQ', '.faq'], ['Settings', '.settings'],
  ];
  for (const [view, content] of surfaces) {
    await page.locator('.sidebar').getByRole('button', { name: new RegExp('^' + view) }).click();
    await expect(page.locator(content).first(), view + ' must contain actual content').toBeVisible();
    for (const [width, height] of [[1440, 900], [1024, 480], [901, 600], [768, 480], [320, 480], [1200, 480]]) {
      await page.setViewportSize({ width, height });
      await assertDocumentFits(page);
      await assertReadableTableLabels(page);
      for (const region of await page.locator('main .scroll:visible').all()) {
        await region.evaluate(element => { element.scrollLeft = element.scrollWidth; });
        const lastCell = region.locator('thead tr').first().locator('th').last();
        if (await lastCell.count()) {
          const cellBox = await lastCell.boundingBox();
          const regionBox = await region.boundingBox();
          expect(cellBox!.x + cellBox!.width, 'last column must be reachable inside its table scroller').toBeLessThanOrEqual(regionBox!.x + regionBox!.width + 1);
        }
        await assertDocumentFits(page);
        await region.evaluate(element => { element.scrollLeft = 0; });
      }
    }
    await page.screenshot({ path: testInfo.outputPath(view.replaceAll(' ', '-') + '.png') });
  }
  expect(errors).toEqual([]);
});

test('enlarged layout keeps status and navigation accessible', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 960 });
  await page.goto('/?preview-desktop&sample');
  await page.locator('.shell').waitFor();
  // CSS zoom exercises enlarged rendering in both engines; it does not emulate
  // the operating system's display scale or the browser's own zoom controls.
  await page.evaluate(() => { document.documentElement.style.zoom = '2'; });
  await assertDocumentFits(page);
  await page.locator('.sidebar').getByRole('button', { name: 'Settings' }).click();
  await expect(page.locator('.settings')).toBeVisible();
  await assertDocumentFits(page);
});

for (const scenario of ['empty', 'error', 'loading']) {
  test('management views render ' + scenario + ' states', async ({ page }) => {
    await page.setViewportSize({ width: 320, height: 480 });
    await page.goto('/?preview-desktop&sample=' + scenario);
    for (const name of ['My orders', 'Price watches', 'Ledger']) {
      await page.locator('.sidebar').getByRole('button', { name }).click();
      await expect(page.locator('main.workspace')).toContainText(
        scenario === 'error' ? /Sample connection failure/ :
        scenario === 'loading' ? /Fetching|No watches|No trades/i : /No active listings|No watches|No trades/i,
      );
      await assertDocumentFits(page);
      if (scenario === 'loading') await expect(page.locator('main.workspace tbody tr').first()).toBeVisible();
    }
  });
}

test.describe('desktop responsive layout', () => {
  test.beforeEach(async ({ page }) => {
    await openDesktop(page);
  });

  test('the shell survives the full resize sweep', async ({ page }) => {
    for (const width of SWEEP_WIDTHS) {
      await page.setViewportSize({ width, height: width <= 560 ? 480 : 700 });
      await assertDocumentFits(page);
      await assertReadableTableLabels(page);
    }
  });

  test('all views and the complete navigation remain reachable', async ({ page }) => {
    for (const width of [320, 560, 899, 900, 901, 1200]) {
      await page.setViewportSize({ width, height: width <= 560 ? 480 : 700 });
      for (const name of DESKTOP_VIEWS) {
        const button = page.getByRole('button', { name: new RegExp(`^${name}`) });
        await assertControlReachable(button);
        await button.click();
        await assertDocumentFits(page);
        await assertReadableTableLabels(page);
      }
    }
  });

  test('menus and dialogs stay inside a narrow viewport', async ({ page }) => {
    await page.setViewportSize({ width: 320, height: 480 });

    await page.getByRole('button', { name: /Refresh/ }).click();
    const refreshMenu = page.locator('.refresh-pop');
    await expect(refreshMenu).toBeVisible();
    await assertWithinViewport(page, refreshMenu);
    await page.getByRole('button', { name: 'Export…' }).click();

    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();
    await assertWithinViewport(page, dialog);
    await dialog.getByRole('button', { name: 'Cancel' }).click();

    await page.locator('.statusbar .cell.end').getByRole('button', { name: 'logged out' }).click();
    const authDialog = page.locator('dialog.cryptobox[open]');
    await expect(authDialog).toBeVisible();
    await assertWithinViewport(page, authDialog);
    await authDialog.getByRole('button', { name: 'Cancel' }).click();

    const filterDisclosure = page.locator('.filter-disclosure > summary');
    await filterDisclosure.click();
    const filters = page.locator('.filters-panel');
    await expect(filters).toBeVisible();
    await assertWithinViewport(page, filters);
    await page.getByRole('button', { name: 'Close filters' }).click();

    await page.evaluate(() => {
      const runtime = globalThis as typeof globalThis & {
        __TAURI__?: { core?: { invoke?: (command: string, args?: Record<string, unknown>) => Promise<unknown> } };
        __TAURI_INTERNALS__?: { invoke?: (command: string, args?: Record<string, unknown>) => Promise<unknown> };
      };
      const original = runtime.__TAURI__?.core?.invoke;
      if (!original || !runtime.__TAURI__?.core || !runtime.__TAURI_INTERNALS__) return;
      const unlocked = (command: string, args?: Record<string, unknown>) =>
        command === 'wfm_auth_status'
          ? Promise.resolve({ logged_in: true, unlocked: true })
          : original(command, args);
      runtime.__TAURI__.core.invoke = unlocked;
      runtime.__TAURI_INTERNALS__.invoke = unlocked;
    });
    await page.getByRole('button', { name: /^List \d+ on WFM$/ }).click();
    const review = page.locator('.modal');
    await expect(review).toBeVisible();
    await assertWithinViewport(page, review);
    const reviewScroll = review.locator('.scroll');
    expect(await reviewScroll.evaluate((region) => region.scrollWidth > region.clientWidth)).toBe(true);
    expect(await reviewScroll.evaluate((region) => region.clientHeight)).toBeGreaterThanOrEqual(80);
    expect(await review.locator('tbody tr').first().locator('td').nth(1).evaluate((cell) => cell.getBoundingClientRect().width)).toBeGreaterThanOrEqual(72);
    await assertDocumentFits(page);
  });
});
