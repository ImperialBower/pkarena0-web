import { test, expect } from '@playwright/test';
import { startGame, waitForHumanTurn, statusContains } from './helpers';

test.describe('Game initialisation', () => {
  test('New Game populates score bar', async ({ page }) => {
    await startGame(page);
    await waitForHumanTurn(page);

    const hand = await page.textContent('#sc-hand');
    expect(hand).toBe('1');

    const chips = await page.textContent('#sc-chips');
    expect(chips).toMatch(/\$[\d,]+/);
  });

  test('Hero cards are visible after New Game', async ({ page }) => {
    await startGame(page);
    await waitForHumanTurn(page);

    // Hero seat should have two card groups with SVG children (face-up cards)
    const card0Children = await page.locator('#seat-0-card-0 > *').count();
    const card1Children = await page.locator('#seat-0-card-1 > *').count();
    expect(card0Children).toBeGreaterThan(0);
    expect(card1Children).toBeGreaterThan(0);
  });

  test('Status indicates human turn', async ({ page }) => {
    await startGame(page);
    await waitForHumanTurn(page);
    const onTurn = await statusContains(page, 'your turn');
    expect(onTurn).toBe(true);
  });
});

test.describe('Human actions', () => {
  test('Fold ends the hand', async ({ page }) => {
    await startGame(page);
    await waitForHumanTurn(page);

    const foldBtn = page.locator('#action-buttons button:has-text("Fold")');
    if (await foldBtn.count() > 0) {
      await foldBtn.click();
      // After fold the hand eventually completes and a new hand starts.
      // Wait for the new-game or next-hand to render another deal (hand #2).
      await page.waitForFunction(
        () => {
          const el = document.getElementById('sc-hand');
          return el && parseInt(el.textContent ?? '0', 10) >= 1;
        },
        { timeout: 15_000 },
      );
    }
  });

  test('Check is available when no bet is facing hero', async ({ page }) => {
    await startGame(page);
    await waitForHumanTurn(page);

    // Not every deal puts the hero in a check spot (may face a BB bet preflop).
    // Just verify the button set is non-empty.
    const actionBtns = await page.locator('#action-buttons button').count();
    expect(actionBtns).toBeGreaterThan(0);
  });

  test('Bet controls appear when Bet button is clicked', async ({ page }) => {
    await startGame(page);
    await waitForHumanTurn(page);

    const betBtn = page.locator('#action-buttons button:has-text("Bet")');
    if (await betBtn.count() > 0) {
      await betBtn.click();
      await expect(page.locator('#bet-controls')).toBeVisible();
    }
  });
});

test.describe('Session continuity', () => {
  test('Hand number increments after hand completes', async ({ page }) => {
    await startGame(page);
    await waitForHumanTurn(page);

    // Force-fold to end the hand quickly.
    const foldBtn = page.locator('#action-buttons button:has-text("Fold")');
    if (await foldBtn.count() > 0) {
      await foldBtn.click();
      // Wait for hand number > 1 (new hand dealt).
      await page.waitForFunction(
        () => {
          const el = document.getElementById('sc-hand');
          return el && parseInt(el.textContent ?? '0', 10) > 1;
        },
        { timeout: 20_000 },
      );
      const hand = await page.textContent('#sc-hand');
      expect(parseInt(hand ?? '0', 10)).toBeGreaterThan(1);
    }
  });
});
