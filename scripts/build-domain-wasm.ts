// Build the native decision contracts for the `?preview-desktop` development
// host: rust/market-domain-wasm compiled to wasm32, copied to where the
// preview loads it. Needs Rust with the wasm32-unknown-unknown target. Run it
// again after changing market-domain; the browser suite builds it fresh.
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const rust = join(root, 'rust');
execFileSync('cargo', ['build', '--release', '--locked', '-p', 'market-domain-wasm', '--target', 'wasm32-unknown-unknown'],
  { cwd: rust, stdio: 'inherit' });
// CARGO_TARGET_DIR may move the output; cargo reports where it went.
const { target_directory } = JSON.parse(execFileSync('cargo', ['metadata', '--format-version', '1', '--no-deps'],
  { cwd: rust, encoding: 'utf8' })) as { target_directory: string };
const out = join(root, 'frontend', 'src', 'dev', 'generated');
mkdirSync(out, { recursive: true });
copyFileSync(join(target_directory, 'wasm32-unknown-unknown', 'release', 'market_domain_wasm.wasm'), join(out, 'market-domain.wasm'));
console.log(`build-domain-wasm: wrote ${join(out, 'market-domain.wasm')}`);
