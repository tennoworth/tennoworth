import { expect, test } from '@playwright/test';
import { readFileSync } from 'node:fs';

// The calendar reads the committed snapshot's own dates; pinning the clock to
// when it was taken keeps its events in the future however old the file is.
const market = JSON.parse(readFileSync(new URL('../../public/market.json', import.meta.url), 'utf8'));
const taken = new Date(market.updated_at);

for (const scheme of ['light', 'dark'] as const) {
  for (const width of [320, 1440]) {
    test(`${scheme} hosted calendar offers three views without page overflow at ${width}px`, async ({ page }) => {
      await page.clock.install({ time: taken });
      await page.emulateMedia({ colorScheme: scheme });
      await page.setViewportSize({ width, height: 800 });
      await page.goto('/');
      const calendar = page.locator('#calendar');
      await expect(calendar.getByRole('heading', { name: 'Calendar' })).toBeVisible();
      for (const view of ['Board', 'Month', 'Lanes']) {
        await calendar.getByRole('button', { name: view, exact: true }).click();
        await expect(calendar.getByRole('button', { name: view, exact: true })).toHaveAttribute('aria-pressed', 'true');
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
      }
      // Lanes is wider than a phone; it scrolls inside its own panel instead of the page.
      if (width < 1440) expect(await calendar.locator('.scroll').evaluate(el => el.scrollWidth > el.clientWidth)).toBe(true);
    });
  }
}

test('month days are chosen from the keyboard and keep focus', async ({ page }) => {
  await page.clock.install({ time: taken });
  await page.goto('/');
  const calendar = page.locator('#calendar');
  await calendar.getByRole('button', { name: 'Month', exact: true }).click();
  const days = calendar.getByRole('button', { name: /^(Monday|Tuesday|Wednesday|Thursday|Friday|Saturday|Sunday) \d+ \w+, / });
  const target = days.nth(20);
  const label = (await target.getAttribute('aria-label'))!.split(',')[0];
  await target.focus();
  await page.keyboard.press('Enter');
  await expect(target).toBeFocused();
  await expect(target).toHaveAttribute('aria-pressed', 'true');
  await expect(calendar.getByRole('region', { name: 'Selected day' })).toContainText(label.replace(/^\w+ /, ''));
});

test('the hosted calendar remembers its view across a reload', async ({ page }) => {
  await page.clock.install({ time: taken });
  await page.goto('/');
  await page.locator('#calendar').getByRole('button', { name: 'Lanes', exact: true }).click();
  await page.reload();
  await expect(page.locator('#calendar').getByRole('button', { name: 'Lanes', exact: true })).toHaveAttribute('aria-pressed', 'true');
});

test('the desktop preview lists the calendar beside Routines', async ({ page }) => {
  await page.clock.install({ time: taken });
  await page.goto('/?preview-desktop&sample');
  await page.getByRole('button', { name: 'Calendar', exact: true }).click();
  await expect(page.getByRole('region', { name: 'Calendar' }).getByRole('region', { name: 'Next up' })).toBeVisible();
});
