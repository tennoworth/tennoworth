import { afterEach, describe, expect, test } from 'bun:test';
import { execFileSync, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, renameSync, rmSync, utimesSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const directories: string[] = [];
afterEach(() => { for (const dir of directories.splice(0)) rmSync(dir, { recursive: true, force: true }); });
const puller = fileURLToPath(new URL('../deploy/pull-app.sh', import.meta.url));
function write(path: string, body: string) { mkdirSync(join(path, '..'), { recursive: true }); writeFileSync(path, body); }
function git(cwd: string, ...args: string[]) { return execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim(); }
// An `undefined` entry means the variable is genuinely absent for the child, not
// the string "undefined" - tests that exercise an unset variable depend on that.
function withoutUndefined(env: Record<string, string | undefined>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [key, value] of Object.entries(env)) if (value !== undefined) out[key] = value;
  return out;
}

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'checkout-pull-')); directories.push(root);
  const remote = join(root, 'remote'), app = join(root, 'app'), installed = join(root, 'installed');
  for (const p of [remote, installed]) mkdirSync(p);
  git(remote, 'init', '-b', 'main'); git(remote, 'config', 'user.email', 'test@example.invalid'); git(remote, 'config', 'user.name', 'Fixture');
  write(join(remote, 'frontend/public/market.json'), '{"revision":"bootstrap"}');
  write(join(remote, 'frontend/public/definitions.json'), '{"revision":"bundled-old"}');
  write(join(remote, 'source.txt'), 'before');
  git(remote, 'add', '.'); git(remote, 'commit', '-m', 'Initial fixture');
  git(root, 'clone', '-o', 'github', remote, app);
  const initial = git(app, 'rev-parse', 'HEAD');
  write(join(remote, 'frontend/public/definitions.json'), '{"revision":"bundled-new"}');
  write(join(remote, 'source.txt'), 'after');
  git(remote, 'add', '-A'); git(remote, 'commit', '-m', 'Next');
  const run = (env: Record<string, string> = {}) => spawnSync('bash', [puller], {
    env: { ...process.env, APP: app, DEPLOY_ROOT: installed, ...env }, encoding: 'utf8',
  });
  return { root, app, remote, initial, run };
}

