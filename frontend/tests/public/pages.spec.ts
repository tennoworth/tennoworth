import { test, expect } from '@playwright/test';
import { pages, pilotItems, pilotSets, itemPath } from '../../src/public-site/pages';
import { readFileSync } from 'node:fs';
import { decisionAnswer, decisionText, formatStampUtc, priceText, publicData } from '../../src/public-site/model';

// The built pages carry the bundled snapshot, which every desktop release
// refreshes. Expected text comes from it; model.test.ts pins the arithmetic.
function builtBo() {
  const answer = decisionAnswer(publicData(JSON.parse(readFileSync('public/market.json', 'utf8'))), 'bo_prime_set')!;
  expect(answer.outcome).not.toBe('unavailable');
  return { answer, text: decisionText(answer), day: formatStampUtc(answer.stamp) };
}

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
    page.on('console', message => { if (message.type() === 'warning' && message.text().includes('hydration')) errors.push(`${page.url()}: ${message.text()}`); });
    for (const path of [...pages.map(page => page.path), itemPath(pilotItems[0])]) {
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
    if (path.includes('/items/') || path === '/guides/sell-prime-sets-or-parts/' || path.includes('/tools/') || path === '/what-to-sell/') await expect(page.getByText('Snapshot ', { exact: false }).first()).toBeVisible();
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
  const bundledAt = Date.parse(bundled.updated_at);
  const newer = structuredClone(bundled);
  newer.updated_at = new Date(bundledAt + 60 * 60 * 1000).toISOString();
  newer.items.bo_prime_set.median_now = 999;
  await page.route('**/market.json', route => route.fulfill({ json:newer }));
  await page.goto('/items/bo_prime_set/');
  await expect(page.getByRole('status')).toContainText('Snapshot checked');
  await expect(page.getByText('999 p', { exact:true }).first()).toBeVisible();
  await expect(page.locator('time')).toHaveAttribute('datetime', newer.updated_at);
  const older = structuredClone(bundled); older.updated_at = new Date(bundledAt - 60 * 60 * 1000).toISOString(); older.items.bo_prime_set.median_now = 888;
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

test('ducat comparisons exclude set totals in initial HTML and after a snapshot refresh', async ({ page, request }) => {
  await page.goto('/items/bo_prime_set/');
  await expect(page.locator('dt').filter({hasText:'Individual-part ducats'}).locator('+ dd')).toHaveText('Unavailable');
  const html = await (await request.get('/tools/ducats-vs-platinum/')).text();
  expect(html.match(/<tbody>([\s\S]*?)<\/tbody>/)?.[1]).not.toMatch(/Prime Set<\/a>/);
  const { readFileSync } = await import('node:fs');
  const snapshot = JSON.parse(readFileSync('public/market.json','utf8'));
  snapshot.updated_at = new Date(Date.parse(snapshot.updated_at) + 1000).toISOString();
  snapshot.items.bo_prime_set.median_now = 0.01;
  await page.route('**/market.json', route => route.fulfill({json:snapshot}));
  await page.goto('/tools/ducats-vs-platinum/');
  await expect(page.getByRole('status')).toContainText('Snapshot checked');
  await expect(page.locator('tbody')).not.toContainText('Prime Set');
  await page.getByRole('searchbox').fill('Bo Prime Ornament');
  await expect(page.locator('tbody')).toContainText('Bo Prime Ornament');
});

test('troubleshooting is a complete static answer without a market request', async ({ page }) => {
  const requests: string[] = [];
  page.on('request', request => { if (request.url().endsWith('/market.json')) requests.push(request.url()); });
  await page.goto('/guides/warframe-market-items-not-selling/');
  await expect(page.getByRole('radio', {name:'System'})).toBeVisible();
  await expect(page.getByRole('heading', {name:'2. Check that the listing is visible'})).toBeVisible();
  await expect(page.getByText('A check does not automatically reprice it.', {exact:false})).toBeVisible();
  expect(requests).toEqual([]);
});

test('desktop demonstration reuses the themed sample and its keyboard tabs', async ({ page }) => {
  await page.goto('/desktop/');
  await expect(page.getByText('Illustrative sample data:', {exact:false})).toBeVisible();
  await page.getByRole('tab', {name:'Sell',exact:true}).focus();
  await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('tab',{name:'Set picks',exact:true})).toHaveAttribute('aria-selected','true');
  await expect(page.getByRole('tabpanel')).toContainText('Saryn Prime Set');
  await page.getByRole('radio',{name:'Dark',exact:true}).click();
  await expect(page.getByRole('tab',{name:'Set picks',exact:true})).toHaveAttribute('aria-selected','true');
});

