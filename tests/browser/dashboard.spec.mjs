import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { test, expect, fixtureUrl } from "./support.mjs";

test("overview shows explicit metadata and year-map keyboard navigation", async ({ page }) => {
  await page.goto(fixtureUrl());
  await expect(page.locator("#tabs .active")).toHaveText("Overview");
  await expect(page.locator(".ycell[title]")).toHaveCount(365);
  await expect(page.getByRole("heading", { name: "Projects", exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Feelings", exact: true })).toBeVisible();
  await expect(page.locator("#app")).toContainText("@demo");
  await expect(page.locator("#app")).toContainText("??focused");
  await expect(page.locator("#foot")).toContainText("2 unreadable lines skipped");
  await page.getByRole("button", { name: "Oct 29 — 3 reflections" }).focus();
  await page.keyboard.press("Enter");
  await expect(page.locator("#tabs .active")).toHaveText("Day");
  await expect(page.locator(".entryrow")).toHaveCount(3);
});

test("day controls preserve metadata, quiet days, and relative times", async ({ page }) => {
  await page.goto(fixtureUrl("dashboard", "#day"));
  await expect(page.locator(".daytitle")).toHaveText("Thursday, October 29, 2026");
  await expect(page.locator(".entryrow")).toHaveCount(3);
  await expect(page.locator(".etime").first()).toHaveText("8:00 AM");
  await expect(page.locator(".chip")).toContainText(["#rust", "@demo", "??focused"]);
  await page.getByRole("button", { name: "Relative", exact: true }).click();
  await expect(page.locator(".etime").first()).toHaveText("3 hours ago");
  await page.getByTitle("Previous day", { exact: true }).click();
  await expect(page.locator(".entryrow")).toHaveCount(1);
  await page.getByRole("button", { name: "Today", exact: true }).click();
  await expect(page.locator(".entryrow")).toHaveCount(3);
  await page.getByTitle("Next day", { exact: true }).click();
  await expect(page.locator(".entryrow")).toHaveCount(0);
  await expect(page.locator("#app")).toContainText("No reflections recorded on this day.");
  await page.getByRole("link", { name: "Month", exact: true }).click();
  await expect(page.locator("#tabs .active")).toHaveText("Month");
  await page.goBack();
  await expect(page.locator("#tabs .active")).toHaveText("Day");
});

test("month selection, OR tag filters, and overview tag shortcut work", async ({ page }) => {
  await page.goto(fixtureUrl("dashboard", "#month"));
  await expect(page.locator(".mtitle")).toHaveText("October 2026");
  const today = page.locator(".mcell.today");
  await today.click();
  await expect(page.locator(".msel .drow")).toHaveCount(3);
  const yesterday = page.locator('.mcell[title="October 28, 2026 — 1 reflection"]');
  await page.getByRole("button", { name: "#rust", exact: true }).click();
  await expect(yesterday).toHaveClass(/dim/);
  await page.getByRole("button", { name: "#outside", exact: true }).click();
  await expect(yesterday).not.toHaveClass(/dim/);
  await yesterday.click();
  await expect(page.locator(".msel .drow")).toHaveCount(1);
  await page.getByRole("button", { name: "clear filter", exact: true }).click();
  await expect(page.locator(".fchip.on")).toHaveCount(0);
  await page.getByRole("link", { name: "Overview", exact: true }).click();
  await page.getByRole("button", { name: "#rust", exact: true }).click();
  await expect(page.locator("#tabs .active")).toHaveText("Month");
  await expect(page.locator(".fchip.on")).toHaveText("#rust");
});

test("empty and unreadable history remain distinct usable offline pages", async ({ page }) => {
  await page.goto(fixtureUrl("empty"));
  await expect(page.getByRole("heading", { name: "Nothing recorded yet" })).toBeVisible();
  await expect(page.locator("#tabs a")).toHaveCount(0);
  await expect(page.locator("#download")).toBeEnabled();
  await page.goto(fixtureUrl("error"));
  await expect(page.getByRole("heading", { name: "History could not be read" })).toBeVisible();
  await expect(page.locator(".hero")).toContainText("Demo history unavailable.");
  await expect(page.getByRole("heading", { name: "Nothing recorded yet" })).toHaveCount(0);
  await expect(page.locator("#download")).toBeEnabled();
});

test("untrusted reflection text remains text, including in an offline download", async ({ page }, info) => {
  await page.goto(fixtureUrl("hostile", "#day"));
  await expect(page.locator(".etext")).toContainText("</script><script>");
  await expect(page.locator("#app img")).toHaveCount(0);
  expect(await page.evaluate(() => window.__naal_injected)).toBeUndefined();
  await downloadAndReopen(page, info, "hostile");
  await page.getByRole("link", { name: "Day", exact: true }).click();
  await expect(page.locator(".etext")).toContainText("</script><script>");
  expect(await page.evaluate(() => window.__naal_injected)).toBeUndefined();
});

test("downloaded HTML preserves its data and works after reopening offline", async ({ page }, info) => {
  await page.goto(fixtureUrl("dashboard", "#month"));
  await page.locator(".mcell.today").click();
  await downloadAndReopen(page, info, "dashboard");
  // The saved copy starts clean, rather than duplicating the old rendered controls.
  await expect(page.locator("#tabs a")).toHaveCount(3);
  await page.getByRole("link", { name: "Day", exact: true }).click();
  await expect(page.locator(".entryrow")).toHaveCount(3);
  await page.getByRole("link", { name: "Month", exact: true }).click();
  await page.locator(".mcell.today").click();
  await expect(page.locator(".msel .drow")).toHaveCount(3);
});

test("capture the three dashboard views from fixed demo data", async ({ page }, info) => {
  for (const view of ["overview", "day", "month"]) {
    await page.goto(fixtureUrl("dashboard", "#" + view));
    if (view === "month") await page.locator(".mcell.today").click();
    await expect(page.locator("#tabs .active")).toHaveText(view[0].toUpperCase() + view.slice(1));
    await page.screenshot({ path: info.outputPath("dashboard-" + view + ".png"), fullPage: true });
  }
});

async function downloadAndReopen(page, info, name) {
  const data = await page.locator("#naal-data").textContent();
  const downloadPromise = page.waitForEvent("download");
  await page.getByRole("button", { name: "Download HTML", exact: true }).click();
  const download = await downloadPromise;
  expect(download.suggestedFilename()).toBe("naal-focus-activity.html");
  const saved = info.outputPath(name + "-download.html");
  await download.saveAs(saved);
  const html = await readFile(saved, "utf8");
  expect(html).not.toContain("__NAAL_DATA__");
  await page.goto(pathToFileURL(saved).href);
  expect(await page.locator("#naal-data").textContent()).toBe(data);
}
