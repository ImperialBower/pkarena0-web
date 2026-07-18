import { test, expect } from '@playwright/test';
import { waitForBoot } from './helpers';

test.describe('prefold decision logic', () => {
  test('folds when facing a bet, checks when free, else null', async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);

    const decide = (legal: string[]) =>
      page.evaluate((l) => (window as any).__PK0__.prefoldDecision(l), legal);

    // Facing a bet: Fold is offered → fold.
    expect(await decide(['Fold', 'Call', 'Raise', 'AllIn'])).toBe('Fold');
    // No bet facing hero: Check is offered, no Fold → check.
    expect(await decide(['Check', 'Bet', 'AllIn'])).toBe('Check');
    // Nothing to do (e.g. hero already all-in): null.
    expect(await decide([])).toBe(null);
    // Fold takes precedence over Check if both somehow present.
    expect(await decide(['Fold', 'Check'])).toBe('Fold');
  });
});
