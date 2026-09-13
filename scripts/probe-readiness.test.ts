import { describe, expect, test } from 'bun:test';
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { runInNewContext } from 'node:vm';

const source = readFileSync(new URL('../rust/tennoworth-desktop/src/shell/probe.rs', import.meta.url), 'utf8');
const script = source.split('const PROBE_JS: &str = r#"')[1].split('"#;')[0]
  .replace('__RUNTAG__', 'fixture').replace('__FIXTURE__', '{}')
  .replace('__DOMAIN_CASES__', readFileSync(new URL('../tests/fixtures/domain-ipc/cases.json', import.meta.url), 'utf8'));

function harness(settleFetch = false) {
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
    fetch() {
      fetches++;
      return settleFetch
        ? Promise.resolve({ ok: true, status: 200, type: 'basic', text: () => Promise.resolve('{}') })
        : new Promise(() => {});
    },
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

  test('reports and exits after a scenario assertion fails', async () => {
    const h = harness(true);
    h.mount();
    await h.advance(50);
    await new Promise(resolve => globalThis.setTimeout(resolve, 0));
    const report = h.calls.find(call => call.command === 'probe_report');
    expect(JSON.parse(report!.args!.payload).fatal).toContain('Probe usage reporting was not disabled');
    expect(h.calls.some(call => call.command === 'probe_exit')).toBe(true);
  });

  test('reports and exits when a scenario stalls without settling', async () => {
    // harness() leaves fetch unsettled, so the chain hangs after the first
    // request and only the watchdog can end the run.
    const h = harness();
    h.mount();
    await h.advance(50);
    expect(h.fetches).toBe(1);
    expect(h.calls.some(call => call.command === 'probe_exit')).toBe(false);
    await h.advance(90000);
    await new Promise(resolve => globalThis.setTimeout(resolve, 0));
    const report = h.calls.find(call => call.command === 'probe_report');
    expect(JSON.parse(report!.args!.payload).fatal).toContain('watchdog');
    expect(h.calls.some(call => call.command === 'probe_exit')).toBe(true);
  });
});

test.skipIf(process.platform === 'win32')('packaged probe runs the supplied AppImage without rebuilding it', () => {
  const root = mkdtempSync(join(tmpdir(), 'tennoworth-package-probe-'));
  const bin = join(root, 'bin');
  const evidence = join(root, 'evidence');
  const artifact = join(root, 'TennoWorth.AppImage');
  const report = {
    updateNotesUiVerified: true,
    updateNotesVerified: true,
    wfm: {
      cancelIdle: { ok: true },
      access: {
        ok: true,
        value: {
          restrictions: JSON.parse(readFileSync(new URL('../tests/fixtures/pacing.json', import.meta.url), 'utf8')),
          revision: 0,
          queue_count: 0,
        },
      },
    },
    usageExcluded: true,
    domainRejectedInvalid: true,
    domainOperations: ['normalize_inventory', 'score_inventory', 'trade_session', 'advisor', 'history', 'relic_plan', 'set_recos', 'ducat_plan', 'build_plan'],
    done: true,
    consoleErrors: [],
    cspViolations: [],
    appMounted: true,
    desktopBadge: true,
    scanButtonFound: true,
  };
  try {
    mkdirSync(bin);
    for (const [name, body] of [
      ['xvfb-run', 'if [ "${1:-}" = "-a" ]; then shift; fi\nexec "$@"'],
      ['dbus-run-session', 'if [ "${1:-}" = "--" ]; then shift; fi\nexec "$@"'],
    ]) {
      const path = join(bin, name);
      writeFileSync(path, `#!/bin/sh\n${body}\n`);
      chmodSync(path, 0o755);
    }
    symlinkSync(process.execPath, join(bin, 'bun'));
    writeFileSync(artifact, `#!/bin/sh
test "${'${APPIMAGE_EXTRACT_AND_RUN:-}'}" = 1
printf '%s' '${JSON.stringify(report)}' > "${'${TENNOWORTH_PROBE_OUT}'}"
`);
    chmodSync(artifact, 0o755);

    const output = execFileSync('bash', ['scripts/probe-smoke-linux.sh', '--artifact', artifact], {
      cwd: new URL('..', import.meta.url),
      env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, TENNOWORTH_PROBE_EVIDENCE_DIR: evidence },
      encoding: 'utf8',
    });
    expect(output).toContain('probe-smoke: OK');
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
