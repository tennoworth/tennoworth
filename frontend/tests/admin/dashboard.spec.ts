import { expect, test } from '@playwright/test';

test('drafts stay private, publish is reviewed, edits keep identity, and withdrawal clears the feed', async ({ page, request }) => {
  await page.goto('/admin.html');
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

for (const mode of ['light', 'dark'] as const) for (const width of [320, 768, 1440]) {
  test(`editor remains usable in ${mode} at ${width}px and short height`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: mode });
    await page.setViewportSize({ width, height: 480 });
    await page.goto('/admin.html');
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
