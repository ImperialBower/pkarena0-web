import { test, expect, type Page } from '@playwright/test';
import { readFileSync } from 'fs';
import path from 'path';

// EPIC-56 Phase 3: the eight contract rules of <pk-table>, one test each,
// against /embed.html — the page a consumer copies from. The contract lives
// in docs/EPIC-56_Embed_Build.md § Design and in bower EPIC-19 § Contract.

const EMBED = /pkarena0_web(_bg\.wasm|\.js)|pk-table\.css/;

function requests(page: Page): string[] {
  const urls: string[] = [];
  page.on('request', r => urls.push(r.url()));
  return urls;
}

/** Set attributes on the nth <pk-table>, then press its Deal. */
async function deal(page: Page, n: number, attrs: Record<string, string> = {}) {
  await page.evaluate(([i, a]) => {
    const el = document.querySelectorAll('pk-table')[i as number] as HTMLElement;
    for (const [k, v] of Object.entries(a as Record<string, string>)) el.setAttribute(k, v);
  }, [n, attrs] as const);
  await page.locator('pk-table').nth(n).locator('button.pk-deal').click();
}

const phase = (n: number) => `pk-table:nth-of-type(${n + 1})`;

async function waitForPhase(page: Page, n: number, want: string) {
  await page.waitForFunction(([i, p]) => {
    const el = document.querySelectorAll('pk-table')[i as number] as HTMLElement;
    return el?.dataset.phase === p;
  }, [n, want] as const, { timeout: 20_000 });
}

/** The engine's state for the nth element, through its public getState(). */
async function stateOf(page: Page, n: number) {
  return page.evaluate((i) => {
    const el = document.querySelectorAll('pk-table')[i] as any;
    return el.getState();
  }, n);
}

/** Act until the hand is over: fold whenever it is our turn. */
async function foldOut(page: Page, n: number) {
  await page.waitForFunction((i) => {
    const el = document.querySelectorAll('pk-table')[i] as HTMLElement;
    return ['WaitingForHuman', 'HandComplete', 'SessionOver'].includes(el.dataset.phase ?? '');
  }, n, { timeout: 20_000 });
  const el = page.locator('pk-table').nth(n);
  if ((await el.getAttribute('data-phase')) === 'WaitingForHuman') {
    await el.locator('.pk-actions button[data-act="fold"]').click();
  }
  await page.waitForFunction((i) => {
    const el = document.querySelectorAll('pk-table')[i] as HTMLElement;
    return Number(el.dataset.hands ?? 0) >= 1;
  }, n, { timeout: 20_000 });
}

