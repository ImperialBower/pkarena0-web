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

// EPIC-47 Phase 3: the adaptive-bots toggle defaults on and persists its state
// to localStorage across reloads (the value is pushed to the WASM decider pool
// via set_adaptive on the next New Game / Start Arena).
test('adaptive-bots toggle defaults on and persists when turned off', async ({ page }) => {
  await page.goto('/');
  await waitForBoot(page);

  await page.click('#settings-btn');
  const toggle = page.locator('#adaptive-toggle');
  await expect(toggle).toBeChecked();

  // The native checkbox is visually hidden behind a styled track; click the
  // wrapping label like a real user to fire the change handler.
  await toggle.locator('xpath=ancestor::label[1]').click();
  await expect(toggle).not.toBeChecked();
  expect(await page.evaluate(() => localStorage.getItem('adaptiveEnabled'))).toBe('false');

  // Survives a reload.
  await page.reload();
  await waitForBoot(page);
  await page.click('#settings-btn');
  await expect(page.locator('#adaptive-toggle')).not.toBeChecked();
});
