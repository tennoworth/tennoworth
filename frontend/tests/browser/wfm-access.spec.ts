import { expect, test } from '@playwright/test';
import defaults from '../../../tests/fixtures/pacing.json' with { type: 'json' };
import { WFM_ACCESS_EVENT } from '../../src/contracts/generated/desktop';

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} market access preserves draft edits through a pause and recovery`, async ({ page }, info) => {
    const errors: string[] = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.emulateMedia({ colorScheme: theme });
    await page.addInitScript(() => {
      const listeners = new Map<string, Set<(event: unknown) => void>>();
      let runtime: any;
      Object.defineProperty(window, '__TAURI__', { configurable: true, get: () => runtime, set: value => {
        runtime = value;
        runtime.event.listen = async (name: string, fn: (event: unknown) => void) => {
          const callbacks = listeners.get(name) ?? new Set(); callbacks.add(fn); listeners.set(name, callbacks);
          return () => callbacks.delete(fn);
        };
      } });
      (window as any).marketEvent = (name: string, payload: unknown) => listeners.get(name)?.forEach(fn => fn({ payload }));
    });
    await page.goto('/?preview-desktop&sample=session');
    await page.getByRole('button', { name: 'Review batch', exact: true }).click();
    const dialog = page.getByRole('dialog');
    const price = dialog.locator('tbody tr').filter({ hasText: 'Primed Flow' }).locator('input[type=number]').nth(2);
    await price.fill('99');
    const status = { revision: 1, reason: 'Temporary market maintenance', cooldown_until_ms: 0, queue_count: 0, outstanding: 0, requests: 0, throttles: 0, cache_hits: 0, cache_misses: 0, queue_rejections: 0, restrictions: { ...defaults, pause_mutations: true } };
    await page.evaluate(({ event, status }) => (window as any).marketEvent(event, status), { event: WFM_ACCESS_EVENT, status });
    await expect(dialog.getByRole('button', { name: /^Send .* listings$/ })).toBeDisabled();
    for (const [width, height] of [[1440, 900], [768, 600], [320, 480]]) {
      await page.setViewportSize({ width, height });
      await expect(price).toHaveValue('99');
      await expect(dialog.getByRole('status')).toContainText('Paused: listing changes');
      await dialog.getByRole('status').scrollIntoViewIfNeeded();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1)).toBe(true);
      await page.screenshot({ path: info.outputPath(`paused-${theme}-${width}.png`) });
    }
    await page.evaluate(({ event, status }) => (window as any).marketEvent(event, status), { event: WFM_ACCESS_EVENT, status: { ...status, revision: 2, restrictions: defaults, queue_count: 3 } });
    await expect(dialog.getByRole('status')).toContainText('Waiting for market access');
    await expect(price).toHaveValue('99');
    await page.evaluate(({ event, status }) => (window as any).marketEvent(event, status), { event: WFM_ACCESS_EVENT, status: { ...status, revision: 2, restrictions: defaults, cooldown_until_ms: Date.now() + 60_000 } });
    await expect(dialog.getByRole('status')).toContainText('cooling down');
    await expect(dialog.getByRole('button', { name: /^Send .* listings$/ })).toBeDisabled();
    await page.evaluate(({ event, status }) => (window as any).marketEvent(event, status), { event: WFM_ACCESS_EVENT, status: { ...status, revision: 3, restrictions: defaults } });
    await expect(dialog.getByRole('button', { name: /^Send .* listings$/ })).toBeEnabled();
    await expect(price).toHaveValue('99');
    expect(errors).toEqual([]);
  });
  test(`${theme} stopping a batch targets its request and waits for the transmitted item`, async ({ page }, info) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.goto('/?preview-desktop&sample=session');
    await expect(page.getByRole('button', { name: 'Review batch', exact: true })).toBeVisible();
    await page.evaluate(() => {
      const runtime = (window as any).__TAURI__;
      const original = runtime.core.invoke;
      (window as any).planRequests = [];
      (window as any).cancelRequests = [];
      runtime.core.invoke = async (cmd: string, args: any) => {
        if (cmd === 'submit_plan') {
          (window as any).planRequests.push(args.requestId);
          return new Promise(resolve => { (window as any).finishCurrentRequest = () => resolve({ plan_id: 'paused', results: args.items.map((item: any) => ({ slug: item.slug, status: 'pending', message: 'Saved for resume.' })) }); });
        }
        if (cmd === 'cancel_plan') { (window as any).cancelRequests.push(args.requestId); return null; }
        return original(cmd, args);
      };
    });
    await page.getByRole('button', { name: 'Review batch', exact: true }).click();
    const dialog = page.getByRole('dialog');
    await dialog.getByRole('button', { name: /^Send .* listings$/ }).click();
    const stop = dialog.getByRole('button', { name: 'Stop after current request' });
    await stop.click();
    await expect(stop).toBeDisabled();
    for (const [width, height] of [[1440, 900], [320, 480]]) {
      await page.setViewportSize({ width, height });
      await expect(dialog.getByText('Finishing the current request; unsent listings will stay saved.')).toBeVisible();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1)).toBe(true);
      await page.screenshot({ path: info.outputPath(`stopping-${theme}-${width}.png`) });
    }
    expect(await page.evaluate(() => (window as any).cancelRequests)).toEqual(await page.evaluate(() => (window as any).planRequests));
    await page.evaluate(() => (window as any).finishCurrentRequest());
    await expect(dialog.getByText(/Batch interrupted/)).toBeVisible();
    await expect(dialog.getByText(/saved for resume/)).toBeVisible();
  });

}
