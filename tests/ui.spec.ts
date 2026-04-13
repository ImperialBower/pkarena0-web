import { test, expect } from '@playwright/test';
import { startGame, waitForHumanTurn, waitForBoot } from './helpers';

test.describe('SVG table', () => {
  test('Poker table SVG is present', async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);
    const svg = page.locator('#poker-table');
    await expect(svg).toBeVisible();
  });

  test('9 seat groups are present in the SVG', async ({ page }) => {
    await page.goto('/');
    for (let i = 0; i < 9; i++) {
      await expect(page.locator('#seat-' + i + '-group')).toBeAttached();
    }
  });

  test('5 board card slots are present', async ({ page }) => {
    await page.goto('/');
    for (let i = 0; i < 5; i++) {
      await expect(page.locator('#board-card-' + i)).toBeAttached();
    }
  });

  test('Pot amount text is present', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#pot-amount')).toBeAttached();
  });
});

test.describe('UI after game starts', () => {
  test('Hero seat name shows "YOU"', async ({ page }) => {
    await startGame(page);
    await waitForHumanTurn(page);

    const name = await page.textContent('#seat-0-name');
    expect(name?.toUpperCase()).toContain('YOU');
  });

  test('Bot seats have names after deal', async ({ page }) => {
    await startGame(page);
    await waitForHumanTurn(page);

    // At least one bot seat should have a non-empty name.
    let found = false;
    for (let i = 1; i < 9; i++) {
      const name = await page.textContent('#seat-' + i + '-name');
      if (name && name.trim().length > 0 && name.trim() !== '—') {
        found = true;
        break;
      }
    }
    expect(found).toBe(true);
  });

  test('Action buttons are enabled on human turn', async ({ page }) => {
    await startGame(page);
    await waitForHumanTurn(page);

    const enabledBtns = await page.locator(
      '#action-buttons button:not([disabled]):not(#btn-new-game)'
    ).count();
    expect(enabledBtns).toBeGreaterThan(0);
  });

  test('Score bar shows hand #1 and chip amount', async ({ page }) => {
    await startGame(page);
    await waitForHumanTurn(page);

    const hand = await page.textContent('#sc-hand');
    expect(hand).toBe('1');

    const chips = await page.textContent('#sc-chips');
    expect(chips).toMatch(/\$[\d,]+/);
  });
});
