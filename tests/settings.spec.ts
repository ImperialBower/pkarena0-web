import { test, expect } from '@playwright/test';
import { waitForBoot } from './helpers';

test('settings has a "view game state" entry that opens the JSON overlay', async ({ page }) => {
  await page.goto('/');
  await waitForBoot(page);
  await page.click('#settings-btn');
  await expect(page.locator('#settings-overlay')).toBeVisible();
  await page.click('#settings-view-state');
  await expect(page.locator('#game-state-overlay')).toBeVisible();
});

test('HUD row is in the DOM but hidden by default (showHud=false)', async ({ page }) => {
  await page.goto('/');
  await waitForBoot(page);
  const hud = page.locator('#table-zone [data-seat="0"] .seat-hud');
  await expect(hud).toBeAttached();
  await expect(hud).toBeHidden();
});
