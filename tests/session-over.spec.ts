import { test, expect, type Page } from '@playwright/test';
import { startGame } from './helpers';

test.use({ viewport: { width: 390, height: 844 } });   // iPhone 14 portrait

/**
 * Shove every time it is the hero's turn until the stack is gone. Clicks via
 * `evaluate` rather than Playwright: the All-In button is `display:none` on
 * mobile (it folds into the raise strip), so a normal click would not resolve.
 */
async function bustOut(page: Page): Promise<void> {
  await page.evaluate(() => (window as any).__PK0__.setInstant());
  for (let i = 0; i < 400; i++) {
    const phase = await page.evaluate(() => (window as any).__PK0__.play.getState().phase);
    if (phase === 'SessionOver') return;
    const shoved = await page.evaluate(() => {
      const b = document.querySelector<HTMLButtonElement>('#action-buttons button[data-act="allin"]');
      if (!b) return false;
      b.click();
      return true;
    });
    if (!shoved) await page.waitForTimeout(40);
  }
  throw new Error('hero never busted out');
}

test.describe('session over on mobile', () => {
  test('offers a New Game control in the dock', async ({ page }) => {
    await startGame(page);
    await bustOut(page);
    const btn = page.locator('#action-buttons button', { hasText: 'New Game' });
    await expect(btn).toBeVisible();
  });

  /**
   * The dock sits at the very bottom of a `height: 100dvh; overflow: hidden`
   * page, so on a real phone it is the first thing the browser's own chrome
   * covers — and there is nothing to scroll. The top bar is never covered, so
   * the top-bar control is the reachable fallback. It is hidden on mobile
   * during play (design README §Mobile) and must come back when the session
   * ends and it is the only thing left to do.
   */
  test('reveals the top-bar New Table control as a reachable fallback', async ({ page }) => {
    await startGame(page);
    await bustOut(page);
    const top = page.locator('#new-table-btn');
    await expect(top).toBeVisible();
    await expect(top).toBeEnabled();
  });

  test('the top-bar fallback actually starts a fresh session', async ({ page }) => {
    await startGame(page);
    await bustOut(page);
    await page.click('#new-table-btn');
    await page.waitForFunction(() => {
      const s = (window as any).__PK0__.play.getState();
      return s.phase !== 'SessionOver' && (s.hero?.chips ?? 0) > 0;
    }, { timeout: 15_000 });
    // Back to a live game, and the fallback hides itself again.
    await expect(page.locator('#new-table-btn')).toBeHidden();
  });

  /** The 390px top bar is already full; the fallback must not push anything
   *  off the right edge. */
  test('the top bar still fits once the fallback appears', async ({ page }) => {
    await startGame(page);
    await bustOut(page);
    const overflow = await page.evaluate(() => {
      const bar = document.getElementById('topbar')!;
      const right = Math.max(...[...bar.children]
        .map(c => c.getBoundingClientRect().right));
      return Math.round(right - bar.getBoundingClientRect().right);
    });
    expect(overflow).toBeLessThanOrEqual(0);
  });

  /**
   * `100vh` on mobile means the *large* viewport — the size with the browser
   * chrome hidden. With `overflow: hidden` there is no scrolling, so any part
   * of the layout past the visible height is unreachable. `dvh` tracks the
   * chrome instead. Headless Chromium has no chrome, so this asserts the
   * declaration rather than the rendered outcome.
   */
  test('the app shell is sized in dynamic viewport units', async ({ page }) => {
    await page.goto('/');
    const css = await page.evaluate(async () => {
      const res = await fetch('css/layout.css');
      return res.text();
    });
    expect(css).toContain('100dvh');
  });
});