describe.skipIf(process.platform === 'win32')('checkout puller', () => {
  test('fast-forwards, so a tracked hotfix reaches the served checkout', () => {
    const f = fixture();
    const result = f.run();
    expect(result.status, result.stderr).toBe(0);
    expect(git(f.app, 'rev-parse', 'HEAD')).toBe(git(f.remote, 'rev-parse', 'HEAD'));
    expect(readFileSync(join(f.app, 'frontend/public/definitions.json'), 'utf8')).toContain('bundled-new');
  });
  test('leaves a checkout that cannot fast-forward exactly as it was', () => {
    const f = fixture(); write(join(f.app, 'source.txt'), 'unfinished local work');
    expect(f.run().status).not.toBe(0);
    expect(git(f.app, 'rev-parse', 'HEAD')).toBe(f.initial);
    expect(readFileSync(join(f.app, 'source.txt'), 'utf8')).toBe('unfinished local work');
  });
  test('refuses to overwrite an untracked file the update would add', () => {
    const f = fixture();
    write(join(f.remote, 'deploy/new.sh'), 'incoming'); git(f.remote, 'add', '-A'); git(f.remote, 'commit', '-m', 'Add');
    write(join(f.app, 'deploy/new.sh'), 'hand-placed');
    const result = f.run();
    expect(result.status).toBe(1); expect(result.stderr).toContain('differs from the incoming version');
    expect(git(f.app, 'rev-parse', 'HEAD')).toBe(f.initial);
    expect(readFileSync(join(f.app, 'deploy/new.sh'), 'utf8')).toBe('hand-placed');
  });
  test('installs a deployed script even when the checkout is already current', () => {
    // An older puller fast-forwards with its own install list, so a script the
    // new commit adds is left uninstalled until a run that has nothing to pull.
    const f = fixture();
    write(join(f.remote, 'deploy/pull-collector.sh'), '#!/bin/sh\necho collector\n');
    git(f.remote, 'add', '-A'); git(f.remote, 'commit', '-m', 'Add a deployed script');
    expect(f.run().status).toBe(0);
    rmSync(join(f.root, 'installed/pull-collector.sh'));
    const result = f.run();
    expect(result.status).toBe(0); expect(result.stdout).toContain('nothing to pull');
    expect(readFileSync(join(f.root, 'installed/pull-collector.sh'), 'utf8')).toContain('echo collector');
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

// The usage counter and the price-report service share one puller and one
// monitor, each unit passing the collector's name.
const collectors = [
  { name: 'usage', binary: 'tennoworth-usage', failed: 'Usage collector health check failed', route: '/api/usage/daily' },
  { name: 'reports', binary: 'tennoworth-reports', failed: 'Reports collector health check failed', route: '/api/reports/prices' },
] as const;

describe.each(collectors)('$name collector units', ({ name }) => {
  test('pass the collector name to the shared scripts', () => {
    // One template per role; the instance is the collector's name.
    for (const [unit, script] of [['pull', 'pull-collector.sh'], ['monitor', 'monitor-collector.sh']]) {
      const body = readFileSync(new URL(`../deploy/tennoworth-collector-${unit}@.service`, import.meta.url), 'utf8');
      expect(body).toContain(`ExecStart=/srv/wfm/${script} %i\n`);
    }
    const service = readFileSync(new URL('../deploy/tennoworth-collector@.service', import.meta.url), 'utf8');
    expect(service).toContain('ExecStart=/srv/wfm/bin/tennoworth-%i\n');
    expect(service).toContain('User=tennoworth-%i\n');
    expect(readFileSync(new URL('../deploy/pull-collector.sh', import.meta.url), 'utf8')).toContain(`binary="tennoworth-$name"`);
    expect(name).toMatch(/^[a-z]+$/);
  });
});

describe.each(collectors)('$name collector rollback', ({ name, binary, failed }) => {
  // `busy` is one 503 while the store lock is held by a request or the
  // maintenance tick: a healthy upgrade, which must not be rolled back.
  for (const failure of ['restart', 'health', 'busy'] as const) {
    test(failure === 'busy' ? 'keeps the new binary when the first health probe finds the store busy' : `restores the preceding binary after ${failure} failure`, () => {
      const root = mkdtempSync(join(tmpdir(), 'collector-pull-'));
      directories.push(root);
      const deploy = join(root, 'deploy');
      const mocks = join(root, 'mocks');
      mkdirSync(join(deploy, 'bin'), { recursive: true });
      mkdirSync(mocks);
      writeFileSync(join(deploy, `bin/${binary}`), 'old-binary');
      const source = readFileSync(new URL('../deploy/pull-collector.sh', import.meta.url), 'utf8');
      writeFileSync(join(root, 'pull.sh'), source.replaceAll('/srv/wfm', deploy));
      writeFileSync(join(mocks, 'curl'), `#!/bin/sh
case "$*" in
  *'/health'*) ${failure === 'busy' ? `[ -f '${root}/probed' ] && exit 0; touch '${root}/probed'; exit 22` : `exit ${failure === 'health' ? 1 : 0}`};;
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
      const result = spawnSync('sh', [join(root, 'pull.sh'), name], { env: { ...process.env, PATH: `${mocks}:${process.env.PATH}` }, encoding: 'utf8' });
      if (failure === 'busy') {
        expect(result.status, result.stderr).toBe(0);
        expect(readFileSync(join(deploy, `bin/${binary}`), 'utf8')).toBe('new-binary');
        return;
      }
      expect(result.status).toBe(1);
      expect(result.stderr).toContain(failed);
      expect(readFileSync(join(deploy, `bin/${binary}`), 'utf8')).toBe('old-binary');
    });
  }
});

describe('live data roots', () => {
  const caddy = readFileSync(fileURLToPath(new URL('../deploy/Caddyfile', import.meta.url)), 'utf8');
  const admin = readFileSync(fileURLToPath(new URL('../deploy/Caddyfile-admin', import.meta.url)), 'utf8');
  const snapshot = readFileSync(fileURLToPath(new URL('./release/snapshot.ts', import.meta.url)), 'utf8');
  const handle = (source: string, matcher: string) => source.slice(source.indexOf(`handle ${matcher} {`), source.indexOf('}', source.indexOf(`handle ${matcher} {`)));

  test('pipeline output is served from the data directory, the tracked hotfix from the checkout', () => {
    expect(caddy).toMatch(/@livedata path \/market\.json \/market-public\.json \/market-home\.json \/wfstat-catalog\.json \/history\.json\n/);
    expect(handle(caddy, '@livedata')).toContain('root * /srv/wfm/data/public');
    expect(handle(caddy, '/definitions.json')).toContain('root * /srv/wfm/app/frontend/public');
    expect(handle(admin, '/market.json')).toContain('root * /srv/wfm/data/public');
  });

  test('release preparation reads the live pair under the sweep\'s lock', () => {
    // run-scrape.sh holds a flock on the publish directory for the whole sweep.
    expect(snapshot).toContain('PUBLISH=/srv/wfm/data/public\nexec 9<"$PUBLISH"');
    expect(snapshot).toContain('cd "$PUBLISH"');
    expect(readFileSync(fileURLToPath(new URL('../deploy/run-scrape.sh', import.meta.url)), 'utf8')).toContain('exec 9<"$PUBLISH"');
  });
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
    const stop = deploy.indexOf('systemctl stop tennoworth-scrape.timer');
    expect(stop, 'the timer must be stopped, not merely observed').toBeGreaterThan(-1);
    // Restoring only after the last check leaves the box with no schedule at all
    // whenever an earlier step fails - silent until someone notices the data is
    // stale - so the restore has to ride the one path both endings take, and it
    // has to be armed no later than the stop in case that call dies in flight.
    const trap = deploy.indexOf('trap restore_timer EXIT');
    expect(trap, 'the restore must be an EXIT trap').toBeGreaterThan(-1);
    expect(trap, 'the restore must be armed before the timer is stopped').toBeLessThan(stop);
    // Pin the call, not the warning text that names the same command.
    expect(deploy, 'the trap must actually start the timer').toMatch(/systemctl start tennoworth-scrape\.timer; else systemctl start wfm-scrape\.timer/);
    // `enable --now` re-arms the timer in the middle of the very window the hold
    // exists to protect; only `enable` may run inside it.
    expect(deploy).toMatch(/systemctl enable tennoworth-scrape\.timer/);
    expect(deploy).not.toMatch(/enable --now tennoworth-scrape\.timer/);
  });

  test('installs only after the schedule is held and a running sweep is drained', () => {
    const stop = deploy.indexOf('systemctl stop tennoworth-scrape.timer');
    const recheck = deploy.indexOf('systemctl is-active tennoworth-scrape.service');
    const liveInstall = deploy.indexOf('install -m 0755 "$RELEASES/$REVISION/tennoworth-pipeline" "/srv/wfm/bin/tennoworth-pipeline"');
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
    expect(deploy, 'an unreadable state must abort').toMatch(/could not read tennoworth-scrape\.service state/);
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
    expect(deploy, 'a rejected policy must abort').toMatch(/\$VERIFIER' '\$STAGING\/published-policy\.json' '\$HOST_ROOT\/policy\/wfm-policy\.json'"[^\n]*\n[^\n]*\|\| die/);
    expect(deploy, 'the fail-open branch must be gone').not.toContain('no verifier or policy on the box yet');
    // The proof runs against the release on the box, and the live paths move
    // only after it passes, so a rejection leaves the running release alone.
    const proof = deploy.indexOf("'$VERIFIER' '$HOST_ROOT/policy/wfm-policy.json'");
    const liveInstall = deploy.indexOf('install -m 0755 "$RELEASES/$REVISION/tennoworth-pipeline" "/srv/wfm/bin/tennoworth-pipeline"');
    expect(proof, 'the release verifier must run').toBeGreaterThan(-1);
    expect(liveInstall, 'the live paths must not move before the proof').toBeGreaterThan(proof);
  });

  test('gates the artifact on the box glibc and on its checksum', () => {
    expect(deploy).toMatch(/objdump -T/);
    expect(deploy).toMatch(/GLIBC_\[0-9\]/);
    expect(deploy).toMatch(/sha256sum \"\$ARTIFACT\"/);
  });

  test('checks the sweep service before installing and keeps the release', () => {
    expect(deploy).toMatch(/systemctl is-active tennoworth-scrape\.service/);
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
    expect(deploy).toMatch(/install -m 0644 "[^"]*\/tennoworth-scrape\.service" \/etc\/systemd\/system\/tennoworth-scrape\.service/);
    expect(deploy).toMatch(/install -m 0644 "[^"]*\/tennoworth-scrape\.timer" \/etc\/systemd\/system\/tennoworth-scrape\.timer/);
  });
});

// Runs the real deploy script against a stubbed box. `ssh`/`scp` execute
// locally, `install` remaps the hard-coded /srv and /etc live paths into the
// fixture, and `systemctl` reads its sweep state from a file so a sequence of
// states can be replayed. The point is that the script's own control flow is
// what the assertions observe, not a copy of it.
//
// With `runCheck` the same harness proves the deployed host check end to end:
// sweep output lives under the remapped box, the deploy stages and installs the
// real check script and its real unit, and `systemctl start
// tennoworth-host-check.service` runs the unit's own ExecStart. HOST_ROOT
// becomes the production /srv/wfm so the unit's paths and the deploy's reads
// resolve to the same files.
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
  writeFileSync(join(live, 'srv/wfm/bin/tennoworth-pipeline'), 'old-live-binary');
  writeFileSync(join(live, 'srv/wfm/bin/wfm-scrape'), 'pre-rename-binary');
  writeFileSync(join(live, 'srv/wfm/run-scrape.sh'), 'old-live-run-scrape');
  mkdirSync(join(live, 'etc/systemd/system'), { recursive: true });
  // And it still carries the retired on-box archive receipt path and the
  // retired observation corpus check, as the production box did, plus the
  // dashboard's job units.
  for (const unit of ['wfm-archive-receipt-pull.service', 'wfm-archive-receipt-pull.timer',
    'wfm-observations-check.service', 'wfm-observations-check.timer', 'tennoworth-job-readiness.path', 'tennoworth-job@.path',
    // The sweep as it was named before the units took the project's name.
    'wfm-scrape.service', 'wfm-scrape.timer', 'tennoworth-job-scrape.path', 'tennoworth-job-host-check.path']) {
    writeFileSync(join(live, 'etc/systemd/system', unit), `# ${unit}\n`);
  }
  write(join(live, 'etc/systemd/system/wfm-observations-check.service.d/preservation.conf'), '[Unit]\nWants=\nAfter=\n');
  writeFileSync(join(live, 'etc/wfm-observations-check.env'), 'OBSERVATIONS_PRESERVATION=external-backup\n');
  writeFileSync(join(live, 'srv/wfm/pull-archive-receipt.sh'), '# retired\n');
  writeFileSync(join(live, 'srv/wfm/observations-check.sh'), '# retired\n');
  mkdirSync(join(root, 'rust'), { recursive: true });
  // The script stages these from the checkout with relative paths, as it does
  // from the repository root.
  for (const unit of ['run-scrape.sh', 'tennoworth-scrape.service', 'tennoworth-scrape.timer',
    'host-check.sh', 'tennoworth-host-check.service', 'tennoworth-host-check.timer']) {
    write(join(root, 'deploy', unit), `# ${unit}\n`);
  }
  if (options.runCheck) {
    // The real script and its unit travel to the box, so the stubbed systemd
    // runs what the deploy actually installed rather than a stand-in.
    for (const name of ['host-check.sh', 'tennoworth-host-check.service']) {
      write(join(root, 'deploy', name), readFileSync(new URL(`../deploy/${name}`, import.meta.url), 'utf8'));
    }
    write(join(box, 'policy/wfm-policy.json'), '{}');
    // The sweep output a healthy box holds, under the paths the check reads.
    write(join(box, 'data/scrape/wfm_results.csv'), 'url_name,median_90d\nkept_a,1\nkept_b,1\n');
    write(join(box, 'data/public/market.json'), '{"catalog":{}}');
  } else {
    write(join(wfm, 'policy/wfm-policy.json'), '{}');
    write(join(wfm, 'data/public/market.json'), '{}');
    mkdirSync(join(wfm, 'data/scrape'), { recursive: true });
    // The host check runs once as part of a deploy, and its verdict gates the
    // deploy; this is the report a healthy box would produce. The systemctl stub
    // writes it when the check is started, to HOST_ROOT - the fixture's /srv/wfm,
    // which the check's --out also points at in production.
    writeFileSync(join(root, 'check-report.json'), '{"ready": true, "errors": []}');
  }

  write(join(root, 'artifacts/tennoworth-pipeline'), '#!/bin/sh\nprintf "usage: tennoworth-pipeline\\n"\n');
  // One argument checks the live policy; two check the published successor
  // against it, which is how a deploy across a key rotation proves its key.
  write(join(root, 'artifacts/wfm-policy'), '#!/bin/sh\n[ $# -ge 2 ] && exit "${FIXTURE_SUCCESSOR_EXIT:-1}"\nexit "${FIXTURE_POLICY_EXIT:-0}"\n');
  write(join(bin, 'curl'), '#!/bin/sh\nwhile [ $# -gt 0 ]; do [ "$1" = -o ] && out="$2"; shift; done\nprintf "{}" > "$out"\nexit "${FIXTURE_CURL_EXIT:-0}"\n');
  chmodSync(join(root, 'artifacts/tennoworth-pipeline'), 0o755);
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
    '  cp "$FIXTURE_ROOT/artifacts/tennoworth-pipeline" "$CARGO_TARGET_DIR/release/tennoworth-pipeline"',
    '  cp "$FIXTURE_ROOT/artifacts/wfm-policy" "$CARGO_TARGET_DIR/release/wfm-policy"',
    '  chmod +x "$CARGO_TARGET_DIR/release/tennoworth-pipeline" "$CARGO_TARGET_DIR/release/wfm-policy"',
    'fi',
    'exit 0',
    '',
  ].join('\n'));
  write(join(bin, 'objdump'), '#!/bin/sh\nprintf "GLIBC_2.34\\n"\n');
  write(join(bin, 'ldd'), '#!/bin/sh\nprintf "ldd (fixture GLIBC) 2.34\\n"\n');
  write(join(bin, 'sleep'), '#!/bin/sh\nexit 0\n');
  // Remote paths are remapped here as well as in `install`, so a remote
  // `sha256sum /srv/wfm/bin/tennoworth-pipeline` reads the copy the stub wrote.
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
    '  case "$a" in */bin/tennoworth-pipeline) printf "live-install\\n" >> "$FIXTURE_ORDER";; esac',
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
    '    tennoworth-scrape.service)',
    '      if [ "${FIXTURE_QUERY_FAIL:-0}" = 1 ]; then exit 255; fi',
    '      next=$(sed -n 1p "$FIXTURE_STATES" 2>/dev/null)',
    '      if [ -n "$next" ]; then sed -i 1d "$FIXTURE_STATES"; else next="${FIXTURE_SWEEP_STATE:-inactive}"; fi',
    '      printf "is-active %s\\n" "$next" >> "$FIXTURE_ORDER"',
    '      printf "%s\\n" "$next"',
    // The pre-rename name is asked about in the same call and is long gone.
    '      [ "${2:-}" = wfm-scrape.service ] && printf "inactive\\n"',
    '      exit 0;;',
    // An absent unit is inactive, which is what the pre-rename timer is once
    // the deploy retires it.
    '    wfm-scrape.timer) [ "$quiet" = 1 ] && exit 3; printf "inactive\\n"; exit 3;;',
    '    tennoworth-scrape.timer)',
    '      state="${FIXTURE_TIMER_STATE:-inactive}"',
    '      if [ "$quiet" = 1 ]; then [ "$state" = active ] && exit 0 || exit 3; fi',
    '      printf "%s\\n" "$state"',
    '      exit 0;;',
    '  esac',
    '  exit 0',
    'fi',
  ];
  if (options.runCheck) {
    // The queries the host check makes of systemd, and the one action that
    // matters: `start tennoworth-host-check.service` runs the installed unit's
    // own ExecStart. The unit passes only the paths it names; the check's
    // remaining defaults are supplied here under the remapped root, which is the
    // same value they have in production.
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
      '    tennoworth-scrape.service:ActiveState) printf "inactive\\n";;',
      '    tennoworth-scrape.service:Result) printf "success\\n";;',
      '    tennoworth-scrape.timer:UnitFileState) printf "enabled\\n";;',
      '    tennoworth-scrape.timer:ActiveState) printf "active\\n";;',
      '    tennoworth-scrape.timer:NextElapseUSecRealtime) LC_ALL=C date -u -d "+2 hours" "+%a %Y-%m-%d %H:%M:%S UTC";;',
      '  esac',
      '  exit 0',
      'fi',
    );
  }
  systemctl.push('printf "%s\\n" "$*" >> "$FIXTURE_SYSTEMCTL"');
  if (options.runCheck) {
    systemctl.push(
      'if [ "$1" = "start" ] && [ "$2" = "tennoworth-host-check.service" ]; then',
      '  unit="$FIXTURE_LIVE/etc/systemd/system/tennoworth-host-check.service"',
      '  exec_line="$(sed -n \'s/^ExecStart=//p\' "$unit" | head -1)"',
      '  exec_line="$(printf "%s" "$exec_line" | sed "s#/srv/#$FIXTURE_LIVE/srv/#g; s#/etc/#$FIXTURE_LIVE/etc/#g")"',
      '  eval "$exec_line --csv $FIXTURE_LIVE/srv/wfm/data/scrape/wfm_results.csv --snapshot $FIXTURE_LIVE/srv/wfm/data/public/market.json --deployed $FIXTURE_LIVE/srv/wfm/deployed.json --binary $FIXTURE_LIVE/srv/wfm/bin/tennoworth-pipeline --app $FIXTURE_LIVE/srv/wfm/app --disk-path $FIXTURE_LIVE/srv/wfm --disk-floor-bytes 1"',
      '  exit $?',
      'fi',
    );
  }
  if (!options.runCheck) {
    // Stands in for the check itself: it writes the report the test prepared,
    // or none at all when the test removed it.
    systemctl.push(
      'if [ "$1" = "start" ] && [ "$2" = "tennoworth-host-check.service" ] && [ -f "$FIXTURE_ROOT/check-report.json" ]; then',
      '  mkdir -p "$FIXTURE_REPORT_DIR"',
      '  cp "$FIXTURE_ROOT/check-report.json" "$FIXTURE_REPORT_DIR/report.json"',
      'fi',
    );
  }
  systemctl.push(
    'if [ "$1" = "start" ] && [ "${FIXTURE_START_FAIL:-0}" = 1 ]; then exit 1; fi',
    'exit 0',
    '',
  );
  write(join(bin, 'systemctl'), systemctl.join('\n'));
  for (const command of ['git', 'cargo', 'objdump', 'ldd', 'sleep', 'ssh', 'scp', 'install', 'systemctl', 'curl']) chmodSync(join(bin, command), 0o755);

  const run = (env: Record<string, string | undefined> = {}) => spawnSync('bash', [fileURLToPath(new URL('./deploy-scrape-host.sh', import.meta.url))], {
    cwd: root,
    encoding: 'utf8',
    env: withoutUndefined({
      ...process.env, PATH: `${bin}:${process.env.PATH}`,
      HOST: 'fixture', HOST_ROOT: hostRoot, REVISION: revision, CARGO_TARGET_DIR: target,
      TENNOWORTH_WFM_POLICY_PUBLIC_KEY: 'fixture-key',
      FIXTURE_ROOT: root, FIXTURE_LIVE: live, FIXTURE_ORDER: order, FIXTURE_STATES: states,
      FIXTURE_SYSTEMCTL: join(root, 'systemctl.log'),
      FIXTURE_REPORT_DIR: join(wfm, 'data/host-check'),
      ...options.env, ...env,
    }),
  });
  return {
    root, wfm, live, box, order, revision, run, systemctlLog: join(root, 'systemctl.log'),
    reportPath: join(box, 'data/host-check/report.json'),
  };
}

describe.skipIf(process.platform === 'win32')('host-direct scrape deploy failure paths', () => {
  const liveBinary = (f: ReturnType<typeof scrapeDeployFixture>) => readFileSync(join(f.live, 'srv/wfm/bin/tennoworth-pipeline'), 'utf8');

  test('aborts before installing when the sweep state cannot be read', () => {
    // Wrapping the query in `|| true` turns a dead connection or a broken
    // systemctl into an empty state, and an empty state ended the wait. A query
    // that produced nothing must abort instead of installing over a sweep.
    const f = scrapeDeployFixture();
    const result = f.run({ FIXTURE_QUERY_FAIL: '1' });
    expect(result.status, result.stderr).not.toBe(0);
    expect(result.stderr).toContain('could not read tennoworth-scrape.service state');
    expect(liveBinary(f), 'the previous release must still be live').toBe('old-live-binary');
  });

  test('aborts before installing on a sweep state it cannot interpret', () => {
    const f = scrapeDeployFixture({ states: ['maintenance'] });
    const result = f.run();
    expect(result.status, result.stderr).not.toBe(0);
    expect(result.stderr).toContain('could not read tennoworth-scrape.service state');
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

  test('deploys across a key rotation only when the published successor verifies', () => {
    // The live policy was signed with the retired key, and the box cannot pull
    // the new one until this deploy installs the new verifier. Refusing here
    // would leave the rotation unable to finish.
    const rotated = scrapeDeployFixture();
    const result = rotated.run({ FIXTURE_POLICY_EXIT: '1', FIXTURE_SUCCESSOR_EXIT: '0' });
    expect(result.status, result.stderr).toBe(0);
    expect(result.stdout).toContain('predates a key rotation');
    expect(readFileSync(join(rotated.live, 'srv/wfm/bin/wfm-policy'), 'utf8'))
      .toBe(readFileSync(join(rotated.root, 'artifacts/wfm-policy'), 'utf8'));
    // A published policy this build cannot verify, or none at all, is a key
    // mismatch, not a rotation.
    for (const env of [{ FIXTURE_SUCCESSOR_EXIT: '1' }, { FIXTURE_SUCCESSOR_EXIT: '0', FIXTURE_CURL_EXIT: '22' }]) {
      const f = scrapeDeployFixture();
      const refused = f.run({ FIXTURE_POLICY_EXIT: '1', ...env });
      expect(refused.status, JSON.stringify(env)).not.toBe(0);
      expect(refused.stderr).toContain('rejects the live policy');
      expect(liveBinary(f)).toBe('old-live-binary');
    }
  });

  test('refuses a box whose live data is still in the checkout', () => {
    const f = scrapeDeployFixture();
    rmSync(join(f.wfm, 'data/public/market.json'));
    const result = f.run();
    expect(result.status, result.stderr).not.toBe(0);
    expect(result.stderr).toContain('move the live data out of the checkout first');
    expect(liveBinary(f)).toBe('old-live-binary');
    expect(existsSync(f.systemctlLog), 'the schedule must not be touched').toBe(false);
  });

  test('replaces the live paths once the release and its policy proof pass', () => {
    const f = scrapeDeployFixture();
    const result = f.run();
    expect(result.status, result.stderr).toBe(0);
    expect(liveBinary(f)).not.toBe('old-live-binary');
    // The box's policy puller verifies downloads with /srv/wfm/bin/wfm-policy;
    // it must be the verifier this release proved, or a key rotation leaves it
    // refusing every new policy.
    expect(readFileSync(join(f.live, 'srv/wfm/bin/wfm-policy'), 'utf8'))
      .toBe(readFileSync(join(f.root, 'artifacts/wfm-policy'), 'utf8'));
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
    expect(calls, 'the monitor must not be armed without a running schedule')
      .not.toContain('enable --now tennoworth-host-check.timer');
  });

  test('the deploy retires what a previous deployment left behind', () => {
    // An empty Wants= in a drop-in does not reset a dependency the base unit
    // declares, so a retired pull kept running and alerting every hour. The
    // deploy removes retired units themselves, and arms only the host check.
    const f = scrapeDeployFixture();
    expect(f.run().status).toBe(0);
    const calls = readFileSync(f.systemctlLog, 'utf8');
    expect(calls).toContain('disable --now wfm-archive-receipt-pull.timer');
    expect(calls).not.toContain('start wfm-archive-receipt-pull.service');
    expect(calls).toContain('disable --now wfm-observations-check.timer tennoworth-job-readiness.path');
    expect(calls).toContain('enable --now tennoworth-host-check.timer');
    expect(calls).toContain('enable --now tennoworth-job@scrape.path tennoworth-job@host-check.path');
    expect(calls).toContain('start tennoworth-host-check.service');
    // A box from before the unit rename: its sweep schedule is held, then removed
    // rather than left to run a second sweep beside the new one.
    expect(calls).toContain('stop tennoworth-scrape.timer wfm-scrape.timer');
    expect(calls).toContain('disable wfm-scrape.timer tennoworth-job-scrape.path tennoworth-job-host-check.path');
    expect(calls).not.toContain('start wfm-scrape.timer');
    expect(existsSync(join(f.live, 'srv/wfm/bin/wfm-scrape'))).toBe(false);
    const etc = join(f.live, 'etc/systemd/system');
    for (const retired of ['wfm-archive-receipt-pull.service', 'wfm-archive-receipt-pull.timer', 'wfm-observations-check.service',
      'wfm-observations-check.timer', 'wfm-observations-check.service.d', 'tennoworth-job-readiness.path',
      'wfm-scrape.service', 'wfm-scrape.timer', 'tennoworth-job-scrape.path', 'tennoworth-job-host-check.path']) {
      expect(existsSync(join(etc, retired)), retired).toBe(false);
    }
    expect(existsSync(join(f.live, 'etc/wfm-observations-check.env'))).toBe(false);
    expect(existsSync(join(f.live, 'srv/wfm/pull-archive-receipt.sh'))).toBe(false);
    expect(existsSync(join(f.live, 'srv/wfm/observations-check.sh'))).toBe(false);
    // The dashboard's template serves both jobs; nothing is copied per job.
    expect(existsSync(join(etc, 'tennoworth-job@.path'))).toBe(true);
  });

  test('a box without the dashboard gets no job unit', () => {
    const f = scrapeDeployFixture();
    rmSync(join(f.live, 'etc/systemd/system/tennoworth-job@.path'));
    expect(f.run().status).toBe(0);
    expect(readFileSync(f.systemctlLog, 'utf8')).not.toContain('tennoworth-job@');
  });

  test('a first check that is not healthy fails the deploy', () => {
    // The installer is where the host check is enforced, so it cannot be
    // passed by ignoring it.
    const f = scrapeDeployFixture();
    writeFileSync(join(f.root, 'check-report.json'), '{"ready": false, "errors": ["tennoworth-scrape.timer is disabled, not enabled"]}');
    const result = f.run();
    expect(result.status, result.stderr).not.toBe(0);
    expect(result.stderr).toContain('does not pass on the box');
  });

  test('a first check that produced no report at all fails the install', () => {
    const f = scrapeDeployFixture();
    rmSync(join(f.root, 'check-report.json'));
    const result = f.run();
    expect(result.status, result.stderr).not.toBe(0);
    expect(result.stderr).toContain('produced no report');
  });

  test('a healthy report left by an earlier deploy does not pass a check that wrote none', () => {
    // The check exits non-zero when not healthy, so its start failure is ignored
    // and the report is the verdict. A report from before this deploy must not
    // stand in for one this check never wrote.
    const f = scrapeDeployFixture();
    write(join(f.wfm, 'data/host-check/report.json'), '{"ready": true, "errors": []}');
    rmSync(join(f.root, 'check-report.json'));
    const result = f.run();
    expect(result.status, result.stderr).not.toBe(0);
    expect(result.stderr).toContain('produced no report');
  });
});

// The whole deployment path, not a wiring assertion: the real deploy script runs
// with the real check script and unit installed, the stubbed systemd executes
// the unit's own ExecStart, and the assertions read the report the check
// actually produced.
describe.skipIf(process.platform === 'win32')('deployed host check', () => {
  test('a deploy comes out healthy', () => {
    const f = scrapeDeployFixture({ runCheck: true });
    const result = f.run();
    expect(result.status, result.stderr).toBe(0);
    expect(readFileSync(f.systemctlLog, 'utf8')).toContain('start tennoworth-host-check.service');
    const report = JSON.parse(readFileSync(f.reportPath, 'utf8'));
    expect(report.ready, report.errors.join('\n')).toBe(true);
    expect(report.errors).toEqual([]);
    expect(report.sweep.csv_rows).toBe(2);
    expect(report.deployment.revision).toBe(f.revision);
    expect(report.deployment.recorded_sha256).toBe(report.deployment.installed_sha256);
    expect(report.preservation, 'the retired corpus sections must be gone').toBeUndefined();
  });
});

// The monitor runs on the box, which has no jq. It used to need one, and failed on
// every tick from the day it was installed; running it with jq absent from PATH
// is what keeps that from coming back.
describe.skipIf(process.platform === 'win32').each(collectors)('$name monitor', ({ name, route }) => {
  // `busy` is how many requests to each endpoint answer like a held store
  // lock (curl --fail exits 22) before the collector answers.
  function monitorFixture(updatedAt: string, busy: { health?: number; data?: number } = {}) {
    const root = mkdtempSync(join(tmpdir(), 'collector-monitor-')); directories.push(root);
    const bin = join(root, 'bin'); mkdirSync(bin);
    const calls = join(root, 'calls');
    write(join(bin, 'curl'), [
      '#!/bin/sh',
      'for a in "$@"; do last="$a"; done',
      'case "$last" in',
      `  */health) echo health >> '${calls}'; [ "$(grep -c health '${calls}')" -le ${busy.health ?? 0} ] && exit 22; printf "ok\\n";;`,
      `  *${route}) echo data >> '${calls}'; [ "$(grep -c data '${calls}')" -le ${busy.data ?? 0} ] && exit 22; printf '{"updated_at":"${updatedAt}","days":[{"day":"2026-09-25","installs":3}]}\\n';;`,
      'esac',
      '',
    ].join('\n'));
    chmodSync(join(bin, 'curl'), 0o755);
    write(join(bin, 'sleep'), '#!/bin/sh\n');
    chmodSync(join(bin, 'sleep'), 0o755);
    // Only the stub and the base tools the script needs: a jq anywhere else on
    // the developer's PATH must not make this pass.
    for (const tool of ['sed', 'date', 'grep']) {
      const real = execFileSync('sh', ['-c', `command -v ${tool}`], { encoding: 'utf8' }).trim();
      write(join(bin, tool), `#!/bin/sh\nexec ${real} "$@"\n`);
      chmodSync(join(bin, tool), 0o755);
    }
    const run = () => spawnSync('/bin/sh', [fileURLToPath(new URL('../deploy/monitor-collector.sh', import.meta.url)), name], {
      encoding: 'utf8', env: { PATH: bin },
    });
    const count = (endpoint: string) => (existsSync(calls) ? readFileSync(calls, 'utf8').split('\n').filter(line => line === endpoint).length : 0);
    return Object.assign(run, { count });
  }

  test('passes on a fresh collector without jq installed', () => {
    const result = monitorFixture(new Date(Date.now() - 60_000).toISOString())();
    expect(result.status, result.stderr).toBe(0);
  });

  test('fails on a collector that has stopped updating', () => {
    const result = monitorFixture(new Date(Date.now() - 3 * 3600_000).toISOString())();
    expect(result.status).not.toBe(0);
  });

  test('fails on a response with no updated_at', () => {
    const result = monitorFixture('')();
    expect(result.status).not.toBe(0);
  });

  // Both endpoints answer 503 while a check-in or the maintenance tick holds
  // the store lock. One busy answer is contention, not an outage to alert on.
  test('passes when the store is briefly busy on either request', () => {
    const fixture = monitorFixture(new Date(Date.now() - 60_000).toISOString(), { health: 2, data: 2 });
    const result = fixture();
    expect(result.status, result.stderr).toBe(0);
    expect(fixture.count('health')).toBe(3);
    expect(fixture.count('data')).toBe(3);
  });

  test('fails when the collector stays unavailable, after a bounded number of tries', () => {
    const fixture = monitorFixture(new Date(Date.now() - 60_000).toISOString(), { health: 99 });
    expect(fixture().status).not.toBe(0);
    expect(fixture.count('health')).toBe(3);
    expect(fixture.count('data')).toBe(0);
  });

  test('a busy store does not hide stale data', () => {
    const fixture = monitorFixture(new Date(Date.now() - 3 * 3600_000).toISOString(), { data: 1 });
    expect(fixture().status).not.toBe(0);
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

  test('the sweep writes only its data directories, never the checkout', () => {
    // The unit runs with ProtectSystem=strict, so the sweep can only write the
    // paths it is granted. A wfm-writable checkout was a route to code root
    // installs, so the checkout is root's and absent from the grant.
    const unit = readFileSync(fileURLToPath(new URL('../deploy/tennoworth-scrape.service', import.meta.url)), 'utf8');
    expect(unit).toMatch(/^ReadWritePaths=\/srv\/wfm\/data\/scrape \/srv\/wfm\/data\/public$/m);
    expect(unit).toMatch(/^WorkingDirectory=\/srv\/wfm\/data\/scrape$/m);
    expect(unit).not.toContain('/srv/wfm/app');
    expect(setup).toMatch(/^chown -R root:root "\$REPO"$/m);
    expect(setup).not.toMatch(/chown -R wfm:wfm "\$REPO"/);
    expect(setup).toMatch(/install -d -m 0750 -o wfm -g wfm \/srv\/wfm\/data\/scrape/);
    expect(setup).toMatch(/install -d -m 0755 -o wfm -g wfm \/srv\/wfm\/data\/public/);
    expect(setup).not.toContain('/srv/wfm/observations');
  });

  test('leaves the scrape pipeline for the deploy script to install', () => {
    // The pipeline has exactly one installer now. The box ran a weeks-old
    // run-scrape.sh because setup-container.sh installed a copy of its own and
    // nothing reconciled it with the checkout.
    for (const file of ['run-scrape.sh', 'pull-scrape.sh', 'tennoworth-scrape.service', 'tennoworth-scrape.timer', 'wfm-scrape-pull.service', 'wfm-scrape-pull.timer']) {
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
  const publish = join(app, 'public'); mkdirSync(publish);
  // A previous generation that clears the row-count floor, so the floor alone
  // cannot catch a scrape that exits 0 without replacing the file.
  writeFileSync(join(app, 'wfm_results.csv'), `url_name,median_90d\n${'prior,1\n'.repeat(1000)}`);
  const bin = join(app, 'scrape-stub');
  writeFileSync(
    bin,
    ['#!/bin/sh', 'case "$1" in', `  scrape) ${scrape} ;;`, `  build) printf "%s\\n" "$*" > '${app}/build-args' ;;`, '  history) exit 0 ;;', 'esac', 'exit 0', ''].join('\n'),
  );
  chmodSync(bin, 0o755);
  const run = () => spawnSync('bash', [fileURLToPath(new URL('../deploy/run-scrape.sh', import.meta.url))], {
    env: { ...process.env, DATA: app, PUBLISH: publish, SCRAPE_BIN: bin, HISTORY: '0' },
    encoding: 'utf8',
  });
  return { app, publish, run };
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
    const { app, publish, run } = runScrapeFixture(REPLACE_CSV);
    const result = run();
    expect(result.stderr, result.stderr).not.toContain('without replacing');
    expect(result.status, result.stderr).toBe(0);
    expect(readFileSync(join(app, 'wfm_results.csv'), 'utf8')).toContain('row999');
    // The build is told both directories; it never falls back to a checkout.
    expect(readFileSync(join(app, 'build-args'), 'utf8').trim()).toBe(`build --data-dir ${app} --publish-dir ${publish}`);
  });
});

// ---- host check ------------------------------------------------------------

const checkScript = fileURLToPath(new URL('../deploy/host-check.sh', import.meta.url));

describe('host check wiring', () => {
  const deploy = readFileSync(fileURLToPath(new URL('./deploy-scrape-host.sh', import.meta.url)), 'utf8');
  const setup = readFileSync(fileURLToPath(new URL('../deploy/setup-container.sh', import.meta.url)), 'utf8');
  const pullApp = readFileSync(fileURLToPath(new URL('../deploy/pull-app.sh', import.meta.url)), 'utf8');
  const unit = readFileSync(fileURLToPath(new URL('../deploy/tennoworth-host-check.service', import.meta.url)), 'utf8');
  const timer = readFileSync(fileURLToPath(new URL('../deploy/tennoworth-host-check.timer', import.meta.url)), 'utf8');
  const job = readFileSync(fileURLToPath(new URL('../deploy/tennoworth-job@.path', import.meta.url)), 'utf8');

  test('the deploy script is the only installer of the check and its units', () => {
    // The check judges the pipeline the deploy installs, so it travels with it.
    // A second installer could advance the check on its own.
    // The job path template is the dashboard's and ships with it; the deploy only
    // enables this job's instance.
    const owned = ['host-check.sh', 'tennoworth-host-check.service', 'tennoworth-host-check.timer'];
    for (const file of owned) {
      expect(deploy, `${file} must be staged`).toContain(`$STAGING/${file}`);
      expect(deploy, `${file} must land in the release`).toContain(`$RELEASES/$REVISION/${file}`);
      expect(setup, `${file} must not be installed by provisioning`).not.toContain(file);
      expect(pullApp, `${file} must not be installed by the checkout puller`).not.toContain(file);
    }
    expect(deploy).toMatch(/install -m 0755 "\$RELEASES\/\$REVISION\/host-check\.sh" "\/srv\/wfm\/host-check\.sh"/);
    // ProtectSystem=strict grants write access to exactly this path, and systemd
    // refuses to start a unit whose ReadWritePaths is missing. Recreating it as
    // root:root on every deploy hid the report from the dashboard's publisher.
    expect(deploy).toMatch(/install -d -m 0750 -o root -g "\\\$\(getent group tennoworth-publisher >\/dev\/null && echo tennoworth-publisher \|\| echo root\)" "\$HOST_ROOT\/data\/host-check"/);
    // The retired receipt pull and corpus check must never be staged or installed again.
    expect(deploy).not.toMatch(/\$STAGING\/[^\s"]*(archive-receipt|observations)/);
    expect(deploy).not.toMatch(/install [^\n]*(archive-receipt|observations-check)/);
  });

  test('the check is armed only after the record and the schedule settle', () => {
    // The check reads deployed.json and the sweep schedule. A persistent timer
    // armed before those settle can fire into the intermediate state and report
    // on a deployment that is still in progress.
    const record = deploy.indexOf("cat > '$HOST_ROOT/deployed.json'");
    const restore = deploy.search(/^restore_timer$/m);
    const monitor = deploy.indexOf('systemctl enable --now tennoworth-host-check.timer');
    expect(record, 'the deployment record must be written').toBeGreaterThan(-1);
    expect(restore, 'the schedule must be restored by the explicit call, not only the trap').toBeGreaterThan(record);
    expect(monitor, 'the check must be armed last').toBeGreaterThan(restore);
    expect(deploy.slice(0, record), 'nothing may arm the check before the record exists')
      .not.toMatch(/enable --now tennoworth-host-check\.timer/);
  });

  test('the deploy gate cannot be skipped', () => {
    expect(deploy).toMatch(/systemctl start tennoworth-host-check\.service/);
    expect(deploy).toMatch(/test -s '\$HOST_ROOT\/data\/host-check\/report\.json'/);
    expect(deploy).toMatch(/the host check does not pass on the box/);
  });

  test('the check unit fails closed and can write only its report', () => {
    expect(unit).toMatch(/^Type=oneshot$/m);
    expect(unit).toMatch(/^OnFailure=tennoworth-alert@%n\.service$/m);
    expect(unit).toMatch(/^ProtectSystem=strict$/m);
    expect(unit).toMatch(/^ReadWritePaths=\/srv\/wfm\/data\/host-check$/m);
    expect(unit).toMatch(/^ExecStart=\/srv\/wfm\/host-check\.sh --out \/srv\/wfm\/data\/host-check\/report\.json$/m);
    // The check's verdict must not depend on a remote host.
    expect(unit).toMatch(/^RestrictAddressFamilies=AF_UNIX$/m);
    // A dependency declared here cannot be undone by a drop-in's empty Wants=;
    // that is how the retired receipt pull stayed alive on the box.
    expect(unit).not.toMatch(/archive-receipt/);
    expect(timer).toMatch(/^OnCalendar=\*-\*-\* \*:30:00 UTC$/m);
    expect(timer).toMatch(/^WantedBy=timers\.target$/m);
    expect(job).toMatch(/^PathChanged=\/srv\/wfm\/jobs\/%i$/m);
    expect(job).toMatch(/^Unit=tennoworth-%i\.service$/m);
  });

  test('a change to the check itself counts as pipeline lag', () => {
    // The check is installed by the pipeline deploy, so an undeployed change to
    // it is exactly the lag it exists to report.
    const check = readFileSync(checkScript, 'utf8');
    for (const source of ['rust/wfm-scrape', 'rust/wfm-client', 'rust/market-domain', 'deploy/host-check.sh', 'deploy/tennoworth-host-check.service']) {
      expect(check).toContain(source);
    }
    expect(check, 'the retired crate must not be a lag source').not.toContain('market-math');
  });
});

// A box fixture: a healthy sweep schedule, the sweep output it implies and the
// deployment record. `systemctl` is a stub on PATH, as in the host-direct
// deploy tests - the script's own control flow is what is observed.
const FIXTURE_NOW = 1789580078;

function hostFixture() {
  const root = mkdtempSync(join(tmpdir(), 'host-check-')); directories.push(root);
  const box = join(root, 'box'), bin = join(root, 'bin');
  for (const path of [join(box, 'app/frontend/public'), join(box, 'bin'), bin]) mkdirSync(path, { recursive: true });
  writeFileSync(join(box, 'app/wfm_results.csv'), 'url_name,median_90d\nkept_a,1\nkept_b,1\n');
  const snapshot = join(box, 'app/frontend/public/market.json');
  writeFileSync(snapshot, '{"catalog":{}}');
  utimesSync(snapshot, FIXTURE_NOW - 1700, FIXTURE_NOW - 1700);
  const binary = join(box, 'bin/tennoworth-pipeline');
  writeFileSync(binary, '#!/bin/sh\necho wfm-scrape\n');
  const sha256 = createHash('sha256').update(readFileSync(binary)).digest('hex');
  writeFileSync(join(box, 'deployed.json'), `{\n  "revision": "fixture-rev",\n  "sha256": "${sha256}"\n}\n`);

  write(join(bin, 'systemctl'), [
    '#!/bin/sh',
    'if [ "$1" = "is-enabled" ]; then printf "%s\\n" "${FIXTURE_RETIRED:-disabled}"; exit 0; fi',
    'unit="$2"; prop="$4"',
    'case "$unit:$prop" in',
    '  tennoworth-scrape.service:ActiveState) printf "%s\\n" "${FIXTURE_SERVICE_STATE:-inactive}";;',
    '  tennoworth-scrape.service:Result) printf "%s\\n" "${FIXTURE_SERVICE_RESULT:-success}";;',
    '  tennoworth-scrape.timer:UnitFileState) printf "%s\\n" "${FIXTURE_TIMER_ENABLED:-enabled}";;',
    '  tennoworth-scrape.timer:ActiveState) printf "active\\n";;',
    '  tennoworth-scrape.timer:NextElapseUSecRealtime) printf "%s\\n" "${FIXTURE_NEXT_ELAPSE-Wed 2026-09-16 18:30:00 UTC}";;',
    'esac',
    'exit 0',
    '',
  ].join('\n'));
  chmodSync(join(bin, 'systemctl'), 0o755);

  const reportPath = join(root, 'report.json');
  const run = (env: Record<string, string | undefined> = {}, extraArgs: string[] = []) => spawnSync('bash', [
    checkScript,
    '--out', reportPath,
    '--csv', join(box, 'app/wfm_results.csv'),
    '--snapshot', snapshot,
    '--deployed', join(box, 'deployed.json'),
    '--binary', binary,
    '--app', join(box, 'app'),
    '--disk-path', box,
    '--disk-floor-bytes', '1',
    '--now', '2026-09-16T17:34:38Z',
    ...extraArgs,
  ], {
    encoding: 'utf8',
    env: withoutUndefined({ ...process.env, PATH: `${bin}:${process.env.PATH}`, ...env }),
  });
  const report = () => JSON.parse(readFileSync(reportPath, 'utf8'));
  return { root, box, run, report };
}

describe.skipIf(process.platform === 'win32')('host check', () => {
  test('a healthy host passes and reports what it read', () => {
    const f = hostFixture();
    const result = f.run();
    expect(result.status, result.stderr).toBe(0);
    const report = f.report();
    expect(report.ready).toBe(true);
    expect(report.sweep).toEqual({ csv_rows: 2, snapshot_age_seconds: 1700 });
    expect(report.timers.sweep_enabled).toBe('enabled');
    expect(report.disk.free_bytes).toBeGreaterThan(0);
    expect(report.deployment.revision).toBe('fixture-rev');
  });

  test('a disabled sweep timer is not healthy', () => {
    const f = hostFixture();
    expect(f.run({ FIXTURE_TIMER_ENABLED: 'disabled' }).status).not.toBe(0);
    expect(f.report().errors.join('\n')).toContain('tennoworth-scrape.timer is disabled, not enabled');
  });

  test('a timer past its boundary is not healthy', () => {
    const f = hostFixture();
    expect(f.run({ FIXTURE_NEXT_ELAPSE: 'Wed 2026-09-16 16:30:00 UTC' }).status).not.toBe(0);
    expect(f.report().errors.join('\n')).toContain('missed its');
  });

  test('missing sweep output is not healthy, and still writes a report', () => {
    const f = hostFixture();
    rmSync(join(f.box, 'app/wfm_results.csv'));
    rmSync(join(f.box, 'app/frontend/public/market.json'));
    expect(f.run().status).not.toBe(0);
    const report = f.report();
    expect(report.sweep).toEqual({ csv_rows: null, snapshot_age_seconds: null });
    expect(report.errors.join('\n')).toContain('wfm_results.csv is missing');
    expect(report.errors.join('\n')).toContain('market.json is missing');
  });

  test('free space under the floor is not healthy', () => {
    const f = hostFixture();
    expect(f.run({}, ['--disk-floor-bytes', String(Number.MAX_SAFE_INTEGER)]).status).not.toBe(0);
    expect(f.report().errors.join('\n')).toContain('under the');
  });

  test('a sweep in progress, which leaves the timer no next elapse, is not a broken timer', () => {
    // systemd schedules the next elapse when the run ends, so a check that lands
    // mid-sweep sees none; reading that as an error alerted every other hour.
    const f = hostFixture();
    const running = f.run({ FIXTURE_SERVICE_STATE: 'activating', FIXTURE_NEXT_ELAPSE: '' });
    expect(running.status, running.stderr).toBe(0);
    const stopped = f.run({ FIXTURE_SERVICE_STATE: 'inactive', FIXTURE_NEXT_ELAPSE: '' });
    expect(stopped.status).not.toBe(0);
    expect(f.report().errors.join('\n')).toContain('has no next elapse');
  });

  describe('deployed scraper against the checkout', () => {
    // The box's checkout as pull-app.sh leaves it, with the deployed revision in
    // its history and the given commits on top, each `hoursAgo` before the
    // evaluation instant.
    function withCheckout(f: ReturnType<typeof hostFixture>, after: Array<{ path: string; hoursAgo: number }>, deployed?: string) {
      const app = join(f.box, 'app');
      git(app, 'init', '-q', '-b', 'main');
      git(app, 'config', 'user.email', 'test@example.invalid'); git(app, 'config', 'user.name', 'Fixture');
      const commit = (path: string, hoursAgo: number) => {
        write(join(app, path), `${path} ${hoursAgo}\n`);
        git(app, 'add', path);
        const date = `@${FIXTURE_NOW - hoursAgo * 3600} +0000`;
        execFileSync('git', ['commit', '-q', '-m', path], { cwd: app, env: { ...process.env, GIT_AUTHOR_DATE: date, GIT_COMMITTER_DATE: date } });
      };
      commit('rust/wfm-scrape/src/lib.rs', 200);
      const revision = deployed ?? git(app, 'rev-parse', 'HEAD');
      for (const c of after) commit(c.path, c.hoursAgo);
      const record = JSON.parse(readFileSync(join(f.box, 'deployed.json'), 'utf8'));
      writeFileSync(join(f.box, 'deployed.json'), JSON.stringify({ ...record, revision }, null, 2));
    }

    test('a deploy that matches the checkout is current', () => {
      const f = hostFixture();
      withCheckout(f, [{ path: 'frontend/src/app.ts', hoursAgo: 100 }]);
      expect(f.run().status).toBe(0);
      expect(f.report().deployment).toMatchObject({ lag: 'current', pending_scraper_commits: 0 });
    });

    test('scraper changes the deploy has waited on past the grace are not healthy', () => {
      const f = hostFixture();
      withCheckout(f, [{ path: 'rust/wfm-client/src/lib.rs', hoursAgo: 30 }, { path: 'rust/wfm-scrape/src/http.rs', hoursAgo: 2 }]);
      const result = f.run();
      expect(result.status).not.toBe(0);
      expect(f.report().deployment).toMatchObject({ lag: 'behind', pending_scraper_commits: 2 });
      expect(f.report().errors.join('\n')).toContain('2 scraper commit(s) behind');
    });

    test('a scraper change inside the grace is a warning', () => {
      const f = hostFixture();
      withCheckout(f, [{ path: 'deploy/run-scrape.sh', hoursAgo: 3 }]);
      const result = f.run();
      expect(result.status, result.stderr).toBe(0);
      expect(f.report().deployment.lag).toBe('behind');
      expect(f.report().warnings.join('\n')).toContain('1 scraper commit(s) behind');
    });

    test('the lock counts only where the scraper\'s own dependencies changed', () => {
      const f = hostFixture();
      const app = join(f.box, 'app');
      // A desktop that may gain a dependency of its own, and a scraper with one.
      // desktopOnly adds xcap and a second csv the scraper does not use. With two
      // csv versions in the lock, Cargo spells the scraper's dependency with its
      // version, which changes the scraper's record text but not what it uses.
      const lock = (desktop: string, scraperDep: string, desktopOnly = false) => [
        `[[package]]\nname = "tennoworth-desktop"\nversion = "${desktop}"\ndependencies = [\n "wfm-client",${desktopOnly ? '\n "xcap",\n "csv 2.0.0",' : ''}\n]\n`,
        `[[package]]\nname = "wfm-client"\nversion = "0.1.0"\n`,
        `[[package]]\nname = "wfm-scrape"\nversion = "0.1.0"\ndependencies = [\n "${desktopOnly ? `csv ${scraperDep}` : 'csv'}",\n "wfm-client",\n]\n`,
        `[[package]]\nname = "csv"\nversion = "${scraperDep}"\n`,
        ...(desktopOnly ? [`[[package]]\nname = "csv"\nversion = "2.0.0"\n`, `[[package]]\nname = "xcap"\nversion = "0.8.3"\n`] : []),
      ].join('\n');
      const commit = (content: string, hoursAgo: number) => {
        write(join(app, 'rust/Cargo.lock'), content);
        git(app, 'add', 'rust/Cargo.lock');
        const date = `@${FIXTURE_NOW - hoursAgo * 3600} +0000`;
        execFileSync('git', ['commit', '-q', '-m', `lock ${hoursAgo}`], { cwd: app, env: { ...process.env, GIT_AUTHOR_DATE: date, GIT_COMMITTER_DATE: date } });
      };
      withCheckout(f, []);
      commit(lock('0.8.4', '1.3.0'), 150);
      const record = JSON.parse(readFileSync(join(f.box, 'deployed.json'), 'utf8'));
      writeFileSync(join(f.box, 'deployed.json'), JSON.stringify({ ...record, revision: git(app, 'rev-parse', 'HEAD') }));

      // A desktop version bump, then dependencies only the desktop uses -
      // including another version of a crate the scraper also uses.
      commit(lock('0.8.5', '1.3.0'), 40);
      commit(lock('0.8.5', '1.3.0', true), 35);
      expect(f.run().status).toBe(0);
      expect(f.report().deployment).toMatchObject({ lag: 'current', pending_scraper_commits: 0 });

      // A scraper dependency moves 2 h ago: lag, but still inside the grace -
      // the older desktop-only commits must not start the clock.
      commit(lock('0.8.5', '1.3.1', true), 2);
      expect(f.run().status, f.report().errors.join('\n')).toBe(0);
      expect(f.report().deployment).toMatchObject({ lag: 'behind', pending_scraper_commits: 1 });
    });

    test('a revision deployed ahead of the checkout is not lag', () => {
      const f = hostFixture();
      withCheckout(f, [], '0123456789abcdef0123456789abcdef01234567');
      expect(f.run().status).toBe(0);
      expect(f.report().deployment.lag).toBe('ahead');
    });

    test('no checkout to compare against is a warning, never a pass', () => {
      const f = hostFixture();
      expect(f.run().status).toBe(0);
      expect(f.report().deployment.lag).toBe('unknown');
      expect(f.report().warnings.join('\n')).toContain('no git checkout');
    });
  });

  test('a timed-out sweep unit is not healthy', () => {
    const f = hostFixture();
    const result = f.run({ FIXTURE_SERVICE_RESULT: 'timeout' });
    expect(result.status).not.toBe(0);
    expect(f.report().service.result).toBe('timeout');
    expect(f.report().errors.join('\n')).toContain('timed out');
  });

  test('an enabled retired pull timer is not healthy', () => {
    const f = hostFixture();
    const result = f.run({ FIXTURE_RETIRED: 'enabled' });
    expect(result.status).not.toBe(0);
    expect(f.report().timers.retired_pull).toBe('enabled');
    expect(f.report().errors.join('\n')).toContain('retired wfm-scrape-pull.timer');
  });

  test('a binary that is not the recorded revision is not healthy', () => {
    const f = hostFixture();
    writeFileSync(join(f.box, 'bin/tennoworth-pipeline'), '#!/bin/sh\necho tampered\n');
    const result = f.run();
    expect(result.status).not.toBe(0);
    const report = f.report();
    expect(report.deployment.recorded_sha256).not.toBe(report.deployment.installed_sha256);
    expect(report.errors.join('\n')).toContain('not the recorded revision');
  });

});

describe.skipIf(process.platform === 'win32')('player-count logger', () => {
  const script = fileURLToPath(new URL('../deploy/log-players.sh', import.meta.url));

  function playersFixture(curl: string) {
    const root = mkdtempSync(join(tmpdir(), 'players-')); directories.push(root);
    const bin = join(root, 'bin'); mkdirSync(bin);
    const dir = join(root, 'players');
    write(join(bin, 'curl'), `#!/bin/sh\n${curl}\n`);
    chmodSync(join(bin, 'curl'), 0o755);
    for (const tool of ['sed', 'date', 'grep', 'mkdir']) {
      const real = execFileSync('sh', ['-c', `command -v ${tool}`], { encoding: 'utf8' }).trim();
      write(join(bin, tool), `#!/bin/sh\nexec ${real} "$@"\n`);
      chmodSync(join(bin, tool), 0o755);
    }
    const run = () => spawnSync('/bin/sh', [script], { encoding: 'utf8', env: { PATH: bin, PLAYERS_DIR: dir } });
    const rows = () => (existsSync(dir)
      ? readdirSync(dir).flatMap(name => readFileSync(join(dir, name), 'utf8').split('\n').filter(Boolean).map(line => JSON.parse(line)))
      : []);
    return { run, rows, dir };
  }

  test('appends one row per tick to the month file', () => {
    const f = playersFixture(`printf '{"response":{"player_count":70198,"result":1}}'`);
    expect(f.run().status).toBe(0);
    expect(f.run().status).toBe(0);
    const rows = f.rows();
    expect(rows).toHaveLength(2);
    expect(rows[0].player_count).toBe(70198);
    expect(rows[0].at).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/);
    expect(readdirSync(f.dir)).toEqual([`players-${new Date().toISOString().slice(0, 7)}.jsonl`]);
  });

  test('a failed request writes nothing and fails the unit', () => {
    const f = playersFixture('exit 22');
    expect(f.run().status).not.toBe(0);
    expect(f.rows()).toEqual([]);
  });

  test('a non-success result code is not a count', () => {
    const f = playersFixture(`printf '{"response":{"player_count":0,"result":42}}'`);
    expect(f.run().status).not.toBe(0);
    expect(f.rows()).toEqual([]);
  });

  test('a body without a count writes nothing', () => {
    const f = playersFixture(`printf '{"response":{"result":1}}'`);
    expect(f.run().status).not.toBe(0);
    expect(f.rows()).toEqual([]);
  });

  test('the checkout puller keeps the script current and reports unit drift', () => {
    const pullApp = readFileSync(puller, 'utf8');
    expect(pullApp).toMatch(/for f in [^;]*log-players\.sh/);
    // Drift covers every unit file but the pipeline deploy's own.
    expect(pullApp).toContain('for src in deploy/*.service deploy/*.timer deploy/*.path; do');
    expect(pullApp).toContain('case "${src#deploy/}" in tennoworth-scrape.*|tennoworth-host-check.*) continue;; esac');
    const unit = readFileSync(fileURLToPath(new URL('../deploy/tennoworth-players.service', import.meta.url)), 'utf8');
    expect(unit).toContain('ExecStart=/srv/wfm/log-players.sh');
    expect(unit).toContain('ReadWritePaths=/srv/wfm/players');
  });
});
