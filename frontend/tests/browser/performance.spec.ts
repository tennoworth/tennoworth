import { expect, test, type Page } from '@playwright/test';
import { ALLOWANCE_CHANGED_EVENT, TRADE_DETECTED_EVENT } from '../../src/contracts/generated/desktop';

async function observeCalls(page: Page) {
  await page.addInitScript(() => {
    (window as any).performanceCalls = [];
    let runtime: any;
    Object.defineProperty(window, '__TAURI__', { configurable: true, get: () => runtime, set: value => {
      runtime = value;
      const invoke = value.core.invoke;
      value.core.invoke = async (cmd: string, args: any) => {
        (window as any).performanceCalls.push({ cmd, operation: args?.request?.operation, bytes: args ? new TextEncoder().encode(JSON.stringify(args)).length : 0 });
        return invoke(cmd, args);
      };
    } });
  });
}
for (const theme of ['light', 'dark'] as const) {
  test(theme + ' large-inventory startup stays within the planner payload budget', async ({ page }, info) => {
    await page.emulateMedia({ colorScheme: theme });
    await observeCalls(page);
    await page.goto('/?preview-desktop&sample=large');
    await expect(page.getByRole('heading', { name: 'Estimated opportunities', exact: true })).toBeVisible();
    await expect.poll(() => page.evaluate(() => (window as any).performanceCalls.filter((call: any) => call.operation === 'set_recos').length)).toBe(1);
    const calls = await page.evaluate(() => (window as any).performanceCalls.filter((call: any) => call.operation));
    expect(calls.some((call: any) => call.operation === 'relic_plan')).toBe(false);
    expect(calls.reduce((sum: number, call: any) => sum + call.bytes, 0)).toBeLessThan(2_000_000);
    await info.attach('planner-payloads', { body: JSON.stringify(calls, null, 2), contentType: 'application/json' });
  });

  test(theme + ' selected Settings stops polling while the document is hidden', async ({ page }) => {
    await observeCalls(page);
    await page.emulateMedia({ colorScheme: theme });
    await page.goto('/?preview-desktop&sample');
    await page.getByRole('navigation', { name: 'App views' }).getByRole('button', { name: 'Settings', exact: true }).click();
    await expect(page.getByRole('heading', { name: 'Settings', exact: true })).toBeVisible();
    await expect.poll(() => page.evaluate(() => (window as any).performanceCalls.filter((call: any) => call.cmd === 'overlay_status').length)).toBeGreaterThan(0);
    await page.evaluate(() => {
      Object.defineProperty(document, 'hidden', { configurable: true, get: () => true });
      document.dispatchEvent(new Event('visibilitychange'));
      (window as any).performanceCalls = [];
    });
    await page.waitForTimeout(2200);
    expect(await page.evaluate(() => (window as any).performanceCalls.filter((call: any) => ['overlay_status', 'auto_scan_status'].includes(call.cmd)))).toEqual([]);
    await page.evaluate(() => {
      delete (document as any).hidden;
      document.dispatchEvent(new Event('visibilitychange'));
    });
    await expect.poll(() => page.evaluate(() => (window as any).performanceCalls.filter((call: any) => call.cmd === 'overlay_status').length)).toBeGreaterThan(0);
  });

  test(theme + ' an unlocked listing session coalesces quantity-event bursts', async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.goto('/?preview-desktop&sample=session');
    await expect(page.getByRole('button', { name: 'Review batch', exact: true })).toBeEnabled();
    await page.evaluate(() => {
      const runtime = (window as any).__TAURI__;
      const invoke = runtime.core.invoke;
      (window as any).quantityStarted = 0;
      runtime.core.invoke = async (cmd: string, args: any) => {
        const result = await invoke(cmd, args);
        if (cmd === 'protection_state') {
          (window as any).quantityStarted++;
          if ((window as any).quantityStarted === 1) return new Promise(resolve => { (window as any).finishQuantity = () => resolve(result); });
        }
        return result;
      };
    });
    const events = [ALLOWANCE_CHANGED_EVENT, TRADE_DETECTED_EVENT];
    await page.evaluate(events => { for (const event of events) (window as any).__TENNOWORTH_PREVIEW_EMIT__(event, {}); }, events);
    await expect.poll(() => page.evaluate(() => (window as any).quantityStarted)).toBe(1);
    await page.evaluate(events => { for (let i = 0; i < 100; i++) (window as any).__TENNOWORTH_PREVIEW_EMIT__(events[i % events.length], {}); }, events);
    expect(await page.evaluate(() => (window as any).quantityStarted)).toBe(1);
    await page.evaluate(() => (window as any).finishQuantity());
    await expect.poll(() => page.evaluate(() => (window as any).quantityStarted)).toBe(2);
    await expect(page.getByRole('button', { name: 'Review batch', exact: true })).toBeEnabled();
  });
}
