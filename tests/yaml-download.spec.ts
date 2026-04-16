import { test, expect } from '@playwright/test';
import * as fs from 'fs';
import * as path from 'path';
import { startGame, waitForHumanTurn, waitForYamlReady } from './helpers';

const FIXTURE_DIR  = path.join(__dirname, 'fixtures');
const FIXTURE_PATH = path.join(FIXTURE_DIR, 'session.yaml');

test.describe('YAML download', () => {
  test('downloads a non-empty HandCollection after one hand', async ({ page }) => {
    await startGame(page, 0.42);
    await waitForHumanTurn(page);

    // Fold immediately so the hand ends fast (bots play out at ~1 s/action).
    await page.locator('#action-buttons button:has-text("Fold")').click();
    await waitForYamlReady(page);

    // Intercept the browser download triggered by the button.
    const [download] = await Promise.all([
      page.waitForEvent('download'),
      page.click('#btn-download-yaml'),
    ]);

    // Save to tests/fixtures/ so the Rust validator (make test-yaml) can read it.
    fs.mkdirSync(FIXTURE_DIR, { recursive: true });
    await download.saveAs(FIXTURE_PATH);

    const yaml = fs.readFileSync(FIXTURE_PATH, 'utf-8');
    expect(yaml.length).toBeGreaterThan(0);
    expect(yaml).toContain('hands:');
    expect(yaml).toContain('pkcore_version:');
    expect(yaml).toContain('holdem');
  });
});
