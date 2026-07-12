// Single source of truth for the Content-Security-Policy.
//
// The CSP ships in THREE places that must agree — prototype/index.html (meta
// tag, works even on hosts that drop header files), prototype/public/_headers
// (Cloudflare Pages / Netlify), and deploy/Caddyfile (self-host) — and they
// were hand-synced, with a standing doc warning instead of tooling. Edit the
// DIRECTIVES below, run `bun run csp` (from prototype/) to rewrite all three;
// CI runs `--check` before every build and fails if any copy drifted.
//
// One deliberate asymmetry: `frame-ancestors` is invalid in a <meta> CSP
// (browsers ignore it there), so the meta tag gets the policy WITHOUT it.

import { readFileSync, writeFileSync } from 'fs';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');

const DIRECTIVES = [
  "default-src 'self'",
  // The two loopback entries are the companion's serve endpoint — the only
  // non-self origin the app ever talks to. No third-party origins: WFM has
  // no CORS, warframestat dropped theirs; everything is baked same-origin.
  "connect-src 'self' http://127.0.0.1:* http://localhost:*",
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
    // The meta tag's content spans multiple indented lines; rewrite the whole
    // content="..." block, preserving the one-directive-per-line formatting.
    re: /(http-equiv="Content-Security-Policy"\s*\n\s*content=")[^"]*(")/,
    replacement: `$1\n        ${metaDirectives.join(';\n        ')};\n      $2`,
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
