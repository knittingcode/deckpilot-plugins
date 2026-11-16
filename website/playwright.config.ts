import { defineConfig, devices } from '@playwright/test';

/**
 * Browser tests run against the two static builds, served the way GitHub
 * Pages serves them (tests/serve.mjs):
 *   site     dist/          the real build (empty registry today)
 *   fixture  dist-fixture/  the test-only catalog (never deployed)
 * Build both first: `npm run build && npm run build:fixture`.
 */
const BASE = '/deckpilot-plugins';
// SITE_URL=https://knittingcode.github.io/deckpilot-plugins/ runs the `site`
// suite against a deployed site instead of the local build (npm run test:live).
const live = process.env.SITE_URL?.replace(/\/?$/, '/');

export default defineConfig({
  testDir: './tests',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI || live ? 1 : 0,
  reporter: process.env.CI ? [['github'], ['list']] : 'list',
  use: { ...devices['Desktop Chrome'], trace: 'retain-on-failure' },
  projects: live
    ? [{ name: 'site', testDir: './tests/site', use: { baseURL: live } }]
    : [
        { name: 'site', testDir: './tests/site', use: { baseURL: `http://127.0.0.1:4321${BASE}/` } },
        { name: 'fixture', testDir: './tests/fixture', use: { baseURL: `http://127.0.0.1:4322${BASE}/` } },
      ],
  webServer: live
    ? undefined
    : [
        { command: 'node tests/serve.mjs dist 4321', url: `http://127.0.0.1:4321${BASE}/en/`, reuseExistingServer: !process.env.CI },
        { command: 'node tests/serve.mjs dist-fixture 4322', url: `http://127.0.0.1:4322${BASE}/en/`, reuseExistingServer: !process.env.CI },
      ],
});
