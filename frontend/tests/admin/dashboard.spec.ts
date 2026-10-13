import { expect, test } from '@playwright/test';

test('drafts stay private, publish is reviewed, edits keep identity, and withdrawal clears the feed', async ({ page, request }) => {
  await page.goto('/admin.html#announcements');
  await expect(page.getByText('Signed in: local-preview@example.invalid')).toBeVisible();
  await page.getByRole('button', { name: 'New announcement', exact: true }).click();
  const title = `Verified change ${Date.now()}`;
  await page.getByLabel('Title', { exact: true }).fill(title);
  await page.getByLabel('Message', { exact: true }).fill('Exchange spare copies before the deadline. Keep crafting copies.');
  await page.getByLabel('Source URL').fill('https://forums.warframe.com/topic/123-test/');
  await page.getByRole('button', { name: 'Save draft', exact: true }).click();
  await expect(page.getByText('Draft saved. The public feed has not changed.')).toBeVisible();
  await page.getByRole('button', { name: 'Review publication', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Publish announcement', exact: true })).toBeVisible();
  // Another tab changes the saved draft. Refresh must not publish wording the editor did not review.
  let concurrent = await (await request.get('http://127.0.0.1:8084/api/announcements')).json();
  const other = concurrent.snapshot.messages.find((entry: { draft: { title: string } }) => entry.draft.title === title);
  const changed = { ...other.draft, body: 'Another tab changed this guidance. Keep crafting copies.' };
  await request.post('http://127.0.0.1:8084/api/announcements/change', { headers: { origin: 'http://127.0.0.1:5194' }, data: { request_id: crypto.randomUUID(), expected_revision: concurrent.snapshot.revision, operation: 'save', id: other.id, content: changed } });
  await page.getByRole('button', { name: 'Refresh status', exact: true }).click();
  await expect(page.getByText(/This draft changed on the publisher/)).toBeVisible();
  await expect(page.getByRole('button', { name: 'Publish announcement', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Review publication', exact: true })).toBeDisabled();
  await page.getByRole('button', { name: 'Reload saved draft', exact: true }).click();
  await expect(page.getByLabel('Message', { exact: true })).toHaveValue(changed.body);
  let state = await (await request.get('http://127.0.0.1:8084/api/announcements')).json();
  const message = state.snapshot.messages.find((entry: { draft: { title: string } }) => entry.draft.title === title);
  expect(state.snapshot.feed.messages.some((entry: { id: string }) => entry.id === message.id)).toBe(false);
  await page.getByRole('button', { name: 'Review publication', exact: true }).click();
  await expect(page.getByText('This is a new announcement. Eligible desktop clients may receive one notification.')).toBeVisible();
  await page.getByRole('button', { name: 'Publish announcement', exact: true }).click();
  await expect(page.getByText('Feed published. Running clients will pick it up on their next check.')).toBeVisible();
  await page.getByLabel('Message', { exact: true }).fill('Updated guidance. Keep crafting copies and compare the Platinum price.');
  await page.getByRole('button', { name: 'Save draft', exact: true }).click();
  await expect(page.getByText('Draft saved. The public feed has not changed.')).toBeVisible();
  await page.getByRole('button', { name: 'Review publication', exact: true }).click();
  await expect(page.getByText('This ID has already been published. This edit will not repeat its desktop notification.')).toBeVisible();
  await page.getByRole('button', { name: 'Publish announcement', exact: true }).click();
  await expect(page.getByText('Feed published. Running clients will pick it up on their next check.')).toBeVisible();
  state = await (await request.get('http://127.0.0.1:8084/api/announcements')).json();
  expect(state.snapshot.feed.messages.find((entry: { id: string }) => entry.id === message.id).body).toContain('Updated guidance');
  await page.getByRole('button', { name: 'Review withdrawal', exact: true }).click();
  await page.getByRole('button', { name: 'Withdraw announcement', exact: true }).click();
  await expect(page.getByText('Withdrawal published. Existing popups cannot be recalled.')).toBeVisible();
  state = await (await request.get('http://127.0.0.1:8084/api/announcements')).json();
  expect(state.snapshot.feed.messages.some((entry: { id: string }) => entry.id === message.id)).toBe(false);
});

async function draft(page: import('@playwright/test').Page, title: string) {
  await page.goto('/admin.html#announcements');
  await expect(page.getByText('Signed in: local-preview@example.invalid')).toBeVisible();
  await page.getByRole('button', { name: 'New announcement', exact: true }).click();
  await page.getByLabel('Title', { exact: true }).fill(title);
  await page.getByLabel('Message', { exact: true }).fill('Exchange spare copies before the deadline. Keep crafting copies.');
  await page.getByLabel('Source URL').fill('https://forums.warframe.com/topic/123-test/');
}

test('an expired sign-in keeps unsaved edits through the reload and retries the same request', async ({ page, request, browserName }) => {
  const title = `Expired session ${Date.now()}`;
  await draft(page, title);
  // Cloudflare Access answers an expired session at the edge with a login redirect,
  // before the publisher sees it. Playwright's WebKit cannot fulfill a redirect, so it
  // checks the same handling through the publisher's own 401.
  await page.route('**/api/announcements/change', route => route.fulfill(browserName === 'webkit'
    ? { status: 401, json: { message: 'Sign in through the private dashboard to continue.' } }
    : { status: 302, headers: { location: 'https://team.cloudflareaccess.com/cdn-cgi/access/login' } }));
  await page.getByRole('button', { name: 'Save draft', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('Your sign-in has expired');
  await page.unroute('**/api/announcements/change');
  await page.reload();
  await expect(page.getByText('Restored your unsaved edits from before the page reloaded.')).toBeVisible();
  await expect(page.getByLabel('Title', { exact: true })).toHaveValue(title);
  await page.getByRole('button', { name: 'Retry request', exact: true }).click();
  await expect(page.getByText('Draft saved. The public feed has not changed.')).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Edit announcement' })).toBeVisible();
  const state = await (await request.get('http://127.0.0.1:8084/api/announcements')).json();
  expect(state.snapshot.messages.filter((entry: { draft: { title: string } }) => entry.draft.title === title)).toHaveLength(1);
  await page.reload();
  await expect(page.getByText('Restored your unsaved edits')).toHaveCount(0);
});

test('a correction can go out as a new announcement so dismissed readers see it', async ({ page, request }) => {
  const title = `Correction source ${Date.now()}`;
  await draft(page, title);
  await page.getByRole('button', { name: 'Save draft', exact: true }).click();
  await page.getByRole('button', { name: 'Review publication', exact: true }).click();
  await page.getByRole('button', { name: 'Publish announcement', exact: true }).click();
  await expect(page.getByText('Feed published. Running clients will pick it up on their next check.')).toBeVisible();
  await page.getByLabel('Message', { exact: true }).fill('Corrected deadline. Keep crafting copies.');
  await page.getByRole('button', { name: 'Save draft', exact: true }).click();
  await page.getByRole('button', { name: 'Review publication', exact: true }).click();
  await expect(page.getByText(/will not see this edit/)).toBeVisible();
  await page.getByRole('button', { name: 'Copy into a new announcement', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'New announcement' })).toBeVisible();
  await expect(page.getByLabel('Message', { exact: true })).toHaveValue('Corrected deadline. Keep crafting copies.');
  await page.getByRole('button', { name: 'Save draft', exact: true }).click();
  await expect(page.getByText('Draft saved. The public feed has not changed.')).toBeVisible();
  const state = await (await request.get('http://127.0.0.1:8084/api/announcements')).json();
  const copies = state.snapshot.messages.filter((entry: { draft: { title: string } }) => entry.draft.title === title);
  expect(copies).toHaveLength(2);
  expect(copies.filter((entry: { ever_published: boolean }) => !entry.ever_published)).toHaveLength(1);
});

test('publication status says whether the public feed serves the newest revision', async ({ page }) => {
  const strip = page.getByRole('region', { name: 'Publication status' });
  await page.route('**/api/announcements/public', async route => {
    const check = await (await route.fetch()).json();
    await route.fulfill({ json: { ...check, url: 'https://tennoworth.app/announcements.json', state: 'behind', served_updated_at: '2026-01-01T00:00:00Z' } });
  });
  await page.goto('/admin.html#announcements');
  await expect(strip).toContainText('tennoworth.app still serves the feed from');
  await page.unroute('**/api/announcements/public');
  // The local publisher has no public site; it checks its own export.
  const title = `Confirmed ${Date.now()}`;
  await draft(page, title);
  await page.getByRole('button', { name: 'Save draft', exact: true }).click();
  await page.getByRole('button', { name: 'Review publication', exact: true }).click();
  await page.getByRole('button', { name: 'Publish announcement', exact: true }).click();
  await expect(strip).toContainText('The local export serves this feed');
  await page.route('**/api/announcements/public', route => route.fulfill({ status: 503, json: { message: 'No public feed is configured for this publisher.' } }));
  await page.getByRole('button', { name: 'Refresh status', exact: true }).click();
  await expect(strip).toContainText('Public feed not checked: No public feed is configured for this publisher.');
});

test('calendar suggestions open as unsaved drafts and stay gone once saved or dismissed', async ({ page, request }) => {
  const hour = 3_600_000;
  const iso = (ms: number) => new Date(ms).toISOString().replace(/\.\d{3}Z$/, 'Z');
  const arrives = Math.ceil(Date.now() / hour) * hour + 72 * hour;
  // Rotation keys carry its dates, so each run proposes fresh suggestions.
  const ends = arrives + Math.floor(Math.random() * 48) * hour;
  await page.route('**/market.json', route => route.fulfill({ json: {
    updated_at: iso(Date.now()), platform: 'pc', item_count: 0, catalog_count: 0, catalog: {}, items: {},
    baro: { activation: iso(arrives), expiry: iso(arrives + 48 * hour), location: `Relay ${ends}` },
    calendar: { resurgence_current: { from: iso(arrives - 30 * 24 * hour), to: iso(ends), frames: ['protea_prime_set'] } },
  } }));
  await page.goto('/admin.html#announcements');
  const panel = page.getByRole('region', { name: 'Suggested drafts' });
  await expect(panel.getByText(/Baro Ki'Teer arrives/)).toBeVisible();
  await expect(panel.getByText(/Protea Prime relics leave Prime Resurgence/)).toBeVisible();

  await panel.getByRole('button', { name: 'Open as draft', exact: true }).first().click();
  await expect(page.getByText('Suggested draft opened.', { exact: false })).toBeVisible();
  await expect(page.getByLabel('Title', { exact: true })).toHaveValue(/^Baro Ki'Teer arrives/);
  await expect(page.getByLabel('Source URL')).toHaveValue('https://wiki.warframe.com/w/Baro_Ki%27Teer');
  await expect(page.getByText('Unsaved edits', { exact: true })).toBeVisible();
  // Nothing reached the publisher yet.
  let state = await (await request.get('http://127.0.0.1:8084/api/announcements')).json();
  const title = await page.getByLabel('Title', { exact: true }).inputValue();
  const before = state.snapshot.messages.filter((entry: { draft: { title: string } }) => entry.draft.title === title).length;
  await page.getByRole('button', { name: 'Save draft', exact: true }).click();
  await expect(page.getByText('Draft saved. The public feed has not changed.')).toBeVisible();
  state = await (await request.get('http://127.0.0.1:8084/api/announcements')).json();
  expect(state.snapshot.messages.filter((entry: { draft: { title: string } }) => entry.draft.title === title)).toHaveLength(before + 1);
  expect(state.snapshot.feed.messages.some((entry: { title: string }) => entry.title === title)).toBe(false);
  await expect(panel.getByText(/Baro Ki'Teer arrives/)).toHaveCount(0);

  await panel.getByRole('button', { name: 'Dismiss', exact: true }).click();
  await expect(panel).toHaveCount(0);
  await page.reload();
  await expect(page.getByText('Signed in: local-preview@example.invalid')).toBeVisible();
  await expect(page.getByRole('region', { name: 'Suggested drafts' })).toHaveCount(0);
});

test('official dates are reviewed, published into the feed, corrected in place and removed', async ({ page, request }) => {
  await page.goto('/admin.html#announcements');
  await expect(page.getByText('Signed in: local-preview@example.invalid')).toBeVisible();
  const panel = page.getByRole('region', { name: 'Official dates' });
  const title = `Devstream ${Date.now()}`;
  const start = new Date(Date.now() + 3 * 86_400_000).toISOString().slice(0, 16);
  await panel.getByLabel('Event kind').selectOption('devstream');
  await panel.getByLabel('Event title').fill(title);
  await panel.getByLabel('Event starts at (UTC)').fill(start);
  // Without an approved source the date cannot be reviewed.
  await expect(panel.getByRole('button', { name: 'Review event' })).toBeDisabled();
  await panel.getByLabel("Link to DE's announcement").fill('https://forums.warframe.com/topic/devstream/');
  await panel.getByRole('button', { name: 'Review event' }).click();
  await expect(panel.getByText('Publish this date to the player calendar?')).toBeVisible();
  await panel.getByRole('button', { name: 'Publish event', exact: true }).click();
  await expect(panel.getByText('Event published. Calendars pick it up on their next feed check.')).toBeVisible();
  let state = await (await request.get('http://127.0.0.1:8084/api/announcements')).json();
  const published = state.snapshot.feed.events.find((event: { title: string }) => event.title === title);
  expect(published).toMatchObject({ kind: 'devstream', starts_at: `${start}:00Z`, source_url: 'https://forums.warframe.com/topic/devstream/' });

  const row = panel.locator('.row', { hasText: title });
  await row.getByRole('button', { name: 'Edit' }).click();
  await panel.getByLabel('Event title').fill(`${title} (moved)`);
  await panel.getByRole('button', { name: 'Review correction' }).click();
  await panel.getByRole('button', { name: 'Publish correction' }).click();
  await expect(panel.getByText('Event corrected.', { exact: false })).toBeVisible();
  state = await (await request.get('http://127.0.0.1:8084/api/announcements')).json();
  const corrected = state.snapshot.feed.events.filter((event: { id: string }) => event.id === published.id);
  expect(corrected.map((event: { title: string }) => event.title)).toEqual([`${title} (moved)`]);

  await panel.locator('.row', { hasText: `${title} (moved)` }).getByRole('button', { name: 'Remove' }).click();
  await panel.getByRole('button', { name: 'Remove event' }).click();
  await expect(panel.getByText('Event removed from the calendar.')).toBeVisible();
  state = await (await request.get('http://127.0.0.1:8084/api/announcements')).json();
  expect((state.snapshot.feed.events ?? []).some((event: { id: string }) => event.id === published.id)).toBe(false);
  expect(state.snapshot.history.slice(0, 3).map((entry: { operation: string }) => entry.operation)).toEqual(['event-remove', 'event-publish', 'event-publish']);
});

for (const mode of ['light', 'dark'] as const) for (const width of [320, 768, 1440]) {
  test(`editor remains usable in ${mode} at ${width}px and short height`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: mode });
    await page.setViewportSize({ width, height: 480 });
    await page.goto('/admin.html#announcements');
    await expect(page.getByText('Signed in: local-preview@example.invalid')).toBeVisible();
    await page.getByLabel('Title', { exact: true }).fill('A long title that should remain readable when the private dashboard is narrow');
    await page.getByLabel('Message', { exact: true }).fill('Unsaved guidance stays in place through refreshes, themes, resizes and failed saves.');
    await page.getByRole('button', { name: 'Refresh status', exact: true }).click();
    await expect(page.getByLabel('Title', { exact: true })).toHaveValue(/A long title/);
    await page.getByRole('button', { name: 'New announcement', exact: true }).click();
    await expect(page.getByRole('button', { name: 'Keep editing', exact: true })).toBeVisible();
    await page.getByRole('button', { name: 'Keep editing', exact: true }).click();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await page.getByLabel('Source URL').fill('https://forums.warframe.com/');
    await page.route('**/api/announcements/change', route => route.fulfill({ status: 503, json: { message: 'Publication storage is unavailable.' } }));
    await page.getByRole('button', { name: 'Save draft', exact: true }).click();
    await expect(page.getByRole('alert')).toHaveText('Publication storage is unavailable.');
    await expect(page.getByLabel('Message', { exact: true })).toHaveValue(/Unsaved guidance/);
    await expect(page.getByRole('button', { name: 'Retry request', exact: true })).toBeVisible();
    await page.screenshot({ path: `test-results/admin/dashboard-${mode}-${width}.png`, fullPage: true });
  });
}
