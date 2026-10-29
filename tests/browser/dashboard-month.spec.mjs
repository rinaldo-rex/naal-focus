import { test, expect, fixtureUrl } from "./support.mjs";

test("month arrows move backward and forward and clear the old selection", async ({ page }) => {
  await page.goto(fixtureUrl("dashboard", "#month"));
  await page.locator(".mcell.today").click();
  await expect(page.locator(".msel .drow")).toHaveCount(3);
  await page.getByTitle("Previous month", { exact: true }).click();
  await expect(page.locator(".mtitle")).toHaveText("September 2026");
  await expect(page.locator(".mcell .mday")).toHaveCount(30);
  await expect(page.locator(".msel")).toHaveText("Select a day to read its reflections.");
  await page.getByTitle("Next month", { exact: true }).click();
  await expect(page.locator(".mtitle")).toHaveText("October 2026");
  await expect(page.locator(".mcell .mday")).toHaveCount(31);
  await page.getByTitle("Next month", { exact: true }).click();
  await expect(page.locator(".mtitle")).toHaveText("November 2026");
  await expect(page.locator(".mcell .mday")).toHaveCount(30);
});

test("month arrows roll across the year boundary in both directions", async ({ page }) => {
  await page.goto(fixtureUrl("year-end", "#month"));
  await expect(page.locator(".mtitle")).toHaveText("December 2026");
  await page.getByTitle("Next month", { exact: true }).click();
  await expect(page.locator(".mtitle")).toHaveText("January 2027");
  await expect(page.locator(".mcell .mday")).toHaveCount(31);
  await page.getByTitle("Previous month", { exact: true }).click();
  await expect(page.locator(".mtitle")).toHaveText("December 2026");
});

test("month arrows preserve leap-day geometry when leaving and returning", async ({ page }) => {
  await page.goto(fixtureUrl("leap", "#month"));
  await expect(page.locator(".mtitle")).toHaveText("February 2024");
  await expect(page.locator(".mcell .mday")).toHaveCount(29);
  await expect(page.locator(".mcell .mday").last()).toHaveText("29");
  await page.getByTitle("Next month", { exact: true }).click();
  await expect(page.locator(".mtitle")).toHaveText("March 2024");
  await expect(page.locator(".mcell .mday")).toHaveCount(31);
  await page.getByTitle("Previous month", { exact: true }).click();
  await expect(page.locator(".mtitle")).toHaveText("February 2024");
  await expect(page.locator(".mcell .mday")).toHaveCount(29);
});
