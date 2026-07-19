// Single source of truth for the Content-Security-Policy.
//
// The HOSTED CSP ships in THREE places that must agree — prototype/index.html
// (meta tag, works even on hosts that drop header files),
// prototype/public/_headers (Cloudflare Pages / Netlify), and deploy/Caddyfile
// (self-host) — and they were hand-synced, with a standing doc warning instead
// of tooling. Edit the DIRECTIVES below, run `bun run csp` (from prototype/) to
// rewrite all three; CI runs `--check` before every build and fails if any copy
// drifted.
//
// One deliberate asymmetry: `frame-ancestors` is invalid in a <meta> CSP
// (browsers ignore it there), so the meta tag gets the policy WITHOUT it.
//
// The DESKTOP (Tauri) build is a build-variant, not a fourth committed copy:
// `--desktop <built-index.html>` rewrites a built dist's meta CSP in place with
// the desktop connect-src (loopback dropped; the Tauri IPC scheme + the one C4
// refresh origin added). It NEVER touches the three hosted copies, so the
// hosted CSP stays byte-identical.

import { readFileSync, writeFileSync } from 'fs';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');

// The one directive that differs between the hosted and desktop builds.
const HOSTED_CONNECT_SRC = "connect-src 'self' http://127.0.0.1:* http://localhost:*";
// Desktop: no loopback companion (there is no HTTP server) — instead the Tauri
// IPC transport (`ipc://localhost` + the `http://ipc.localhost` fast path) and
// the single C4 remote-refresh origin. Verified against the desktop spike's
// captured `connect-src` violations.
const DESKTOP_CONNECT_SRC =
  "connect-src 'self' ipc://localhost http://ipc.localhost https://tennoworth.app";

const DIRECTIVES = [
  "default-src 'self'",
  // The two loopback entries are the companion's serve endpoint — the only
  // non-self origin the app ever talks to. No third-party origins: WFM has
  // no CORS, warframestat dropped theirs; everything is baked same-origin.
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

// The meta content block the desktop dist should carry: same directives, the
// connect-src swapped, frame-ancestors dropped (invalid in a meta tag, as above).
const desktopMetaDirectives = metaDirectives.map((d) =>
  d === HOSTED_CONNECT_SRC ? DESKTOP_CONNECT_SRC : d,
);

// The meta tag's content spans multiple indented lines; both the source and the
// Vite-built index.html keep that formatting, so one regex rewrites either.
const META_RE = /(http-equiv="Content-Security-Policy"\s*\n\s*content=")[^"]*(")/;
const metaReplacement = (directives) =>
  `$1\n        ${directives.join(';\n        ')};\n      $2`;

// `--desktop <path>`: rewrite ONE built index.html with the desktop CSP, then
// exit. Isolated from the hosted sync so it can never mutate the committed copies.
const desktopIdx = process.argv.indexOf('--desktop');
if (desktopIdx !== -1) {
  const target = process.argv[desktopIdx + 1];
  if (!target) {
    console.error('sync-csp: --desktop needs a path to the built index.html');
    process.exit(2);
  }
  const before = readFileSync(target, 'utf8');
  if (!META_RE.test(before)) {
    console.error(`sync-csp: no CSP meta tag found in ${target} — desktop rewrite failed`);
    process.exit(2);
  }
  const after = before.replace(META_RE, metaReplacement(desktopMetaDirectives));
  writeFileSync(target, after);
  console.log(`sync-csp: wrote desktop CSP into ${target}`);
  console.log(`sync-csp: ${DESKTOP_CONNECT_SRC}`);
  process.exit(0);
}

const TARGETS = [
  {
    path: 'prototype/public/_headers',
    re: /^(\s*Content-Security-Policy: ).*$/m,
    replacement: `$1${full}`,
  },
  {
    path: 'deploy/Caddyfile',
    re: /^(\s*Content-Security-Policy ").*(")$/m,
    replacement: `$1${full}$2`,
  },
  {
    path: 'prototype/index.html',
    re: META_RE,
    replacement: metaReplacement(metaDirectives),
  },
];

const check = process.argv.includes('--check');
let drifted = 0;

for (const t of TARGETS) {
  const abs = join(ROOT, t.path);
  const before = readFileSync(abs, 'utf8');
  if (!t.re.test(before)) {
    console.error(`sync-csp: no CSP found in ${t.path} — pattern needs updating`);
    process.exit(2);
  }
  const after = before.replace(t.re, t.replacement);
  if (after !== before) {
    drifted += 1;
    if (check) {
      console.error(`sync-csp: ${t.path} is out of sync with scripts/sync-csp.mjs`);
    } else {
      writeFileSync(abs, after);
      console.log(`sync-csp: rewrote ${t.path}`);
    }
  }
}

if (check && drifted > 0) {
  console.error('sync-csp: run `bun run csp` (in prototype/) and commit the result.');
  process.exit(1);
}
if (drifted === 0) console.log('sync-csp: all three copies in sync');
