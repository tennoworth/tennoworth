import { afterEach, describe, expect, test } from 'bun:test';
import { execFileSync, spawnSync } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
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
    // stale - so the restore has to ride the one path both endings take.
    const trap = deploy.indexOf('trap restore_timer EXIT');
    expect(trap, 'the restore must be an EXIT trap').toBeGreaterThan(stop);
    expect(deploy).toMatch(/systemctl start wfm-scrape\.timer/);
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
    // started. A single sample can still leave one running through the install,
    // so the re-check has to wait it out.
    expect(deploy.slice(recheck), 'the re-check must drain, not sample').toMatch(/(while|until)[\s\S]*is-active wfm-scrape\.service/);
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
