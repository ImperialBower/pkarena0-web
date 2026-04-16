import { type Page } from '@playwright/test';

/** Wait for the WASM module to finish booting (New Game button becomes enabled). */
export async function waitForBoot(page: Page): Promise<void> {
  await page.waitForSelector('#btn-new-game:not([disabled])', { timeout: 15_000 });
}

/** Click New Game with a fixed seed so test runs are reproducible. */
export async function startGame(page: Page, seed: number = 0.42): Promise<void> {
  await page.addInitScript(({ s }: { s: number }) => {
    Math.random = () => s;
  }, { s: seed });
  await page.goto('/');
  await waitForBoot(page);
  await page.click('#btn-new-game');
}

/**
 * Wait until action buttons are enabled (human's turn) or status indicates
 * hand/session ended. Handles the BOT_THINK_MS delay in the JS game loop.
 */
export async function waitForHumanTurn(page: Page): Promise<void> {
  await page.waitForFunction(
    () => {
      const btns = document.querySelectorAll<HTMLButtonElement>('#action-buttons button');
      return [...btns].some(b => !b.disabled && b.id !== 'btn-new-game');
    },
    { timeout: 15_000 },
  );
}

/** True when the status message contains the given substring (case-insensitive). */
export async function statusContains(page: Page, text: string): Promise<boolean> {
  const msg = await page.textContent('#status-msg');
  return (msg ?? '').toLowerCase().includes(text.toLowerCase());
}

/** Wait until at least one hand has completed and YAML is ready to download. */
export async function waitForYamlReady(page: Page): Promise<void> {
  await page.waitForSelector('#btn-download-yaml:not([disabled])', { timeout: 30_000 });
}