test('dated history loads only on demand, preserves gaps, pages and retries failures', async ({ page }) => {
  let loads = 0;
  await page.route('**/history.json', route => { loads++; return route.fulfill({status:503,body:'Unavailable'}); });
  await page.goto('/items/bo_prime_set/');
  await expect(page.getByRole('radio',{name:'System'})).toBeVisible();
  expect(loads).toBe(0);
  await page.getByRole('button',{name:'Load dated history'}).click();
  await expect(page.getByText('Dated history is unavailable or cannot be matched', {exact:false})).toBeVisible();
  const start = '2025-12-01'; const days = 120;
  const through = new Date(Date.parse(start+'T00:00:00Z') + (days-1)*86_400_000).toISOString().slice(0,10);
  const medians: Array<number|null> = Array(days).fill(12); medians[119] = null;
  const history = {start,days,through,generated_at:'2026-04-01T00:00:00Z',items:{bo_prime_set:{median:medians,volume:Array(days).fill(0)}},missing_days:[through]};
  await page.route('**/history.json', route => { loads++; return route.fulfill({json:history}); });
  await page.getByRole('button',{name:'Retry dated history'}).click();
  const region = page.locator('.public-table').filter({has:page.getByRole('columnheader',{name:'Date (UTC)'})});
  await expect(region.locator('tbody tr')).toHaveCount(100);
  await expect(region.locator('tbody tr').first()).toContainText(through);
  await expect(region.locator('tbody tr').first()).toContainText('Unavailable');
  await page.getByRole('navigation',{name:'Price history pages'}).getByRole('button',{name:'Next'}).click();
  await expect(region.locator('tbody tr')).toHaveCount(20);
  await page.getByRole('radio',{name:'Dark',exact:true}).click();
  await expect(region.locator('tbody tr')).toHaveCount(20);
  for (const mode of ['Light', 'Dark']) for (const [width, height] of [[320,740], [900,480], [1440,900]]) {
    await page.setViewportSize({width,height});
    await page.getByRole('radio',{name:mode,exact:true}).click();
    await expect(region.locator('tbody tr')).toHaveCount(20);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await page.getByRole('heading',{name:'Dated price history',exact:true}).evaluate(element => element.scrollIntoView({block:'start'}));
    await page.screenshot({path:`test-results/public/history-${test.info().project.name}-${mode}-${width}.png`});
  }
  expect(loads).toBe(2);
});

test('desktop sample styles are present before JavaScript', async ({ browser }) => {
  const context = await browser.newContext({javaScriptEnabled:false,viewport:{width:900,height:480}});
  const page = await context.newPage();await page.goto('http://127.0.0.1:5189/desktop/');
  await expect(page.getByRole('tabpanel')).toBeVisible();
  expect(await page.locator('.rail-panel .tabs').evaluate(element => getComputedStyle(element).display)).toBe('flex');
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await context.close();
});


