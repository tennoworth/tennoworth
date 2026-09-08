import { expect, test } from '@playwright/test';

test('complete sets compete with their parts and edited quantities cannot reuse components', async ({ page }) => {
  await page.goto('/?preview-desktop&sample=session-sets');
  await page.getByRole('button', { name: /^Plat per Trade/ }).click();
  await page.getByLabel('Trade budget').fill('2');
  const table = page.locator('.session-table');
  await expect(table).toContainText('Akbolto Prime Set');
  await expect(table).toContainText('Akbolto Prime Barrel ×2');
  await page.getByLabel('Include complete owned sets').uncheck();
  await expect(table).not.toContainText('Akbolto Prime Set');
  await page.getByLabel('Include complete owned sets').check();
  await page.getByRole('button', { name: 'Review batch', exact: true }).click();
  const dialog = page.getByRole('dialog');
  const set = dialog.locator('tbody tr').filter({ hasText: 'Akbolto Prime Set' });
  await expect(set).toBeVisible();
  await set.locator('input[type="number"]').first().fill('2');
  // The part selected beside this set consumes one of the same components;
  // two sets need the complete pool, so the edited batch must be rejected.
  await expect(dialog).toContainText('reuse components');
  await expect(dialog.getByRole('button', { name: /^Send/ })).toBeDisabled();
});
