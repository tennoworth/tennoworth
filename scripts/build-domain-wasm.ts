// Compile market-domain to wasm32 for the frontend. Needs Rust with the
// wasm32-unknown-unknown target; run again after changing market-domain.
//
//   (no argument)  rust/market-domain-wasm -> src/dev/generated/market-domain.wasm,
//                  the `?preview-desktop` host's evaluate_domain. Dev only.
//   web            rust/market-domain-web -> src/generated/market-domain-web.wasm,
//                  the view rules every surface loads before it renders
//                  (src/domain/native.ts). The frontend's build, test and
//                  desktop-build scripts run this first.
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const builds = {
  preview: { crate: 'market-domain-wasm', dir: ['src', 'dev', 'generated'], file: 'market-domain.wasm' },
  web: { crate: 'market-domain-web', dir: ['src', 'generated'], file: 'market-domain-web.wasm' },
};
const which = process.argv[2] ?? 'preview';
if (!(which in builds)) {
  console.error(`build-domain-wasm: unknown build "${which}" (expected: web)`);
  process.exit(2);
}
const { crate, dir, file } = builds[which as keyof typeof builds];
const root = fileURLToPath(new URL('..', import.meta.url));
const rust = join(root, 'rust');
execFileSync('cargo', ['build', '--release', '--locked', '-p', crate, '--target', 'wasm32-unknown-unknown'],
  { cwd: rust, stdio: 'inherit' });
// CARGO_TARGET_DIR may move the output; cargo reports where it went.
const { target_directory } = JSON.parse(execFileSync('cargo', ['metadata', '--format-version', '1', '--no-deps'],
  { cwd: rust, encoding: 'utf8' })) as { target_directory: string };
const out = join(root, 'frontend', ...dir);
mkdirSync(out, { recursive: true });
copyFileSync(join(target_directory, 'wasm32-unknown-unknown', 'release', `${crate.replaceAll('-', '_')}.wasm`), join(out, file));
console.log(`build-domain-wasm: wrote ${join(out, file)}`);
