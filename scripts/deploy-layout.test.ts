import { afterEach, describe, expect, test } from 'bun:test';
import { execFileSync, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, renameSync, rmSync, utimesSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gunzipSync } from 'node:zlib';

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

describe('usage collector rollback', () => {  for (const failure of ['restart', 'health']) {
    test(`restores the preceding binary after ${failure} failure`, () => {
      const root = mkdtempSync(join(tmpdir(), 'usage-pull-'));
      directories.push(root);
      const deploy = join(root, 'deploy');
      const mocks = join(root, 'mocks');
      mkdirSync(join(deploy, 'bin'), { recursive: true });
      mkdirSync(mocks);
      writeFileSync(join(deploy, 'bin/tennoworth-usage'), 'old-binary');
      const source = readFileSync(new URL('../deploy/pull-usage.sh', import.meta.url), 'utf8');
      writeFileSync(join(root, 'pull.sh'), source.replaceAll('/srv/wfm', deploy));
      writeFileSync(join(mocks, 'curl'), `#!/bin/sh
case "$*" in
  *'/health'*) exit ${failure === 'health' ? 1 : 0};;
esac
for arg do previous="$last"; last="$arg"; done
case "$last" in
  *.sha256) printf 'unused' > "$last";;
  *) printf 'new-binary' > "$last";;
esac
`);
      writeFileSync(join(mocks, 'sha256sum'), '#!/bin/sh\nexit 0\n');
      writeFileSync(join(mocks, 'sleep'), '#!/bin/sh\nexit 0\n');
      writeFileSync(join(mocks, 'systemctl'), `#!/bin/sh
if [ ! -f '${root}/restarted' ]; then
  touch '${root}/restarted'
  exit ${failure === 'restart' ? 1 : 0}
fi
exit 0
`);
      for (const command of ['curl', 'sha256sum', 'sleep', 'systemctl']) chmodSync(join(mocks, command), 0o755);
      const result = spawnSync('sh', [join(root, 'pull.sh')], { env: { ...process.env, PATH: `${mocks}:${process.env.PATH}` }, encoding: 'utf8' });
      expect(result.status).toBe(1);
      expect(result.stderr).toContain('Usage collector health check failed');
      expect(readFileSync(join(deploy, 'bin/tennoworth-usage'), 'utf8')).toBe('old-binary');
    });
  }
});

