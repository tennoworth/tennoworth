import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

// vitePreprocess lets us use `<script lang="ts">` in .svelte files. It
// pipes TS through Vite's esbuild for transpilation; svelte-check
// handles the actual type-checking against tsconfig.json.
export default {
  preprocess: vitePreprocess(),
};
