import { expect, test, type Page } from '@playwright/test';

async function desktop(page: Page) {
  await page.goto('/?preview-desktop&sample');
  await expect(page.locator('.sidebar')).toBeVisible();
  await expect(page.getByRole('textbox', { name: 'Filter by name', exact: true })).toBeVisible();
}
const helpButton = (page: Page) => page.getByRole('button', { name: /^Keyboard shortcuts/ });
const help = (page: Page) => page.getByRole('dialog', { name: 'Keyboard shortcuts', exact: true });

for (const theme of ['light', 'dark'] as const) {
  test(`${theme}: keyboard help and tables fit wide, narrow and short windows`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme });
    await desktop(page);
    for (const size of [{ width: 1440, height: 900 }, { width: 760, height: 480 }, { width: 320, height: 480 }]) {
      await page.setViewportSize(size);
      await helpButton(page).click();
      await expect(help(page)).toBeVisible();
      const title = help(page).getByRole('heading', { name: 'Keyboard shortcuts' });
      const close = help(page).getByRole('button', { name: 'Close', exact: true });
      await expect(title).toBeInViewport(); await expect(close).toBeInViewport();
      expect(await title.evaluate(el => {
        const a = el.getBoundingClientRect(), b = el.closest('dialog')!.getBoundingClientRect();
        return a.top >= b.top && a.left >= b.left && a.right <= b.right;
      })).toBe(true);
      await help(page).getByRole('button', { name: 'Reset shortcut choices' }).focus();
      await page.keyboard.press('Tab'); await expect(close).toBeFocused();
      await page.keyboard.press('Escape'); await expect(helpButton(page)).toBeFocused();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    }
    const table = page.locator('main').getByRole('region', { name: 'Inventory table; scroll for all columns' }).last();
    await table.focus(); await page.keyboard.press('ArrowRight');
    await expect.poll(() => table.evaluate(el => el.scrollLeft)).toBeGreaterThan(0);
  });
}

test('skip links and keyboard view changes bypass sidebar while preserving filters', async ({ page }) => {
  await desktop(page);
  await page.keyboard.press('Tab');
  await expect(page.getByRole('link', { name: 'Skip to current view' })).toBeFocused();
  await page.keyboard.press('Enter'); await expect(page.getByRole('main')).toBeFocused();
  await page.keyboard.press('/');
  const search = page.getByRole('textbox', { name: 'Filter by name', exact: true });
  await expect(search).toBeFocused(); await search.fill('primed');
  await page.locator('.sidebar').getByRole('button', { name: /^My orders/ }).focus(); await page.keyboard.press('Enter');
  await expect(page.getByRole('heading', { name: 'My orders', exact: true })).toBeFocused();
  await expect(page.locator('.sidebar').getByRole('button', { name: /^My orders/ })).toHaveAttribute('aria-current', 'page');
  await page.keyboard.press('/'); await expect(page.getByRole('textbox', { name: 'Filter orders by name' })).toBeFocused();
  await page.locator('.sidebar').getByRole('button', { name: /^Sell/ }).focus(); await page.keyboard.press('Enter');
  await expect(search).toHaveValue('primed');
  await page.keyboard.press('F6'); await expect(page.locator('.refresh-trigger')).toBeFocused();
  await page.keyboard.press('F6'); await expect(page.locator('.sidebar').getByRole('button', { name: /^Sell/ })).toBeFocused();
  await page.keyboard.press('Shift+F6'); await expect(page.locator('.refresh-trigger')).toBeFocused();
  await page.keyboard.press('Control+k');
  const picker = page.getByRole('combobox', { name: 'Find a view' });
  await expect(picker).toBeFocused(); await picker.fill('price watches'); await page.keyboard.press('Enter');
  await expect(page.getByRole('heading', { name: 'Price watches', exact: true })).toBeFocused();
  await page.keyboard.press('/'); await expect(page.getByRole('textbox', { name: 'Item to watch' })).toBeFocused();
});

test('hosted search, custom characters, disable and unsupported layout fallback', async ({ page }) => {
  await page.addInitScript(() => Object.defineProperty(navigator, 'keyboard', { value: undefined, configurable: true }));
  await page.goto('/');
  const search = page.getByRole('textbox', { name: 'Search items' });
  await expect(search).toBeVisible();
  await page.keyboard.press('Tab'); await expect(page.getByRole('link', { name: 'Skip to item search' })).toBeFocused();
  await page.keyboard.press('Enter'); await expect(search).toBeFocused();
  await search.fill('primed?'); await expect(help(page)).not.toBeVisible();
  await helpButton(page).click(); await expect(help(page)).toContainText('Automatic key labels are unavailable');
  await help(page).getByRole('button', { name: 'Change search key' }).click(); await page.evaluate(() => document.activeElement?.dispatchEvent(new KeyboardEvent('keydown', { key: 'é', code: 'Digit2', bubbles: true, cancelable: true })));
  await page.keyboard.press('Escape'); await search.blur();
  await page.evaluate(() => document.activeElement?.dispatchEvent(new KeyboardEvent('keydown', { key: 'é', code: 'Digit2', bubbles: true, cancelable: true }))); await expect(search).toBeFocused();
  await page.reload(); await expect(search).toBeVisible();
  await page.evaluate(() => document.activeElement?.dispatchEvent(new KeyboardEvent('keydown', { key: 'é', code: 'Digit2', bubbles: true, cancelable: true }))); await expect(search).toBeFocused();
  await helpButton(page).click(); await help(page).getByRole('checkbox', { name: 'Enable single-character shortcuts' }).uncheck();
  await page.keyboard.press('Escape'); await page.evaluate(() => document.activeElement?.dispatchEvent(new KeyboardEvent('keydown', { key: 'é', code: 'Digit2', bubbles: true, cancelable: true }))); await expect(search).not.toBeFocused();
  await helpButton(page).click(); await help(page).getByRole('button', { name: 'Reset shortcut choices' }).click();
  await page.keyboard.press('Escape'); await page.keyboard.press('/'); await expect(search).toBeFocused();
});

