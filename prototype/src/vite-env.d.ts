/// <reference types="svelte" />
/// <reference types="vite/client" />

// Injected by vite.config.ts `define` at build time. Only the build commit:
// the web app is continuously deployed with no release tags, so the semver
// was noise next to the commit - see the define in vite.config.ts.
declare const __APP_COMMIT__: string;
