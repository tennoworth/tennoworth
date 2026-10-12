import { expect, test } from '@playwright/test';
import published from '../../public/announcements.json' with { type: 'json' };

const title = published.messages[0].title;
test('a blocked dismissal save leaves the announcement visible and allows retry', async ({ page }) => {
  await page.clock.setFixedTime(new Date('2026-10-12T00:00:00Z'));
  await page.goto('/');
  const banner = page.getByRole('region', { name: title, exact: true });
  await expect(banner).toBeVisible();
  await page.evaluate(() => {
    const original = Storage.prototype.setItem;
    Storage.prototype.setItem = function(key, value) {
      if (key === 'wfminv:dismissed-announcements-v1') throw new DOMException('Storage is full', 'QuotaExceededError');
      return original.call(this, key, value);
    };
    Object.assign(window, { restoreAnnouncementStorage: () => { Storage.prototype.setItem = original; } });
  });
  await banner.getByRole('button', { name: `Dismiss ${title}`, exact: true }).click();
  await expect(banner.getByRole('alert')).toContainText('Could not save dismissal. Try again.');
  await expect(banner).toBeVisible();
  await page.evaluate(() => (window as unknown as { restoreAnnouncementStorage: () => void }).restoreAnnouncementStorage());
  await banner.getByRole('button', { name: `Dismiss ${title}`, exact: true }).click();
  await expect(banner).toHaveCount(0);
});

for (const theme of ['light', 'dark'] as const) {
  for (const surface of ['hosted', 'desktop'] as const) {
    test(`${theme} ${surface} market announcement stays readable and dismisses`, async ({ page }, testInfo) => {
      await page.emulateMedia({ colorScheme: theme });
      await page.clock.setFixedTime(new Date('2026-10-12T00:00:00Z'));
      await page.goto(surface === 'hosted' ? '/' : '/?preview-desktop&sample=announcements');
      const banner = page.getByRole('region', { name: title, exact: true });
      await expect(banner).toContainText('15:00 UTC (11 a.m. ET)');
      await expect(banner).toContainText("Opening relics alone doesn't lock in the value.");
      await expect(banner.getByRole('link', { name: 'Source announcement' })).toHaveAttribute('href', published.messages[0].source_url);
      for (const width of [1440, 768, 320]) {
        await page.setViewportSize({ width, height: 480 });
        await banner.scrollIntoViewIfNeeded();
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
        expect(await banner.evaluate(element => element.scrollWidth <= element.clientWidth + 1)).toBe(true);
        await banner.screenshot({ path: testInfo.outputPath(`${surface}-${theme}-${width}.png`) });
      }
      await banner.getByRole('button', { name: `Dismiss ${title}`, exact: true }).click();
      await expect(banner).toHaveCount(0);
      if (surface === 'hosted') {
        await page.reload();
        await expect(page.getByTestId('market-browser')).toBeVisible();
        await expect(banner).toHaveCount(0);
      } else {
        await page.locator('.sidebar').getByRole('button', { name: /^Notifications/ }).click();
        const entry = page.getByRole('article', { name: title, exact: true });
        await expect(entry).toContainText('20 extra Ducats');
        await expect(entry.getByRole('link', { name: 'Source announcement' })).toHaveAttribute('href', published.messages[0].source_url);
        await page.getByRole('button', { name: 'Notification settings', exact: true }).click();
        await page.getByLabel('Market announcements in inbox', { exact: true }).uncheck();
        await expect(page.getByLabel('Market announcements popup', { exact: true })).toBeDisabled();
      }
    });

    test(`${theme} ${surface} scheduled, failed, withdrawn and expired feeds preserve the right message`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: theme });
      await page.clock.setFixedTime(new Date('2026-10-12T00:00:00Z'));
      let feed: unknown = { ...published, messages: [{ ...published.messages[0], starts_at: '2026-10-13T00:00:00Z' }] };
      let offline = false;
      await page.route('**/announcements.json', route => offline ? route.abort() : route.fulfill({ json: feed }));
      await page.goto(surface === 'hosted' ? '/' : '/?preview-desktop&sample');
      const banner = page.getByRole('region', { name: title, exact: true });
      await expect(page.getByTestId('market-browser').or(page.locator('.workspace'))).toBeVisible();
      await expect(banner).toHaveCount(0);
      await page.clock.setFixedTime(new Date('2026-10-13T00:00:00Z'));
      await page.evaluate(() => window.dispatchEvent(new Event('focus')));
      await expect(banner).toBeVisible();
      feed = { version: 1, updated_at: '2026-10-14T00:00:00Z', messages: [{ id: 'broken' }] };
      await page.evaluate(() => window.dispatchEvent(new Event('focus')));
      await expect(banner).toBeVisible();
      offline = true;
      await page.evaluate(() => window.dispatchEvent(new Event('focus')));
      await expect(banner).toBeVisible();
      offline = false;
      feed = { version: 1, updated_at: '2026-10-14T00:00:00Z', messages: [] };
      await page.evaluate(() => window.dispatchEvent(new Event('focus')));
      await expect(banner).toHaveCount(0);
      feed = published;
      await page.evaluate(() => window.dispatchEvent(new Event('focus')));
      await expect(banner).toHaveCount(0);
      feed = { ...published, updated_at: '2026-10-14T01:00:00Z' };
      await page.evaluate(() => window.dispatchEvent(new Event('focus')));
      await expect(banner).toBeVisible();
      await page.clock.setFixedTime(new Date('2026-10-15T15:00:00Z'));
      await page.evaluate(() => window.dispatchEvent(new Event('focus')));
      await expect(banner).toHaveCount(0);
    });
  }
}