test('layout labels update locally and denied discovery keeps navigation usable', async ({ page }) => {
  await page.addInitScript(() => {
    const source = new EventTarget(); let label = 'л';
    Object.assign(source, { getLayoutMap: async () => new Map([['KeyK', label]]) });
    Object.defineProperty(navigator, 'keyboard', { configurable: true, value: source });
    window.addEventListener('test-layout-change', () => { label = 'k'; source.dispatchEvent(new Event('layoutchange')); });
  });
  await desktop(page);
  await helpButton(page).click();
  await expect(help(page)).toContainText('Ctrl+Л');
  await page.keyboard.press('Escape');
  await page.evaluate(() => document.dispatchEvent(new KeyboardEvent('keydown', { key: 'л', code: 'KeyK', ctrlKey: true, bubbles: true, cancelable: true })));
  await expect(page.getByRole('combobox', { name: 'Find a view' })).toBeFocused();
  await page.keyboard.press('Escape');
  await page.evaluate(() => window.dispatchEvent(new Event('test-layout-change')));
  await helpButton(page).click();
  await expect(help(page)).toContainText('Ctrl+K');
  await page.keyboard.press('Escape');
  await page.addInitScript(() => Object.defineProperty(navigator, 'keyboard', { configurable: true, value: { getLayoutMap: async () => { throw new Error('Denied'); } } }));
  await page.reload();
  await helpButton(page).click();
  await expect(help(page)).toContainText('Ctrl+K');
  await page.keyboard.press('Escape');
  await page.keyboard.press('Control+k'); await expect(page.getByRole('combobox', { name: 'Find a view' })).toBeFocused();
});

test('shortcuts leave dialogs, composition and expanded menus alone', async ({ page }) => {
  await desktop(page); await helpButton(page).click();
  await help(page).getByRole('button', { name: 'Close', exact: true }).focus();
  await page.keyboard.press('/'); await expect(help(page).getByRole('button', { name: 'Close', exact: true })).toBeFocused();
  await page.keyboard.press('Escape');
  await page.locator('.refresh-trigger').focus(); await page.keyboard.press('Enter');
  await expect(page.getByTestId('desktop-scan')).toBeFocused();
  await page.keyboard.press('/'); await expect(page.getByTestId('desktop-scan')).toBeFocused();
  await page.keyboard.press('Escape'); await expect(page.locator('.refresh-trigger')).toBeFocused();
  await page.evaluate(() => document.activeElement?.dispatchEvent(new KeyboardEvent('keydown', { key: '/', isComposing: true, bubbles: true })));
  await expect(page.locator('.refresh-trigger')).toBeFocused();
});

test('a custom view key works without layout discovery, and picker handles arrows and no matches', async ({ page }) => {
  await page.addInitScript(() => Object.defineProperty(navigator, 'keyboard', { value: undefined, configurable: true }));
  await desktop(page);
  await page.keyboard.press('?');
  await expect(help(page)).toBeVisible();
  await help(page).getByRole('button', { name: 'Change view key' }).click();
  await page.evaluate(() => document.activeElement?.dispatchEvent(new KeyboardEvent('keydown', { key: 'ж', code: 'Semicolon', bubbles: true, cancelable: true })));
  await page.keyboard.press('Escape');
  await helpButton(page).click();
  await expect(help(page)).toContainText('Ctrl+ж');
  await page.keyboard.press('Escape');
  await page.evaluate(() => document.dispatchEvent(new KeyboardEvent('keydown', { key: 'ж', code: 'Semicolon', ctrlKey: true, bubbles: true, cancelable: true })));
  const picker = page.getByRole('combobox', { name: 'Find a view' });
  await expect(picker).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('option', { name: 'Trade Session' })).toHaveAttribute('aria-selected', 'true');
  await page.keyboard.press('Enter');
  await expect(page.getByRole('heading', { name: 'Trade Session', exact: true })).toBeFocused();
  await helpButton(page).click();
  await help(page).getByRole('button', { name: /^Go to a view/ }).click();
  await picker.fill('no such destination'); await expect(page.getByText('No matching views.')).toBeVisible();
  await page.keyboard.press('Enter'); await expect(picker).toBeFocused();
  await page.keyboard.press('Escape'); await expect(helpButton(page)).toBeFocused();
});

 test('shortcut utilities live in the footer and leave view navigation first', async ({ page }) => {
  await desktop(page);
  await expect(page.locator('.sidebar nav').getByRole('button').first()).toHaveText(/^Sell/);
  await expect(page.locator('.sfoot').getByRole('button', { name: /^Keyboard shortcuts/ })).toBeVisible();
  await expect(page.getByRole('button', { name: /^Go to/ })).toHaveCount(0);
});
