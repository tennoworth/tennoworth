import { defineConfig } from '@playwright/test';
export default defineConfig({
  testDir: './tests/public', outputDir: 'test-results/public',
  use: { baseURL: 'http://127.0.0.1:5189', screenshot: 'only-on-failure', trace: 'retain-on-failure' },
  webServer: { command: 'bun tests/public/server.ts', url: 'http://127.0.0.1:5189', reuseExistingServer: false },
  projects: [{ name: 'chromium', use: { browserName: 'chromium' } }, { name: 'webkit', use: { browserName: 'webkit' } }],
});
