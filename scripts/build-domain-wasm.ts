// Compile market-domain to wasm32 for the frontend. Needs Rust with the
// wasm32-unknown-unknown target; run again after changing market-domain.
//
//   (no argument)  rust/market-domain-wasm -> src/dev/generated/market-domain.wasm,
//                  the `?preview-desktop` host's evaluate_domain. Dev only.
//   web            rust/market-domain-web, the view rules a surface loads before
//                  it renders (src/domain/native.ts), in two builds:
//                  src/generated/market-domain-web.wasm with every op (desktop,
//                  overlay, unit suite) and src/generated/market-domain-site.wasm
//                  without the desktop-only ops (hosted site and prerender).
//                  The frontend's dev, build, test and desktop-build scripts run
//                  this first.
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

interface Build { crate: string; dir: string[]; file: string; args?: string[]; subdir?: string }
const builds: Record<string, Build[]> = {
  preview: [{ crate: 'market-domain-wasm', dir: ['src', 'dev', 'generated'], file: 'market-domain.wasm' }],
  web: [
    { crate: 'market-domain-web', dir: ['src', 'generated'], file: 'market-domain-web.wasm' },
    // Its own target directory: alternating feature sets in one directory would
    // rebuild market-domain on every run.
    { crate: 'market-domain-web', dir: ['src', 'generated'], file: 'market-domain-site.wasm', args: ['--no-default-features'], subdir: 'site' },
  ],
};
const which = process.argv[2] ?? 'preview';
if (!(which in builds)) {
  console.error(`build-domain-wasm: unknown build "${which}" (expected: web)`);
  process.exit(2);
}
const root = fileURLToPath(new URL('..', import.meta.url));
const rust = join(root, 'rust');
// CARGO_TARGET_DIR may move the output; cargo reports where it goes.
const { target_directory } = JSON.parse(execFileSync('cargo', ['metadata', '--format-version', '1', '--no-deps'],
  { cwd: rust, encoding: 'utf8' })) as { target_directory: string };
for (const { crate, dir, file, args = [], subdir } of builds[which]!) {
  const target = subdir ? join(target_directory, subdir) : target_directory;
  execFileSync('cargo', ['build', '--release', '--locked', '-p', crate, '--target', 'wasm32-unknown-unknown', '--target-dir', target, ...args],
    { cwd: rust, stdio: 'inherit' });
  const out = join(root, 'frontend', ...dir);
  mkdirSync(out, { recursive: true });
  copyFileSync(join(target, 'wasm32-unknown-unknown', 'release', `${crate.replaceAll('-', '_')}.wasm`), join(out, file));
  console.log(`build-domain-wasm: wrote ${join(out, file)}`);
}
