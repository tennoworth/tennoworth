import { afterEach, describe, expect, test } from 'bun:test';
import { execFileSync, spawnSync } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const directories: string[] = [];
afterEach(() => { for (const dir of directories.splice(0)) rmSync(dir, { recursive: true, force: true }); });
const puller = fileURLToPath(new URL('../deploy/pull-app.sh', import.meta.url));
function write(path: string, body: string) { mkdirSync(join(path, '..'), { recursive: true }); writeFileSync(path, body); }
function git(cwd: string, ...args: string[]) { return execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim(); }

function fixture(rename = true) {
  const root = mkdtempSync(join(tmpdir(), 'deployment-layout-')); directories.push(root);
  const remote = join(root, 'remote'), app = join(root, 'app'), installed = join(root, 'installed'), bin = join(root, 'bin');
  for (const p of [remote, installed, bin]) mkdirSync(p);
  git(remote, 'init', '-b', 'main'); git(remote, 'config', 'user.email', 'test@example.invalid'); git(remote, 'config', 'user.name', 'Fixture');
  write(join(remote, 'prototype/package.json'), '{}');
  write(join(remote, 'prototype/public/market.json'), '{"revision":"bundled-old"}');
  write(join(remote, 'prototype/public/wfstat-catalog.json'), '["bundled-old"]');
  write(join(remote, 'prototype/public/definitions.json'), '{"revision":"bundled-old"}');
  write(join(remote, 'source.txt'), 'before');
  write(join(remote, '.gitignore'), '**/dist/\n**/history.json\nwfm_results.csv\n');
  git(remote, 'add', '.'); git(remote, 'commit', '-m', 'Initial fixture');
  git(root, 'clone', '-o', 'github', remote, app);
  write(join(app, 'prototype/public/market.json'), '{"revision":"live-newer"}');
  write(join(app, 'prototype/public/wfstat-catalog.json'), '["live-newer"]');
  write(join(app, 'prototype/public/history.json'), '{"history":"retain"}');
  if (rename) write(join(app, 'prototype/public/definitions.json'), '{"revision":"live-hotfix"}');
  write(join(app, 'prototype/dist/index.html'), '<p>Previously served site</p>');
  write(join(app, 'prototype/dist/assets/site.js'), 'old-bundle');
  write(join(app, 'wfm_results.csv'), 'preserve,csv\n');
  const initial = git(app, 'rev-parse', 'HEAD');
  if (rename) renameSync(join(remote, 'prototype'), join(remote, 'frontend'));
  else write(join(remote, 'prototype/public/definitions.json'), '{"revision":"bundled-new"}');
  write(join(remote, 'source.txt'), 'after');
  git(remote, 'add', '-A'); git(remote, 'commit', '-m', 'Next layout');
  write(join(bin, 'systemctl'), '#!/bin/sh\nprintf "%s\\n" "${FIXTURE_SERVICE_STATE:-inactive}"\n'); chmodSync(join(bin, 'systemctl'), 0o755);
  const run = (args: string[] = [], env: Record<string, string> = {}) => spawnSync('bash', [puller, ...args], {
    env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, APP: app, DEPLOY_ROOT: installed, ...env }, encoding: 'utf8',
  });
  return { root, app, remote, initial, run };
}

describe.skipIf(process.platform === 'win32')('deployment layout transition', () => {
  test('refuses an unattended root rename before touching live artifacts', () => {
    const f = fixture(); const result = f.run();
    expect(result.status).toBe(1); expect(result.stderr).toContain('--migrate-layout');
    expect(git(f.app, 'rev-parse', 'HEAD')).toBe(f.initial);
    expect(readFileSync(join(f.app, 'prototype/public/market.json'), 'utf8')).toContain('live-newer');
  });
  test('moves newer live data and the served bundle into the new layout', () => {
    const f = fixture(); const result = f.run(['--migrate-layout']);
    expect(result.status).toBe(0);
    expect(git(f.app, 'rev-parse', 'HEAD')).toBe(git(f.remote, 'rev-parse', 'HEAD'));
    expect(readFileSync(join(f.app, 'frontend/public/market.json'), 'utf8')).toContain('live-newer');
    expect(readFileSync(join(f.app, 'prototype/public/market.json'), 'utf8')).toContain('live-newer');
    expect(readFileSync(join(f.app, 'frontend/public/wfstat-catalog.json'), 'utf8')).toContain('live-newer');
    expect(readFileSync(join(f.app, 'frontend/public/history.json'), 'utf8')).toContain('retain');
    expect(readFileSync(join(f.app, 'frontend/public/definitions.json'), 'utf8')).toContain('live-hotfix');
    expect(readFileSync(join(f.app, 'prototype/public/definitions.json'), 'utf8')).toContain('live-hotfix');
    expect(readFileSync(join(f.app, 'frontend/dist/assets/site.js'), 'utf8')).toBe('old-bundle');
    expect(readFileSync(join(f.app, 'wfm_results.csv'), 'utf8')).toBe('preserve,csv\n');
  });
  test('restores live data to the old layout when the fast-forward fails', () => {
    const f = fixture(); write(join(f.app, 'source.txt'), 'unfinished local work');
    const result = f.run(['--migrate-layout']); expect(result.status).not.toBe(0);
    expect(git(f.app, 'rev-parse', 'HEAD')).toBe(f.initial);
    expect(readFileSync(join(f.app, 'prototype/public/market.json'), 'utf8')).toContain('live-newer');
    expect(readFileSync(join(f.app, 'source.txt'), 'utf8')).toBe('unfinished local work');
    expect(readFileSync(join(f.app, 'prototype/public/definitions.json'), 'utf8')).toContain('live-hotfix');
  });
  test('keeps ordinary updates working without migration mode', () => {
    const f = fixture(false); expect(f.run().status).toBe(0);
    expect(readFileSync(join(f.app, 'prototype/public/market.json'), 'utf8')).toContain('live-newer');
    expect(readFileSync(join(f.app, 'prototype/public/definitions.json'), 'utf8')).toContain('bundled-new');
  });
  test('does not change a checkout while the scrape service is activating', () => {
    const f = fixture(); const result = f.run(['--migrate-layout'], { FIXTURE_SERVICE_STATE: 'activating' });
    expect(result.status).toBe(75); expect(git(f.app, 'rev-parse', 'HEAD')).toBe(f.initial);
    expect(existsSync(join(f.app, 'frontend'))).toBe(false);
  });
});

