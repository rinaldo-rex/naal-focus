import { stat, readdir } from "node:fs/promises";
import { join, dirname, extname, relative, isAbsolute } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { test, expect, root } from "./support.mjs";

const guide = pathToFileURL(join(root, "docs", "html", "index.html")).href;

test("guide anchors, local targets, and screenshot references resolve", async ({ page }) => {
  await page.goto(guide);
  const ids = await page.locator("[id]").evaluateAll((nodes) => nodes.map((node) => node.id));
  expect(new Set(ids).size, "guide IDs must be unique").toBe(ids.length);
  const targets = await page.locator("a[href], img[src]").evaluateAll((nodes) =>
    nodes.map((node) => node.tagName === "IMG" ? node.src : node.href));
  expect(targets.length).toBeGreaterThan(0);
  const referencedImages = new Set();
  for (const href of new Set(targets)) {
    const target = new URL(href);
    expect(target.protocol, "guide references must remain local: " + href).toBe("file:");
    const file = fileURLToPath(target);
    const local = relative(root, file);
    expect(isAbsolute(local), "target outside repository: " + href).toBe(false);
    expect(local, "target outside repository: " + href).not.toMatch(/^\.\.(?:[\\/]|$)/);
    expect((await stat(file)).isFile(), "missing guide target: " + href).toBe(true);
    if (extname(file) === ".png") referencedImages.add(file);
    if (target.hash) {
      expect(file, "cross-page anchors need their own validation").toBe(fileURLToPath(guide));
      expect(ids).toContain(decodeURIComponent(target.hash.slice(1)));
    }
  }
  const screenshots = join(dirname(fileURLToPath(guide)), "screenshots");
  for (const name of await readdir(screenshots)) {
    if (extname(name) === ".png")
      expect(referencedImages.has(join(screenshots, name)), "unreferenced screenshot: " + name).toBe(true);
  }
});

test("guide images load at their documented dimensions with useful alt text", async ({ page }) => {
  await page.goto(guide);
  const images = await page.locator("img").evaluateAll((nodes) => nodes.map((image) => ({
    src: image.getAttribute("src"),
    alt: image.alt.trim(),
    loaded: image.complete,
    actual: [image.naturalWidth, image.naturalHeight],
    declared: [Number(image.getAttribute("width")), Number(image.getAttribute("height"))],
  })));
  expect(images.length).toBeGreaterThan(0);
  for (const image of images) {
    expect(image.loaded, image.src).toBe(true);
    expect(image.alt.length, image.src).toBeGreaterThan(0);
    expect(image.actual[0], image.src).toBeGreaterThan(0);
    expect(image.actual, "stale screenshot dimensions: " + image.src).toEqual(image.declared);
  }
});

test("guide fits desktop widths and section links remain usable", async ({ page }, info) => {
  await page.goto(guide);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  for (const label of ["Applet", "Reminder", "Reflection & context", "Settings", "Dashboard", "Privacy & boundaries", "macOS prototype"]) {
    const link = page.getByRole("navigation").getByRole("link", { name: label, exact: false });
    const hash = await link.getAttribute("href");
    await link.click();
    await expect(page).toHaveURL(guide + hash);
    await expect(page.locator(hash)).toBeInViewport();
  }
  await page.screenshot({ path: info.outputPath("guide.png"), fullPage: true });
});
