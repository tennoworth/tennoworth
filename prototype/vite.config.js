import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { execSync } from 'node:child_process';
import { readFileSync } from 'node:fs';

// Version + build commit, baked in and shown in the UI. The semver is the
// source of truth in package.json; the short commit lets you tell which build
// the auto-pull box is actually serving (the semver only moves on a manual
// bump, the commit moves every build). git is present in CI (build-web checks
// out the repo) and local dev; the box never builds, so it just serves what CI
// baked. 'dev' is the fallback when git isn't reachable.
const pkg = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8'));
let commit = 'dev';
try {
  commit = execSync('git rev-parse --short HEAD', { stdio: ['ignore', 'pipe', 'ignore'] })
    .toString()
    .trim();
} catch {
  /* not a git checkout — keep 'dev' */
}

// No proxies — the market snapshot is served from /public/market.json,
// warframestat.us has CORS, and we never talk to warframe.market from the
// browser. The GitHub Actions cron job is the only thing that hits WFM.
export default defineConfig({
  plugins: [svelte()],
  server: { port: 5173, host: '127.0.0.1' },
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
    __APP_COMMIT__: JSON.stringify(commit),
  },
});
