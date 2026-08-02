/// <reference types="svelte" />
/// <reference types="vite/client" />

// Injected by vite.config.js `define` at build time. Only the build commit:
// the web app is continuously deployed with no release tags, so the semver
// was noise next to the commit — see the define in vite.config.js.
declare const __APP_COMMIT__: string;

// Local Network Access fetch opt-in. Our HTTPS-hosted app fetches the HTTP
// loopback companion; 2026 browsers gate that cross-address-space request
// behind this hint, which isn't in TypeScript's DOM lib yet. Merge it onto
// RequestInit so the loopback call sites stay typed instead of `as any`.
interface RequestInit {
  targetAddressSpace?: 'loopback' | 'local' | 'public';
}
