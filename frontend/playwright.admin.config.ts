import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests/admin', outputDir: 'test-results/admin', workers: 1, timeout: 30_000,
  use: { baseURL: 'http://127.0.0.1:5194', screenshot: 'only-on-failure', trace: 'retain-on-failure' },
  webServer: [
    // A cold Rust build can take longer than the browser server's normal startup budget.
    { command: 'mkdir -p ../target/publisher-tmp && TENNOWORTH_PUBLISHER_DEV_STATE=../target/admin-browser-state CARGO_TARGET_DIR=../target/publisher-cargo TMPDIR="$PWD/../target/publisher-tmp" cargo run --manifest-path ../rust/Cargo.toml -p tennoworth-publisher --features dev-server --bin tennoworth-publisher-dev', url: 'http://127.0.0.1:8084/api/announcements', timeout: 600_000, reuseExistingServer: !process.env.CI },
    { command: 'bun run dev:admin', url: 'http://127.0.0.1:5194/admin.html', reuseExistingServer: !process.env.CI },
  ],
  projects: [{ name: 'chromium', use: { browserName: 'chromium' } }, { name: 'webkit', use: { browserName: 'webkit' } }],
});
