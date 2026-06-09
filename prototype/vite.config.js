import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// No proxies — the market snapshot is served from /public/market.json,
// warframestat.us has CORS, and we never talk to warframe.market from the
// browser. The GitHub Actions cron job is the only thing that hits WFM.
export default defineConfig({
  plugins: [svelte()],
  server: { port: 5173, host: '127.0.0.1' },
});
