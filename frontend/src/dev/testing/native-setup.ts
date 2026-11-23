// The unit suite runs the real view rules: the wasm32 build of
// rust/market-domain-web, as every surface loads it before rendering.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { installNative, MISSING_NATIVE } from '../../domain/native';

// A path, not a URL: under jsdom the global URL is jsdom's, which fs rejects.
const path = resolve(import.meta.dirname, '../../generated/market-domain-web.wasm');
let bytes: Uint8Array<ArrayBuffer>;
try {
  bytes = new Uint8Array(readFileSync(path));
} catch (error) {
  throw new Error(`${MISSING_NATIVE} (${String(error)})`);
}
installNative(bytes);
