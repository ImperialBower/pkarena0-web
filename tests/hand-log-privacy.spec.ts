import { test, expect } from '@playwright/test';
import { startGame, waitForHumanTurn } from './helpers';

// PLAY mode must not reveal opponents' hole cards in the running action log.
// Bot action lines (name: folds/raises/…) carry NO cards; hole cards appear
// only in the showdown reveal block (★ / indented reveal lines) and on the
// hero's own action lines. Regression guard for the "discarded hands shown
// during play" bug.
test('bot action lines never reveal hole cards during play', async ({ page }) => {
  test.setTimeout(120_000);
  await startGame(page, 0.42);          // deterministic seed → reaches showdowns
  await page.click('#log-toggle');
  // page.$eval runs the fn against the DOM element (Playwright API), not JS eval().
  await page.$eval('#speed-slider', (el: HTMLInputElement) => {
    el.value = '10';                    // Turbo, so a multi-hand call-down fits the budget
    el.dispatchEvent(new Event('input', { bubbles: true }));
  });

  // Call-down: hero never folds, so hands reach showdown while bots fold/raise.
  for (let i = 0; i < 60; i++) {
    try { await waitForHumanTurn(page); } catch { break; }
    const check = page.locator('#action-buttons button:has-text("Check")');
    const call = page.locator('#action-buttons button:has-text("Call")');
    if (await check.count()) await check.first().click();
    else if (await call.count()) await call.first().click();
    else break;
  }

  const lines = await page.locator('#hand-log p').allTextContents();

  // A "bot action line" is one of the running action entries (folds/checks/
  // calls/raises/bets/all-in) that is NOT the hero's own line ("You …"), NOT a
  // showdown-reveal line (starts with ★), and NOT an indented reveal loser line
  // (starts with two spaces).
  const ACTION = /: (folds|checks|calls|raises|bets|is all-in|all-in)/;
  const isBotAction = (l: string) =>
    ACTION.test(l) && !l.startsWith('You ') && !l.startsWith('★') && !l.startsWith('  ');

  const botActionLines = lines.filter(isBotAction);

  // Test isn't vacuous: the call-down must have produced bot action lines.
  expect(botActionLines.length).toBeGreaterThan(0);

  // The bug: bot action lines leaked cards like "gto [K♥ 9♦]: folds".
  for (const l of botActionLines) {
    expect(l, `bot action line leaked hole cards: "${l}"`).not.toContain('[');
  }

  // Guard against over-hiding: the showdown reveal must still show cards.
  expect(lines.some(l => /★ .+\[.+\].+wins \$/.test(l))).toBe(true);
});
