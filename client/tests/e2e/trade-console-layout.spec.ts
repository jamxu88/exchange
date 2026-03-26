import { expect, test, type Page } from "@playwright/test";

const previewPath = "/preview/trade-console";

const viewportFixtures = [
  { height: 982, name: "macbook", width: 1512 },
  { height: 900, name: "laptop", width: 1440 },
  { height: 768, name: "small-desktop", width: 1280 },
];

async function boxWithinViewport(
  page: Page,
  testId: string,
) {
  const box = await page.getByTestId(testId).boundingBox();
  expect(box).not.toBeNull();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.y).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(page.viewportSize()!.width + 1);
  expect(box!.y + box!.height).toBeLessThanOrEqual(page.viewportSize()!.height + 1);
}

for (const viewport of viewportFixtures) {
  test.describe(`${viewport.name} viewport`, () => {
    test.use({ viewport });

    test("fills the viewport and keeps primary panels visible", async ({ page }) => {
      await page.goto(previewPath);

      const root = page.getByTestId("trade-console-root");
      await expect(root).toBeVisible();
      await expect(page.getByTestId("trade-console-header")).toBeVisible();
      await expect(page.getByTestId("positions-panel")).toBeVisible();
      await expect(page.getByTestId("orderbook-panel")).toBeVisible();
      await expect(page.getByTestId("ticket-panel")).toBeVisible();
      await expect(page.getByTestId("messages-panel")).toBeVisible();

      const rootBox = await root.boundingBox();
      expect(rootBox).not.toBeNull();
      expect(Math.abs(rootBox!.width - viewport.width)).toBeLessThanOrEqual(1);
      expect(Math.abs(rootBox!.height - viewport.height)).toBeLessThanOrEqual(1);

      await boxWithinViewport(page, "trade-console-header");
      await boxWithinViewport(page, "positions-panel");
      await boxWithinViewport(page, "orderbook-panel");
      await boxWithinViewport(page, "ticket-panel");
      await boxWithinViewport(page, "messages-panel");

      const overflow = await page.evaluate(() => ({
        scrollHeight: document.documentElement.scrollHeight,
        scrollWidth: document.documentElement.scrollWidth,
      }));

      expect(overflow.scrollWidth).toBeLessThanOrEqual(viewport.width + 1);
      expect(overflow.scrollHeight).toBeLessThanOrEqual(viewport.height + 1);
    });
  });
}
