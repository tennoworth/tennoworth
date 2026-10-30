import { expect, test } from '@playwright/test';

// The local publisher simulates the box: a requested job "runs" for a few seconds.
test('status leads, a job runs only after confirmation, and repeats are refused', async ({ page }) => {
  await page.goto('/admin.html');
  await expect(page.getByRole('heading', { name: 'Box status' })).toBeVisible();
  await expect(page.getByRole('link', { name: 'Status' })).toHaveAttribute('aria-current', 'page');
  const jobs = page.getByRole('region', { name: 'Run now' });
  // Earlier runs against a reused local server may still be pacing a job; use one that is free.
  const run = jobs.getByRole('button', { name: /^Run / }).and(page.locator(':enabled')).first();
  const label = (await run.getAttribute('aria-label'))!.replace(/^Run /, '');
  const row = jobs.getByRole('row').filter({ hasText: label });
  await run.click();
  const confirm = page.getByRole('group', { name: 'Confirm job' });
  await expect(confirm).toContainText(`Start ${label.toLowerCase()} now?`);
  await confirm.getByRole('button', { name: 'Cancel' }).click();
  await expect(confirm).toHaveCount(0);
  await run.click();
  await page.getByRole('group', { name: 'Confirm job' }).getByRole('button', { name: 'Start now' }).click();
  await expect(page.getByRole('status')).toContainText(`${label} requested.`);
  await expect(row).toContainText('Running now');
  await expect(row).toContainText('by local-preview@example.invalid');
  await expect(row.getByRole('button', { name: `Run ${label}` })).toBeDisabled();
  await expect(row).toContainText(/Finished just now|Finished \d+ min ago/, { timeout: 20_000 });
  await expect(row).toContainText('Requested recently');
  await expect(row.getByRole('button', { name: `Run ${label}` })).toBeDisabled();
});

test('a refused job explains itself and a draft survives a look at the box', async ({ page }) => {
  await page.goto('/admin.html#announcements');
  await page.getByRole('button', { name: 'New announcement', exact: true }).click();
  await page.getByLabel('Title', { exact: true }).fill('Draft kept across tabs');
  await page.getByRole('link', { name: 'Status' }).click();
  await expect(page.getByRole('heading', { name: 'Box status' })).toBeVisible();
  await page.route('**/api/jobs/run', route => route.fulfill({ status: 409, json: { message: 'Run market sweep was not started. Running now.' } }));
  const run = page.getByRole('region', { name: 'Run now' }).getByRole('button', { name: /^Run / }).and(page.locator(':enabled')).first();
  await run.click();
  await page.getByRole('group', { name: 'Confirm job' }).getByRole('button', { name: 'Start now' }).click();
  await expect(page.getByRole('alert')).toHaveText('Run market sweep was not started. Running now.');
  await page.getByRole('link', { name: 'Announcements' }).click();
  await expect(page.getByLabel('Title', { exact: true })).toHaveValue('Draft kept across tabs');
});

test('a sweep in progress still shows when the last one finished', async ({ page }) => {
  const checkedAt = new Date(Date.now() - 10 * 60_000);
  const unit = (name: string, extra: object = {}) => ({ unit: name, installed: true, active_state: 'inactive', sub_state: 'dead', result: 'success', started_at: null, finished_at: null, ...extra });
  // systemd reports no exit time for a sweep while the next one runs.
  const sweep = unit('wfm-scrape.service', { active_state: 'activating', sub_state: 'start', started_at: new Date(Date.now() - 30 * 60_000).toISOString() });
  await page.route('**/api/status', route => route.fulfill({ json: {
    actor: 'local-preview@example.invalid',
    status: {
      collected_at: new Date().toISOString(), problems: [], failed_units: [], alerts: [], web_bundle: null,
      backup: { latest_at: new Date(Date.now() - 2 * 60_000).toISOString(), count: 3, error: null },
      host_check: { ready: false, checked_at: checkedAt.toISOString(), errors: ['scraper is behind'], warnings: [], csv_rows: 2631, snapshot_age_seconds: 82 * 60, deploy_revision: null, checkout_head: null, deploy_lag: 'behind', pending_scraper_commits: 4, disk_free_bytes: null, disk_floor_bytes: null },
      services: [{ label: 'Market sweep', service: sweep, scheduled: true, next_run: null, health: null }],
      jobs: [],
    },
  } }));
  await page.goto('/admin.html');
  const summary = page.getByRole('region', { name: 'Box summary' });
  await expect(summary).toContainText('1 error · 0 warnings');
  await expect(summary).toContainText('Another sweep is running, started 30 min ago');
  await expect(summary).not.toContainText('Never');
  await expect(summary).toContainText('1 h 32 min ago');
});

for (const mode of ['light', 'dark'] as const) for (const width of [320, 768, 1440]) {
  test(`status page fits in ${mode} at ${width}px`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: mode });
    await page.setViewportSize({ width, height: 480 });
    await page.goto('/admin.html');
    await expect(page.getByRole('region', { name: 'Run now' })).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await page.screenshot({ path: `test-results/admin/status-${mode}-${width}.png`, fullPage: true });
  });
}
