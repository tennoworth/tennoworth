// Single source of truth for the Content-Security-Policy.
//
// The hosted site and private dashboard each carry a meta tag and a server
// header. Hand-syncing previously allowed drift; edit DIRECTIVES below and
// run `bun run csp` from frontend/. CI checks every committed copy.
//
// One deliberate asymmetry: `frame-ancestors` is invalid in a <meta> CSP
// (browsers ignore it there), so the meta tag gets the policy WITHOUT it.
//
// The DESKTOP (Tauri) policy is a committed copy in tauri.conf.json, which Tauri
// sends as a response header for the app's own pages, so `frame-ancestors`
// applies there too. `--desktop <built-index.html>` removes the hosted meta
// tag from a built desktop dist so the webview enforces one policy, not the
// intersection of two. It NEVER touches the hosted copies.

import { readFileSync, writeFileSync } from 'fs';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');

// The one directive that differs between the hosted and desktop builds.
// The hosted site is informational: everything it loads is baked same-origin,
// so there are no loopback or third-party connect origins.
const HOSTED_CONNECT_SRC = "connect-src 'self'";
// Desktop: the Tauri IPC transport (`ipc://localhost` + the
// `http://ipc.localhost` fast path). No remote origin: the market, history,
// definitions and usage requests to tennoworth.app are made in Rust, and the
// webview only fetches its own bundled files.
const DESKTOP_CONNECT_SRC = "connect-src 'self' ipc://localhost http://ipc.localhost";

const DIRECTIVES = [
  "default-src 'self'",
  // No non-self origins: WFM has no CORS, warframestat dropped theirs;
  // everything (market.json, wfstat-catalog.json) is baked same-origin.
  HOSTED_CONNECT_SRC,
  "script-src 'self'",
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' data:",
  "font-src 'self'",
  "object-src 'none'",
  "base-uri 'self'",
  "form-action 'self'",
  "frame-ancestors 'none'",
];

const full = DIRECTIVES.join('; ');
const metaDirectives = DIRECTIVES.filter((d) => !d.startsWith('frame-ancestors'));

// The desktop header: same directives with the connect-src swapped.
const desktop = DIRECTIVES.map((d) => (d === HOSTED_CONNECT_SRC ? DESKTOP_CONNECT_SRC : d)).join('; ');

// The meta tag's content spans multiple indented lines; both the source and the
// Vite-built index.html keep that formatting, so one regex rewrites either.
const META_RE = /(http-equiv="Content-Security-Policy"\s*\n\s*content=")[^"]*(")/;
const metaReplacement = (directives) =>
  `$1\n        ${directives.join(';\n        ')};\n      $2`;
const META_TAG_RE = /\n\s*<meta\s+http-equiv="Content-Security-Policy"\s+content="[^"]*"\s*\/?>/;

// `--desktop <path>`: strip the meta CSP from ONE built index.html, then exit.
// Isolated from the hosted sync so it can never mutate the committed copies.
const desktopIdx = process.argv.indexOf('--desktop');
if (desktopIdx !== -1) {
  const target = process.argv[desktopIdx + 1];
  if (!target) {
    console.error('sync-csp: --desktop needs a path to the built index.html');
    process.exit(2);
  }
  const before = readFileSync(target, 'utf8');
  if (!META_TAG_RE.test(before)) {
    console.error(`sync-csp: no CSP meta tag found in ${target} - desktop rewrite failed`);
    process.exit(2);
  }
  writeFileSync(target, before.replace(META_TAG_RE, ''));
  console.log(`sync-csp: removed the meta CSP from ${target}; tauri.conf.json carries the desktop policy`);
  process.exit(0);
}

const TARGETS = [
  {
    path: 'deploy/Caddyfile-admin',
    re: /^(\s*Content-Security-Policy ").*(")$/m,
    replacement: `$1${full}$2`,
  },
  {
    path: 'frontend/admin.html',
    re: META_RE,
    replacement: metaReplacement(metaDirectives),
  },
  {
    path: 'deploy/Caddyfile',
    re: /^(\s*Content-Security-Policy ").*(")$/m,
    replacement: `$1${full}$2`,
  },
  {
    // Cloudflare serves the hosted site's static files with these headers.
    path: 'frontend/public/_headers',
    re: /^(\s*Content-Security-Policy: ).*$/m,
    replacement: `$1${full}`,
  },
  {
    path: 'frontend/index.html',
    re: META_RE,
    replacement: metaReplacement(metaDirectives),
  },
  {
    path: 'rust/tennoworth-desktop/tauri.conf.json',
    re: /("csp": )(?:null|"[^"]*")/,
    replacement: `$1${JSON.stringify(desktop)}`,
  },
];

const check = process.argv.includes('--check');
let drifted = 0;

for (const t of TARGETS) {
  const abs = join(ROOT, t.path);
  const before = readFileSync(abs, 'utf8');
  if (!t.re.test(before)) {
    console.error(`sync-csp: no CSP found in ${t.path} - pattern needs updating`);
    process.exit(2);
  }
  const after = before.replace(t.re, t.replacement);
  if (after !== before) {
    drifted += 1;
    if (check) {
      console.error(`sync-csp: ${t.path} is out of sync with scripts/sync-csp.ts`);
    } else {
      writeFileSync(abs, after);
      console.log(`sync-csp: rewrote ${t.path}`);
    }
  }
}

if (check && drifted > 0) {
  console.error('sync-csp: run `bun run csp` (in frontend/) and commit the result.');
  process.exit(1);
}
if (drifted === 0) console.log('sync-csp: All CSP copies are in sync');
