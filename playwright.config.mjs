import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/browser",
  testMatch: "**/*.spec.mjs",
  globalSetup: "./tests/browser/setup.mjs",
  outputDir: "./target/browser-tests",
  workers: 1,
  retries: 0,
  timeout: 15_000,
  expect: { timeout: 3_000 },
  reporter: "list",
  use: {
    channel: "chrome",
    headless: true,
    offline: true,
    serviceWorkers: "block",
    acceptDownloads: true,
    colorScheme: "dark",
    locale: "en-US",
    timezoneId: "UTC",
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
  },
  projects: [
    { name: "desktop-compact", use: { viewport: { width: 1000, height: 660 }, deviceScaleFactor: 1 } },
    { name: "desktop-wide", use: { viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 } },
    { name: "desktop-scaled", use: { viewport: { width: 1707, height: 960 }, deviceScaleFactor: 2 } },
  ],
});