describe.skipIf(process.platform === 'win32')('signed policy bootstrap', () => {
  const workflow = Bun.YAML.parse(readFileSync(fileURLToPath(new URL('../.github/workflows/publish-wfm-policy.yml', import.meta.url)), 'utf8')) as {
    jobs: { publish: { steps: Array<{ name?: string; run?: string }> } };
  };
  const verify = workflow.jobs.publish.steps.find(step => step.name === 'Verify signed revision')!.run!;
  function run(ref: string, first: boolean, status: number) {
    const root = mkdtempSync(join(tmpdir(), 'policy-bootstrap-')); directories.push(root);
    const bin = join(root, 'bin'); mkdirSync(bin);
    const commands = {
      curl: '#!/bin/sh\nprintf "%s" "$FIXTURE_HTTP_STATUS"\n',
      gh: '#!/bin/sh\nprintf "%s\\n" "$*" >> "$FIXTURE_CALLS"\n',
      cargo: '#!/bin/sh\nprintf "%s\\n" "$*" >> "$FIXTURE_CALLS"\nmkdir -p rust/target/release\nprintf verifier > rust/target/release/wfm-policy\n',
    };
    for (const [name, body] of Object.entries(commands)) { write(join(bin, name), body); chmodSync(join(bin, name), 0o755); }
    const calls = join(root, 'calls');
    const result = spawnSync('bash', ['-c', verify], { cwd: root, encoding: 'utf8', env: {
      ...process.env, PATH: `${bin}:${process.env.PATH}`, GITHUB_REF: ref, FIRST_PUBLICATION: String(first),
      TENNOWORTH_WFM_POLICY_PUBLIC_KEY: 'fixture', ENVELOPE_BASE64: Buffer.from('{}').toString('base64'),
      GH_TOKEN: 'fixture', GITHUB_REPOSITORY: 'fixture/repository', FIXTURE_HTTP_STATUS: String(status), FIXTURE_CALLS: calls,
    } });
    return { result, calls: existsSync(calls) ? readFileSync(calls, 'utf8') : '', artifact: existsSync(join(root, 'policy/wfm-policy')) };
  }
  test('develop can bootstrap only a demonstrably absent first policy', () => {
    const success = run('refs/heads/develop', true, 404);
    expect(success.result.status).toBe(0); expect(success.artifact).toBe(true);
    expect(success.calls).toContain('-- policy/wfm-policy.json');
    for (const status of [200, 403, 500]) {
      const rejected = run('refs/heads/develop', true, status);
      expect(rejected.result.status).not.toBe(0); expect(rejected.calls).toBe(''); expect(rejected.artifact).toBe(false);
    }
  });
  test('updates require main and compare against the published policy', () => {
    const success = run('refs/heads/main', false, 200);
    expect(success.result.status).toBe(0);
    expect(success.calls).toContain('policy/wfm-policy.json previous/wfm-policy.json');
    for (const [ref, first] of [['refs/heads/develop', false], ['refs/heads/feature', true], ['refs/heads/main', true]] as const) {
      const rejected = run(ref, first, 200);
      expect(rejected.result.status).not.toBe(0); expect(rejected.calls).toBe('');
    }
    expect(run('refs/heads/main', false, 404).result.status).not.toBe(0);
  });
});
