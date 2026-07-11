import { test, expect } from '@playwright/test';
import { startGame, waitForHumanTurn, waitForYamlReady } from './helpers';

test('replay overlay renders a table for the completed hand', async ({ page }) => {
  await startGame(page, 0.42);
  await waitForHumanTurn(page);
  await page.locator('#action-buttons button:has-text("Fold")').click();
  await waitForYamlReady(page);
  await page.waitForSelector('#btn-replay:not([disabled])', { timeout: 30_000 });
  await page.click('#btn-replay');
  await expect(page.locator('#replay-overlay')).toBeVisible();
  await page.click('#replay-load-session');
  await page.waitForSelector('#replay-slider:not([disabled])', { timeout: 15_000 });
  await expect(page.locator('#replay-table-wrapper [data-seat]')).toHaveCount(9);
  await expect(page.locator('#replay-table-wrapper .pot')).toBeAttached();
});