test('decision summaries and dated metadata are in the initial HTML for pilot sets', async ({ request }) => {
  const { readFileSync } = await import('node:fs');
  const bundled = JSON.parse(readFileSync('public/market.json', 'utf8'));
  const snapshotDay = formatStampUtc(bundled.updated_at);
  for (const slug of pilotSets) {
    const html = await (await request.get(itemPath(slug))).text();
    expect(html, slug).toContain('Set or parts?');
    expect(html, slug).toMatch(/class="decision/);
    expect(html, slug).toContain('summed parts');
    expect(html, slug).toContain('quantity-weighted sum of component medians, not a bundle median');
    expect(html, slug).toContain('Copy decision');
    expect(html, slug).toContain('Decision text');
    expect(html, slug).toMatch(/<meta property="og:title" content="[^"]+summed parts[^"]*" \/>/);
    expect(html, slug).toMatch(new RegExp(`<meta\\s+property="og:description"\\s+content="[^"]+Snapshot ${snapshotDay}[^"]*"`));
  }
  const bo = await (await request.get(itemPath('bo_prime_set'))).text();
  expect(bo).toContain(builtBo().text.headline);
  expect(bo).toContain('One set versus 4 component copies across 3 distinct items.');
  expect(bo).toContain('<title>Bo Prime Set Prices &amp; Selling Context | TennoWorth</title>');
  const part = await (await request.get(itemPath('bo_prime_blueprint'))).text();
  expect(part).not.toContain('Set or parts?');
  expect(part).not.toContain('Check part orders');
});

test('a newer snapshot refreshes the decision atomically while the built preview stays dated', async ({ page, request }) => {
  const { readFileSync } = await import('node:fs');
  const bundled = JSON.parse(readFileSync('public/market.json', 'utf8'));
  const newer = structuredClone(bundled);
  newer.updated_at = new Date(Date.parse(bundled.updated_at) + 3_600_000).toISOString();
  newer.items.bo_prime_set.median_now = 70;
  newer.items.bo_prime_set.vol = 72;
  newer.items.bo_prime_blueprint.median_now = 12;
  newer.items.bo_prime_handle.median_now = 15;
  newer.items.bo_prime_ornament.median_now = 19;
  await page.route('**/market.json', route => route.fulfill({ json: newer }));
  await page.goto(itemPath('bo_prime_set'));
  const block = page.locator('.decision');
  await expect(block).toContainText('The set is 5p above the summed parts.');
  await expect(block).toContainText('65p');
  const built = builtBo();
  await expect(block).toContainText(`Updated since this page was built (${built.day})`);
  if (built.answer.partsTotal !== 65) await expect(block).not.toContainText(priceText(built.answer.partsTotal));
  await expect(page.getByText('65 p', { exact: true }).first()).toBeVisible();
  await expect(block.locator('pre')).toContainText('65p');
  await expect(block.locator('pre')).toContainText('not evidence of sale speed');
  const html = await (await request.get(itemPath('bo_prime_set'))).text();
  expect(html).toContain(`${priceText(built.answer.setPrice)} set vs ${priceText(built.answer.partsTotal)} summed parts (${built.day})`);
});

test('missing evidence suppresses the verdict and keeps the links', async ({ page }) => {
  const { readFileSync } = await import('node:fs');
  const newer = JSON.parse(readFileSync('public/market.json', 'utf8'));
  newer.updated_at = new Date(Date.parse(newer.updated_at) + 3_600_000).toISOString();
  delete newer.items.bo_prime_ornament.median_now;
  await page.route('**/market.json', route => route.fulfill({ json: newer }));
  await page.goto(itemPath('bo_prime_set'));
  const block = page.locator('.decision');
  await expect(block).toContainText('Not enough comparable evidence for a set-versus-parts verdict.');
  await expect(block).not.toContainText('above the set');
  await expect(block).toContainText('Unavailable');
  await expect(block.getByRole('link', { name: 'Check current set orders' })).toBeVisible();
  await expect(block.getByRole('button', { name: 'Copy decision' })).toBeEnabled();
});

test('decision answer survives without JavaScript and announces copy failure', async ({ browser }) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  await page.goto('http://127.0.0.1:5189' + itemPath('bo_prime_set'));
  await expect(page.getByRole('heading', { name: builtBo().text.headline })).toBeVisible();
  await expect(page.locator('.decision-quantities')).toHaveText('One set versus 4 component copies across 3 distinct items.');
  await expect(page.getByRole('button', { name: 'Copy decision' })).toBeDisabled();
  await expect(page.getByText('Decision text')).toBeVisible();
  await expect(page.getByRole('link', { name: 'Check current set orders' })).toHaveAttribute('href', 'https://warframe.market/items/bo_prime_set');
  await context.close();
});

