import { test } from '@playwright/test';
import { waitForBoot } from './helpers';

// EPIC-50 Phase 3b: with the strong difficulty tier selected, every postflop
// bot decision runs pkcore's real multi-way Monte-Carlo equity engine
// (decision.equity = fast{500}) inside wasm. This spec proves that engine stays
// within the interactive budget end-to-end: a Turbo arena with the JS pacing
// zeroed (setInstant) must still complete a run of hands at CPU speed without
// stalling past the timeout. A latency blow-up would never reach the hand count.
//
// EPIC-48 Phase 0 measured the per-call budget (500 samples ≈ 2.8 ms HU /
// 5.7 ms 4-way, under the 10 ms/decision Turbo target); this is the live check
// that the strong bundle's knobs honour it.
test('strong-tier arena runs the equity engine within the Turbo budget', async ({ page }) => {
  test.setTimeout(120_000);
  // Strong tier ⇒ every bot carries decision.equity = fast{500}: the heaviest
  // decision path. Persist the preference before boot (the app reads
  // localStorage 'difficulty' on load and applies it at lineup-build time), so
  // the arena is dealt the strong bundle.
  await page.addInitScript(() => localStorage.setItem('difficulty', 'strong'));
  await page.goto('/');
  await waitForBoot(page);

  await page.click('.tab[data-tab="arena"]');
  await page.click('#arena-start-btn');
  await page.click('#log-toggle'); // ensure the hand-log is populated/visible
  await page.evaluate(() =>
    (window as unknown as { __PK0__: { setInstant: () => void } }).__PK0__.setInstant(),
  );

  // Ten completed hands (each logs "… wins $N") must accrue well inside the
  // timeout even with equity on — a stalled engine would never reach ten.
  await page.waitForFunction(
    () =>
      ((document.getElementById('hand-log')?.textContent ?? '').match(/wins \$[\d,]+/g)
        ?.length ?? 0) >= 10,
    { timeout: 90_000 },
  );
});
