import { test, expect } from '@playwright/test';
import { waitForBoot } from './helpers';

// EPIC-47 Phase 4: per-seat opponent-model HUD (VPIP/PFR/AF). The badge is
// absent until a seat's identity has at least one completed hand in the
// session StatsRegistry, then renders as "VPIP/PFR/AF".

/** Count seat HUDs that are both non-empty and actually displayed. */
function visibleHudCount(page: import('@playwright/test').Page): Promise<number> {
  return page.evaluate(() =>
    [...document.querySelectorAll('#table-zone .seat-hud')].filter(
      (e) =>
        (e.textContent || '').trim() !== '' &&
        getComputedStyle(e as HTMLElement).display !== 'none',
    ).length,
  );
}

test('seat HUD is absent at hand 1 and appears once hands complete', async ({ page }) => {
  test.setTimeout(60_000);

  await page.goto('/');
  await waitForBoot(page);

  // Fresh boot deals play-mode hand 1 against an empty registry: no badges yet.
  expect(await visibleHudCount(page)).toBe(0);

  // Run the arena at instant speed. Completed hands populate the registry, so
  // active bots' HUD badges appear, formatted as three slash-separated fields.
  await page.click('.tab[data-tab="arena"]');
  await page.click('#arena-start-btn');
  await page.evaluate(() =>
    (window as unknown as { __PK0__: { setInstant: () => void } }).__PK0__.setInstant(),
  );

  await page.waitForFunction(
    () =>
      [...document.querySelectorAll('#table-zone .seat-hud')].some((e) => {
        const t = (e.textContent || '').trim();
        return (
          t.split('/').length === 3 &&
          /\d/.test(t) &&
          getComputedStyle(e as HTMLElement).display !== 'none'
        );
      }),
    { timeout: 30_000 },
  );
});
