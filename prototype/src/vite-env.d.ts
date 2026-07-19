/// <reference types="svelte" />
/// <reference types="vite/client" />

// Injected by vite.config.js `define` at build time.
declare const __APP_VERSION__: string;
declare const __APP_COMMIT__: string;

// Local Network Access fetch opt-in. Our HTTPS-hosted app fetches the HTTP
// loopback companion; 2026 browsers gate that cross-address-space request
// behind this hint, which isn't in TypeScript's DOM lib yet. Merge it onto
// RequestInit so the loopback call sites stay typed instead of `as any`.
interface RequestInit {
  targetAddressSpace?: 'loopback' | 'local' | 'public';
}
