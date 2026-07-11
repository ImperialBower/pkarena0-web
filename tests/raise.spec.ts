import { test, expect } from '@playwright/test';
import { startGame, waitForHumanTurn } from './helpers';

test.describe('hero dock + raise strip', () => {
  test('hero sub-line shows pot odds and SPR on the human turn', async ({ page }) => {
    await startGame(page);
    await waitForHumanTurn(page);
    const title = await page.textContent('#hero-title');
    expect(title).toContain('You');
    const sub = await page.textContent('#hero-sub');
    expect(sub).toMatch(/POT ODDS/);
    expect(sub).toMatch(/SPR/);
  });

  test('opening the raise strip reveals slider + quick buttons; MIN sets the min-raise amount', async ({ page }) => {
    await startGame(page);
    await waitForHumanTurn(page);
    const raiseBtn = page.locator('#action-buttons button[data-act="raise"], #action-buttons button[data-act="bet"]');
    // Skip gracefully if this deal offers no raise/bet (rare: hero only legal to check/fold).
    if (await raiseBtn.count() === 0) test.skip();
    await raiseBtn.first().click();
    await expect(page.locator('#raise-strip')).toBeVisible();
    await expect(page.locator('#bet-slider')).toBeVisible();
    await page.click('#raise-min');
    const min = await page.evaluate(() =>
      Number(document.getElementById('bet-slider').min));
    const val = await page.evaluate(() =>
      Number(document.getElementById('bet-input').value));
    expect(val).toBe(min);
  });
});
