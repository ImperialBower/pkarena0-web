import { test, expect } from '@playwright/test';
import { startGame, waitForHumanTurn, waitForBoot } from './helpers';

test.describe('design2.0 table', () => {
  test('felt is present', async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);
    await expect(page.locator('#table-zone .felt')).toBeVisible();
  });

  test('9 seats are present', async ({ page }) => {
    await page.goto('/');
    for (let i = 0; i < 9; i++) {
      await expect(page.locator(`#table-zone [data-seat="${i}"]`)).toBeAttached();
    }
  });

  test('5 board card slots are present', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#table-zone .board-slot')).toHaveCount(5);
  });

  test('Pot pill is present', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#table-zone .pot')).toBeAttached();
  });
});

test.describe('UI after game starts', () => {
  test('Hero seat name shows "YOU"', async ({ page }) => {
    await startGame(page);
    await waitForHumanTurn(page);
    const name = await page.textContent('#table-zone [data-seat="0"] .seat-name');
    expect(name?.toUpperCase()).toContain('YOU');
  });

  test('Bot seats have names after deal', async ({ page }) => {
    await startGame(page);
    await waitForHumanTurn(page);
    let found = false;
    for (let i = 1; i < 9; i++) {
      const name = await page.textContent(`#table-zone [data-seat="${i}"] .seat-name`);
      if (name && name.trim().length > 0 && name.trim() !== '—') { found = true; break; }
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
    expect(await page.textContent('#sc-hand')).toBe('1');
    expect(await page.textContent('#sc-chips')).toMatch(/\$[\d,]+/);
  });
});
