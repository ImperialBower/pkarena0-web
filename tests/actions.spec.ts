import { test, expect } from '@playwright/test';

// EPIC-56 Phase 1: the action row's logic, lifted out of main.js into a pure
// function so the embed can render it into its own row. These three states
// pin what renderActionButtons produced before the lift (main.js:852-900 at
// ccb874e); the lift must not change a label, an amount, or an order.
async function buttonsFor(page: import('@playwright/test').Page, state: object) {
  return page.evaluate(async (s) => {
    const { buttonsFor } = await import('/js/actions.js');
    return buttonsFor(s);
  }, state);
}

test.describe('buttonsFor(state)', () => {
  test.beforeEach(async ({ page }) => { await page.goto('/'); });

  test('facing a bet: fold, call, min, pot, raise…, all-in', async ({ page }) => {
    const btns = await buttonsFor(page, {
      legal_actions: ['Fold', 'Call', 'Raise', 'AllIn'],
      to_call: 300, min_raise: 600, max_bet: 10000, pot: 1000, can_undo: false,
    });
    expect(btns).toEqual([
      { label: 'Fold', cls: 'danger', action: 'Fold' },
      { label: 'Call $300', cls: '', action: 'Call' },
      { label: 'Min $600', cls: 'primary', action: 'bet-direct', betAction: 'Raise', amount: 600 },
      // ½ pot is $500, below the min raise, so it is not offered.
      { label: 'Pot $1,000', cls: 'primary', action: 'bet-direct', betAction: 'Raise', amount: 1000 },
      { label: 'Raise…', cls: 'primary', action: 'bet-open', betAction: 'Raise' },
      { label: 'All-In $10,000', cls: 'danger', action: 'allin', amount: 10000 },
    ]);
  });

  test('option to check: check, min, pot, bet…, all-in', async ({ page }) => {
    const btns = await buttonsFor(page, {
      legal_actions: ['Check', 'Bet', 'AllIn'],
      to_call: 0, min_raise: 100, max_bet: 9900, pot: 150, can_undo: false,
    });
    expect(btns).toEqual([
      { label: 'Check', cls: 'safe', action: 'Check' },
      { label: 'Min $100', cls: 'primary', action: 'bet-direct', betAction: 'Bet', amount: 100 },
      { label: 'Pot $150', cls: 'primary', action: 'bet-direct', betAction: 'Bet', amount: 150 },
      { label: 'Bet…', cls: 'primary', action: 'bet-open', betAction: 'Bet' },
      { label: 'All-In $9,900', cls: 'danger', action: 'allin', amount: 9900 },
    ]);
  });

  test('short stack with undo: the call is clamped and marked all-in', async ({ page }) => {
    const btns = await buttonsFor(page, {
      legal_actions: ['Fold', 'Call', 'AllIn'],
      to_call: 500, min_raise: 1000, max_bet: 200, pot: 1200, can_undo: true,
    });
    expect(btns).toEqual([
      { label: 'Fold', cls: 'danger', action: 'Fold' },
      { label: 'Call $200 (all-in)', cls: '', action: 'Call' },
      { label: 'All-In $200', cls: 'danger', action: 'allin', amount: 200 },
      { label: '↶ Undo', cls: '', action: 'undo' },
    ]);
  });

  test('no legal actions is an empty row, not a crash', async ({ page }) => {
    expect(await buttonsFor(page, {})).toEqual([]);
  });
});
