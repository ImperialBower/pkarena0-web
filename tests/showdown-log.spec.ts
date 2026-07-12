import { test, expect } from '@playwright/test';
import { startGame, waitForHumanTurn } from './helpers';

// Drive the hero as a pure call-down (never fold) so hands reach showdown,
// then assert the persistent hand log records winners and reveals hands.
//
// Note: the game loop runs in real time (BOT_ACTION_MS / HAND_COMPLETE_MS
// pauses between actions and hands). We select the Turbo speed preset so a
// multi-hand call-down completes well inside the (raised) test budget.
test('hand log records winners and reveals showdown hands', async ({ page }) => {
  test.setTimeout(120_000);

  await startGame(page, 0.42);            // fixed Math.random -> deterministic
  await page.click('#log-toggle');        // open the (collapsed) hand-log aside

  // Turbo (10×): bot 75ms / hand 400ms — the fastest preset (slider max).
  await page.$eval('#speed-slider', (el: HTMLInputElement) => {
    el.value = '10';
    el.dispatchEvent(new Event('input', { bubbles: true }));
  });

  // Play up to N hero decisions, always Check or Call, never Fold.
  for (let i = 0; i < 60; i++) {
    try {
      await waitForHumanTurn(page);
    } catch {
      break; // session over or no more turns
    }
    const check = page.locator('#action-buttons button:has-text("Check")');
    const call = page.locator('#action-buttons button:has-text("Call")');
    if (await check.count()) {
      await check.first().click();
    } else if (await call.count()) {
      await call.first().click();
    } else {
      break; // only Fold/Bet available in an odd spot; stop driving
    }
  }

  const log = (await page.locator('#hand-log').textContent()) ?? '';

  // The log must now record hand winners (old code never did).
  expect(log).toMatch(/wins \$[\d,]+/);
  // A genuine showdown must produce a winner-first marked reveal line.
  expect(log).toMatch(/★ .+: .+ — wins \$[\d,]+/);
});
