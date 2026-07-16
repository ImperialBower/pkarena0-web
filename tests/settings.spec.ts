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

// EPIC-47 regression: the persisted preference must reach the WASM *engine* on
// load, not just the checkbox. A boot-order bug once left the engine on its
// default (adaptive ON) while the UI showed OFF, silently, because nothing
// surfaced the engine's flag. get_state().adaptive now exposes it so the two
// can be asserted in lockstep. The toggle-only test above cannot catch that
// divergence — this one reads the engine directly through both tab instances.
test('get_state().adaptive reflects the persisted "off" preference on load', async ({ page }) => {
  // Persist "off" before any page script runs, then load fresh so boot() must
  // carry the preference into WASM itself.
  await page.addInitScript(() => localStorage.setItem('adaptiveEnabled', 'false'));
  await page.goto('/');
  await waitForBoot(page);

  const readAdaptive = (tab: 'play' | 'arena') =>
    page.evaluate((t) => {
      const pk = (window as unknown as {
        __PK0__: Record<string, { getState: () => { adaptive: boolean } }>;
      }).__PK0__;
      return pk[t].getState().adaptive;
    }, tab);

  expect(await readAdaptive('play')).toBe(false);
  expect(await readAdaptive('arena')).toBe(false);
});

// Complementary happy-path: with no stored preference the engine defaults on,
// matching the WASM-side ADAPTIVE default and the checkbox.
test('get_state().adaptive defaults to true with no stored preference', async ({ page }) => {
  await page.goto('/');
  await waitForBoot(page);
  const adaptive = await page.evaluate(() =>
    (window as unknown as {
      __PK0__: { play: { getState: () => { adaptive: boolean } } };
    }).__PK0__.play.getState().adaptive,
  );
  expect(adaptive).toBe(true);
});
