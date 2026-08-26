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
    // "Done" is either terminal state: the whole table finished (SessionOver,
    // funded < 2), or just the hero's seat finished while bots play on. The
    // second is the ordinary case at a 9-handed table — see heroIsEliminated
    // in main.js. Waiting only for SessionOver used to work by accident: the
    // bot loop ran unattended until the bots busted each other out.
    const done = await page.evaluate(() => {
      const s = (window as any).__PK0__.play.getState();
      return s.phase === 'SessionOver'
          || (s.hero?.state === 'Out' && s.hero?.chips === 0 && (s.hand_number ?? 0) >= 1);
    });
    if (done) return;
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

/**
 * A GameState in which the hero has been eliminated but the table has not
 * ended. `init_game` seats hero + 8 bots, so this is the *ordinary* way a
 * session ends for the player — `SessionOver` needs `funded < 2`
 * (src/lib.rs:748) and eight bots are still funded.
 */
function bustedHeroState(handNumber = 23) {
  return {
    hand_number: handNumber,
    phase: 'BotsActing',
    street: 'Preflop',
    pot: 0,
    board: [],
    hero: { seat: 0, name: '', chips: 0, bet: 0, state: 'Out', hole_cards: null },
    players: [],
    legal_actions: [],
    to_call: 0, min_raise: 0, max_bet: 0,
    dealer_seat: 1, sb_seat: 2, bb_seat: 3,
    small_blind: 50, big_blind: 100,
    session_over: false, can_undo: false, forced_fold_count: 0,
  };
}

test.describe('hero busts but the table plays on', () => {
  test('the dock offers New Game', async ({ page }) => {
    await startGame(page);
    await page.evaluate(s => (window as any).__PK0__.play.render(s), bustedHeroState());
    await expect(page.locator('#action-buttons button', { hasText: 'New Game' })).toBeVisible();
  });

  test('the top-bar New Table control is reachable', async ({ page }) => {
    await startGame(page);
    await page.evaluate(s => (window as any).__PK0__.play.render(s), bustedHeroState());
    await expect(page.locator('body')).toHaveClass(/session-over/);
    const top = page.locator('#new-table-btn');
    await expect(top).toBeVisible();
    await expect(top).toBeEnabled();
  });

  test('an all-in hero is not offered an exit mid-pot', async ({ page }) => {
    await startGame(page);
    const allIn = { ...bustedHeroState(), hero: { ...bustedHeroState().hero, state: 'AllIn' } };
    await page.evaluate(s => (window as any).__PK0__.play.render(s), allIn);
    await expect(page.locator('body')).not.toHaveClass(/session-over/);
  });

  /**
   * The three tests above drive `renderState` with a hand-authored state. This
   * one busts for real: shove until the engine reports hero chips 0 / state
   * 'Out'. The deal is not seeded, so the hand it happens on varies — but it
   * always happens, and always with bots still funded.
   */
  test('a real bust surfaces both exits without waiting for the bots', async ({ page }) => {
    await startGame(page);
    await page.evaluate(() => (window as any).__PK0__.setInstant());
    for (let i = 0; i < 400; i++) {
      const s = await page.evaluate(() => (window as any).__PK0__.play.getState());
      if (s.hero?.state === 'Out' && s.hero?.chips === 0) {
        // Bots must still be in the game — otherwise this is plain SessionOver
        // and proves nothing about the busted-hero branch.
        expect(s.phase).not.toBe('SessionOver');
        break;
      }
      const shoved = await page.evaluate(() => {
        const b = document.querySelector<HTMLButtonElement>('#action-buttons button[data-act="allin"]');
        if (!b) return false;
        b.click();
        return true;
      });
      if (!shoved) await page.waitForTimeout(40);
    }
    await expect(page.locator('body')).toHaveClass(/session-over/);
    await expect(page.locator('#new-table-btn')).toBeVisible();
    await expect(page.locator('#new-table-btn')).toBeEnabled();
    await expect(page.locator('#action-buttons button', { hasText: 'New Game' })).toBeVisible();
  });

  test('a booting table does not read as busted', async ({ page }) => {
    await page.goto('/');
    await page.waitForSelector('#btn-new-game:not([disabled])', { timeout: 15_000 });
    // empty_player_view reports state 'Out' / chips 0 before any hand exists.
    await expect(page.locator('body')).not.toHaveClass(/session-over/);
  });
});

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
