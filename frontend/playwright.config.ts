import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests/browser',
  outputDir: 'test-results/responsive',
  fullyParallel: false,
  // GitHub's Linux runners have 4 cores and Playwright defaults to half of
  // them. The suite is the critical path of audit, so CI uses all four.
  workers: process.env.CI ? 4 : undefined,
  timeout: 45_000,
  expect: { timeout: 10_000 },
  use: {
    baseURL: process.env.RESPONSIVE_BASE_URL || 'http://127.0.0.1:5173',
    colorScheme: 'dark',
    screenshot: 'only-on-failure',
    trace: 'retain-on-failure',
  },
  webServer: process.env.RESPONSIVE_BASE_URL ? undefined : {
    command: 'bun run dev --host 127.0.0.1',
    url: 'http://127.0.0.1:5173',
    reuseExistingServer: !process.env.CI,
  },
  projects: [
    { name: 'chromium', use: { browserName: 'chromium' } },
    { name: 'webkit', use: { browserName: 'webkit' } },
  ],
});
