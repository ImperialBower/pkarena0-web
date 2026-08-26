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

/**
 * Regression guards for the mobile-only fixes that landed across the recent
 * PRs (compact top bar, seat-list, log drawer, raise strip, session-over
 * fallback). Each of those changed CSS that only exists below 760px, so a
 * desktop-viewport spec cannot catch a regression in any of them.
 */
test.describe('mobile top bar chrome', () => {
  test('desktop-only controls are hidden and the view toggle is revealed', async ({ page }) => {
    await startGame(page);
    await waitForHumanTurn(page);
    // Design README §Mobile: logo · HAND · P&L · theme · view-toggle · LOG · ⚙.
    await expect(page.locator('#sc-version')).toBeHidden();
    await expect(page.locator('#tab-bar')).toBeHidden();
    await expect(page.locator('#deck-toggle')).toBeHidden();
    // The dock carries the actions during play, so the top-bar exit stays away.
    await expect(page.locator('#new-table-btn')).toBeHidden();
    await expect(page.locator('#view-toggle')).toBeVisible();
    await expect(page.locator('#log-toggle')).toBeVisible();
    await expect(page.locator('#theme-select')).toBeVisible();
  });

  test('only HAND and P&L survive in the stats strip', async ({ page }) => {
    await startGame(page);
    await waitForHumanTurn(page);
    const strip = page.locator('.topbar-stats');
    await expect(strip).toContainText('HAND');
    await expect(strip).toContainText('P&L');
    // BLINDS and STACK move to the dock on mobile.
    await expect(strip.locator('span.play-only').nth(0)).toBeHidden();
    await expect(strip.locator('span.play-only').nth(1)).toBeHidden();
  });

  /** The session-over spec asserts this for the fallback state. During play the
   *  bar carries a different set of controls, and it must fit too. */
  test('the top bar fits at 390px during play', async ({ page }) => {
    await startGame(page);
    await waitForHumanTurn(page);
    const overflow = await page.evaluate(() => {
      const bar = document.getElementById('topbar')!;
      const right = Math.max(...[...bar.children]
        .map(c => c.getBoundingClientRect().right));
      return Math.round(right - bar.getBoundingClientRect().right);
    });
    expect(overflow).toBeLessThanOrEqual(0);
  });
});

test.describe('mobile dock', () => {
  test('action buttons meet the 48px tap target', async ({ page }) => {
    await startGame(page);
    await waitForHumanTurn(page);
    const heights = await page.evaluate(() =>
      [...document.querySelectorAll('#action-buttons button')]
        .filter(b => (b as HTMLElement).offsetParent !== null)
        .map(b => Math.round(b.getBoundingClientRect().height)));
    expect(heights.length).toBeGreaterThan(0);
    for (const h of heights) expect(h).toBeGreaterThanOrEqual(48);
  });

  /**
   * `env(safe-area-inset-bottom)` resolves to 0 without `viewport-fit=cover`,
   * which would drop the dock's action row behind the iPhone home indicator.
   * Headless Chromium has no inset to measure, so assert the declaration.
   */
  test('the viewport meta opts into the safe-area insets', async ({ page }) => {
    await page.goto('/');
    const content = await page.getAttribute('meta[name="viewport"]', 'content');
    expect(content).toContain('viewport-fit=cover');
  });
});

test.describe('mobile raise strip', () => {
  test('MIN and ALL-IN leave the dock and the strip docks to the bottom edge', async ({ page }) => {
    await startGame(page);
    await waitForHumanTurn(page);
    const raiseBtn = page.locator('#action-buttons button[data-act="raise"], #action-buttons button[data-act="bet"]');
    if (await raiseBtn.count() === 0) test.skip();

    // On mobile these two fold into the strip's quick buttons instead of
    // taking dock width (layout.css §mobile). The session-over spec depends on
    // this: it has to click All-In through `evaluate`, not Playwright.
    await expect(page.locator('#action-buttons button[data-act="allin"]')).toBeHidden();
    await expect(page.locator('#action-buttons button[data-act="min"]')).toBeHidden();

    await raiseBtn.first().click();
    const strip = page.locator('#raise-strip');
    await expect(strip).toBeVisible();
    const box = (await strip.boundingBox())!;
    // Full-bleed sheet pinned to the bottom, not the centred desktop popover.
    expect(Math.round(box.x)).toBe(0);
    expect(Math.round(box.width)).toBe(390);
    expect(Math.round(box.y + box.height)).toBe(844);
    // Slider takes its own full-width row above the quick buttons.
    const slider = (await page.locator('#bet-slider').boundingBox())!;
    const quick = (await page.locator('.raise-quick').first().boundingBox())!;
    expect(slider.y + slider.height).toBeLessThanOrEqual(quick.y + 1);
  });
});
