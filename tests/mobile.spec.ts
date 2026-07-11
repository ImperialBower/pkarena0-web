import { test, expect } from '@playwright/test';
import { startGame, waitForHumanTurn, waitForBoot } from './helpers';

test.use({ viewport: { width: 390, height: 844 } });

test.describe('mobile mini-table', () => {
  test('top bar compacts to 48px and felt renders', async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);
    const h = await page.evaluate(() =>
      Math.round(document.getElementById('topbar').getBoundingClientRect().height));
    expect(h).toBe(48);
    await expect(page.locator('#table-zone .felt')).toBeVisible();
  });

  test('board slots shrink to the mobile mini size', async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);
    const w = await page.evaluate(() =>
      Math.round(document.querySelector('#table-zone .board-slot').getBoundingClientRect().width));
    expect(w).toBe(26);
  });

  test('seats use mobile coordinates (hero seat re-anchored)', async ({ page }) => {
    await startGame(page);
    await waitForHumanTurn(page);
    // Desktop hero x=71%; mobile mx=50%. At 390px the seat's resolved left must
    // be ~50% of the table-zone width, not ~71%.
    const ratio = await page.evaluate(() => {
      const seat = document.querySelector('#table-zone [data-seat="0"]');
      const zone = document.getElementById('table-zone');
      return seat.getBoundingClientRect().left / zone.getBoundingClientRect().width;
    });
    expect(ratio).toBeLessThan(0.6); // 50%, not 71%
  });
});

test.describe('mobile seat-list', () => {
  test('view toggle switches to the seat list and persists', async ({ page }) => {
    await startGame(page);
    await waitForHumanTurn(page);
    // Default = table mode: mini-table visible, list hidden.
    await expect(page.locator('#table-zone .felt')).toBeVisible();
    await expect(page.locator('#table-zone .seat-list')).toBeHidden();

    await page.click('#view-toggle');
    expect(await page.evaluate(() => document.body.classList.contains('seats-list'))).toBe(true);
    await expect(page.locator('#table-zone .seat-list')).toBeVisible();
    await expect(page.locator('#table-zone .felt')).toBeHidden();
    await expect(page.locator('#table-zone .list-row')).toHaveCount(9);
    await expect(page.locator('#table-zone .board-strip')).toBeVisible();

    await page.reload();
    await waitForBoot(page);  // no active game after reload; just wait for the module to init
    expect(await page.evaluate(() => document.body.classList.contains('seats-list'))).toBe(true);
  });
});

test.describe('mobile log drawer', () => {
  test('LOG opens a bottom sheet; backdrop tap closes it', async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);
    await expect(page.locator('#log-aside')).toBeHidden();
    await page.click('#log-toggle');
    await expect(page.locator('#log-aside')).toBeVisible();
    // Bottom sheet: taller than a third of the viewport, anchored to the bottom.
    const box = await page.locator('#log-aside').boundingBox();
    expect(box.height).toBeGreaterThan(844 * 0.5);
    await expect(page.locator('#log-backdrop')).toBeVisible();
    // Tap the visible scrim above the 62vh sheet (the backdrop's centre is
    // covered by the sheet, which sits at a higher z-index).
    await page.click('#log-backdrop', { position: { x: 8, y: 8 } });
    await expect(page.locator('#log-aside')).toBeHidden();
  });
});
