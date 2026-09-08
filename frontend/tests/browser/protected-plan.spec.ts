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