test.describe('<pk-table> contract', () => {
  test('1. nothing is fetched until Deal; the picture and a Deal button show', async ({ page }) => {
    const urls = requests(page);
    await page.goto('/embed.html');
    await expect(page.locator('pk-table').first().locator('button.pk-deal')).toBeVisible();
    await expect(page.locator('pk-table').first().locator('img')).toBeVisible();
    expect(urls.filter(u => EMBED.test(u))).toEqual([]);
  });

  test('2. each element is its own engine', async ({ page }) => {
    await page.goto('/embed.html');
    await deal(page, 0, { seed: '0.11', delay: '0' });
    await deal(page, 1, { seed: '0.77', delay: '0' });
    await foldOut(page, 0);
    const a = await stateOf(page, 0);
    const b = await stateOf(page, 1);
    // The first table has moved on to hand 2; the second is still in hand 1.
    expect(a.hand_number).toBe(2);
    expect(b.hand_number).toBe(1);
    // `seed` seeds the bot lineup (init_game shuffles the profile pool with
    // it); the deck is pkcore's entropy shuffle, so cards are never pinned.
    const lineup = (s: any) => s.players.map((p: any) => p.name);
    expect(lineup(a)).not.toEqual(lineup(b));
  });

  test('2b. the same seed seats the same bots', async ({ page }) => {
    await page.goto('/embed.html');
    await deal(page, 0, { seed: '0.33', delay: '0' });
    await deal(page, 1, { seed: '0.33', delay: '0' });
    await waitForPhase(page, 0, 'WaitingForHuman');
    await waitForPhase(page, 1, 'WaitingForHuman');
    const a = await stateOf(page, 0);
    const b = await stateOf(page, 1);
    expect(a.players.map((p: any) => p.name)).toEqual(b.players.map((p: any) => p.name));
  });

  test('3. a wasm that fails to load leaves the picture and says so', async ({ page }) => {
    await page.route('**/pkarena0_web_bg.wasm', r => r.abort());
    await page.goto('/embed.html');
    const errors: string[] = [];
    await page.exposeFunction('__pkErr', (m: string) => errors.push(m));
    await page.evaluate(() => {
      document.querySelector('pk-table')!.addEventListener('pk-table:error',
        (e: any) => (window as any).__pkErr(e.detail.message));
    });
    await deal(page, 0, { delay: '0' });
    await waitForPhase(page, 0, 'error');
    const el = page.locator('pk-table').first();
    await expect(el.locator('img')).toBeVisible();
    await expect(el.locator('button.pk-deal')).toBeDisabled();
    await expect(el.locator('.pk-status')).toContainText('did not load');
    expect(el.locator('.pk-spinner')).toHaveCount(0);
    expect(errors.length).toBe(1);
  });

  test('4. every request goes to one origin', async ({ page }) => {
    const urls = requests(page);
    await page.goto('/embed.html');
    await deal(page, 0, { seed: '0.5', delay: '0' });
    await foldOut(page, 0);
    const origins = new Set(urls.map(u => new URL(u).origin));
    expect([...origins]).toEqual(['http://localhost:8080']);
  });

  test('5. a hand plays to its end and the next one is offered', async ({ page }) => {
    await page.goto('/embed.html');
    const hands: any[] = [];
    await page.exposeFunction('__pkHand', (s: any) => hands.push(s));
    await page.evaluate(() => {
      document.querySelector('pk-table')!.addEventListener('pk-table:hand-over',
        (e: any) => (window as any).__pkHand(e.detail.state));
    });
    await deal(page, 0, { seed: '0.5', delay: '0' });
    await foldOut(page, 0);
    expect(hands.length).toBe(1);
    expect(hands[0].last_result?.length ?? 0).toBeGreaterThan(0);
    const el = page.locator('pk-table').first();
    await expect(el.locator('.pk-actions button[data-act="next"]')).toBeVisible();
    await expect(el.locator('.pk-status')).toContainText(/won|split/);
  });

  test('6. focus survives a render of the action row', async ({ page }) => {
    await page.goto('/embed.html');
    await deal(page, 0, { seed: '0.5', delay: '0' });
    await waitForPhase(page, 0, 'WaitingForHuman');
    const act = await page.evaluate(() => {
      const el = document.querySelector('pk-table') as any;
      const root = el.shadowRoot as ShadowRoot;
      (root.querySelector('.pk-actions button[data-act="fold"]') as HTMLButtonElement).focus();
      el.refresh();   // re-renders the row from the same state
      return (root.activeElement as HTMLElement | null)?.dataset.act ?? null;
    });
    expect(act).toBe('fold');
  });

  test('7. styles are scoped, fonts are local, the surround follows the host', async ({ page }) => {
    await page.goto('/embed.html');
    await page.evaluate(() => document.documentElement.style.setProperty('--bg', 'rgb(1, 2, 3)'));
    await deal(page, 0, { seed: '0.5', delay: '0' });
    await waitForPhase(page, 0, 'WaitingForHuman');
    const got = await page.evaluate(() => {
      const root = (document.querySelector('pk-table') as any).shadowRoot as ShadowRoot;
      const frame = root.querySelector('.pk-frame') as HTMLElement;
      const status = root.querySelector('.pk-status') as HTMLElement;
      const felt = root.querySelector('.felt') as HTMLElement;
      return {
        frameBg: getComputedStyle(frame).backgroundColor,
        fonts: getComputedStyle(status).fontFamily + ' ' + getComputedStyle(root.querySelector('.pot')!).fontFamily,
        feltPosition: getComputedStyle(felt).position,
        hostSeats: document.querySelectorAll('.seat').length,
      };
    });
    expect(got.frameBg).toBe('rgb(1, 2, 3)');
    // Once the table is live the picture steps aside; it is still in the light
    // DOM (rule 3 brings it back on failure), just not shown.
    await expect(page.locator('pk-table').first().locator('img')).toBeHidden();
    expect(got.fonts).not.toMatch(/Space Grotesk|IBM Plex|Archivo|JetBrains|Marcellus/);
    expect(got.feltPosition).toBe('absolute');
    expect(got.hostSeats).toBe(0);
  });

  test('8. the badge shows the engine version, which is the crate version', async ({ page }) => {
    const cargo = readFileSync(path.join(__dirname, '..', 'Cargo.toml'), 'utf8');
    const version = /^version\s*=\s*"([^"]+)"/m.exec(cargo)![1];
    await page.goto('/embed.html');
    await deal(page, 0, { seed: '0.5', delay: '0' });
    await waitForPhase(page, 0, 'WaitingForHuman');
    await expect(page.locator('pk-table').first().locator('.pk-badge')).toHaveText(`pkarena0-web v${version}`);
  });
});

test.describe('<pk-table> with script off', () => {
  test.use({ javaScriptEnabled: false });
  test('is the picture and nothing else', async ({ page }) => {
    await page.goto('/embed.html');
    const el = page.locator('pk-table').first();
    await expect(el.locator('img')).toBeVisible();
    await expect(el.locator('button')).toHaveCount(0);
  });
});