describe('caddy listener boundary', () => {
  const caddyfile = readFileSync(fileURLToPath(new URL('../deploy/Caddyfile', import.meta.url)), 'utf8');

  /** The body of the first site block addressed by a bare port (`:8081`). */
  function barePortSite(source: string): { address: string; lines: string[] } | null {
    const lines = source.split('\n');
    const start = lines.findIndex(line => /^:[0-9]+ \{\s*$/.test(line));
    if (start === -1) return null;
    const end = lines.findIndex((line, index) => index > start && line === '}');
    return { address: lines[start], lines: lines.slice(start + 1, end) };
  }

  test('a bare-port site address binds loopback explicitly', () => {
    // `:8081` with no host binds every interface, so the comment claiming
    // "loopback only" describes an intention rather than a boundary. The tunnel
    // needs loopback; nothing else should be able to reach the listener.
    const site = barePortSite(caddyfile);
    expect(site, 'expected a bare-port site block in the Caddyfile').not.toBeNull();
    const bind = site!.lines.find(line => /^\s*bind\s+/.test(line)) ?? '';
    expect(bind, 'the site must bind loopback explicitly').toMatch(/127\.0\.0\.1/);
    // `localhost` resolves to ::1 on hosts where IPv6 is preferred, and the
    // tunnel dials `localhost` - so binding only IPv4 silently breaks it.
    expect(bind, 'the bind must cover IPv6 loopback too').toMatch(/::1/);
  });
});

describe('host-direct scrape deploy script', () => {
  const deploy = readFileSync(fileURLToPath(new URL('./deploy-scrape-host.sh', import.meta.url)), 'utf8');

  test('refuses anything but a reviewed, clean revision', () => {
    expect(deploy).toMatch(/git status --porcelain/);
    expect(deploy).toMatch(/merge-base --is-ancestor "\$REVISION" "\$REMOTE\/develop"/);
  });

  test('refuses to build without the policy public key', () => {
    // A build without it silently ignores the signed policy, so the script must
    // fail loudly instead of shipping a scraper that cannot verify one.
    expect(deploy).toMatch(/TENNOWORTH_WFM_POLICY_PUBLIC_KEY:\?/);
  });

  test('holds the sweep timer across the install and restores it on every exit', () => {
    // Live files are rewritten one at a time, so a timer elapse inside that
    // window lets a single sweep run the `scrape` phase from one release and the
    // `build` phase from the next. The schedule has to be held for the whole
    // operation, not just checked before it.
    const stop = deploy.indexOf('systemctl stop wfm-scrape.timer');
    expect(stop, 'the timer must be stopped, not merely observed').toBeGreaterThan(-1);
    // Restoring only after the last check leaves the box with no schedule at all
    // whenever an earlier step fails - silent until someone notices the data is
    // stale - so the restore has to ride the one path both endings take, and it
    // has to be armed no later than the stop in case that call dies in flight.
    const trap = deploy.indexOf('trap restore_timer EXIT');
    expect(trap, 'the restore must be an EXIT trap').toBeGreaterThan(-1);
    expect(trap, 'the restore must be armed before the timer is stopped').toBeLessThan(stop);
    // Pin the call, not the warning text that names the same command.
    expect(deploy, 'the trap must actually start the timer').toMatch(/\$SSH "\$HOST" "systemctl start wfm-scrape\.timer"/);
    // `enable --now` re-arms the timer in the middle of the very window the hold
    // exists to protect; only `enable` may run inside it.
    expect(deploy).toMatch(/systemctl enable wfm-scrape\.timer/);
    expect(deploy).not.toMatch(/enable --now wfm-scrape\.timer/);
  });

  test('installs only after the schedule is held and a running sweep is drained', () => {
    const stop = deploy.indexOf('systemctl stop wfm-scrape.timer');
    const recheck = deploy.indexOf('systemctl is-active wfm-scrape.service');
    const liveInstall = deploy.indexOf('install -m 0755 "$RELEASES/$REVISION/wfm-scrape" "/srv/wfm/bin/wfm-scrape"');
    expect(recheck, 'the service must be re-checked after the timer stop').toBeGreaterThan(stop);
    expect(liveInstall, 'live files must not be written before the re-check').toBeGreaterThan(recheck);
    // Stopping the timer does not stop a sweep a previous elapse already
    // started, and one sample can leave it running through the install, so the
    // re-check has to loop until the state is terminal.
    expect(deploy, 'the re-check must loop').toMatch(/while :; do[\s\S]*sweep_state/);
    // Transitional states are still a running sweep; only inactive and failed
    // may end the wait.
    expect(deploy, 'transitional states must keep waiting').toMatch(/active\|activating\|deactivating\)/);
    expect(deploy, 'only a terminal state may end the wait').toMatch(/inactive\|failed\) break/);
    // An empty or unrecognised query result used to read as idle and install
    // over a live sweep.
    expect(deploy, 'an unreadable state must abort').toMatch(/could not read wfm-scrape\.service state/);
  });

  test('proves the deployed revision itself against the live policy or aborts', () => {
    // The proof used to run whatever /srv/wfm/bin/wfm-policy already existed:
    // a binary from some other revision, whose success said nothing about the
    // scraper being installed. The verifier now comes from this revision.
    expect(deploy).toMatch(/cargo build[^\n]*-p wfm-client --bin wfm-policy/);
    expect(deploy).toMatch(/\$RELEASES\/\$REVISION\/wfm-policy/);
    // The proof has to run the verifier installed with this release; pointing it
    // back at /srv/wfm/bin/wfm-policy is the original defect.
    expect(deploy, 'the proof must run the release verifier').toMatch(/VERIFIER="\$RELEASES\/\$REVISION\/wfm-policy"/);
    // The checksum has to be compared, not merely computed: an unverified
    // verifier is one more binary of unknown provenance.
    expect(deploy, 'the installed verifier must be the one that was built')
      .toMatch(/sha256sum '\$VERIFIER'[\s\S]{0,80}VERIFIER_CHECKSUM/);
    // Missing inputs used to warn and continue, which is exactly the case the
    // check exists for: fail open and a scraper built without the key - or a box
    // whose policy never arrived - ships with the key unproven.
    expect(deploy, 'a missing verifier must abort').toMatch(/test -x [^\n]*\|\| die/);
    expect(deploy, 'a missing policy must abort').toMatch(/test -f [^\n]*policy\/wfm-policy\.json[^\n]*\|\| die/);
    expect(deploy, 'a rejected policy must abort').toMatch(/\$VERIFIER' '\$HOST_ROOT\/policy\/wfm-policy\.json'[\s\S]{0,80}\|\| die/);
    expect(deploy, 'the fail-open branch must be gone').not.toContain('no verifier or policy on the box yet');
    // The proof runs against the release on the box, and the live paths move
    // only after it passes, so a rejection leaves the running release alone.
    const proof = deploy.indexOf("'$VERIFIER' '$HOST_ROOT/policy/wfm-policy.json'");
    const liveInstall = deploy.indexOf('install -m 0755 "$RELEASES/$REVISION/wfm-scrape" "/srv/wfm/bin/wfm-scrape"');
    expect(proof, 'the release verifier must run').toBeGreaterThan(-1);
    expect(liveInstall, 'the live paths must not move before the proof').toBeGreaterThan(proof);
  });

  test('gates the artifact on the box glibc and on its checksum', () => {
    expect(deploy).toMatch(/objdump -T/);
    expect(deploy).toMatch(/GLIBC_\[0-9\]/);
    expect(deploy).toMatch(/sha256sum \"\$ARTIFACT\"/);
  });

  test('checks the sweep service before installing and keeps the release', () => {
    expect(deploy).toMatch(/systemctl is-active wfm-scrape\.service/);
    expect(deploy).toMatch(/install -d -m 0755 -o root -g root \"\$RELEASES\/\$REVISION\"/);
  });

  test('never carries a private signing key', () => {
    expect(deploy).not.toMatch(/wfm-policy\.key/);
    expect(deploy).not.toMatch(/minisign -S[mG]/);
  });

  test('is the only installer of the driver script and the units', () => {
    // Everything the units execute moves together here, so a change to the
    // pipeline cannot reach the box without passing the gates above. A second
    // installer is the regression: it can advance one piece on its own.
    expect(deploy).toMatch(/install -m 0755 "[^"]*\/run-scrape\.sh" "\/srv\/wfm\/run-scrape\.sh"/);
    expect(deploy).toMatch(/install -m 0644 "[^"]*\/wfm-scrape\.service" \/etc\/systemd\/system\/wfm-scrape\.service/);
    expect(deploy).toMatch(/install -m 0644 "[^"]*\/wfm-scrape\.timer" \/etc\/systemd\/system\/wfm-scrape\.timer/);
  });
});

// Runs the real deploy script against a stubbed box. `ssh`/`scp` execute
// locally, `install` remaps the hard-coded /srv and /etc live paths into the
// fixture, and `systemctl` reads its sweep state from a file so a sequence of
// states can be replayed. The point is that the script's own control flow is
// what the assertions observe, not a copy of it.
//
// With `runCheck` the same harness proves the deployed readiness path end to
// end: a real corpus lives under the remapped box, the deploy stages and
// installs the real check script and its real unit, and `systemctl start
// wfm-observations-check.service` runs the unit's own ExecStart with the
// EnvironmentFile the deploy wrote. HOST_ROOT becomes the production /srv/wfm
// so the unit's paths and the deploy's reads resolve to the same files.
function scrapeDeployFixture(options: { states?: string[]; env?: Record<string, string>; runCheck?: boolean } = {}) {
  const root = mkdtempSync(join(tmpdir(), 'scrape-deploy-')); directories.push(root);
  const bin = join(root, 'bin'); mkdirSync(bin);
  const wfm = join(root, 'wfm'); mkdirSync(wfm, { recursive: true });
  const live = join(root, 'live'); mkdirSync(join(live, 'srv/wfm/bin'), { recursive: true });
  const box = join(live, 'srv/wfm');
  const hostRoot = options.runCheck ? '/srv/wfm' : wfm;
  const target = join(root, 'target'); mkdirSync(join(target, 'release'), { recursive: true });
  const order = join(root, 'order'); writeFileSync(order, '');
  const states = join(root, 'states'); writeFileSync(states, (options.states ?? []).map(state => state + '\n').join(''));
  const revision = 'fixture-rev';
  const realInstall = execFileSync('sh', ['-c', 'command -v install'], { encoding: 'utf8' }).trim();

  // The box already runs a deployment; every failure path asserts it survives.
  writeFileSync(join(live, 'srv/wfm/bin/wfm-scrape'), 'old-live-binary');
  writeFileSync(join(live, 'srv/wfm/run-scrape.sh'), 'old-live-run-scrape');
  mkdirSync(join(live, 'etc/systemd/system'), { recursive: true });
  mkdirSync(join(root, 'rust'), { recursive: true });
  // The script stages these from the checkout with relative paths, as it does
  // from the repository root.
  for (const unit of ['run-scrape.sh', 'wfm-scrape.service', 'wfm-scrape.timer',
    'observations-check.sh', 'wfm-observations-check.service', 'wfm-observations-check.timer',
    'pull-archive-receipt.sh', 'wfm-archive-receipt-pull.service', 'wfm-archive-receipt-pull.timer']) {
    write(join(root, 'deploy', unit), `# ${unit}\n`);
  }
  if (options.runCheck) {
    // The real script and its unit travel to the box, so the stubbed systemd
    // runs what the deploy actually installed rather than a stand-in.
    for (const name of ['observations-check.sh', 'wfm-observations-check.service']) {
      write(join(root, 'deploy', name), readFileSync(new URL(`../deploy/${name}`, import.meta.url), 'utf8'));
    }
    write(join(box, 'policy/wfm-policy.json'), '{}');
    // A sound corpus under the box paths the unit names. The check's own
    // defaults for the paths the unit does not pass are supplied by the
    // systemctl stub below.
    const newestStart = FIXTURE_NOW - 6000;
    for (let i = 0; i < 3; i++) {
      const start = newestStart - (2 - i) * 7200;
      const stamp = new Date(start * 1000).toISOString().replace('.000Z', 'Z');
      const path = join(box, 'observations', `sweep-${stamp.replaceAll(':', '-')}.jsonl`);
      write(path, sweepLog(start, 4, 2));
      utimesSync(path, start + 4200, start + 4200);
    }
    write(join(box, 'app/wfm_results.csv'), 'url_name,median_90d\nkept_a,1\nkept_b,1\n');
    const snapshot = join(box, 'app/frontend/public/market.json');
    write(snapshot, '{"catalog":{}}');
    utimesSync(snapshot, FIXTURE_NOW - 100, FIXTURE_NOW - 100);
    writeFileSync(join(root, 'journal'), journalText([
      { at: newestStart, message: 'Starting wfm-scrape.service - Refresh Warframe market.json from warframe.market + warframestat...' },
      { at: newestStart, message: 'scraper: /srv/wfm/bin/wfm-scrape scrape', invocation: FIXTURE_INVOCATION },
      { at: newestStart + 4200, message: sweepMetrics(), invocation: FIXTURE_INVOCATION },
      { at: newestStart + 4200, message: 'wfm-scrape.service: Deactivated successfully.' },
    ]));
    write(join(bin, 'journalctl'), '#!/bin/sh\ncat "$FIXTURE_JOURNAL"\n');
  } else {
    write(join(wfm, 'policy/wfm-policy.json'), '{}');
    // The readiness check runs once as part of a deploy, and its verdict gates the
    // deploy; this is the report a sound box would have produced. HOST_ROOT is the
    // fixture's /srv/wfm, which the check's --out also points at in production.
    write(join(wfm, 'data/observations-check/report.json'), '{"ready": true, "errors": []}');
  }

  write(join(root, 'artifacts/wfm-scrape'), '#!/bin/sh\nprintf "usage: wfm-scrape\\n"\n');
  write(join(root, 'artifacts/wfm-policy'), '#!/bin/sh\nexit "${FIXTURE_POLICY_EXIT:-0}"\n');
  chmodSync(join(root, 'artifacts/wfm-scrape'), 0o755);
  chmodSync(join(root, 'artifacts/wfm-policy'), 0o755);

  write(join(bin, 'git'), [
    '#!/bin/sh',
    'case "$*" in',
    '  "status --porcelain") exit 0;;',
    `  "rev-parse HEAD") printf '%s\\n' '${revision}';;`,
    `  "rev-parse --short "*|"rev-parse --short") printf '%s\\n' '${revision}';;`,
    `  "rev-parse --show-toplevel") printf '%s\\n' '${root}';;`,
    '  "rev-parse --verify --quiet "*) exit 0;;',
    '  "merge-base --is-ancestor "*) exit 0;;',
    '  "branch --show-current") printf "fixture\\n";;',
    'esac',
    'exit 0',
    '',
  ].join('\n'));
  write(join(bin, 'cargo'), [
    '#!/bin/sh',
    'if [ "$1" = "build" ]; then',
    '  mkdir -p "$CARGO_TARGET_DIR/release"',
    '  cp "$FIXTURE_ROOT/artifacts/wfm-scrape" "$CARGO_TARGET_DIR/release/wfm-scrape"',
    '  cp "$FIXTURE_ROOT/artifacts/wfm-policy" "$CARGO_TARGET_DIR/release/wfm-policy"',
    '  chmod +x "$CARGO_TARGET_DIR/release/wfm-scrape" "$CARGO_TARGET_DIR/release/wfm-policy"',
    'fi',
    'exit 0',
    '',
  ].join('\n'));
  write(join(bin, 'objdump'), '#!/bin/sh\nprintf "GLIBC_2.34\\n"\n');
  write(join(bin, 'ldd'), '#!/bin/sh\nprintf "ldd (fixture GLIBC) 2.34\\n"\n');
  write(join(bin, 'sleep'), '#!/bin/sh\nexit 0\n');
  // Remote paths are remapped here as well as in `install`, so a remote
  // `sha256sum /srv/wfm/bin/wfm-scrape` reads the copy the stub wrote.
  write(join(bin, 'ssh'), [
    '#!/bin/bash',
    'shift',
    'cmd="$*"',
    'cmd="$(printf "%s" "$cmd" | sed "s#/srv/#$FIXTURE_LIVE/srv/#g; s#/etc/#$FIXTURE_LIVE/etc/#g")"',
    'eval "$cmd"',
    '',
  ].join('\n'));
  write(join(bin, 'scp'), [
    '#!/bin/sh',
    '[ "$1" = "-q" ] && shift',
    'last=""',
    'for a in "$@"; do last="$a"; done',
    'dest="${last#*:}"',
    // The same remap the ssh and install stubs apply, so a deploy whose
    // HOST_ROOT is the production /srv/wfm lands in the fixture too.
    'dest="$(printf "%s" "$dest" | sed "s#/srv/#$FIXTURE_LIVE/srv/#g; s#/etc/#$FIXTURE_LIVE/etc/#g")"',
    'case "$dest" in */) mkdir -p "$dest";; esac',
    'for a in "$@"; do',
    '  [ "$a" = "$last" ] && continue',
    '  cp "$a" "$dest"',
    'done',
    '',
  ].join('\n'));
  write(join(bin, 'install'), [
    '#!/bin/bash',
    'args=()',
    'skip=0',
    'for a in "$@"; do',
    '  if [ "$skip" = 1 ]; then skip=0; continue; fi',
    '  case "$a" in',
    '    -o|-g) skip=1; continue;;',
    '    /srv/*|/etc/*) a="${FIXTURE_LIVE}${a}";;',
    '  esac',
    '  args+=("$a")',
    'done',
    'for a in "${args[@]}"; do',
    '  case "$a" in */bin/wfm-scrape) printf "live-install\\n" >> "$FIXTURE_ORDER";; esac',
    'done',
    `exec ${realInstall} "\${args[@]}"`,
    '',
  ].join('\n'));
  const systemctl = [
    '#!/bin/bash',
    'if [ "$1" = "is-active" ]; then',
    '  shift',
    '  quiet=0',
    '  if [ "$1" = "--quiet" ]; then quiet=1; shift; fi',
    '  unit="$1"',
    '  case "$unit" in',
    '    wfm-scrape.service)',
    '      if [ "${FIXTURE_QUERY_FAIL:-0}" = 1 ]; then exit 255; fi',
    '      next=$(sed -n 1p "$FIXTURE_STATES" 2>/dev/null)',
    '      if [ -n "$next" ]; then sed -i 1d "$FIXTURE_STATES"; else next="${FIXTURE_SWEEP_STATE:-inactive}"; fi',
    '      printf "is-active %s\\n" "$next" >> "$FIXTURE_ORDER"',
    '      printf "%s\\n" "$next"',
    '      exit 0;;',
    '    wfm-scrape.timer)',
    '      state="${FIXTURE_TIMER_STATE:-inactive}"',
    '      if [ "$quiet" = 1 ]; then [ "$state" = active ] && exit 0 || exit 3; fi',
    '      printf "%s\\n" "$state"',
    '      exit 0;;',
    '  esac',
    '  exit 0',
    'fi',
  ];
  if (options.runCheck) {
    // The queries the readiness check makes of systemd, and the one action that
    // matters: `start wfm-observations-check.service` runs the installed unit's
    // own ExecStart with the EnvironmentFile the deploy wrote. The unit passes
    // only the paths it names; the check's remaining defaults are supplied here
    // under the remapped root, which is the same value they have in production.
    systemctl.push(
      'if [ "$1" = "is-enabled" ]; then',
      '  case "$2" in',
      '    wfm-scrape-pull.timer) printf "disabled\\n";;',
      '    *) printf "%s\\n" "${FIXTURE_RETIRED:-disabled}";;',
      '  esac',
      '  exit 0',
      'fi',
      'if [ "$1" = "show" ]; then',
      '  unit="$2"; prop="$4"',
      '  case "$unit:$prop" in',
      '    wfm-scrape.service:ActiveState) printf "inactive\\n";;',
      '    wfm-scrape.service:Result) printf "success\\n";;',
      '    wfm-scrape.timer:UnitFileState) printf "enabled\\n";;',
      '    wfm-scrape.timer:ActiveState) printf "active\\n";;',
      '    wfm-scrape.timer:NextElapseUSecRealtime) LC_ALL=C date -u -d "+2 hours" "+%a %Y-%m-%d %H:%M:%S UTC";;',
      '  esac',
      '  exit 0',
      'fi',
    );
  }
  systemctl.push('printf "%s\\n" "$*" >> "$FIXTURE_SYSTEMCTL"');
  if (options.runCheck) {
    systemctl.push(
      'if [ "$1" = "start" ] && [ "$2" = "wfm-observations-check.service" ]; then',
      '  unit="$FIXTURE_LIVE/etc/systemd/system/wfm-observations-check.service"',
      '  exec_line="$(sed -n \'s/^ExecStart=//p\' "$unit" | head -1)"',
      '  exec_line="$(printf "%s" "$exec_line" | sed "s#/srv/#$FIXTURE_LIVE/srv/#g; s#/etc/#$FIXTURE_LIVE/etc/#g")"',
      '  env_file="$(sed -n \'s/^EnvironmentFile=-*//p\' "$unit" | head -1)"',
      '  env_file="$(printf "%s" "$env_file" | sed "s#/etc/#$FIXTURE_LIVE/etc/#g")"',
      '  if [ -n "$env_file" ] && [ -f "$env_file" ]; then set -a; . "$env_file"; set +a; fi',
      '  eval "$exec_line --csv $FIXTURE_LIVE/srv/wfm/app/wfm_results.csv --snapshot $FIXTURE_LIVE/srv/wfm/app/frontend/public/market.json --deployed $FIXTURE_LIVE/srv/wfm/deployed.json --binary $FIXTURE_LIVE/srv/wfm/bin/wfm-scrape"',
      '  exit $?',
      'fi',
    );
  }
  systemctl.push(
    'if [ "$1" = "start" ] && [ "${FIXTURE_START_FAIL:-0}" = 1 ]; then exit 1; fi',
    'exit 0',
    '',
  );
  write(join(bin, 'systemctl'), systemctl.join('\n'));
  for (const command of ['git', 'cargo', 'objdump', 'ldd', 'sleep', 'ssh', 'scp', 'install', 'systemctl', ...(options.runCheck ? ['journalctl'] : [])]) chmodSync(join(bin, command), 0o755);

  const run = (env: Record<string, string> = {}) => spawnSync('bash', [fileURLToPath(new URL('./deploy-scrape-host.sh', import.meta.url))], {
    cwd: root,
    encoding: 'utf8',
    env: {
      ...process.env, PATH: `${bin}:${process.env.PATH}`,
      HOST: 'fixture', HOST_ROOT: hostRoot, REVISION: revision, CARGO_TARGET_DIR: target,
      TENNOWORTH_WFM_POLICY_PUBLIC_KEY: 'fixture-key',
      FIXTURE_ROOT: root, FIXTURE_LIVE: live, FIXTURE_ORDER: order, FIXTURE_STATES: states,
      ...(options.runCheck ? { FIXTURE_JOURNAL: join(root, 'journal') } : {}),
      FIXTURE_SYSTEMCTL: join(root, 'systemctl.log'),
      ...options.env, ...env,
    },
  });
  return {
    root, wfm, live, box, order, revision, run, systemctlLog: join(root, 'systemctl.log'),
    reportPath: join(box, 'data/observations-check/report.json'),
  };
}

describe.skipIf(process.platform === 'win32')('host-direct scrape deploy failure paths', () => {
  const liveBinary = (f: ReturnType<typeof scrapeDeployFixture>) => readFileSync(join(f.live, 'srv/wfm/bin/wfm-scrape'), 'utf8');

  test('aborts before installing when the sweep state cannot be read', () => {
    // Wrapping the query in `|| true` turns a dead connection or a broken
    // systemctl into an empty state, and an empty state ended the wait. A query
    // that produced nothing must abort instead of installing over a sweep.
    const f = scrapeDeployFixture();
    const result = f.run({ FIXTURE_QUERY_FAIL: '1' });
    expect(result.status, result.stderr).not.toBe(0);
    expect(result.stderr).toContain('could not read wfm-scrape.service state');
    expect(liveBinary(f), 'the previous release must still be live').toBe('old-live-binary');
  });

  test('aborts before installing on a sweep state it cannot interpret', () => {
    const f = scrapeDeployFixture({ states: ['maintenance'] });
    const result = f.run();
    expect(result.status, result.stderr).not.toBe(0);
    expect(result.stderr).toContain('could not read wfm-scrape.service state');
    expect(liveBinary(f)).toBe('old-live-binary');
  });

  test('waits through transitional sweep states before installing', () => {
    // `deactivating` is a running phase, not a terminal one: replacing files
    // during it still lets one sweep span two releases.
    const f = scrapeDeployFixture({ states: ['deactivating', 'active'] });
    const result = f.run();
    expect(result.status, result.stderr).toBe(0);
    expect(result.stdout).toContain('a sweep is deactivating');
    expect(result.stdout).toContain('a sweep is active');
    const order = readFileSync(f.order, 'utf8').split('\n').filter(Boolean);
    expect(order.filter(line => line === 'is-active active')).toHaveLength(1);
    expect(order.indexOf('live-install'), 'live files must wait for a terminal state').toBeGreaterThan(order.lastIndexOf('is-active active'));
  });

  test('leaves the live paths on the previous release when the policy is rejected', () => {
    // The release is installed and proven first; only a passing proof may move
    // the live paths, otherwise a rejected policy leaves a half-swapped tree
    // that the restored timer then runs against.
    const f = scrapeDeployFixture();
    const result = f.run({ FIXTURE_POLICY_EXIT: '1' });
    expect(result.status, result.stderr).not.toBe(0);
    expect(result.stderr).toContain('rejects the live policy');
    expect(liveBinary(f)).toBe('old-live-binary');
    expect(existsSync(join(f.wfm, 'releases', f.revision, 'wfm-policy')), 'the release install must have run').toBe(true);
  });

  test('replaces the live paths once the release and its policy proof pass', () => {
    const f = scrapeDeployFixture();
    const result = f.run();
    expect(result.status, result.stderr).toBe(0);
    expect(liveBinary(f)).not.toBe('old-live-binary');
    expect(readFileSync(join(f.wfm, 'deployed.json'), 'utf8')).toContain(f.revision);
  });

  test('a deploy that cannot restore the sweep timer does not report success', () => {
    // The failure path swallows this into a warning; the success path must not,
    // or a deploy can leave the box with no schedule and exit zero.
    const f = scrapeDeployFixture();
    const result = f.run({ FIXTURE_START_FAIL: '1' });
    expect(result.status, result.stderr).not.toBe(0);
    expect(result.stdout).toContain('restore the timer by hand');
    const calls = readFileSync(f.systemctlLog, 'utf8');
    expect(calls, 'the monitors must not be armed without a running schedule')
      .not.toContain('enable --now wfm-archive-receipt-pull.timer');
  });

  test('the external-backup deploy neither runs nor arms the receipt pull', () => {
    // Preservation for this deployment is the host-level Proxmox backup, so the
    // receipt pull has nothing to gate and must not be started or enabled; a
    // leftover timer from an earlier archive deployment has to be switched off.
    const f = scrapeDeployFixture();
    expect(f.run().status).toBe(0);
    const calls = readFileSync(f.systemctlLog, 'utf8');
    expect(calls, 'external backups do not fetch a receipt').not.toContain('start wfm-archive-receipt-pull.service');
    expect(calls, 'the check still runs').toContain('start wfm-observations-check.service');
    expect(calls).toContain('enable --now wfm-observations-check.timer');
    expect(calls, 'a switched deployment must stop the old pull').toContain('disable --now wfm-archive-receipt-pull.timer');
  });

  test('the on-box archive deploy still runs the pull before the check', () => {
    // Two persistent timers can elapse in either order after downtime, so the
    // initial pair is explicit rather than left to them.
    const f = scrapeDeployFixture({ env: { PRESERVATION_MODE: 'on-box-archive' } });
    expect(f.run().status).toBe(0);
    const calls = readFileSync(f.systemctlLog, 'utf8').split('\n');
    const pull = calls.indexOf('start wfm-archive-receipt-pull.service');
    const check = calls.indexOf('start wfm-observations-check.service');
    expect(pull, 'the pull must be started').toBeGreaterThan(-1);
    expect(check, 'the check must be started').toBeGreaterThan(pull);
    expect(calls).toContain('enable --now wfm-archive-receipt-pull.timer wfm-observations-check.timer');
  });

  test('a first check that is not ready fails the deploy', () => {
    // Deployment is held until the archive path works end to end; the installer
    // is where that gate is enforced, so it cannot be passed by ignoring it.
    const f = scrapeDeployFixture();
    writeFileSync(join(f.wfm, 'data/observations-check/report.json'), '{"ready": false, "errors": ["no archive receipt"]}');
    const result = f.run();
    expect(result.status, result.stderr).not.toBe(0);
    expect(result.stderr).toContain('does not pass on the box');
  });

  test('a first check that produced no report at all fails the install', () => {
    const f = scrapeDeployFixture();
    rmSync(join(f.wfm, 'data/observations-check/report.json'));
    const result = f.run();
    expect(result.status, result.stderr).not.toBe(0);
    expect(result.stderr).toContain('produced no report');
  });
});

// The whole deployment path, not a wiring assertion: the real deploy script runs
// with the real check script and unit installed, the stubbed systemd executes
// the unit's own ExecStart, and the assertions read the readiness report the
// check actually produced. No archive receipt exists or is produced anywhere.
describe.skipIf(process.platform === 'win32')('external-backup deployment readiness', () => {
  test('a receipt-free deploy comes out ready and declares preservation external', () => {
    const f = scrapeDeployFixture({ runCheck: true });
    const result = f.run();
    expect(result.status, result.stderr).toBe(0);
    expect(existsSync(join(f.box, 'data/observations-check/archive-receipt.jsonl'))).toBe(false);

    // The archive path is present as files but is neither run nor armed, and a
    // leftover pull timer from the previous deployment is switched off.
    const calls = readFileSync(f.systemctlLog, 'utf8');
    expect(calls, 'the receipt pull must not run').not.toContain('start wfm-archive-receipt-pull.service');
    expect(calls, 'nor be enabled').not.toContain('enable --now wfm-archive-receipt-pull.timer');
    expect(calls).toContain('disable --now wfm-archive-receipt-pull.timer');
    expect(calls).toContain('start wfm-observations-check.service');

    // The check ran with the deployment's own declaration and dependency reset.
    expect(readFileSync(join(f.live, 'etc/wfm-observations-check.env'), 'utf8'))
      .toContain('OBSERVATIONS_PRESERVATION=external-backup');
    expect(existsSync(join(f.live, 'etc/systemd/system/wfm-observations-check.service.d/preservation.conf'))).toBe(true);
    expect(readFileSync(join(f.live, 'etc/systemd/system/wfm-observations-check.service.d/preservation.conf'), 'utf8'))
      .toMatch(/^Wants=$/m);

    // And the report it produced is ready, honest about what was not verified.
    const report = JSON.parse(readFileSync(f.reportPath, 'utf8'));
    expect(report.ready).toBe(true);
    expect(report.errors).toEqual([]);
    expect(report.preservation.mode).toBe('external-backup');
    expect(report.preservation.status).toBe('declared-external');
    expect(report.preservation.independently_verified_from_this_host).toBe(false);
    expect(report.archive.status).toBe('not-applicable');
  });

  test('the same box in on-box-archive mode is not ready without a receipt', () => {
    const f = scrapeDeployFixture({ runCheck: true, env: { PRESERVATION_MODE: 'on-box-archive' } });
    const result = f.run();
    expect(result.status, result.stderr).not.toBe(0);
    expect(result.stderr).toContain('does not pass on the box');
    // The check really ran and returned its own verdict, rather than the deploy
    // failing before the check existed.
    const report = JSON.parse(readFileSync(f.reportPath, 'utf8'));
    expect(report.ready).toBe(false);
    expect(report.preservation.mode).toBe('on-box-archive');
    expect(report.archive.status).toBe('missing');
    expect(report.errors.join('\n')).toContain('not being preserved');
    expect(existsSync(join(f.box, 'data/observations-check/archive-receipt.jsonl'))).toBe(false);
  });
});

describe('deployment ownership boundary', () => {
  const setup = readFileSync(fileURLToPath(new URL('../deploy/setup-container.sh', import.meta.url)), 'utf8');

  test('does not hand the whole deployment root to the service user', () => {
    // Root executes the scripts under /srv/wfm. A recursive chown makes each of
    // them replaceable through its parent directory even after the files
    // themselves are root-owned, which is a direct local escalation path.
    expect(setup).not.toMatch(/^\s*chown\s+-R\s+wfm:wfm\s+\/srv\/wfm\s*$/m);
  });

  test('root-owns the deployment root and the scripts root executes', () => {
    expect(setup).toMatch(/^\s*chown\s+root:root\s+\/srv\/wfm\s*$/m);
    expect(setup).toMatch(/chown\s+-R\s+root:root[^\n]*\/srv\/wfm\/bin/);
  });

  test('gives the observation log directory to the service user alone', () => {
    // The unit runs with ProtectSystem=strict, so the sweep can only write the
    // paths it is granted. Nothing but the pipeline needs to read these rows.
    expect(setup).toMatch(/mkdir\s+-p\s+\/srv\/wfm\/observations/);
    expect(setup).toMatch(/^\s*chown\s+wfm:wfm\s+\/srv\/wfm\/observations\s*$/m);
    expect(setup).toMatch(/^\s*chmod\s+750\s+\/srv\/wfm\/observations\s*$/m);
    const unit = readFileSync(
      fileURLToPath(new URL('../deploy/wfm-scrape.service', import.meta.url)),
      'utf8',
    );
    expect(unit, 'the unit must be allowed to write it').toMatch(
      /^\s*ReadWritePaths=.*\/srv\/wfm\/observations/m,
    );
  });

  test('leaves the scrape pipeline for the deploy script to install', () => {
    // The pipeline has exactly one installer now. The box ran a weeks-old
    // run-scrape.sh because setup-container.sh installed a copy of its own and
    // nothing reconciled it with the checkout.
    for (const file of ['run-scrape.sh', 'pull-scrape.sh', 'wfm-scrape.service', 'wfm-scrape.timer', 'wfm-scrape-pull.service', 'wfm-scrape-pull.timer']) {
      expect(setup, `${file} must not be installed here`).not.toMatch(new RegExp(`install[^\\n]*${file.replace(/\./g, '\\.')}`));
    }
  });
});

describe('scrape pipeline delivery boundary', () => {
  const pullApp = readFileSync(fileURLToPath(new URL('../deploy/pull-app.sh', import.meta.url)), 'utf8');

  test('the checkout puller never installs or drift-reports a scrape-owned file', () => {
    // scripts/deploy-scrape-host.sh is the only writer of the pipeline. The box
    // once ran a weeks-old copy because the checkout puller and setup-container
    // both installed one; a name reappearing in either list re-opens that.
    expect(pullApp).not.toMatch(/for f in [^;]*run-scrape\.sh/);
    expect(pullApp).not.toMatch(/for f in [^;]*pull-scrape\.sh/);
    expect(pullApp).not.toMatch(/for u in [^;]*wfm-scrape(-pull)?( |$)/m);
  });

  test('no workflow republishes the pipeline through a GitHub release', () => {
    // Host-direct deploy is the only delivery path now; a workflow that still
    // published scrape-latest or ran pull-scrape would reopen the relay.
    const dir = fileURLToPath(new URL('../.github/workflows', import.meta.url));
    for (const name of readdirSync(dir)) {
      const body = readFileSync(join(dir, name), 'utf8');
      expect(body, `${name} must not publish a scrape-latest release`).not.toContain('scrape-latest');
      expect(body, `${name} must not reference the retired puller`).not.toContain('pull-scrape');
    }
  });
});

// ---- run-scrape.sh: a run that publishes nothing must not republish --------

function runScrapeFixture(scrape: string) {
  const app = mkdtempSync(join(tmpdir(), 'run-scrape-'));
  directories.push(app);
  // A previous generation that clears the row-count floor, so the floor alone
  // cannot catch a scrape that exits 0 without replacing the file.
  writeFileSync(join(app, 'wfm_results.csv'), `url_name,median_90d\n${'prior,1\n'.repeat(1000)}`);
  const bin = join(app, 'scrape-stub');
  writeFileSync(
    bin,
    ['#!/bin/sh', 'case "$1" in', `  scrape) ${scrape} ;;`, '  build) exit 0 ;;', '  history) exit 0 ;;', 'esac', 'exit 0', ''].join('\n'),
  );
  chmodSync(bin, 0o755);
  const run = () => spawnSync('bash', [fileURLToPath(new URL('../deploy/run-scrape.sh', import.meta.url))], {
    env: { ...process.env, APP: app, SCRAPE_BIN: bin, HISTORY: '0' },
    encoding: 'utf8',
  });
  return { app, run };
}

const REPLACE_CSV =
  'rm -f wfm_results.csv; { printf "url_name,median_90d\\n"; i=0; while [ "$i" -lt 1000 ]; do printf "row%s,1\\n" "$i"; i=$((i+1)); done; } > wfm_results.csv';

describe('run-scrape publication guard', () => {
  test('a scrape that exits 0 without replacing the CSV aborts instead of rebuilding', () => {
    const { run } = runScrapeFixture('exit 0');
    const result = run();
    expect(result.status, 'must not publish the previous generation').not.toBe(0);
    expect(result.stderr).toContain('without replacing');
  });

  test('a scrape that replaces the CSV still publishes', () => {
    const { app, run } = runScrapeFixture(REPLACE_CSV);
    const result = run();
    expect(result.stderr, result.stderr).not.toContain('without replacing');
    expect(result.status, result.stderr).toBe(0);
    expect(readFileSync(join(app, 'wfm_results.csv'), 'utf8')).toContain('row999');
  });
});

// ---- observation corpus: readiness check and off-box archive ---------------

const checkScript = fileURLToPath(new URL('../deploy/observations-check.sh', import.meta.url));
const archiveScript = fileURLToPath(new URL('./archive-observations.sh', import.meta.url));
const pullScript = fileURLToPath(new URL('../deploy/pull-archive-receipt.sh', import.meta.url));
const backupScript = fileURLToPath(new URL('./check-lxc-backup.sh', import.meta.url));
const retentionFixture = JSON.parse(
  readFileSync(fileURLToPath(new URL('../tests/fixtures/observation-retention.json', import.meta.url)), 'utf8'),
) as { max_bytes: number; max_age_days: number };

describe('observation corpus check wiring', () => {
  const deploy = readFileSync(fileURLToPath(new URL('./deploy-scrape-host.sh', import.meta.url)), 'utf8');
  const setup = readFileSync(fileURLToPath(new URL('../deploy/setup-container.sh', import.meta.url)), 'utf8');
  const pullApp = readFileSync(fileURLToPath(new URL('../deploy/pull-app.sh', import.meta.url)), 'utf8');
  const check = readFileSync(checkScript, 'utf8');
  const unit = readFileSync(fileURLToPath(new URL('../deploy/wfm-observations-check.service', import.meta.url)), 'utf8');
  const timer = readFileSync(fileURLToPath(new URL('../deploy/wfm-observations-check.timer', import.meta.url)), 'utf8');
  const rust = readFileSync(fileURLToPath(new URL('../rust/wfm-scrape/src/observations.rs', import.meta.url)), 'utf8');

  test('the deploy script is the only installer of the check, the pull and their units', () => {
    // The check reads scrape-owned state and is host-only infrastructure, so it
    // travels with the pipeline that writes that state. A second installer is
    // the regression this pins: it could advance the check on its own, judging a
    // corpus by a ruleset the deployed pipeline does not have.
    const owned = ['observations-check.sh', 'wfm-observations-check.service', 'wfm-observations-check.timer',
      'pull-archive-receipt.sh', 'wfm-archive-receipt-pull.service', 'wfm-archive-receipt-pull.timer'];
    for (const file of owned) {
      expect(deploy, `${file} must be staged`).toContain(`$STAGING/${file}`);
      expect(deploy, `${file} must land in the release`).toContain(`$RELEASES/$REVISION/${file}`);
    }
    expect(deploy).toMatch(/install -m 0755 "\$RELEASES\/\$REVISION\/observations-check\.sh" "\/srv\/wfm\/observations-check\.sh"/);
    expect(deploy).toMatch(/install -m 0644 "\$RELEASES\/\$REVISION\/wfm-observations-check\.service" \/etc\/systemd\/system\/wfm-observations-check\.service/);
    expect(deploy).toMatch(/install -m 0644 "\$RELEASES\/\$REVISION\/wfm-observations-check\.timer" \/etc\/systemd\/system\/wfm-observations-check\.timer/);
    expect(deploy).toMatch(/install -m 0755 "\$RELEASES\/\$REVISION\/pull-archive-receipt\.sh" "\/srv\/wfm\/pull-archive-receipt\.sh"/);
    expect(deploy).toMatch(/install -m 0644 "\$RELEASES\/\$REVISION\/wfm-archive-receipt-pull\.service" \/etc\/systemd\/system\/wfm-archive-receipt-pull\.service/);
    expect(deploy).toMatch(/install -m 0644 "\$RELEASES\/\$REVISION\/wfm-archive-receipt-pull\.timer" \/etc\/systemd\/system\/wfm-archive-receipt-pull\.timer/);
    // ProtectSystem=strict grants write access to exactly this path, and systemd
    // refuses to start a unit whose ReadWritePaths is missing.
    expect(deploy).toMatch(/install -d -m 0750 -o root -g root "\$HOST_ROOT\/data\/observations-check"/);
    expect(deploy).toMatch(/systemctl enable --now wfm-archive-receipt-pull\.timer wfm-observations-check\.timer/);
    for (const file of owned) {
      expect(setup, `${file} must not be installed by provisioning`).not.toContain(file);
      expect(pullApp, `${file} must not be installed by the checkout puller`).not.toContain(file);
    }
    // The archive host's own units are not the box's and must not travel here.
    expect(deploy).not.toContain('archive-observations.service');
    expect(deploy).not.toContain('archive-observations.timer');
  });

  test('the monitors are armed only after the record and the schedule settle', () => {
    // Both monitors read deployed.json and the sweep schedule. A persistent
    // timer armed before those settle can fire into the intermediate state and
    // report on a deployment that is still in progress.
    const record = deploy.indexOf("cat > '$HOST_ROOT/deployed.json'");
    const restore = deploy.search(/^restore_timer$/m);
    const monitors = deploy.indexOf('systemctl enable --now wfm-archive-receipt-pull.timer wfm-observations-check.timer');
    expect(record, 'the deployment record must be written').toBeGreaterThan(-1);
    expect(restore, 'the schedule must be restored by the explicit call, not only the trap').toBeGreaterThan(record);
    expect(monitors, 'the monitors must be armed last').toBeGreaterThan(restore);
    expect(deploy.slice(0, record), 'nothing may arm a monitor before the record exists')
      .not.toMatch(/enable --now wfm-(observations-check|archive-receipt-pull)\.timer/);
  });

  test('the receipt pull is separate, read-only and atomic', () => {
    // The check itself runs with AF_UNIX only so its verdict cannot depend on a
    // remote host; retrieval is its own unit with its own alert, which is why
    // the address-family restriction stays on one side of the split.
    const pull = readFileSync(pullScript, 'utf8');
    expect(pull).toMatch(/SCP="\$\{SCP:-scp\}"/);
    expect(pull).toMatch(/ARCHIVE_RECEIPT_SOURCE:-\}/);
    expect(pull).toMatch(/\$SCP \$SCP_OPTS -q "\$SOURCE" "\$tmp"/);
    expect(pull).toMatch(/mv "\$tmp" "\$OUT"/);
    expect(pull).toMatch(/"kind":"archive_receipt"/);
    expect(unit).toMatch(/^RestrictAddressFamilies=AF_UNIX$/m);
    const pullUnit = readFileSync(fileURLToPath(new URL('../deploy/wfm-archive-receipt-pull.service', import.meta.url)), 'utf8');
    expect(pullUnit).toMatch(/^OnFailure=wfm-alert@%n\.service$/m);
    expect(pullUnit).toMatch(/^EnvironmentFile=-\/etc\/wfm-archive-receipt\.env$/m);
    expect(pullUnit).toMatch(/^ReadWritePaths=\/srv\/wfm\/data\/observations-check$/m);
    expect(pullUnit).toMatch(/^RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6$/m);
    // The pull has to run before the check, or the check reads yesterday's
    // receipt and calls a stopped archive fresh.
    const pullTimer = readFileSync(fileURLToPath(new URL('../deploy/wfm-archive-receipt-pull.timer', import.meta.url)), 'utf8');
    const at = (body: string) => {
      const match = body.match(/^OnCalendar=\S+ (\S+):(\d{2}):\d{2} UTC$/m);
      expect(match, 'every timer states its timezone explicitly').not.toBeNull();
      return { hour: match![1]!, minute: Number(match![2]) };
    };
    const pullMinutes = at(pullTimer);
    const checkMinutes = at(timer);
    expect(pullMinutes.hour, 'the pull must run hourly, not once a day').toBe('*');
    expect(checkMinutes.hour, 'the check must run hourly too').toBe('*');
    expect(pullMinutes.minute).toBeLessThan(checkMinutes.minute);
  });

  test('the monitors run hourly in UTC and the check pulls before it reads', () => {
    // A 36-hour deadline evaluated once a day can breach and stand for most of
    // another day; both sides of the pair therefore run hourly, in UTC, and the
    // check starts the pull itself so two persistent timers cannot elapse in the
    // wrong order after downtime.
    expect(timer).toMatch(/^OnCalendar=\*-\*-\* \*:30:00 UTC$/m);
    const pullTimer = readFileSync(fileURLToPath(new URL('../deploy/wfm-archive-receipt-pull.timer', import.meta.url)), 'utf8');
    expect(pullTimer).toMatch(/^OnCalendar=\*-\*-\* \*:15:00 UTC$/m);
    const archiveTimer = readFileSync(fileURLToPath(new URL('../deploy/archive-observations.timer', import.meta.url)), 'utf8');
    expect(archiveTimer).toMatch(/^OnCalendar=\*-\*-\* 05:30:00 UTC$/m);
    expect(unit).toMatch(/^Wants=wfm-archive-receipt-pull\.service$/m);
    expect(unit).toMatch(/^After=wfm-archive-receipt-pull\.service$/m);
  });

  test('the archive host installer provisions and verifies the alert path', () => {
    // `OnFailure=wfm-alert@%n.service` provisions nothing: without the template
    // and the handler on that host, an archive failure alerts nobody.
    const install = readFileSync(fileURLToPath(new URL('./install-archive-host.sh', import.meta.url)), 'utf8');
    for (const file of ['archive-observations.sh', 'alert.sh', 'wfm-alert@.service',
      'archive-observations.service', 'archive-observations.timer']) {
      expect(install, `${file} must be installed`).toContain(file);
    }
    expect(install, 'the template path must be rewritten for this host').toMatch(/s#\/srv\/wfm\/alert\.sh#\$PREFIX\/alert\.sh#/);
    expect(install).toMatch(/SYSTEMD_ANALYZE:-systemd-analyze/);
    expect(install).toMatch(/"\$SYSTEMD_ANALYZE" verify/);
    expect(install, 'the handler must actually be run').toMatch(/"\$PREFIX\/alert\.sh" archive-observations\.service/);
    expect(install, 'and required to have recorded something').toMatch(/\[ -s "\$recorded_log" \]/);
    expect(install).toMatch(/enable --now archive-observations\.timer/);
  });

  test('the pull and the deploy gate are bounded and cannot be skipped', () => {
    const pull = readFileSync(pullScript, 'utf8');
    expect(pull).toMatch(/timeout "\$COMMAND_TIMEOUT" \$SCP \$SCP_OPTS/);
    expect(pull).toMatch(/connecttimeout|ConnectTimeout/);
    // The deploy refuses to report success on a box the check says is not ready,
    // and it runs the pair itself rather than trusting timer ordering.
    expect(deploy).toMatch(/systemctl start wfm-archive-receipt-pull\.service/);
    expect(deploy).toMatch(/test -s '\$HOST_ROOT\/data\/observations-check\/report\.json'/);
    const pullStart = deploy.indexOf('systemctl start wfm-archive-receipt-pull.service');
    const checkStart = deploy.indexOf('systemctl start wfm-observations-check.service');
    expect(checkStart).toBeGreaterThan(pullStart);
    expect(deploy).toMatch(/the readiness check does not pass on the box/);
  });

  test('the archive host has its own schedule and failure notification', () => {
    const service = readFileSync(fileURLToPath(new URL('../deploy/archive-observations.service', import.meta.url)), 'utf8');
    const archiveTimer = readFileSync(fileURLToPath(new URL('../deploy/archive-observations.timer', import.meta.url)), 'utf8');
    expect(service).toMatch(/^OnFailure=wfm-alert@%n\.service$/m);
    expect(service).toMatch(/^EnvironmentFile=-\/etc\/tennoworth-archive\.env$/m);
    expect(service).toMatch(/^ExecStart=\/usr\/local\/bin\/archive-observations\.sh$/m);
    // A run that never happened is caught on the box by the receipt going
    // stale; Persistent makes a host that was off at the elapse catch up.
    expect(archiveTimer).toMatch(/^Persistent=true$/m);
    expect(archiveTimer).toMatch(/^WantedBy=timers\.target$/m);
  });

  test('the check unit fails closed and can write only its report', () => {
    expect(unit).toMatch(/^Type=oneshot$/m);
    expect(unit).toMatch(/^OnFailure=wfm-alert@%n\.service$/m);
    expect(unit).toMatch(/^ProtectSystem=strict$/m);
    expect(unit).toMatch(/^ReadWritePaths=\/srv\/wfm\/data\/observations-check$/m);
    expect(unit).toMatch(/^ExecStart=\/srv\/wfm\/observations-check\.sh --observations \/srv\/wfm\/observations --archive-receipt \/srv\/wfm\/data\/observations-check\/archive-receipt\.jsonl --out \/srv\/wfm\/data\/observations-check\/report\.json$/m);
    expect(timer).toMatch(/^OnCalendar=\*-\*-\* \*:30:00 UTC$/m);
    expect(timer).toMatch(/^WantedBy=timers\.target$/m);
  });

  test('the log contract is pinned by a shared fixture on both sides', () => {
    // The writer stamps the format and prunes with Rust constants, the box check
    // reads the format and judges the corpus against shell defaults. The fixture
    // is the only place the two can disagree loudly; the Rust side reads it in
    // observations::tests::log_contract_matches_the_shared_fixture.
    expect(rust, 'the writer must keep its side of the fixture gate').toContain('observation-retention.json');
    expect(Number(check.match(/FORMAT_SUPPORTED=(\d+)/)![1])).toBe(retentionFixture.format);
    // The backup job reads the archived logs by the same format rule, so it has
    // to agree with the writer and the box check as well.
    const backup = readFileSync(backupScript, 'utf8');
    expect(Number(backup.match(/FORMAT_SUPPORTED=(\d+)/)![1])).toBe(retentionFixture.format);
    expect(Number(check.match(/RETENTION_BYTES=(\d+)/)![1])).toBe(retentionFixture.max_bytes);
    expect(Number(check.match(/RETENTION_AGE_DAYS=(\d+)/)![1])).toBe(retentionFixture.max_age_days);
    expect(retentionFixture.format).toBe(1);
    expect(retentionFixture.max_bytes).toBe(2 * 1024 * 1024 * 1024);
    expect(retentionFixture.max_age_days).toBe(56);
  });

  test('the readiness check reads the catalog count instead of hard-coding it', () => {
    // 3,840 is today's catalog. Written into the check it would silently stop
    // matching the log it is verifying the first time the catalog moves.
    expect(check).not.toContain('3840');
    expect(check).toMatch(/record_field "\$header" items/);
    expect(check).toMatch(/baseline_items=/);
  });

  test('collection readiness stays separate from the schedule decision', () => {
    // "The corpus is sound" and "the proposed refresh schedule meets its
    // thresholds" are different questions; folding the second into the first
    // would let a good collection pass for a good schedule. The check may name
    // that other owner, but it must never invoke it.
    expect(check, 'the scope must be stated in the script').toMatch(/is the corpus sound and complete/);
    expect(check).not.toMatch(/--schedule/);
    expect(check).not.toMatch(/\breplay\b[^\n]*--/);
  });

  test('the archive script verifies what it stored and records honest provenance', () => {
    const archive = readFileSync(archiveScript, 'utf8');
    expect(archive).toMatch(/SSH="\$\{SSH:-ssh\}"/);
    expect(archive).toMatch(/SCP="\$\{SCP:-scp\}"/);
    expect(archive).toMatch(/DEST="\$\{DEST:-\$HOME\/\.local\/share\/tennoworth\/observations-archive\}"/);
    expect(archive).toMatch(/gzip -9 -c/);
    // Verification has to decompress the stored bytes, not trust the compress
    // step: comparing the pre-compression file proves nothing about the .gz.
    expect(archive).toMatch(/gzip -dc "\$packed_tmp" \| sha256sum/);
    expect(archive).toMatch(/mv "\$packed_tmp" "\$packed"/);
    // A manifest row is not proof of an archive. The stored artifact and both
    // recorded hashes have to agree, or the file is re-fetched.
    expect(archive).toMatch(/\[ -f "\$DEST\/\$name\.gz" \] \|\| return 1/);
    expect(archive).toMatch(/gzip -dc "\$DEST\/\$name\.gz" \| sha256sum/);
    expect(archive).toMatch(/manifest_upsert/);
    // The receipt is the box-readable heartbeat, published atomically and only
    // after a run in which nothing failed.
    expect(archive).toMatch(/mv "\$STAGE\/receipt\.jsonl" "\$RECEIPT"/);
    expect(archive).toMatch(/if \[ "\$failed" != 0 \]; then[\s\S]*die /);
    // Every claim is re-verified before the receipt renews it, because the
    // per-file loop only sees what the box still lists.
    expect(archive).toMatch(/verify_manifest\(\)/);
    expect(archive, 'all claims must be verified before the receipt publishes')
      .toMatch(/if ! verify_manifest; then[\s\S]*no receipt was published[\s\S]*publish_receipt/);
    // Unattended runs are bounded, and two of them cannot share the manifest.
    expect(archive).toMatch(/timeout "\$COMMAND_TIMEOUT" \$SSH \$SSH_OPTS/);
    expect(archive).toMatch(/timeout "\$COMMAND_TIMEOUT" \$SCP \$SCP_OPTS/);
    expect(archive).toMatch(/flock -n 9/);
    for (const field of ['"name"', '"bytes"', '"sha256"', '"gz_sha256"', '"archived_at"',
      '"deployed_revision_at_archive"', '"producer_revision":null', '"kind":"archive_receipt"', '"verified_at"']) {
      expect(archive, `the manifest must record ${field}`).toContain(field);
    }
    expect(archive, 'the box is read-only to this script').not.toMatch(/\$SSH[^\n]*\b(rm|mv|cp|install)\b/);
  });

  test('the preservation mode is an explicit input that fails closed when absent', () => {
    // The check can only see this box. A mode it was never told must not read as
    // "no news is good news": undeclared preservation is a not-ready verdict,
    // and the report has to say which claim it is making.
    expect(check).toMatch(/PRESERVATION="\$\{OBSERVATIONS_PRESERVATION:-\}"/);
    expect(check).toMatch(/--preservation\) PRESERVATION="\$2"/);
    expect(check).toContain('external-backup');
    expect(check).toContain('on-box-archive');
    expect(check).toContain('preservation mode not declared');
    expect(check).toMatch(/independently_verified_from_this_host/);
    // The unit gets the mode from a file the deploy writes, so the declaration
    // travels with the deployment rather than living in a shell default.
    expect(unit).toMatch(/^EnvironmentFile=-\/etc\/wfm-observations-check\.env$/m);
  });

  test('the deploy declares the preservation mode and releases the receipt dependency for it', () => {
    expect(deploy).toMatch(/PRESERVATION_MODE="\$\{PRESERVATION_MODE:-external-backup\}"/);
    expect(deploy).toContain('/etc/wfm-observations-check.env');
    expect(deploy, 'the declaration must actually be written').toMatch(/^OBSERVATIONS_PRESERVATION=\$PRESERVATION_MODE$/m);
    // external-backup cannot see the host-level backups, so the check must not
    // wait on or pull the archive host's receipt in that mode. A drop-in resets
    // the unit's own Wants/After, which stay in the unit for the archive mode.
    expect(deploy).toMatch(/wfm-observations-check\.service\.d\/preservation\.conf/);
    expect(deploy).toMatch(/^Wants=$/m);
    expect(deploy).toMatch(/^After=$/m);
    expect(deploy).toMatch(/if \[ "\$PRESERVATION_MODE" = on-box-archive \]; then/);
    // The on-box branch is kept, not replaced: the archive machinery is intact
    // and selectable, it is simply not what this deployment uses.
    expect(deploy).toMatch(/systemctl enable --now wfm-archive-receipt-pull\.timer wfm-observations-check\.timer/);
  });

  test('the backup pull-and-verify script hard-codes no node, vmid or local path', () => {
    const backup = readFileSync(backupScript, 'utf8');
    expect(backup).toMatch(/HOST="\$\{HOST:-\}"/);
    expect(backup).toMatch(/VMID="\$\{VMID:-\}"/);
    expect(backup).toMatch(/DUMP_DIR="\$\{DUMP_DIR:-\/var\/lib\/vz\/dump\}"/);
    expect(backup).toMatch(/DEST="\$\{DEST:-\$HOME\/backups\/tennoworth\}"/);
    expect(backup).toMatch(/KEEP="\$\{KEEP:-30\}"/);
    // A sandboxed session cannot read the system ssh config, so SSH and SCP have
    // to carry their own options and stay unquoted at the call sites.
    expect(backup).toMatch(/SSH="\$\{SSH:-ssh\}"/);
    expect(backup).toMatch(/SCP="\$\{SCP:-scp\}"/);
    expect(backup).toMatch(/\$SSH \$SSH_OPTS "\$HOST"/);
    expect(backup).toMatch(/\$SCP \$SCP_OPTS/);
    // The whole point is receipt-side verification, not a trusted copy.
    expect(backup).toMatch(/zstd -t/);
    expect(backup).toMatch(/tar --zstd -tf/);
    // The run header is parsed as JSON, not pattern-matched: a prefix test and a
    // grep for `"items":` accepted a truncated record, trailing text after the
    // object, and a wrong format as a verified backup.
    expect(backup).toMatch(/json\.loads/);
    expect(backup).toMatch(/command -v python3/);
    expect(backup, 'the prefix test must be gone').not.toContain("'{\"kind\":\"run\"'*)");
    expect(backup).toMatch(/--dry-run/);
  });
});

/** One sweep's JSONL in the shape `src/observations.rs` writes. */
function sweepLog(startSeconds: number, items: number, kept: number, format = 1): string {
  const stamp = new Date(startSeconds * 1000).toISOString().replace('.000Z', 'Z');
  const lines = [JSON.stringify({
    kind: 'run', format, version: '0.1.0', platform: 'pc', filter: '', exclude: '',
    min_volume: 1, items, workers: 2, started_at: stamp,
  })];
  for (let i = 0; i < items - kept; i++) {
    lines.push(JSON.stringify({
      kind: 'item', slug: `thin_${startSeconds}_${i}`, name: 'Thin', tags: [], ducats: null,
      outcome: 'below_volume', subtype: null, volume_48h: 1, median_now: 1, median_90d: 1,
      medians_7d: [1], avg_price_48h: 1, donch_top_90d: 1, donch_bot_90d: 1,
    }));
  }
  for (let i = 0; i < kept; i++) {
    lines.push(JSON.stringify({
      kind: 'item', slug: `kept_${startSeconds}_${i}`, name: 'Kept', tags: [], ducats: null,
      outcome: 'kept', subtype: null, volume_48h: 9, median_now: 9, median_90d: 9,
      medians_7d: [9], avg_price_48h: 9, donch_top_90d: 9, donch_bot_90d: 9,
      book: { live_buys: 1, live_sells: 1, buy_sell_ratio: 1, top_buy_price: 1, low_sell_price: 1, low5_avg_unclamped: 1, score: 1 },
    }));
  }
  lines.push(JSON.stringify({ kind: 'summary', scanned: items, kept, coercions: 0 }));
  return lines.join('\n') + '\n';
}

// The evaluation instant every fixture is built around, and the invocation id
// its journal carries. systemd stamps `_SYSTEMD_INVOCATION_ID` on the unit's own
// messages but not on its own Starting/Finished lines, which is why the journal
// is JSON and the pairing below is explicit about the start line.
const FIXTURE_NOW = 1789580078;
const FIXTURE_INVOCATION = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';

function sweepMetrics(statistics: number | string = 4, orders: number | string = 2, throttles = 0): string {
  return `sweep metrics: attempts[catalog=1 statistics=${statistics} orders=${orders} riven=0 other=0] ok[catalog=1 statistics=${statistics} orders=${orders} riven=0 other=0] failed[catalog=0 statistics=0 orders=0 riven=0 other=0] retries=0 wire_requests=7 throttles=${throttles} cooldown_waits=0 cooldown_wait_ms=0 decoded_bytes=1 elapsed_ms=1000 snapshot_age_s=0`;
}

function journalText(entries: Array<{ at: number; message: string; invocation?: string }>): string {
  return entries.map(entry => JSON.stringify({
    __REALTIME_TIMESTAMP: String(entry.at * 1_000_000),
    ...(entry.invocation ? { _SYSTEMD_INVOCATION_ID: entry.invocation } : {}),
    MESSAGE: entry.message,
  })).join('\n') + '\n';
}

// A box fixture: three completed sweeps two hours apart, the newest an hour
// before the evaluation instant, with the snapshot, CSV, deployment record and
// archive receipt they imply. `journalctl` and `systemctl` are stubs on PATH, as
// in the host-direct deploy tests - the script's own control flow is what is
// observed.
function corpusFixture() {
  const root = mkdtempSync(join(tmpdir(), 'obs-check-')); directories.push(root);
  const box = join(root, 'box'), bin = join(root, 'bin');
  for (const path of [join(box, 'observations'), join(box, 'app/frontend/public'), join(box, 'bin'), bin]) {
    mkdirSync(path, { recursive: true });
  }
  const now = FIXTURE_NOW;
  const newestStart = now - 6000;
  const firstStart = newestStart - 2 * 7200;
  const names: string[] = [];
  for (let i = 0; i < 3; i++) {
    const start = firstStart + i * 7200;
    const stamp = new Date(start * 1000).toISOString().replace('.000Z', 'Z');
    const name = `sweep-${stamp.replaceAll(':', '-')}.jsonl`;
    const path = join(box, 'observations', name);
    writeFileSync(path, sweepLog(start, 4, 2));
    utimesSync(path, start + 4200, start + 4200);
    names.push(name);
  }
  writeFileSync(join(box, 'app/wfm_results.csv'), 'url_name,median_90d\nkept_a,1\nkept_b,1\n');
  const snapshot = join(box, 'app/frontend/public/market.json');
  writeFileSync(snapshot, '{"catalog":{}}');
  utimesSync(snapshot, now - 1700, now - 1700);
  const binary = join(box, 'bin/wfm-scrape');
  writeFileSync(binary, '#!/bin/sh\necho wfm-scrape\n');
  const sha256 = createHash('sha256').update(readFileSync(binary)).digest('hex');
  writeFileSync(join(box, 'deployed.json'), `{\n  "revision": "fixture-rev",\n  "sha256": "${sha256}"\n}\n`);

  const iso = (seconds: number) => new Date(seconds * 1000).toISOString().replace('.000Z', 'Z');
  const receipt = join(root, 'receipt.jsonl');
  const writeReceipt = (options: { verifiedAt?: number; covered?: string[]; headerFields?: Record<string, unknown> } = {}) => {
    const covered = options.covered ?? names;
    const header = JSON.stringify({
      kind: 'archive_receipt', format: 1, verified_at: iso(options.verifiedAt ?? now - 600),
      archive_host: 'archive-host', source_host: 'wfm', source_dir: '/srv/wfm/observations',
      deployed_revision_at_archive: 'fixture-rev', files: covered.length,
      ...options.headerFields,
    });
    const rows = covered.map(name => JSON.stringify({
      name, bytes: 1,
      sha256: createHash('sha256').update(readFileSync(join(box, 'observations', name))).digest('hex'),
      gz_sha256: 'bb', archived_at: iso(now - 600),
      deployed_revision_at_archive: 'fixture-rev', producer_revision: null,
    }));
    writeFileSync(receipt, [header, ...rows].join('\n') + '\n');
  };
  writeReceipt();

  const journal = join(root, 'journal');
  const writeJournal = (entries: Array<{ at: number; message: string; invocation?: string }>) =>
    writeFileSync(journal, journalText(entries));
  writeJournal([
    { at: newestStart, message: 'Starting wfm-scrape.service - Refresh Warframe market.json from warframe.market + warframestat...' },
    { at: newestStart, message: 'scraper: /srv/wfm/bin/wfm-scrape scrape', invocation: FIXTURE_INVOCATION },
    { at: newestStart + 4200, message: sweepMetrics(), invocation: FIXTURE_INVOCATION },
    { at: newestStart + 4200, message: 'wfm-scrape.service: Deactivated successfully.' },
  ]);

  write(join(bin, 'systemctl'), [
    '#!/bin/sh',
    'if [ "$1" = "is-enabled" ]; then printf "%s\\n" "${FIXTURE_RETIRED:-disabled}"; exit 0; fi',
    'unit="$2"; prop="$4"',
    'case "$unit:$prop" in',
    '  wfm-scrape.service:ActiveState) printf "inactive\\n";;',
    '  wfm-scrape.service:Result) printf "%s\\n" "${FIXTURE_SERVICE_RESULT:-success}";;',
    '  wfm-scrape.timer:UnitFileState) printf "enabled\\n";;',
    '  wfm-scrape.timer:ActiveState) printf "active\\n";;',
    '  wfm-scrape.timer:NextElapseUSecRealtime) printf "Fri 2026-09-18 06:30:00 UTC\\n";;',
    'esac',
    'exit 0',
    '',
  ].join('\n'));
  write(join(bin, 'journalctl'), '#!/bin/sh\ncat "${FIXTURE_JOURNAL:?}"\n');
  chmodSync(join(bin, 'systemctl'), 0o755);
  chmodSync(join(bin, 'journalctl'), 0o755);

  const reportPath = join(root, 'report.json');
  const run = (env: Record<string, string> = {}, extraArgs: string[] = []) => spawnSync('bash', [
    checkScript,
    '--observations', join(box, 'observations'),
    '--out', reportPath,
    '--csv', join(box, 'app/wfm_results.csv'),
    '--snapshot', snapshot,
    '--deployed', join(box, 'deployed.json'),
    '--binary', binary,
    '--archive-receipt', receipt,
    '--now', '2026-09-16T17:34:38Z',
    ...extraArgs,
  ], {
    encoding: 'utf8',
    // The unit gets the mode from EnvironmentFile=/etc/wfm-observations-check.env,
    // which the deploy writes; a test overrides it the same way a redeploy would.
    env: {
      ...process.env, PATH: `${bin}:${process.env.PATH}`,
      OBSERVATIONS_PRESERVATION: 'on-box-archive', FIXTURE_JOURNAL: journal, ...env,
    },
  });
  const report = () => JSON.parse(readFileSync(reportPath, 'utf8'));
  const observationPath = (name: string) => join(box, 'observations', name);
  return {
    root, box, names, newestStart, journal, receipt, run, report,
    observationPath, writeReceipt, writeJournal,
  };
}

describe.skipIf(process.platform === 'win32')('observation corpus readiness', () => {
  test('a sound corpus is ready and reports the window it holds', () => {
    const f = corpusFixture();
    const result = f.run();
    expect(result.status, result.stderr).toBe(0);
    const report = f.report();
    expect(report.ready).toBe(true);
    expect(report.sweeps.valid).toBe(3);
    expect(report.sweeps.expected).toBe(3);
    expect(report.sweeps.missing).toBe(0);
    expect(report.anomalies).toEqual({ partial: 0, malformed: 0, missing_summary: 0, unsupported_format: 0 });
    expect(report.completeness.failed).toEqual([]);
    expect(report.retention.bytes).toBeGreaterThan(0);
    expect(report.retention.max_age_seconds).toBe(retentionFixture.max_age_days * 86400);
    expect(report.retention.cap_bytes).toBe(retentionFixture.max_bytes);
    expect(report.csv.rows).toBe(2);
    expect(report.timers.sweep_enabled).toBe('enabled');
    // The service evidence is tied to one invocation and one sweep, so a green
    // service section means this sweep's own numbers were checked.
    expect(report.service.invocation).toBe(FIXTURE_INVOCATION);
    expect(report.service.sweep_log).toBe(f.names[2]!);
    expect(report.service.wall_seconds).toBe(4200);
    expect(report.service.drift_checked).toBe(true);
    expect(report.service.statistics_attempts).toBe(4);
    expect(report.service.orders_attempts).toBe(2);
    expect(report.archive.status).toBe('ok');
    expect(report.archive.covered).toBe(3);
    expect(report.archive.producer_revision_unknown).toBe(3);
  });

  test('a completed sweep with no log is not ready', () => {
    const f = corpusFixture();
    rmSync(f.observationPath(f.names[1]!));
    const result = f.run();
    expect(result.status).not.toBe(0);
    const report = f.report();
    expect(report.ready).toBe(false);
    expect(report.sweeps.expected).toBe(3);
    expect(report.sweeps.valid).toBe(2);
    expect(report.errors.join('\n')).toContain('have no log');
  });

  test('a malformed log is not ready', () => {
    const f = corpusFixture();
    writeFileSync(f.observationPath(f.names[2]!), 'not a record at all\n', { flag: 'a' });
    const result = f.run();
    expect(result.status).not.toBe(0);
    expect(f.report().anomalies.malformed).toBe(1);
    expect(f.report().errors.join('\n')).toContain('malformed');
  });

  test('a log without a summary is not ready', () => {
    const f = corpusFixture();
    const path = f.observationPath(f.names[2]!);
    const lines = readFileSync(path, 'utf8').trimEnd().split('\n');
    writeFileSync(path, lines.slice(0, -1).join('\n') + '\n');
    const result = f.run();
    expect(result.status).not.toBe(0);
    expect(f.report().anomalies.missing_summary).toBe(1);
    expect(f.report().errors.join('\n')).toContain('no summary');
  });

  test('an unsupported observation format is not ready', () => {
    const f = corpusFixture();
    const path = f.observationPath(f.names[2]!);
    writeFileSync(path, readFileSync(path, 'utf8').replace('"format":1', '"format":2'));
    const result = f.run();
    expect(result.status).not.toBe(0);
    expect(f.report().anomalies.unsupported_format).toBe(1);
    expect(f.report().errors.join('\n')).toContain('unsupported observation format');
  });

  test('a partial log old enough to be a stuck sweep is not ready', () => {
    const f = corpusFixture();
    const partial = f.observationPath('sweep-2026-09-16T19-04-38Z.jsonl.partial');
    writeFileSync(partial, '');
    utimesSync(partial, 1789580078 - 4 * 3600, 1789580078 - 4 * 3600);
    const result = f.run();
    expect(result.status).not.toBe(0);
    expect(f.report().anomalies.partial).toBe(1);
    expect(f.report().errors.join('\n')).toContain('stuck');
  });

  test('a sweep that missed its own CSV row count is not ready', () => {
    const f = corpusFixture();
    writeFileSync(join(f.box, 'app/wfm_results.csv'), 'url_name,median_90d\nkept_a,1\nkept_b,1\nkept_c,1\n');
    const result = f.run();
    expect(result.status).not.toBe(0);
    expect(f.report().errors.join('\n')).toContain('holds 3');
  });

  test('a completed log past its archival deadline is not ready', () => {
    const f = corpusFixture();
    // The oldest log completed well over a day and a half ago and the archive
    // host's receipt does not cover it: daily archival plus grace has been missed.
    utimesSync(f.observationPath(f.names[0]!), FIXTURE_NOW - 200000, FIXTURE_NOW - 200000);
    f.writeReceipt({ covered: f.names.slice(1) });
    const result = f.run();
    expect(result.status).not.toBe(0);
    expect(f.report().archive.status).toBe('lagging');
    expect(f.report().archive.overdue).toEqual([f.names[0]!]);
    expect(f.report().errors.join('\n')).toContain('past their archival deadline');
  });

  test('a missing archive receipt is not ready', () => {
    // An unmonitored archive must never render as ready: "nobody is copying
    // this" is the failure the receipt exists to make visible.
    const f = corpusFixture();
    const result = f.run({}, ['--archive-receipt', join(f.root, 'does-not-exist.jsonl')]);
    expect(result.status).not.toBe(0);
    expect(f.report().archive.status).toBe('missing');
    expect(f.report().errors.join('\n')).toContain('not being preserved');
  });

  test('a file that is not an archive receipt is not ready', () => {
    const f = corpusFixture();
    const other = join(f.root, 'not-a-receipt.jsonl');
    writeFileSync(other, 'not a receipt\n');
    const result = f.run({}, ['--archive-receipt', other]);
    expect(result.status).not.toBe(0);
    expect(f.report().archive.status).toBe('unreadable');
    expect(f.report().errors.join('\n')).toContain('is not an archive receipt');
  });

  test('a stale archive receipt is not ready', () => {
    // The heartbeat case: the archive job stopped running, so the receipt ages
    // even though every file it names is still intact on the archive host.
    const f = corpusFixture();
    f.writeReceipt({ verifiedAt: FIXTURE_NOW - 200000 });
    const result = f.run();
    expect(result.status).not.toBe(0);
    expect(f.report().archive.status).toBe('stale');
    expect(f.report().errors.join('\n')).toContain('has not succeeded');
  });

  test('a receipt whose recorded hash disagrees with the box is not ready', () => {
    const f = corpusFixture();
    const lines = readFileSync(f.receipt, 'utf8').trimEnd().split('\n');
    lines[1] = lines[1]!.replace(/"sha256":"[0-9a-f]*"/, `"sha256":"${'0'.repeat(64)}"`);
    writeFileSync(f.receipt, lines.join('\n') + '\n');
    const result = f.run();
    expect(result.status).not.toBe(0);
    expect(f.report().archive.hash_mismatch).toEqual([f.names[0]!]);
    expect(f.report().errors.join('\n')).toContain('does not match the box');
  });

  test('a receipt missing an optional field is reported, not an abort with no report', () => {
    // The field is provenance context, not a gate - but absence used to abort
    // the run under `set -o pipefail` before any report existed, which is the
    // one outcome a fail-closed check must never produce.
    const f = corpusFixture();
    writeFileSync(f.receipt, readFileSync(f.receipt, 'utf8').replace('"deployed_revision_at_archive":"fixture-rev",', ''));
    const result = f.run();
    expect(existsSync(join(f.root, 'report.json')), 'the report must still be written').toBe(true);
    expect(result.status, result.stderr).toBe(0);
    expect(f.report().archive.deployed_revision_at_archive).toBeNull();
    expect(f.report().archive.producer_revision_unknown).toBe(3);
  });

  test('an unreadable receipt still produces a report and fails', () => {
    // An existing receipt that cannot be read (permissions, I/O) is a verdict
    // about the archive, not a reason to die with no evidence.
    const f = corpusFixture();
    chmodSync(f.receipt, 0o000);
    const result = f.run();
    expect(existsSync(join(f.root, 'report.json')), 'the report must still be written').toBe(true);
    expect(result.status, result.stderr).not.toBe(0);
    expect(f.report().archive.status).toBe('unreadable');
    expect(f.report().errors.join('\n')).toContain('cannot read the archive receipt');
  });

  test('a metrics line with no elapsed_ms is a failure, not a missing report', () => {
    const f = corpusFixture();
    f.writeJournal([
      { at: f.newestStart, message: 'Starting wfm-scrape.service - x' },
      { at: f.newestStart, message: 'scraper: x', invocation: FIXTURE_INVOCATION },
      { at: f.newestStart + 4200, message: sweepMetrics().replace('elapsed_ms=1000 ', ''), invocation: FIXTURE_INVOCATION },
    ]);
    const result = f.run();
    expect(existsSync(join(f.root, 'report.json')), 'the report must still be written').toBe(true);
    expect(result.status, result.stderr).not.toBe(0);
    expect(f.report().service.metrics_elapsed_ms).toBeNull();
    expect(f.report().errors.join('\n')).toContain('no elapsed_ms');
  });

  test('a metrics line with no throttles count is a failure, not a missing report', () => {
    const f = corpusFixture();
    f.writeJournal([
      { at: f.newestStart, message: 'Starting wfm-scrape.service - x' },
      { at: f.newestStart, message: 'scraper: x', invocation: FIXTURE_INVOCATION },
      { at: f.newestStart + 4200, message: sweepMetrics().replace('throttles=0 ', ''), invocation: FIXTURE_INVOCATION },
    ]);
    const result = f.run();
    expect(existsSync(join(f.root, 'report.json')), 'the report must still be written').toBe(true);
    expect(result.status, result.stderr).not.toBe(0);
    expect(f.report().service.throttles).toBeNull();
    expect(f.report().errors.join('\n')).toContain('no throttles count');
  });

  test('a receipt dated materially in the future is not ready', () => {
    // Modest clock skew is allowed; a claim dated hours ahead is not freshness.
    const f = corpusFixture();
    f.writeReceipt({ verifiedAt: FIXTURE_NOW + 7200 });
    const result = f.run();
    expect(result.status).not.toBe(0);
    expect(f.report().archive.status).toBe('future');
    expect(f.report().errors.join('\n')).toContain('in the future');
  });

  test('a missing corpus is not ready', () => {
    const f = corpusFixture();
    rmSync(join(f.box, 'observations'), { recursive: true });
    const result = f.run();
    expect(result.status).not.toBe(0);
    expect(f.report().errors.join('\n')).toContain('does not exist');
  });

  test('a timed-out sweep unit is not ready', () => {
    const f = corpusFixture();
    const result = f.run({ FIXTURE_SERVICE_RESULT: 'timeout' });
    expect(result.status).not.toBe(0);
    expect(f.report().service.result).toBe('timeout');
    expect(f.report().errors.join('\n')).toContain('timed out');
  });

  test('an enabled retired pull timer is not ready', () => {
    const f = corpusFixture();
    const result = f.run({ FIXTURE_RETIRED: 'enabled' });
    expect(result.status).not.toBe(0);
    expect(f.report().timers.retired_pull).toBe('enabled');
    expect(f.report().errors.join('\n')).toContain('retired wfm-scrape-pull.timer');
  });

  test('a binary that is not the recorded revision is not ready', () => {
    const f = corpusFixture();
    writeFileSync(join(f.box, 'bin/wfm-scrape'), '#!/bin/sh\necho tampered\n');
    const result = f.run();
    expect(result.status).not.toBe(0);
    const report = f.report();
    expect(report.deployment.recorded_sha256).not.toBe(report.deployment.installed_sha256);
    expect(report.errors.join('\n')).toContain('not the recorded revision');
  });

  test('a sweep approaching the unit timeout is urgent, and one merely slow warns', () => {
    const f = corpusFixture();
    // elapsed_ms is deliberately absurd: the thresholds must gate the wall clock
    // of the invocation, because that field is a sum of per-request latencies.
    const journal = (minutes: number) => journalText([
      { at: f.newestStart, message: 'Starting wfm-scrape.service - Refresh Warframe market.json from warframe.market + warframestat...' },
      { at: f.newestStart, message: 'scraper: /srv/wfm/bin/wfm-scrape scrape', invocation: FIXTURE_INVOCATION },
      { at: f.newestStart + minutes * 60, message: sweepMetrics(), invocation: FIXTURE_INVOCATION },
    ]);
    const urgentJournal = join(f.root, 'urgent-journal');
    writeFileSync(urgentJournal, journal(106));
    const urgent = f.run({ FIXTURE_JOURNAL: urgentJournal });
    expect(urgent.status).not.toBe(0);
    expect(f.report().service.wall_seconds).toBe(106 * 60);
    expect(f.report().errors.join('\n')).toContain('close to the unit');

    const slowJournal = join(f.root, 'slow-journal');
    writeFileSync(slowJournal, journal(95));
    const slow = f.run({ FIXTURE_JOURNAL: slowJournal });
    expect(slow.status, slow.stderr).toBe(0);
    expect(f.report().ready).toBe(true);
    expect(f.report().warnings.join('\n')).toContain('took 95 minutes');
  });

  test('a throttled sweep is flagged', () => {
    const f = corpusFixture();
    const journal = join(f.root, 'throttled-journal');
    writeFileSync(journal, journalText([
      { at: f.newestStart, message: 'Starting wfm-scrape.service - Refresh Warframe market.json from warframe.market + warframestat...' },
      { at: f.newestStart, message: 'scraper: x', invocation: FIXTURE_INVOCATION },
      { at: f.newestStart + 4200, message: sweepMetrics(4, 2, 100), invocation: FIXTURE_INVOCATION },
    ]));
    const result = f.run({ FIXTURE_JOURNAL: journal });
    expect(result.status).not.toBe(0);
    expect(f.report().service.throttles).toBe(100);
    expect(f.report().errors.join('\n')).toContain('throttle');
  });

  test('a sweep that recorded no statistics requests is not ready', () => {
    // Zero is a missing measurement, not agreement: the drift check cannot pass
    // what it never saw.
    const f = corpusFixture();
    f.writeJournal([
      { at: f.newestStart, message: 'Starting wfm-scrape.service - x' },
      { at: f.newestStart, message: 'scraper: x', invocation: FIXTURE_INVOCATION },
      { at: f.newestStart + 4200, message: sweepMetrics(0, 2), invocation: FIXTURE_INVOCATION },
    ]);
    const result = f.run();
    expect(result.status).not.toBe(0);
    expect(f.report().errors.join('\n')).toContain('no statistics requests against a 4-item catalog');
  });

  test('a metrics line with no statistics count at all is not ready', () => {
    const f = corpusFixture();
    const noStatistics = sweepMetrics().replace('statistics=4 orders=2 riven=0 other=0] ok[catalog=1 statistics=4 orders=2',
      'orders=2 riven=0 other=0] ok[catalog=1 orders=2');
    f.writeJournal([
      { at: f.newestStart, message: 'Starting wfm-scrape.service - x' },
      { at: f.newestStart, message: 'scraper: x', invocation: FIXTURE_INVOCATION },
      { at: f.newestStart + 4200, message: noStatistics, invocation: FIXTURE_INVOCATION },
    ]);
    const result = f.run();
    expect(result.status).not.toBe(0);
    expect(f.report().service.statistics_attempts).toBeNull();
    expect(f.report().errors.join('\n')).toContain('no statistics requests');
  });

  test('orders requests are checked against the sweep that ran them', () => {
    const f = corpusFixture();
    f.writeJournal([
      { at: f.newestStart, message: 'Starting wfm-scrape.service - x' },
      { at: f.newestStart, message: 'scraper: x', invocation: FIXTURE_INVOCATION },
      { at: f.newestStart + 4200, message: sweepMetrics(4, 9), invocation: FIXTURE_INVOCATION },
    ]);
    const result = f.run();
    expect(result.status).not.toBe(0);
    expect(f.report().service.orders_attempts).toBe(9);
    expect(f.report().errors.join('\n')).toContain('9 orders requests against 2 kept rows');
  });

  test('an invocation whose start cannot be established is not ready', () => {
    // Without the invocation's own start line there is no wall clock to gate on,
    // and a silently skipped gate is how a hung sweep passes.
    const f = corpusFixture();
    f.writeJournal([
      { at: f.newestStart - 9000, message: 'wfm-scrape.service: Deactivated successfully.' },
      { at: f.newestStart, message: 'scraper: x', invocation: FIXTURE_INVOCATION },
      { at: f.newestStart + 4200, message: sweepMetrics(), invocation: FIXTURE_INVOCATION },
    ]);
    const result = f.run();
    expect(result.status).not.toBe(0);
    expect(f.report().service.wall_seconds).toBeNull();
    expect(f.report().errors.join('\n')).toContain('no start timestamp');
  });

  test('an invocation interleaved with another sweep is not ready', () => {
    const f = corpusFixture();
    f.writeJournal([
      { at: f.newestStart, message: 'Starting wfm-scrape.service - x' },
      { at: f.newestStart, message: 'scraper: x', invocation: FIXTURE_INVOCATION },
      { at: f.newestStart + 10, message: 'scraper: other', invocation: 'b'.repeat(32) },
      { at: f.newestStart + 4200, message: sweepMetrics(), invocation: FIXTURE_INVOCATION },
    ]);
    const result = f.run();
    expect(result.status).not.toBe(0);
    expect(f.report().service.wall_seconds).toBeNull();
    expect(f.report().errors.join('\n')).toContain('interleaves invocation');
  });

  test('an invocation that pairs with no observation log is not ready', () => {
    const f = corpusFixture();
    f.writeJournal([
      { at: f.newestStart - 3600, message: 'Starting wfm-scrape.service - x' },
      { at: f.newestStart - 3600, message: 'scraper: x', invocation: FIXTURE_INVOCATION },
      { at: f.newestStart - 3500, message: sweepMetrics(), invocation: FIXTURE_INVOCATION },
    ]);
    const result = f.run();
    expect(result.status).not.toBe(0);
    expect(f.report().service.sweep_log).toBeNull();
    expect(f.report().errors.join('\n')).toContain('does not pair with any observation log');
  });

  test('external-backup is ready with no receipt and says preservation is not verified here', () => {
    // The corpus is protected by host-level backups of the whole container,
    // which this check cannot see. Requiring a receipt it knows nothing about
    // would report a preserved corpus as broken; claiming the backup is verified
    // would report an unverified one as safe. The report has to say which.
    const f = corpusFixture();
    const missing = join(f.root, 'no-receipt.jsonl');
    const result = f.run(
      { OBSERVATIONS_PRESERVATION: 'external-backup' },
      ['--archive-receipt', missing],
    );
    expect(result.status, result.stderr).toBe(0);
    const report = f.report();
    expect(report.ready).toBe(true);
    expect(report.preservation.mode).toBe('external-backup');
    expect(report.preservation.status).toBe('declared-external');
    expect(report.preservation.independently_verified_from_this_host).toBe(false);
    expect(report.archive.status).toBe('not-applicable');
    expect(report.errors).toEqual([]);
    expect(report.errors.join('\n')).not.toMatch(/preserv/i);
  });

  test('the --preservation flag overrides the mode the unit declared', () => {
    const f = corpusFixture();
    const result = f.run({}, ['--preservation', 'external-backup', '--archive-receipt', join(f.root, 'gone.jsonl')]);
    expect(result.status, result.stderr).toBe(0);
    expect(f.report().preservation.mode).toBe('external-backup');
  });

  test('on-box-archive still requires a fresh receipt', () => {
    const f = corpusFixture();
    const result = f.run({}, ['--archive-receipt', join(f.root, 'does-not-exist.jsonl')]);
    expect(result.status).not.toBe(0);
    expect(f.report().preservation.mode).toBe('on-box-archive');
    expect(f.report().archive.status).toBe('missing');
    expect(f.report().errors.join('\n')).toContain('not being preserved');
  });

  test('an undeclared preservation mode is not ready', () => {
    // No mode is not the same as the friendly mode. The report has to say the
    // declaration is missing instead of leaving preservation unmentioned.
    const f = corpusFixture();
    const result = f.run({ OBSERVATIONS_PRESERVATION: '' });
    expect(result.status, result.stderr).not.toBe(0);
    const report = f.report();
    expect(report.ready).toBe(false);
    expect(report.preservation.mode).toBeNull();
    expect(report.preservation.status).toBe('undeclared');
    expect(report.errors.join('\n')).toContain('preservation mode not declared');
  });

  test('an unrecognised preservation mode is not ready', () => {
    const f = corpusFixture();
    const result = f.run({ OBSERVATIONS_PRESERVATION: 'tape-drive' });
    expect(result.status, result.stderr).not.toBe(0);
    expect(f.report().preservation.status).toBe('undeclared');
    expect(f.report().errors.join('\n')).toContain("unrecognised preservation mode 'tape-drive'");
  });
});

describe.skipIf(process.platform === 'win32')('observation archive', () => {
  function archiveFixture(options: { corrupt?: boolean; complete?: number } = {}) {
    const root = mkdtempSync(join(tmpdir(), 'obs-archive-')); directories.push(root);
    const remote = join(root, 'remote'), bin = join(root, 'bin'), dest = join(root, 'dest');
    for (const path of [remote, bin, dest]) mkdirSync(path);
    // `ssh` runs the remote command locally; SOURCE_DIR and DEPLOYED point into
    // the fixture, so the script's own listing and hashing run unmodified. The
    // connection bounds the script now passes are skipped like ssh would.
    write(join(bin, 'fake-ssh'), [
      '#!/bin/sh',
      'while [ $# -gt 0 ]; do',
      '  case "$1" in',
      '    -o) shift 2;;',
      '    -*) shift;;',
      '    *) break;;',
      '  esac',
      'done',
      'host="$1"; shift',
      'exec sh -c "$*"',
      '',
    ].join('\n'));
    write(join(bin, 'fake-scp'), [
      '#!/bin/sh',
      'while [ $# -gt 0 ]; do',
      '  case "$1" in',
      '    -o) shift 2;;',
      '    -*) shift;;',
      '    *) break;;',
      '  esac',
      'done',
      'src="$1"; dest="$2"',
      options.corrupt ? 'printf "tampered bytes" > "$dest"' : 'cp "${src#*:}" "$dest"',
      '',
    ].join('\n'));
    chmodSync(join(bin, 'fake-ssh'), 0o755);
    chmodSync(join(bin, 'fake-scp'), 0o755);
    const body = '{"kind":"run","format":1}\n';
    for (let i = 0; i < (options.complete ?? 1); i++) {
      const stamp = new Date((1789560162 + i * 60) * 1000).toISOString().replace('.000Z', 'Z');
      writeFileSync(join(remote, `sweep-${stamp.replaceAll(':', '-')}.jsonl`), body.repeat(20));
    }
    writeFileSync(join(remote, 'sweep-2026-09-16T14-05-32Z.jsonl.partial'), 'half a sweep\n');
    writeFileSync(join(remote, 'deployed.json'), '{\n  "revision": "fixture-rev",\n  "sha256": "deadbeef"\n}\n');
    const run = (env: Record<string, string> = {}) => spawnSync('bash', [archiveScript], {
      encoding: 'utf8',
      env: {
        ...process.env, SSH: join(bin, 'fake-ssh'), SCP: join(bin, 'fake-scp'),
        SOURCE_DIR: remote, DEPLOYED: join(remote, 'deployed.json'), DEST: dest, ...env,
      },
    });
    const manifestPath = join(dest, 'manifest.jsonl');
    const receiptPath = join(dest, 'receipt.jsonl');
    return { root, remote, dest, run, manifestPath, receiptPath };
  }

  test('verifies each file after transfer and skips what is already archived', () => {
    const f = archiveFixture();
    const first = f.run();
    expect(first.stderr, first.stderr).toBe('');
    expect(first.status, first.stderr).toBe(0);
    const manifest = readFileSync(f.manifestPath, 'utf8').trimEnd().split('\n');
    expect(manifest).toHaveLength(1);
    const entry = JSON.parse(manifest[0]!);
    const source = readFileSync(join(f.remote, 'sweep-2026-09-16T12-02-42Z.jsonl'));
    expect(entry.name).toBe('sweep-2026-09-16T12-02-42Z.jsonl');
    expect(entry.bytes).toBe(source.length);
    expect(entry.sha256).toBe(createHash('sha256').update(source).digest('hex'));
    // The deployment observed at archive time is recorded as exactly that; the
    // producing revision is unknown, because nothing in the log header carries
    // one, and the field must not imply otherwise.
    expect(entry.deployed_revision_at_archive).toBe('fixture-rev');
    expect(entry.producer_revision).toBeNull();
    expect(entry.archived_at).toMatch(/^\d{4}-\d{2}-\d{2}T/);
    const stored = readFileSync(join(f.dest, `${entry.name}.gz`));
    expect(gunzipSync(stored).equals(source)).toBe(true);
    expect(entry.gz_sha256).toBe(createHash('sha256').update(stored).digest('hex'));
    expect(readdirSync(f.dest).filter(name => name.includes('partial'))).toEqual([]);

    // The receipt is the box-readable heartbeat: a header naming the source and
    // the verification time, then one row per covered file.
    const receipt = readFileSync(f.receiptPath, 'utf8').trimEnd().split('\n');
    const header = JSON.parse(receipt[0]!);
    expect(header.kind).toBe('archive_receipt');
    expect(header.source_host).toBe('wfm');
    expect(header.verified_at).toMatch(/^\d{4}-\d{2}-\d{2}T/);
    expect(header.files).toBe(1);
    expect(JSON.parse(receipt[1]!).name).toBe('sweep-2026-09-16T12-02-42Z.jsonl');

    const second = f.run();
    expect(second.status, second.stderr).toBe(0);
    expect(readFileSync(f.manifestPath, 'utf8')).toBe(manifest.join('\n') + '\n');
    expect(second.stdout).toContain('0 new, 0 repaired, 1 intact');
  });

  test('a deleted stored artifact is repaired, not reported as already archived', () => {
    // The manifest row alone used to be the skip test, so deleting the .gz left
    // every later run reporting success over an archive that no longer existed.
    const f = archiveFixture();
    expect(f.run().status).toBe(0);
    const stored = join(f.dest, 'sweep-2026-09-16T12-02-42Z.jsonl.gz');
    rmSync(stored);
    const result = f.run();
    expect(result.status, result.stderr).toBe(0);
    expect(result.stdout).toContain('repairing sweep-2026-09-16T12-02-42Z.jsonl');
    expect(result.stdout).toContain('0 new, 1 repaired, 0 intact');
    expect(existsSync(stored)).toBe(true);
    expect(gunzipSync(readFileSync(stored)).equals(
      readFileSync(join(f.remote, 'sweep-2026-09-16T12-02-42Z.jsonl')),
    )).toBe(true);
    // One row per file, rewritten to describe what is now stored.
    const rows = readFileSync(f.manifestPath, 'utf8').trimEnd().split('\n');
    expect(rows).toHaveLength(1);
    expect(JSON.parse(rows[0]!).gz_sha256).toBe(createHash('sha256').update(readFileSync(stored)).digest('hex'));
  });

  test('a corrupt stored artifact is repaired from the box', () => {
    const f = archiveFixture();
    expect(f.run().status).toBe(0);
    const stored = join(f.dest, 'sweep-2026-09-16T12-02-42Z.jsonl.gz');
    writeFileSync(stored, 'garbage that is not a gzip stream');
    const result = f.run();
    expect(result.status, result.stderr).toBe(0);
    expect(result.stdout).toContain('repairing sweep-2026-09-16T12-02-42Z.jsonl');
    expect(gunzipSync(readFileSync(stored)).equals(
      readFileSync(join(f.remote, 'sweep-2026-09-16T12-02-42Z.jsonl')),
    )).toBe(true);
    expect(readFileSync(f.manifestPath, 'utf8').trimEnd().split('\n')).toHaveLength(1);
  });

  test('a repair that cannot reach the box fails hard and does not refresh the receipt', () => {
    const f = archiveFixture();
    expect(f.run().status).toBe(0);
    const receiptBefore = readFileSync(f.receiptPath, 'utf8');
    rmSync(join(f.dest, 'sweep-2026-09-16T12-02-42Z.jsonl.gz'));
    const result = f.run({ SCP: join(f.root, 'no-such-scp') });
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain('failed verification');
    // A receipt refreshed over an incomplete archive would be the silent
    // success this script exists to refuse.
    expect(readFileSync(f.receiptPath, 'utf8')).toBe(receiptBefore);
  });

  test('a claim whose source was pruned is not renewed when its artifact is gone', () => {
    // The verification loop only visits what the box still lists. Once the box
    // prunes a log, a fresh receipt would otherwise renew a claim nobody ever
    // re-checks - and the file can no longer be repaired from the source.
    const f = archiveFixture();
    expect(f.run().status).toBe(0);
    const receiptBefore = readFileSync(f.receiptPath, 'utf8');
    rmSync(join(f.remote, 'sweep-2026-09-16T12-02-42Z.jsonl'));
    rmSync(join(f.dest, 'sweep-2026-09-16T12-02-42Z.jsonl.gz'));
    // A later sweep keeps the listing non-empty, as it would be on the box.
    writeFileSync(join(f.remote, 'sweep-2026-09-16T16-07-14Z.jsonl'), '{"kind":"run","format":1}\n'.repeat(10));
    const result = f.run();
    expect(result.status, 'a claim that cannot be verified must stop the run').not.toBe(0);
    expect(result.stdout).toContain('still claimed but its stored artifact is gone');
    expect(result.stderr).toContain('can no longer be verified');
    expect(readFileSync(f.receiptPath, 'utf8')).toBe(receiptBefore);
  });

  test('a pruned source whose artifact is intact leaves the claim verified', () => {
    const f = archiveFixture();
    expect(f.run().status).toBe(0);
    const receiptBefore = readFileSync(f.receiptPath, 'utf8');
    rmSync(join(f.remote, 'sweep-2026-09-16T12-02-42Z.jsonl'));
    writeFileSync(join(f.remote, 'sweep-2026-09-16T16-07-14Z.jsonl'), '{"kind":"run","format":1}\n'.repeat(10));
    const result = f.run();
    expect(result.status, result.stderr).toBe(0);
    const rows = readFileSync(f.manifestPath, 'utf8').trimEnd().split('\n');
    expect(rows).toHaveLength(2);
    expect(readFileSync(f.receiptPath, 'utf8')).not.toBe(receiptBefore);
  });

  test('exactly 256 unverifiable claims still block publication', () => {
    // A shell status is taken modulo 256, so returning the count made exactly
    // 256 failures indistinguishable from success - and published a receipt
    // over 256 missing artifacts.
    const f = archiveFixture({ complete: 256 });
    const first = f.run();
    expect(first.status, first.stderr).toBe(0);
    const receiptBefore = readFileSync(f.receiptPath, 'utf8');
    const archived = readdirSync(f.remote).filter(name => name.endsWith('.jsonl'));
    expect(archived).toHaveLength(256);
    for (const name of archived) {
      rmSync(join(f.remote, name));
      rmSync(join(f.dest, `${name}.gz`));
    }
    // A later sweep keeps the listing non-empty, as it would be on the box.
    writeFileSync(join(f.remote, 'sweep-2026-09-17T00-00-00Z.jsonl'), '{"kind":"run","format":1}\n'.repeat(10));
    const result = f.run();
    expect(result.status, '256 failures must not read as success').not.toBe(0);
    expect((result.stdout.match(/still claimed but its stored artifact is gone/g) ?? [])).toHaveLength(256);
    expect(result.stderr).toContain('can no longer be verified');
    expect(readFileSync(f.receiptPath, 'utf8')).toBe(receiptBefore);
    // Hashing 256 artifacts twice is real work for a test process.
  }, 60_000);

  test('a transfer that does not match the box fails non-zero and stores nothing', () => {
    const f = archiveFixture({ corrupt: true });
    const result = f.run();
    expect(result.status, 'a bad archive must not pass silently').not.toBe(0);
    expect(result.stderr).toContain('failed verification');
    expect(existsSync(f.manifestPath)).toBe(false);
    expect(existsSync(f.receiptPath)).toBe(false);
    expect(readdirSync(f.dest).filter(name => name.endsWith('.gz'))).toEqual([]);
    expect(readdirSync(f.dest).filter(name => !name.startsWith('.'))).toEqual([]);
  });
});

// The archive host is a different machine with a different layout, so its
// installer is exercised against a temp prefix and stubbed systemd. It must
// install the alert template and handler too - `OnFailure=` alone provisions
// nothing - and it must prove the handler runs rather than assume it.
describe.skipIf(process.platform === 'win32')('archive host installation', () => {
  const installScript = fileURLToPath(new URL('./install-archive-host.sh', import.meta.url));
  const repoRoot = fileURLToPath(new URL('..', import.meta.url));

  function installFixture() {
    const root = mkdtempSync(join(tmpdir(), 'archive-host-')); directories.push(root);
    const prefix = join(root, 'bin'), units = join(root, 'units'), stub = join(root, 'stub');
    for (const path of [prefix, units, stub]) mkdirSync(path);
    write(join(stub, 'id'), '#!/bin/sh\nprintf "0\\n"\n');
    write(join(stub, 'systemctl'), [
      '#!/bin/sh',
      'printf "%s\\n" "$*" >> "$FIXTURE_SYSTEMCTL"',
      'if [ "$1" = "is-enabled" ]; then printf "enabled\\n"; fi',
      'exit 0',
      '',
    ].join('\n'));
    write(join(stub, 'systemd-analyze'), '#!/bin/sh\nprintf "%s\\n" "$*" >> "$FIXTURE_ANALYZE"\nexit 0\n');
    write(join(stub, 'journalctl'), '#!/bin/sh\nexit 0\n');
    for (const command of ['id', 'systemctl', 'systemd-analyze', 'journalctl']) chmodSync(join(stub, command), 0o755);

    const envFile = join(root, 'tennoworth-archive.env');
    const alertEnv = join(root, 'wfm-alert.env');
    const alertLog = join(root, 'alerts.log');
    const run = (env: Record<string, string> = {}) => spawnSync('bash', [installScript], {
      encoding: 'utf8',
      env: {
        ...process.env, PATH: `${stub}:${process.env.PATH}`,
        PREFIX: prefix, UNIT_DIR: units, ENV_FILE: envFile, ALERT_ENV_FILE: alertEnv,
        ALERT_LOG: alertLog, REPO_ROOT: repoRoot,
        FIXTURE_SYSTEMCTL: join(root, 'systemctl.log'),
        FIXTURE_ANALYZE: join(root, 'analyze.log'),
        ...env,
      },
    });
    return { root, prefix, units, envFile, alertEnv, alertLog, run };
  }

  test('installs every piece and proves the alert path runs', () => {
    const f = installFixture();
    const result = f.run();
    expect(result.status, result.stderr).toBe(0);
    for (const file of ['archive-observations.sh', 'alert.sh']) {
      expect(existsSync(join(f.prefix, file)), `${file} must land in the prefix`).toBe(true);
    }
    for (const unit of ['wfm-alert@.service', 'archive-observations.service', 'archive-observations.timer']) {
      expect(existsSync(join(f.units, unit)), `${unit} must be installed`).toBe(true);
    }
    // The box's handler path does not exist here, so the template is rewritten.
    expect(readFileSync(join(f.units, 'wfm-alert@.service'), 'utf8')).toContain(`${f.prefix}/alert.sh`);
    expect(readFileSync(join(f.units, 'wfm-alert@.service'), 'utf8')).not.toContain('/srv/wfm/alert.sh');
    // Verification is not a claim: systemd parsed the units and the handler ran
    // and recorded its own test line.
    expect(readFileSync(join(f.root, 'analyze.log'), 'utf8')).toContain('verify');
    expect(readFileSync(join(f.alertLog), 'utf8')).toContain('archive-observations.service');
    expect(readFileSync(join(f.root, 'systemctl.log'), 'utf8')).toContain('enable --now archive-observations.timer');
  });

  test('never repoints an existing archive destination', () => {
    const f = installFixture();
    expect(f.run().status).toBe(0);
    const first = readFileSync(f.envFile, 'utf8');
    writeFileSync(f.envFile, `${first}\nHOST=already-configured\n`);
    const second = f.run();
    expect(second.status, second.stderr).toBe(0);
    expect(second.stdout).toContain('keeping existing');
    expect(readFileSync(f.envFile, 'utf8')).toContain('HOST=already-configured');
  });
});



// ---- LXC backup pull-and-verify --------------------------------------------
//
// The real tar and zstd run here. The point of the script is that it proves the
// archive it stored is listable and its corpus readable; a stub would only prove
// that it called a stub. ssh and scp run locally against a fixture dump
// directory, in the same shape as the archive tests above.

function tarZstd(staging: string, outDir: string, name: string, members: Array<{ path: string; body: string }>): string {
  const stage = join(staging, name);
  for (const member of members) write(join(stage, member.path), member.body);
  const out = join(outDir, name);
  const built = spawnSync('tar', ['--zstd', '-cf', out, '-C', stage, '.'], { encoding: 'utf8' });
  expect(built.status, built.stderr).toBe(0);
  return out;
}

const CORPUS_HEADER = '{"kind":"run","format":1,"items":3800,"workers":2}\n';

describe.skipIf(process.platform === 'win32')('lxc backup pull and verify', () => {
  function fixture() {
    const root = mkdtempSync(join(tmpdir(), 'lxc-backup-')); directories.push(root);
    const dump = join(root, 'dump'), dest = join(root, 'dest'), bin = join(root, 'bin'), staging = join(root, 'staging');
    for (const path of [dump, dest, bin, staging]) mkdirSync(path);
    // ssh runs the remote command locally, so the script's own listing and
    // stat calls run unmodified against the fixture. scp copies locally and
    // records that it ran, which is how the dry run proves it fetched nothing.
    write(join(bin, 'fake-ssh'), [
      '#!/bin/sh',
      'while [ $# -gt 0 ]; do',
      '  case "$1" in',
      '    -o) shift 2;;',
      '    -*) shift;;',
      '    *) break;;',
      '  esac',
      'done',
      'host="$1"; shift',
      'exec sh -c "$*"',
      '',
    ].join('\n'));
    write(join(bin, 'fake-scp'), [
      '#!/bin/sh',
      'printf "%s\\n" "$*" >> "$FIXTURE_SCP_CALLS"',
      'while [ $# -gt 0 ]; do',
      '  case "$1" in',
      '    -o) shift 2;;',
      '    -*) shift;;',
      '    *) break;;',
      '  esac',
      'done',
      'src="$1"; dest="$2"',
      'cp "${src#*:}" "$dest"',
      '',
    ].join('\n'));
    chmodSync(join(bin, 'fake-ssh'), 0o755);
    chmodSync(join(bin, 'fake-scp'), 0o755);
    const scpCalls = join(root, 'scp.calls');
    writeFileSync(scpCalls, '');

    // One sweep's JSONL in the shape the writer emits, inside the container
    // path a vzdump archive stores it under.
    const sweep = (name: string, header = CORPUS_HEADER) => ({
      path: `srv/wfm/observations/${name}`,
      body: header + '{"kind":"item","slug":"x","outcome":"kept"}\n{"kind":"summary","scanned":1,"kept":1}\n',
    });
    const archive = (name: string, options: { logs?: string[]; header?: string; mtime?: number } = {}) => {
      const logs = options.logs ?? ['sweep-2026-09-17T01-04-38Z.jsonl'];
      const path = tarZstd(staging, dump, name, logs.map(log => sweep(log, options.header)));
      utimesSync(path, options.mtime ?? 1000, options.mtime ?? 1000);
      return path;
    };
    const sidecar = (name: string) => writeFileSync(join(dump, name.replace(/\.tar\.zst$/, '.log')), 'INFO: Backup finished\n');
    const manifestPath = join(dest, 'manifest.jsonl');
    const run = (args: string[] = [], env: Record<string, string> = {}) => spawnSync('bash', [backupScript, ...args], {
      encoding: 'utf8',
      env: {
        ...process.env, HOST: 'fixture-node', VMID: '110', DUMP_DIR: dump, DEST: dest,
        SSH: join(bin, 'fake-ssh'), SCP: join(bin, 'fake-scp'),
        FIXTURE_SCP_CALLS: scpCalls, ...env,
      },
    });
    return {
      root, dump, dest, staging, sidecar, archive, run, manifestPath,
      manifest: () => readFileSync(manifestPath, 'utf8'),
    };
  }

  test('a nightly archive is copied and proven restorable, and the receipt records it', () => {
    const f = fixture();
    const name = 'vzdump-lxc-110-2026_09_17-03_00_02.tar.zst';
    const logs = [
      'sweep-2026-09-15T01-04-38Z.jsonl',
      'sweep-2026-09-16T01-04-38Z.jsonl',
      'sweep-2026-09-17T01-04-38Z.jsonl',
    ];
    f.archive(name, { mtime: 1000, logs });
    f.sidecar(name);
    const result = f.run();
    expect(result.status, result.stderr).toBe(0);
    const local = join(f.dest, name);
    expect(existsSync(local), 'the archive must be copied').toBe(true);
    expect(existsSync(join(f.dest, name.replace(/\.tar\.zst$/, '.log'))), 'and its log').toBe(true);
    const rows = f.manifest().trimEnd().split('\n').map(line => JSON.parse(line));
    expect(rows).toHaveLength(1);
    expect(rows[0].name).toBe(name);
    expect(rows[0].bytes).toBe(readFileSync(local).length);
    expect(rows[0].sha256).toBe(createHash('sha256').update(readFileSync(local)).digest('hex'));
    expect(rows[0].verified_at).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/);
    expect(rows[0].corpus_logs).toBe(3);
    expect(rows[0].newest_corpus_log).toBe('srv/wfm/observations/sweep-2026-09-17T01-04-38Z.jsonl');
  });

  test('re-running with the same newest archive downloads nothing and repeats nothing', () => {
    const f = fixture();
    const name = 'vzdump-lxc-110-2026_09_17-03_00_02.tar.zst';
    f.archive(name, { mtime: 1000 });
    expect(f.run().status).toBe(0);
    const before = f.manifest();
    writeFileSync(join(f.root, 'scp.calls'), '');
    const second = f.run();
    expect(second.status, second.stderr).toBe(0);
    expect(second.stdout).toContain('already verified');
    expect(readFileSync(join(f.root, 'scp.calls'), 'utf8'), 'nothing may be re-fetched').toBe('');
    expect(f.manifest()).toBe(before);
  });

  test('--dry-run reports the archive without fetching it', () => {
    const f = fixture();
    const name = 'vzdump-lxc-110-2026_09_17-03_00_02.tar.zst';
    f.archive(name, { mtime: 1000 });
    const result = f.run(['--dry-run']);
    expect(result.status, result.stderr).toBe(0);
    expect(result.stdout).toContain(name);
    expect(readFileSync(join(f.root, 'scp.calls'), 'utf8')).toBe('');
    expect(readdirSync(f.dest)).toEqual([]);
    expect(existsSync(f.manifestPath)).toBe(false);
  });

  // A corrupt archive must fail loudly and must not touch what was already
  // proven: the .part download is verified before it can replace a good copy.
  const corruptions: Array<{ label: string; message: RegExp; make: (f: ReturnType<typeof fixture>, name: string) => void }> = [
    {
      label: 'a body that is not a zstd stream',
      message: /zstd/,
      make: (f, name) => {
        const path = join(f.dump, name);
        writeFileSync(path, 'this is not a zstd stream');
        utimesSync(path, 2000, 2000);
      },
    },
    {
      label: 'a zstd stream that is not a tar archive',
      message: /tar/,
      make: (f, name) => {
        const path = join(f.dump, name);
        spawnSync('zstd', ['-q', '-f', '-o', path], { input: Buffer.from('this is not a tar archive') });
        utimesSync(path, 2000, 2000);
      },
    },
    {
      label: 'a corpus log whose first record has a kind other than run',
      message: /kind/,
      make: (f, name) => f.archive(name, { header: '{"kind":"item","format":1,"items":3}\n', mtime: 2000 }),
    },
    {
      label: 'a run header that reports no items',
      message: /positive integer/,
      make: (f, name) => f.archive(name, { header: '{"kind":"run","format":1,"items":0}\n', mtime: 2000 }),
    },
    {
      // A prefix test accepted this: the line starts with `{"kind":"run"` and a
      // greedy grep finds `"items":3`. A truncated record is not a run header,
      // and a replay cannot read the fields out of a line that is not JSON.
      label: 'a truncated run header',
      message: /valid JSON/,
      make: (f, name) => f.archive(name, { header: '{"kind":"run","format":1,"items":3', mtime: 2000 }),
    },
    {
      label: 'a run header followed by text that is not part of the object',
      message: /valid JSON/,
      make: (f, name) => f.archive(name, { header: '{"kind":"run","format":1,"items":3} trailing\n', mtime: 2000 }),
    },
    {
      label: 'a run header for an unsupported format',
      message: /format/,
      make: (f, name) => f.archive(name, { header: '{"kind":"run","format":2,"items":3}\n', mtime: 2000 }),
    },
    {
      label: 'a run header whose items is a string rather than a number',
      message: /positive integer/,
      make: (f, name) => f.archive(name, { header: '{"kind":"run","format":1,"items":"3"}\n', mtime: 2000 }),
    },
  ];

  for (const corruption of corruptions) {
    test(`a new archive with ${corruption.label} is not recorded as good`, () => {
      const f = fixture();
      const good = 'vzdump-lxc-110-2026_09_17-03_00_02.tar.zst';
      f.archive(good, { mtime: 1000 });
      expect(f.run().status).toBe(0);
      const before = f.manifest();
      const corrupt = 'vzdump-lxc-110-2026_09_18-03_00_02.tar.zst';
      corruption.make(f, corrupt);
      const result = f.run();
      expect(result.status, 'a corrupt archive must not pass').not.toBe(0);
      expect(result.stderr).toMatch(corruption.message);
      expect(f.manifest(), 'the previous receipt must survive').toBe(before);
      expect(existsSync(join(f.dest, good)), 'the proven archive must survive').toBe(true);
      expect(existsSync(join(f.dest, corrupt)), 'the failed copy must not land').toBe(false);
    });
  }

  test('a transfer that cannot reach the node records nothing', () => {
    const f = fixture();
    f.archive('vzdump-lxc-110-2026_09_17-03_00_02.tar.zst', { mtime: 1000 });
    const result = f.run([], { SCP: join(f.root, 'no-such-scp') });
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain('could not fetch');
    expect(existsSync(f.manifestPath)).toBe(false);
    expect(readdirSync(f.dest)).toEqual([]);
  });

  test('a corrupted re-fetch of the same name leaves the proven copy and receipt in place', () => {
    // The transfer verifies before it replaces anything: a bad copy of an
    // archive already held must not overwrite the bytes that were proven.
    const f = fixture();
    const name = 'vzdump-lxc-110-2026_09_17-03_00_02.tar.zst';
    f.archive(name, { mtime: 1000 });
    expect(f.run().status).toBe(0);
    const before = f.manifest();
    const original = readFileSync(join(f.dest, name));
    writeFileSync(join(f.dump, name), 'truncated');
    utimesSync(join(f.dump, name), 3000, 3000);
    const result = f.run();
    expect(result.status).not.toBe(0);
    expect(f.manifest()).toBe(before);
    expect(readFileSync(join(f.dest, name)).equals(original), 'the proven archive must be byte-for-byte intact').toBe(true);
  });

  test('a dump directory with no matching archive is a refusal, not an empty success', () => {
    const f = fixture();
    const result = f.run();
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain('no vzdump');
    expect(existsSync(f.dest)).toBe(true);
    expect(readdirSync(f.dest)).toEqual([]);
  });

  test('rotation keeps the newest KEEP archives and drops their receipts with them', () => {
    const f = fixture();
    const names = ['2026_09_14', '2026_09_15', '2026_09_16', '2026_09_17']
      .map(stamp => `vzdump-lxc-110-${stamp}-03_00_02.tar.zst`);
    for (const [index, name] of names.entries()) {
      f.archive(name, { mtime: 1000 + index });
      f.sidecar(name);
      expect(f.run([], { KEEP: '2' }).status).toBe(0);
    }
    expect(readdirSync(f.dest).filter(entry => entry.endsWith('.tar.zst')).sort()).toEqual([names[2]!, names[3]!]);
    expect(readdirSync(f.dest).filter(entry => entry.endsWith('.log')).sort())
      .toEqual([names[2]!.replace(/\.tar\.zst$/, '.log'), names[3]!.replace(/\.tar\.zst$/, '.log')]);
    const rows = f.manifest().trimEnd().split('\n').map(line => JSON.parse(line));
    expect(rows.map(row => row.name).sort()).toEqual([names[2]!, names[3]!]);
  });

  test('a KEEP below one is refused rather than deleting what was just proven', () => {
    const f = fixture();
    f.archive('vzdump-lxc-110-2026_09_17-03_00_02.tar.zst', { mtime: 1000 });
    const result = f.run([], { KEEP: '0' });
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain('KEEP');
    expect(readdirSync(f.dest)).toEqual([]);
  });

  test('a missing HOST or VMID aborts before anything is fetched', () => {
    const f = fixture();
    f.archive('vzdump-lxc-110-2026_09_17-03_00_02.tar.zst', { mtime: 1000 });
    for (const env of [{ HOST: '' }, { VMID: '' }]) {
      const result = f.run([], env);
      expect(result.status).not.toBe(0);
      expect(readFileSync(join(f.root, 'scp.calls'), 'utf8')).toBe('');
    }
  });
});
