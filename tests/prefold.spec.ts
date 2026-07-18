import { test, expect } from '@playwright/test';
import { startGame, waitForBoot } from './helpers';

test.describe('prefold decision logic', () => {
  test('folds when facing a bet, checks when free, else null', async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);

    const decide = (legal: string[]) =>
      page.evaluate((l) => (window as any).__PK0__.prefoldDecision(l), legal);

    // Facing a bet: Fold is offered → fold.
    expect(await decide(['Fold', 'Call', 'Raise', 'AllIn'])).toBe('Fold');
    // No bet facing hero: Check is offered, no Fold → check.
    expect(await decide(['Check', 'Bet', 'AllIn'])).toBe('Check');
    // Nothing to do (e.g. hero already all-in): null.
    expect(await decide([])).toBe(null);
    // Fold takes precedence over Check if both somehow present.
    expect(await decide(['Fold', 'Check'])).toBe('Fold');
  });
});

// Drive the hand passively until the prefold toggle is visible. Returns true
// once found, false if it never appeared within `maxHeroActions` decisions.
async function reachPrefoldWindow(page, maxHeroActions = 12): Promise<boolean> {
  for (let i = 0; i < maxHeroActions; i++) {
    const found = await page
      .waitForSelector('[data-act="prefold"]', { timeout: 8000, state: 'visible' })
      .then(() => true)
      .catch(() => false);
    if (found) return true;
    // No window yet — if it's the hero's turn, pass passively and try the next street.
    const heroTurn = await page
      .waitForFunction(() => {
        const btns = document.querySelectorAll('#action-buttons button');
        return [...btns].some((b: any) => !b.disabled && b.id !== 'btn-new-game');
      }, { timeout: 8000 })
      .then(() => true)
      .catch(() => false);
    if (!heroTurn) return false;
    const check = page.locator('#action-buttons button[data-act="check"]');
    const call = page.locator('#action-buttons button[data-act="call"]');
    if (await check.count()) await check.first().click();
    else if (await call.count()) await call.first().click();
    else break; // only fold/raise available — passing would change the test's intent
  }
  return false;
}

test.describe('prefold toggle', () => {
  test('appears while bots act and arms/disarms on click', async ({ page }) => {
    await startGame(page);
    await reachPrefoldWindow(page); // may act the hero forward a street or two

    const toggle = page.locator('[data-act="prefold"]');
    await expect(toggle).toBeVisible();
    await expect(toggle).not.toHaveClass(/armed/);

    await toggle.click();
    await expect(toggle).toHaveClass(/armed/);
    expect(await page.evaluate(() => (window as any).__PK0__.play.getPrefold())).toBe(true);

    await toggle.click();
    await expect(toggle).not.toHaveClass(/armed/);
    expect(await page.evaluate(() => (window as any).__PK0__.play.getPrefold())).toBe(false);
  });
});
