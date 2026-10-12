import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { cpSync } from 'node:fs';

export default defineConfig({
  plugins: [svelte(), {
    name: 'admin-static-assets',
    apply: 'build',
    closeBundle() {
      cpSync('public/fonts', 'dist-admin/fonts', { recursive: true });
      cpSync('public/favicon.svg', 'dist-admin/favicon.svg');
    },
  }],
  server: {
    host: '127.0.0.1', port: 5194, strictPort: true,
    proxy: { '/api/announcements': 'http://127.0.0.1:8084', '/api/status': 'http://127.0.0.1:8084', '/api/jobs': 'http://127.0.0.1:8084' },
  },
  build: { outDir: 'dist-admin', emptyOutDir: true, copyPublicDir: false, rollupOptions: { input: 'admin.html' } },
});
