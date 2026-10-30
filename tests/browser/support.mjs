import { test as base, expect } from "@playwright/test";
import { fileURLToPath, pathToFileURL } from "node:url";
import { join } from "node:path";

export const root = fileURLToPath(new URL("../../", import.meta.url));
export const fixtures = join(root, "target", "browser-fixtures");
export const fixtureUrl = (name = "dashboard", hash = "") =>
  pathToFileURL(join(fixtures, name + ".html")).href + hash;

// Catch script failures even when the button click itself succeeds.
export const test = base.extend({
  page: async ({ page }, use) => {
    const errors = [];
    const network = [];
    page.on("pageerror", (error) => errors.push(error.message));
    page.on("request", (request) => {
      if (/^https?:/.test(request.url())) network.push(request.url());
    });
    await use(page);
    expect(errors, "page must not throw JavaScript errors").toEqual([]);
    expect(network, "offline pages must not attempt network requests").toEqual([]);
  },
});

export { expect };
