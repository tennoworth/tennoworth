import { expect, type Locator } from '@playwright/test';

/** Chooses `value` in the app's drawn dropdown (ui/Select.svelte) the way a person does: open, click, closed. */
export async function pickOption(combobox: Locator, value: string): Promise<void> {
  // Read before opening: the open list carries the same name, so a label locator would then match both.
  const list = combobox.page().locator(`#${await combobox.getAttribute('aria-controls')}`);
  await combobox.click();
  await list.locator(`[role="option"][data-value="${value}"]`).click();
  await expect(list).toBeHidden();
}
