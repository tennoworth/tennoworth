import { test, expect } from '@playwright/test';
import { pages, pilotItems, itemPath } from '../../src/public-site/pages';

const routes = ['/', ...pages.map(p => p.path), ...pilotItems.map(itemPath)];
test('built public routes expose their answers, metadata and discoverable links', async ({ request }) => {
  const sitemap = await (await request.get('/sitemap.xml')).text();
  const links = new Set<string>();
  for (const path of routes) {
    const response = await request.get(path);
    expect(response.status(), path).toBe(200);
    const html = await response.text();
    expect(html).toContain(`<link rel="canonical" href="https://tennoworth.app${path}"`);
    expect(html).toMatch(/<h1[^>]*>[^<]+/);
    expect(html).toContain('Sell Prime sets or parts?');
    for (const [,link] of html.matchAll(/<a[^>]+href="(\/[^"]*)"/g)) links.add(link.split('#')[0]);
    expect(sitemap).toContain(`<loc>https://tennoworth.app${path}</loc>`);
  }
  for (const link of links) expect((await request.get(link || '/')).status(), link).toBe(200);
  expect((await request.get('/items/unknown-pilot-item/')).status()).toBe(404);
  expect((await request.get('/assets/unknown.js')).status()).toBe(404);
});

test('a failed interactive shell leaves the prerendered homepage readable', async ({ page }) => {
  let blocked = 0;
  await page.route(/\/HostedShell-[^/]+\.js$/, route => { blocked++; return route.abort(); });
  await page.goto('/');
  await expect(page.getByRole('heading', { level: 1, name: 'TennoWorth' })).toBeVisible();
  await expect(page.getByRole('link', { name: 'Sell Prime sets or parts?' })).toBeVisible();
  // A renamed chunk would stop matching the route; fail instead of passing blind.
  expect(blocked).toBeGreaterThan(0);
});

for (const mode of ['light', 'dark'] as const) for (const size of [{ width:320, height:740 }, { width:900, height:480 }, { width:1440, height:900 }]) {
  test(`public pages retain their theme and fit ${mode} ${size.width}x${size.height}`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: mode }); await page.setViewportSize(size);
    const errors: string[] = [];
    page.on('pageerror', error => errors.push(error.message));
    page.on('console', message => { if (message.type() === 'warning' && message.text().includes('hydration')) errors.push(message.text()); });
    for (const path of [pages[0].path, pages[1].path, pages[2].path, itemPath(pilotItems[0])]) {
      await page.goto(path);
      await expect(page.getByRole('heading', { level:1 })).toHaveCount(1);
      await expect(page.getByRole('radio', { name: 'System' })).toBeVisible();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
      expect(await page.locator('html').getAttribute('data-mode')).toBe(mode);
      await page.screenshot({ path: `test-results/public/${test.info().project.name}-${mode}-${size.width}-${path.split('/').filter(Boolean).join('-')}.png` });
    }
    expect(errors).toEqual([]);
  });
}

test('dated answers survive without JavaScript', async ({ browser }) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  for (const path of routes) {
    await page.goto(`http://127.0.0.1:5189${path}`);
    await expect(page.getByRole('heading', { level:1 })).toBeVisible();
    if (path.includes('/items/') || path.includes('/guides/') || path.includes('/tools/')) await expect(page.getByText('Snapshot ', { exact: false }).first()).toBeVisible();
  }
  await context.close();
});

test('search handles empty results and refresh failure preserves the dated comparison', async ({ page }) => {
  await page.route('**/market.json', route => route.abort());
  await page.goto('/tools/ducats-vs-platinum/');
  await expect(page.getByRole('status')).toContainText('Refresh unavailable');
  const search = page.getByRole('searchbox');
  await search.fill('no-such-prime-item');
  await expect(page.getByText('No priced parts match this search.')).toBeVisible();
  await search.fill('');
  await expect(page.locator('tbody tr')).toHaveCount(60);
  await page.getByRole('radio', { name:'Light', exact:true }).click();
  await expect(search).toHaveValue('');
});

test('new snapshots replace comparisons together; older or malformed data cannot erase them', async ({ page }) => {
  const { readFileSync } = await import('node:fs');
  const bundled = JSON.parse(readFileSync('public/market.json', 'utf8'));
  const newer = structuredClone(bundled);
  newer.updated_at = '2026-10-20T12:00:00Z';
  newer.items.bo_prime_set.median_now = 999;
  await page.route('**/market.json', route => route.fulfill({ json:newer }));
  await page.goto('/items/bo_prime_set/');
  await expect(page.getByRole('status')).toContainText('Snapshot checked');
  await expect(page.getByText('999 p', { exact:true }).first()).toBeVisible();
  await expect(page.locator('time')).toHaveAttribute('datetime', newer.updated_at);
  const older = structuredClone(bundled); older.updated_at = '2025-01-01T00:00:00Z'; older.items.bo_prime_set.median_now = 888;
  await page.route('**/market.json', route => route.fulfill({ json:older }));
  await page.reload();
  await expect(page.getByRole('status')).toContainText('Snapshot checked');
  await expect(page.locator('time')).toHaveAttribute('datetime', bundled.updated_at);
  await expect(page.getByText('888 p', { exact:true })).toHaveCount(0);
  await page.route('**/market.json', route => route.fulfill({ json:null }));
  await page.reload();
  await expect(page.getByRole('status')).toContainText('Refresh unavailable');
  await expect(page.locator('time')).toHaveAttribute('datetime', bundled.updated_at);
});

test('enlarged text and keyboard navigation keep the comparison reachable', async ({ page }) => {
  await page.setViewportSize({ width:640, height:480 });
  await page.goto('/guides/sell-prime-sets-or-parts/');
  await expect(page.getByRole('radio', { name:'System' })).toBeVisible();
  await page.evaluate(() => { document.documentElement.style.zoom = '2'; });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  const table = page.getByRole('region', { name:/Bo Prime price comparison/ });
  await table.focus(); await page.keyboard.press('ArrowRight');
  await expect(table).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await expect(page.getByRole('link', { name:/Skip bo prime price comparison/ })).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('note', { name:'End of bo prime price comparison' })).toBeFocused();
});