test('decision copy reports success', async ({ browser, browserName }) => {
  test.skip(browserName === 'webkit', 'WebKit does not expose clipboard permissions');
  const context = await browser.newContext();
  await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: 'http://127.0.0.1:5189' });
  const page = await context.newPage();
  await page.goto(itemPath('bo_prime_set'));
  await page.getByRole('button', { name: 'Copy decision' }).click();
  await expect(page.getByText('Decision copied.')).toBeVisible();
  const copied = await page.evaluate(() => navigator.clipboard.readText());
  const { answer } = builtBo();
  expect(copied).toContain(`Bo Prime Set: set ${priceText(answer.setPrice)} vs ${priceText(answer.partsTotal)} summed parts`);
  expect(copied).toContain('not evidence of sale speed');
  await context.close();
});

test('decision copy falls back when the Clipboard API is absent', async ({ browser }) => {
  const context = await browser.newContext();
  const page = await context.newPage();
  await page.addInitScript(() => {
    Object.defineProperty(Navigator.prototype, 'clipboard', { configurable: true, get: () => undefined });
    document.execCommand = () => true;
  });
  await page.goto(itemPath('bo_prime_set'));
  await page.getByRole('button', { name: 'Copy decision' }).click();
  await expect(page.getByText('Decision copied.')).toBeVisible();
  await context.close();
});

test('decision copy failure opens the selectable text and announces it', async ({ browser }) => {
  const context = await browser.newContext();
  const page = await context.newPage();
  await page.addInitScript(() => {
    Object.defineProperty(Navigator.prototype, 'clipboard', { configurable: true, get: () => ({ writeText: () => Promise.reject(new Error('blocked')) }) });
    document.execCommand = () => false;
  });
  await page.goto(itemPath('bo_prime_set'));
  await page.getByRole('button', { name: 'Copy decision' }).click();
  await expect(page.getByText(/Couldn.t copy/)).toBeVisible();
  await expect(page.locator('.decision-text')).toHaveJSProperty('open', true);
  await context.close();
});

test('equal or older snapshots do not replace the decision', async ({ page }) => {
  const { readFileSync } = await import('node:fs');
  const bundled = JSON.parse(readFileSync('public/market.json', 'utf8'));
  const equal = structuredClone(bundled);
  equal.items.bo_prime_set.median_now = 777;
  await page.route('**/market.json', route => route.fulfill({ json: equal }));
  await page.goto(itemPath('bo_prime_set'));
  await expect(page.locator('.decision')).not.toContainText('777p');
  await expect(page.locator('.decision')).toContainText(builtBo().text.headline);
  await expect(page.getByRole('status')).toContainText('Snapshot checked');
});

test('decision is before the prose and exposes component orders on set pages only', async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 740 });
  await page.goto(itemPath('bo_prime_set'));
  const decision = await page.locator('.decision').boundingBox();
  const prose = await page.locator('.public-prose > p').first().boundingBox();
  expect(decision!.y).toBeLessThan(prose!.y);
  const partOrders = await page.getByRole('link', { name: 'Check part orders' }).evaluateAll(links => links.map(link => link.getAttribute('href')));
  expect(partOrders).toEqual(['https://warframe.market/items/bo_prime_blueprint', 'https://warframe.market/items/bo_prime_handle', 'https://warframe.market/items/bo_prime_ornament']);
  const action = await page.getByRole('button', { name: 'Copy decision' }).boundingBox();
  expect(action!.y).toBeLessThan(740);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.goto(itemPath('bo_prime_blueprint'));
  await expect(page.locator('.decision')).toHaveCount(0);
  await expect(page.getByRole('link', { name: 'Check part orders' })).toHaveCount(0);
});

