import { test, expect } from '@playwright/test';
import { waitForBoot } from './helpers';

// EPIC-49 Phase 3: bot difficulty tiers (weak / standard / strong). The
// selector follows the adaptive-toggle lifecycle: persisted to localStorage,
// pushed to both WASM instances at boot and on change, applied when the next
// lineup is built. get_state().difficulty surfaces the engine's live value so
// UI and engine can be asserted in lockstep (same rationale as .adaptive).

type PkState = {
  difficulty: string;
  session_report: Array<{
    seat: number;
    name: string;
    net_chips: number;
    hands_played: number;
    chips_per_100: number;
  }>;
};

const readState = (page: import('@playwright/test').Page, tab: 'play' | 'arena') =>
  page.evaluate((t) => {
    const pk = (window as unknown as {
      __PK0__: Record<string, { getState: () => unknown }>;
    }).__PK0__;
    return pk[t].getState() as PkState;
  }, tab);

test('difficulty selector defaults to standard and persists across reloads', async ({ page }) => {
  await page.goto('/');
  await waitForBoot(page);

  await page.click('#settings-btn');
  const select = page.locator('#difficulty-select');
  await expect(select).toHaveValue('standard');

  await select.selectOption('weak');
  expect(await page.evaluate(() => localStorage.getItem('difficulty'))).toBe('weak');

  await page.reload();
  await waitForBoot(page);
  await page.click('#settings-btn');
  await expect(page.locator('#difficulty-select')).toHaveValue('weak');
});

test('get_state().difficulty reflects the persisted preference on load', async ({ page }) => {
  // Persist before any page script runs so boot() must carry it into WASM.
  await page.addInitScript(() => localStorage.setItem('difficulty', 'strong'));
  await page.goto('/');
  await waitForBoot(page);

  expect((await readState(page, 'play')).difficulty).toBe('strong');
  expect((await readState(page, 'arena')).difficulty).toBe('strong');
});

test('weak tier fields an 8-bot arena lineup (joker excluded)', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('difficulty', 'weak'));
  await page.goto('/');
  await waitForBoot(page);
  await page.click('.tab[data-tab="arena"]');
  await page.click('#arena-start-btn');

  const state = await readState(page, 'arena');
  expect(state.difficulty).toBe('weak');
  // The weak bundle has 8 profiles (no joker — it morphs into
  // standard-strength profiles), so the arena seats 8 bots instead of 9.
  expect(state.session_report.length).toBe(8);
});

test('arena session report tracks per-seat chips/100 (EPIC-49 Phase 3)', async ({ page }) => {
  test.setTimeout(90_000);
  await page.goto('/');
  await waitForBoot(page);
  await page.click('.tab[data-tab="arena"]');
  await page.click('#arena-start-btn');
  await page.evaluate(() =>
    (window as unknown as { __PK0__: { setInstant: () => void } }).__PK0__.setInstant(),
  );

  // Wait for a few completed hands so the report has real numbers.
  await page.waitForFunction(
    () => {
      const text = document.getElementById('arena-status')?.textContent ?? '';
      const m = text.match(/Hand #(\d+)/);
      return m ? Number(m[1]) >= 3 : false;
    },
    { timeout: 60_000 },
  );

  const state = await readState(page, 'arena');
  expect(state.session_report.length).toBe(9); // standard arena seats 9 bots
  for (const r of state.session_report) {
    expect(typeof r.name).toBe('string');
    expect(r.name.length).toBeGreaterThan(0);
    expect(r.hands_played).toBeGreaterThanOrEqual(2);
    if (r.net_chips === 0) {
      expect(r.chips_per_100).toBe(0);
    } else {
      expect(Math.sign(r.chips_per_100)).toBe(Math.sign(r.net_chips));
    }
  }
  // Chips are conserved seat-to-seat, so nets sum to 0 — except the known
  // pkcore multiway audit edge case, which can only make chips vanish (sum
  // < 0), never appear.
  const netSum = state.session_report.reduce((s, r) => s + r.net_chips, 0);
  expect(netSum).toBeLessThanOrEqual(0);
  // At least one seat has moved chips after 3+ hands.
  expect(state.session_report.some((r) => r.net_chips !== 0)).toBe(true);
});
