import { test, expect } from '@playwright/test';
import { waitForBoot } from './helpers';

// Arena mode is a 9-bot spectator mode (no human). Two properties must hold
// that differ from normal play:
//   1. Every bot's hole cards are face-up at all times (nothing to hide).
//   2. The hand log records each bot action and each hand's outcome.
// Both were broken because the arena code path (`runArena()` / `IS_ALL_BOT`)
// never mirrored the play path's reveal + logging behaviour.

/** Boot the app, switch to the Arena tab, and start an arena run. */
async function startArena(page: import('@playwright/test').Page): Promise<void> {
  await page.goto('/');
  await waitForBoot(page);
  await page.click('.tab[data-tab="arena"]');
  await page.click('#arena-start-btn');
}

/** Set the speed slider to a preset (1 = Very Slow … 10 = Turbo). */
async function setSpeed(page: import('@playwright/test').Page, value: number): Promise<void> {
  await page.$eval(
    '#speed-slider',
    (el: HTMLInputElement, v: number) => {
      el.value = String(v);
      el.dispatchEvent(new Event('input', { bubbles: true }));
    },
    value,
  );
}

test('arena shows every bot hole card face-up during play', async ({ page }) => {
  test.setTimeout(60_000);
  await startArena(page);

  // Default speed leaves ~1s between bot actions — a wide mid-hand window in
  // which opponents would be face-down under the old (play-mode) reveal rule.
  await page.waitForFunction(
    () => document.querySelectorAll('.seat-cards .card-rank').length > 0,
    { timeout: 20_000 },
  );

  // Sample across several bot actions: a face-down seat card (the "__" sentinel,
  // rendered as .card-down) must NEVER appear in arena mode.
  for (let i = 0; i < 20; i++) {
    expect(await page.locator('.seat-cards .card-down').count()).toBe(0);
    await page.waitForTimeout(400);
  }
});

test('arena writes bot actions and hand outcomes to the hand log', async ({ page }) => {
  test.setTimeout(90_000);
  await startArena(page);
  await page.click('#log-toggle');           // open the (collapsed) hand-log aside
  await setSpeed(page, 10);                   // Turbo so a hand completes quickly

  // Per-action lines must appear (e.g. "Name: raises to $300", "Name: folds").
  await page.waitForFunction(
    () => /(folds|checks|calls|bets|raises|all-in)/i.test(
      document.getElementById('hand-log')?.textContent ?? '',
    ),
    { timeout: 30_000 },
  );

  // At least one completed hand must record a winner in the log.
  await page.waitForFunction(
    () => /wins \$[\d,]+/.test(document.getElementById('hand-log')?.textContent ?? ''),
    { timeout: 60_000 },
  );
});
