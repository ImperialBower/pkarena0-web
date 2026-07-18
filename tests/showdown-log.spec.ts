import { test, expect } from '@playwright/test';
import { startGame, waitForHumanTurn } from './helpers';

// Drive the hero as a pure call-down (never fold) so hands reach showdown,
// then assert the persistent hand log records winners and reveals hands.
//
// The game loop runs in real time (BOT_ACTION_MS / HAND_COMPLETE_MS pauses
// between actions and hands). setInstant() zeroes those so a multi-hand
// call-down — hundreds of bot actions across a variable, unseeded number of
// hands — plays at CPU speed instead of flaking near the timeout under parallel
// load. (See project memory: playwright game-speed.)
test('hand log records winners and reveals showdown hands', async ({ page }) => {
  test.setTimeout(120_000);

  await startGame(page, 0.42);            // stubs Math.random (bot decisions; the deal is NOT seeded)
  await page.click('#log-toggle');        // open the (collapsed) hand-log aside

  await page.evaluate(() =>
    (window as unknown as { __PK0__: { setInstant: () => void } }).__PK0__.setInstant(),
  );

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
