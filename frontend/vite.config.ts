import { defineConfig, type Plugin } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { execSync } from 'node:child_process';

// Build commit, baked in and shown in the footer. The web app is continuously
// deployed with no release tags, so the commit is the ONLY meaningful
// identifier - there is deliberately no __APP_VERSION__ define, and
// package.json carries no version field at all (it used to mirror the desktop
// version; nothing consumed it, and it was the pin that drifted, so it was
// removed - the desktop version lives in rust/tennoworth-desktop/
// Cargo.toml alone). The footer shows the commit, because the web app ships
// continuously and a desktop version number pinned to it would be a lie about
// when this build was made.
// git is present in CI (build-web checks out the repo) and local dev; the box
// never builds, so it just serves what CI baked. 'dev' is the fallback when
// git isn't reachable.
let commit = 'dev';
try {
  commit = execSync('git rev-parse --short HEAD', { stdio: ['ignore', 'pipe', 'ignore'] })
    .toString()
    .trim();
} catch {
  /* not a git checkout - keep 'dev' */
}

// No proxies, and no third-party origins at all. The market snapshot is
// served from /public/market.json and the resolver catalog from
// /public/wfstat-catalog.json - both baked at build time. warframestat.us
// used to be fetched directly, but it dropped its CORS headers on 2026-06-09
// (and varied on Accept-Language, so non-English browsers got names that
// matched nothing on WFM). We never talk to warframe.market from the browser.
// Don't reintroduce a runtime fetch to either.
// The `?preview-desktop` host runs the native calculations from WebAssembly
// (src/dev/domain-wasm.ts), which a CSP may only allow with 'wasm-unsafe-eval'.
// Granted on the dev server alone: production and the desktop build never load
// wasm, and their CSP copies (scripts/sync-csp.ts) stay exactly as written.
const devServerWasm: Plugin = {
  name: 'dev-server-wasm-csp',
  apply: 'serve',
  transformIndexHtml(html) {
    const granted = html.replace("script-src 'self';", "script-src 'self' 'wasm-unsafe-eval';");
    if (granted === html) throw new Error("dev-server-wasm-csp: index.html has no \"script-src 'self';\" to extend");
    return granted;
  },
};

export default defineConfig({
  plugins: [svelte(), devServerWasm],
  server: { port: 5173, host: '127.0.0.1' },
  define: {
    __APP_COMMIT__: JSON.stringify(commit),
  },
});
