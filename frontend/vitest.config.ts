import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Vitest-specific config. Re-uses the Svelte plugin so `<script lang="ts">`
// in tested components transpiles.
export default defineConfig({
  plugins: [svelte()],
  // @testing-library/svelte mounts components through svelte's client API;
  // without the browser condition the server build resolves and mount() dies
  // with lifecycle_function_unavailable.
  resolve: {
    conditions: ['browser'],
  },
  test: {
    environment: 'jsdom',
    globals: false,
    include: ['src/**/*.test.{ts,svelte}'],
  },
});
