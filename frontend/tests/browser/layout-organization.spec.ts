import { expect, test } from '@playwright/test';

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} workspace keeps readable Plex type and related controls together`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.setViewportSize({ width: 2560, height: 1440 });
    await page.goto('/?preview-desktop&sample=session');
    await expect(page.getByLabel('Trade budget')).toHaveValue('8');
    await page.evaluate(() => document.fonts.ready);
    const type = await page.locator('main .view-header h2').first().evaluate(title => ({
      family: getComputedStyle(title).fontFamily,
      root: parseFloat(getComputedStyle(document.documentElement).fontSize),
      data: parseFloat(getComputedStyle(document.querySelector('.session-table')!).fontSize),
    }));
    expect(type.family).toContain('IBM Plex Sans Condensed');
    expect(type.root).toBe(18);
    expect(type.data).toBeGreaterThanOrEqual(16);
    const modes = await page.getByRole('group', { name: 'Trade intent' }).boundingBox();
    const budget = await page.getByLabel('Trade budget').boundingBox();
    expect(budget!.x - (modes!.x + modes!.width)).toBeLessThan(50);
    const summary = await page.getByRole('region', { name: 'Suggested batch', exact: true }).getByRole('heading', { level: 3 }).last().boundingBox();
    const check = await page.getByRole('button', { name: 'Check live prices', exact: true }).boundingBox();
    expect(check!.x - (summary!.x + summary!.width)).toBeLessThan(50);
    await page.getByText('Target and complete sets', { exact: true }).click();
    const target = page.getByLabel('Platinum target', { exact: true });
    await target.fill('250');
    await page.getByRole('checkbox', { name: 'Include complete owned sets' }).uncheck();
    await page.locator('.sidebar').getByRole('button', { name: 'Settings', exact: true }).click();
    await page.locator('.sidebar').getByRole('button', { name: 'Trade Session', exact: true }).click();
    await expect(target).toHaveValue('250');
    await expect(page.getByRole('checkbox', { name: 'Include complete owned sets' })).not.toBeChecked();
    await page.setViewportSize({ width: 320, height: 480 });
    await expect(target).toHaveValue('250');
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
  });

  test(`${theme} hosted lookup shares the readable type and local font`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.setViewportSize({ width: 2560, height: 1440 });
    await page.goto('/');
    await expect(page.locator('.statement')).toBeVisible();
    const type = await page.locator('.lookup input').first().evaluate(() => ({
      ui: getComputedStyle(document.documentElement).getPropertyValue('--font-ui'),
      caption: parseFloat(getComputedStyle(document.documentElement).fontSize) * parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--text-caption')),
    }));
    expect(type.ui).toContain('IBM Plex Sans Condensed');
    expect(type.caption).toBeGreaterThanOrEqual(14);
    await page.setViewportSize({ width: 320, height: 480 });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
  });
}
