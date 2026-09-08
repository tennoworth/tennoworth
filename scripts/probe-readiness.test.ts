import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

const source = readFileSync(new URL('../rust/tennoworth-desktop/src/shell/probe.rs', import.meta.url), 'utf8');
const script = source.split('const PROBE_JS: &str = r#"')[1].split('"#;')[0]
  .replace('__RUNTAG__', 'fixture').replace('__FIXTURE__', '{}');

function harness() {
  let now = 0;
  let mounted = false;
  let fetches = 0;
  const calls: { command: string; args?: { payload: string } }[] = [];
  const timers: { at: number; callback: () => void }[] = [];
  runInNewContext(script, {
    Date: class extends Date { static now() { return now; } },
    setTimeout(callback: () => void, ms: number) { timers.push({ at: now + ms, callback }); },
    document: {
      readyState: 'complete', title: 'Fixture', body: { innerText: '' },
      addEventListener() {},
      querySelector(selector: string) { return selector === '#app' ? { childElementCount: mounted ? 1 : 0 } : null; },
    },
    window: { __TAURI__: { core: { invoke(command: string, args?: { payload: string }) {
      calls.push({ command, args }); return Promise.resolve(null);
    } } } },
    location: { origin: 'tauri://localhost', href: 'tauri://localhost', protocol: 'tauri:' },
    localStorage: { getItem() { return null; }, setItem() {} },
    console: { error() {} },
    // Stop after startup evidence is captured; later scenarios are native gates.
    fetch() { fetches++; return new Promise(() => {}); },
  });
  return {
    mount() { mounted = true; },
    get fetches() { return fetches; },
    calls,
    async advance(ms: number) {
      const end = now + ms;
      while (timers.length && timers[0].at <= end) {
        const timer = timers.shift()!; now = timer.at; timer.callback();
        await Promise.resolve(); await Promise.resolve();
      }
      now = end;
      await Promise.resolve(); await Promise.resolve();
    },
  };
}

describe('native probe startup readiness', () => {
  test('waits for a late shell mount before starting scenarios', async () => {
    const h = harness();
    await h.advance(1400);
    expect(h.fetches).toBe(0);
    h.mount();
    await h.advance(50);
    expect(h.fetches).toBe(1);
    expect(h.calls).toEqual([]);
  });

  test('reports and exits when the shell never mounts', async () => {
    const h = harness();
    await h.advance(15000);
    expect(h.fetches).toBe(0);
    const report = h.calls.find(call => call.command === 'probe_report');
    expect(JSON.parse(report!.args!.payload).fatal).toContain('did not mount');
    expect(h.calls.some(call => call.command === 'probe_exit')).toBe(true);
  });
});
