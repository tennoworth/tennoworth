import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Vitest-specific config. Re-uses the Svelte plugin so `<script lang="ts">`
// in tested components transpiles, and globs both .test.ts and .test.js
// during the migration window (drop the .js side once everything's TS).
export default defineConfig({
  plugins: [svelte()],
  test: {
    environment: 'jsdom',
    globals: false,
    include: ['src/**/*.test.{ts,js}'],
  },
});