const missing = ['/no-such-page/', '/items/not-a-real-item/', '/guides/nope'];
test('missing pages are a real 404 with the public chrome and no indexable copy', async ({ request }) => {
  const sitemap = await (await request.get('/sitemap.xml')).text();
  expect(sitemap).not.toContain('404');
  for (const path of missing) {
    const response = await request.get(path);
    expect(response.status(), path).toBe(404);
    const html = await response.text();
    expect(html).toContain('<title>Page not found | TennoWorth</title>');
    expect(html).toContain('<meta name="robots" content="noindex">');
    expect(html).not.toContain('rel="canonical"');
    expect(html).not.toMatch(/<script type=|modulepreload|public-page-data/);
    expect(html).toContain('<script src="/theme-boot.js"></script>');
    expect(html).toContain('aria-label="Selling guides"');
    for (const [,link] of html.matchAll(/<a[^>]+href="(\/[^"#]*)/g)) expect((await request.get(link)).status(), link).toBe(200);
  }
});

for (const mode of ['light', 'dark'] as const) test(`the 404 page is themed and fits ${mode}`, async ({ page }) => {
  await page.emulateMedia({ colorScheme: mode });
  for (const size of [{ width:320, height:740 }, { width:900, height:480 }, { width:1440, height:900 }]) {
    await page.setViewportSize(size);
    const response = await page.goto('/no-such-page/');
    expect(response!.status()).toBe(404);
    await expect(page.getByRole('heading', { level:1, name:'Page not found' })).toBeVisible();
    await expect(page.getByRole('navigation', { name:'Selling guides' }).getByRole('link')).toHaveCount(pages.length);
    expect(await page.locator('html').getAttribute('data-look')).toBe('yorha');
    expect(await page.locator('html').getAttribute('data-mode')).toBe(mode);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await page.screenshot({ path: `test-results/public/${test.info().project.name}-${mode}-${size.width}-404.png`, fullPage: true });
  }
});

test('the 404 search works without JavaScript and lands on the restored search', async ({ browser, page }) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const noJs = await context.newPage();
  await noJs.goto('http://127.0.0.1:5189/no-such-page/');
  await noJs.getByRole('searchbox', { name:'Search the market' }).fill('bo prime');
  await noJs.getByRole('button', { name:'Search' }).click();
  expect(new URL(noJs.url()).search).toBe('?q=bo+prime');
  await context.close();
  await page.goto('/no-such-page/');
  await page.getByRole('searchbox', { name:'Search the market' }).fill('bo prime');
  await page.keyboard.press('Enter');
  await expect(page.getByRole('textbox', { name:'Search items' })).toHaveValue('bo prime');
  await expect(page.getByRole('region', { name:/Search results/ }).getByRole('link', { name:'Bo Prime Set', exact:true })).toBeVisible();
});

test('Back from an item page returns to the same search', async ({ page }) => {
  await page.goto('/');
  const search = page.getByRole('textbox', { name:'Search items' });
  await search.fill('bo prime');
  await expect(page).toHaveURL(/\/\?q=bo\+prime$/);
  const results = page.getByRole('region', { name:/Search results/ });
  await results.getByRole('link', { name:'Bo Prime Set', exact:true }).click();
  await expect(page).toHaveURL(/\/items\/bo_prime_set\/$/);
  await page.goBack();
  await expect(page).toHaveURL(/\/\?q=bo\+prime$/);
  await expect(search).toHaveValue('bo prime');
  await expect(results.getByRole('link', { name:'Bo Prime Set', exact:true })).toBeVisible();
});
