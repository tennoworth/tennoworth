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
function scrapeDeployFixture(options: { states?: string[]; env?: Record<string, string> } = {}) {
  const root = mkdtempSync(join(tmpdir(), 'scrape-deploy-')); directories.push(root);
  const bin = join(root, 'bin'); mkdirSync(bin);
  const wfm = join(root, 'wfm'); mkdirSync(wfm, { recursive: true });
  const live = join(root, 'live'); mkdirSync(join(live, 'srv/wfm/bin'), { recursive: true });
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
    'observations-check.sh', 'wfm-observations-check.service', 'wfm-observations-check.timer']) {
    write(join(root, 'deploy', unit), `# ${unit}\n`);
  }
  write(join(wfm, 'policy/wfm-policy.json'), '{}');

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
  write(join(bin, 'systemctl'), [
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
    'printf "%s\\n" "$*" >> "$FIXTURE_SYSTEMCTL"',
    'exit 0',
    '',
  ].join('\n'));
  for (const command of ['git', 'cargo', 'objdump', 'ldd', 'sleep', 'ssh', 'scp', 'install', 'systemctl']) chmodSync(join(bin, command), 0o755);

  const run = (env: Record<string, string> = {}) => spawnSync('bash', [fileURLToPath(new URL('./deploy-scrape-host.sh', import.meta.url))], {
    cwd: root,
    encoding: 'utf8',
    env: {
      ...process.env, PATH: `${bin}:${process.env.PATH}`,
      HOST: 'fixture', HOST_ROOT: wfm, REVISION: revision, CARGO_TARGET_DIR: target,
      TENNOWORTH_WFM_POLICY_PUBLIC_KEY: 'fixture-key',
      FIXTURE_ROOT: root, FIXTURE_LIVE: live, FIXTURE_ORDER: order, FIXTURE_STATES: states,
      FIXTURE_SYSTEMCTL: join(root, 'systemctl.log'),
      ...options.env, ...env,
    },
  });
  return { root, wfm, live, order, revision, run };
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

  test('the deploy script is the only installer of the check and its units', () => {
    // The check reads scrape-owned state and is host-only infrastructure, so it
    // travels with the pipeline that writes that state. A second installer is
    // the regression this pins: it could advance the check on its own, judging a
    // corpus by a ruleset the deployed pipeline does not have.
    for (const file of ['observations-check.sh', 'wfm-observations-check.service', 'wfm-observations-check.timer']) {
      expect(deploy, `${file} must be staged`).toContain(`$STAGING/${file}`);
      expect(deploy, `${file} must land in the release`).toContain(`$RELEASES/$REVISION/${file}`);
    }
    expect(deploy).toMatch(/install -m 0755 "\$RELEASES\/\$REVISION\/observations-check\.sh" "\/srv\/wfm\/observations-check\.sh"/);
    expect(deploy).toMatch(/install -m 0644 "\$RELEASES\/\$REVISION\/wfm-observations-check\.service" \/etc\/systemd\/system\/wfm-observations-check\.service/);
    expect(deploy).toMatch(/install -m 0644 "\$RELEASES\/\$REVISION\/wfm-observations-check\.timer" \/etc\/systemd\/system\/wfm-observations-check\.timer/);
    // ProtectSystem=strict grants write access to exactly this path, and systemd
    // refuses to start a unit whose ReadWritePaths is missing.
    expect(deploy).toMatch(/install -d -m 0750 -o root -g root "\$HOST_ROOT\/data\/observations-check"/);
    expect(deploy).toMatch(/systemctl enable --now wfm-observations-check\.timer/);
    for (const file of ['observations-check.sh', 'wfm-observations-check.service', 'wfm-observations-check.timer']) {
      expect(setup, `${file} must not be installed by provisioning`).not.toContain(file);
      expect(pullApp, `${file} must not be installed by the checkout puller`).not.toContain(file);
    }
  });

  test('the check unit fails closed and can write only its report', () => {
    expect(unit).toMatch(/^Type=oneshot$/m);
    expect(unit).toMatch(/^OnFailure=wfm-alert@%n\.service$/m);
    expect(unit).toMatch(/^ProtectSystem=strict$/m);
    expect(unit).toMatch(/^ReadWritePaths=\/srv\/wfm\/data\/observations-check$/m);
    expect(unit).toMatch(/^ExecStart=\/srv\/wfm\/observations-check\.sh --observations \/srv\/wfm\/observations --out \/srv\/wfm\/data\/observations-check\/report\.json$/m);
    expect(timer).toMatch(/^OnCalendar=\*-\*-\* 06:30:00$/m);
    expect(timer).toMatch(/^WantedBy=timers\.target$/m);
  });

  test('the log contract is pinned by a shared fixture on both sides', () => {
    // The writer stamps the format and prunes with Rust constants, the box check
    // reads the format and judges the corpus against shell defaults. The fixture
    // is the only place the two can disagree loudly; the Rust side reads it in
    // observations::tests::log_contract_matches_the_shared_fixture.
    expect(rust, 'the writer must keep its side of the fixture gate').toContain('observation-retention.json');
    expect(Number(check.match(/FORMAT_SUPPORTED=(\d+)/)![1])).toBe(retentionFixture.format);
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

  test('the archive script is incremental, verified, and writes atomically', () => {
    const archive = readFileSync(archiveScript, 'utf8');
    expect(archive).toMatch(/SSH="\$\{SSH:-ssh\}"/);
    expect(archive).toMatch(/SCP="\$\{SCP:-scp\}"/);
    expect(archive).toMatch(/DEST="\$\{DEST:-\$HOME\/\.local\/share\/tennoworth\/observations-archive\}"/);
    expect(archive).toMatch(/gzip -9 -c/);
    // Verification has to decompress the stored bytes, not trust the compress
    // step: comparing the pre-compression file proves nothing about the .gz.
    expect(archive).toMatch(/gzip -dc "\$packed_tmp" \| sha256sum/);
    expect(archive).toMatch(/mv "\$packed_tmp" "\$packed"/);
    for (const field of ['"name"', '"bytes"', '"sha256"', '"gz_sha256"', '"archived_at"', '"source_revision"']) {
      expect(archive, `the manifest must record ${field}`).toContain(field);
    }
    expect(archive).toMatch(/\[ "\$failed" = 0 \] \|\| die/);
    expect(archive, 'the box is read-only to this script').not.toMatch(/\$SSH[^\n]*\b(rm|mv|cp|install)\b/);
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

// A box fixture: three completed sweeps two hours apart, the newest an hour
// before the evaluation instant, with the snapshot, CSV and deployment record
// they imply. `journalctl` and `systemctl` are stubs on PATH, as in the
// host-direct deploy tests - the script's own control flow is what is observed.
function corpusFixture() {
  const root = mkdtempSync(join(tmpdir(), 'obs-check-')); directories.push(root);
  const box = join(root, 'box'), bin = join(root, 'bin');
  for (const path of [join(box, 'observations'), join(box, 'app/frontend/public'), join(box, 'bin'), bin]) {
    mkdirSync(path, { recursive: true });
  }
  const now = 1789580078;
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

  const short = (seconds: number) => new Date(seconds * 1000).toISOString().replace('.000Z', '+00:00');
  const journal = join(root, 'journal');
  writeFileSync(journal, [
    `${short(newestStart)} host systemd[1]: Starting wfm-scrape.service - Refresh Warframe market.json from warframe.market + warframestat...`,
    `${short(newestStart + 4200)} host run-scrape.sh[1]: sweep metrics: attempts[catalog=1 statistics=4 orders=2 riven=0 other=0] ok[catalog=1 statistics=4 orders=2 riven=0 other=0] failed[catalog=0 statistics=0 orders=0 riven=0 other=0] retries=0 wire_requests=7 throttles=0 cooldown_waits=0 cooldown_wait_ms=0 decoded_bytes=1 elapsed_ms=1000 snapshot_age_s=0`,
    `${short(newestStart + 4200)} host systemd[1]: wfm-scrape.service: Deactivated successfully.`,
  ].join('\n') + '\n');

  write(join(bin, 'systemctl'), [
    '#!/bin/sh',
    'if [ "$1" = "is-enabled" ]; then printf "%s\\n" "${FIXTURE_RETIRED:-disabled}"; exit 0; fi',
    'unit="$2"; prop="$4"',
    'case "$unit:$prop" in',
    '  wfm-scrape.service:ActiveState) printf "inactive\\n";;',
    '  wfm-scrape.service:Result) printf "success\\n";;',
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
    '--now', '2026-09-16T17:34:38Z',
    ...extraArgs,
  ], {
    encoding: 'utf8',
    env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, FIXTURE_JOURNAL: journal, ...env },
  });
  const report = () => JSON.parse(readFileSync(reportPath, 'utf8'));
  const observationPath = (name: string) => join(box, 'observations', name);
  return { root, box, names, run, report, observationPath };
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

  test('an archive that lags the box is not ready', () => {
    const f = corpusFixture();
    const manifest = join(f.root, 'manifest.jsonl');
    const entries = f.names.slice(0, 2).map(name => JSON.stringify({
      name, bytes: 1, sha256: 'aa', gz_sha256: 'bb', archived_at: '2026-09-16T17:00:00Z', source_revision: 'fixture-rev',
    }));
    writeFileSync(manifest, entries.join('\n') + '\n');
    const result = f.run({}, ['--archive-manifest', manifest]);
    expect(result.status, 'the manifest must be passed through the run').not.toBe(0);
    expect(f.report().archive.unarchived).toBe(1);
    expect(f.report().errors.join('\n')).toContain('not in the archive manifest');
  });

  test('an unreadable archive manifest is an error, not an absent check', () => {
    const f = corpusFixture();
    const result = f.run({}, ['--archive-manifest', join(f.root, 'does-not-exist.jsonl')]);
    expect(result.status).not.toBe(0);
    expect(f.report().archive.status).toBe('unreadable');
    expect(f.report().errors.join('\n')).toContain('not readable');
  });

  test('a missing corpus is not ready', () => {
    const f = corpusFixture();
    rmSync(join(f.box, 'observations'), { recursive: true });
    const result = f.run();
    expect(result.status).not.toBe(0);
    expect(f.report().errors.join('\n')).toContain('does not exist');
  });
});

describe.skipIf(process.platform === 'win32')('observation archive', () => {
  function archiveFixture(options: { corrupt?: boolean } = {}) {
    const root = mkdtempSync(join(tmpdir(), 'obs-archive-')); directories.push(root);
    const remote = join(root, 'remote'), bin = join(root, 'bin'), dest = join(root, 'dest');
    for (const path of [remote, bin, dest]) mkdirSync(path);
    // `ssh` runs the remote command locally; SOURCE_DIR and DEPLOYED point into
    // the fixture, so the script's own listing and hashing run unmodified.
    write(join(bin, 'fake-ssh'), '#!/bin/sh\nhost="$1"; shift\nexec sh -c "$*"\n');
    write(join(bin, 'fake-scp'), [
      '#!/bin/sh',
      '[ "$1" = "-q" ] && shift',
      'src="$1"; dest="$2"',
      options.corrupt ? 'printf "tampered bytes" > "$dest"' : 'cp "${src#*:}" "$dest"',
      '',
    ].join('\n'));
    chmodSync(join(bin, 'fake-ssh'), 0o755);
    chmodSync(join(bin, 'fake-scp'), 0o755);
    const body = '{"kind":"run","format":1}\n';
    writeFileSync(join(remote, 'sweep-2026-09-16T12-02-42Z.jsonl'), body.repeat(20));
    writeFileSync(join(remote, 'sweep-2026-09-16T14-05-32Z.jsonl.partial'), 'half a sweep\n');
    writeFileSync(join(remote, 'deployed.json'), '{\n  "revision": "fixture-rev",\n  "sha256": "deadbeef"\n}\n');
    const run = () => spawnSync('bash', [archiveScript], {
      encoding: 'utf8',
      env: {
        ...process.env, SSH: join(bin, 'fake-ssh'), SCP: join(bin, 'fake-scp'),
        SOURCE_DIR: remote, DEPLOYED: join(remote, 'deployed.json'), DEST: dest,
      },
    });
    const manifestPath = join(dest, 'manifest.jsonl');
    return { root, remote, dest, run, manifestPath };
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
    expect(entry.source_revision).toBe('fixture-rev');
    expect(entry.archived_at).toMatch(/^\d{4}-\d{2}-\d{2}T/);
    const stored = readFileSync(join(f.dest, `${entry.name}.gz`));
    expect(gunzipSync(stored).equals(source)).toBe(true);
    expect(entry.gz_sha256).toBe(createHash('sha256').update(stored).digest('hex'));
    expect(readdirSync(f.dest).filter(name => name.includes('partial'))).toEqual([]);

    const second = f.run();
    expect(second.status, second.stderr).toBe(0);
    expect(readFileSync(f.manifestPath, 'utf8')).toBe(manifest.join('\n') + '\n');
    expect(second.stdout).toContain('0 new, 1 already present');
  });

  test('a transfer that does not match the box fails non-zero and stores nothing', () => {
    const f = archiveFixture({ corrupt: true });
    const result = f.run();
    expect(result.status, 'a bad archive must not pass silently').not.toBe(0);
    expect(result.stderr).toContain('failed verification');
    expect(existsSync(f.manifestPath)).toBe(false);
    expect(readdirSync(f.dest).filter(name => name.endsWith('.gz'))).toEqual([]);
    expect(readdirSync(f.dest).filter(name => !name.startsWith('.'))).toEqual([]);
  });
});

