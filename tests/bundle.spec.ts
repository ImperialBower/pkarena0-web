import { test, expect } from '@playwright/test';
import { execFileSync } from 'child_process';
import { createHash } from 'crypto';
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync, rmSync } from 'fs';
import path from 'path';

// EPIC-56 Phase 4: the release is one flat directory made by
// scripts/embed-bundle.mjs. This drives the script the way release.yml does
// and then uses its output the way a consumer does: two lines, no attributes.
const ROOT = path.join(__dirname, '..');
const DIST = path.join(ROOT, 'www', 'dist');
const FILES = [
  'LICENSE-APACHE', 'LICENSE-MIT', 'MANIFEST.sha256',
  'pk-table.css', 'pk-table.js', 'pkarena0_web.js', 'pkarena0_web_bg.wasm',
];
const version = /^version\s*=\s*"([^"]+)"/m.exec(readFileSync(path.join(ROOT, 'Cargo.toml'), 'utf8'))![1];

test.describe('the embed bundle', () => {
  // One worker, in order: beforeAll rebuilds www/dist, and a second worker's
  // beforeAll would delete it under the first worker's tests.
  test.describe.configure({ mode: 'serial' });
  test.beforeAll(() => {
    rmSync(DIST, { recursive: true, force: true });
    execFileSync('node', ['scripts/embed-bundle.mjs', DIST, '--tag', 'v' + version], { cwd: ROOT, stdio: 'pipe' });
    // A consumer's page: the module beside the element, nothing else said.
    writeFileSync(path.join(DIST, 'test.html'), `<!doctype html><meta charset="utf-8"><title>bundle</title>
<script type="module" src="pk-table.js"></script>
<pk-table><img src="../img/table-still.svg" alt="table"></pk-table>`);
  });

  test('is exactly the seven files and a manifest that verifies', () => {
    const names = readdirSync(DIST).filter(n => n !== 'test.html').sort();
    expect(names).toEqual(FILES);
    const manifest = readFileSync(path.join(DIST, 'MANIFEST.sha256'), 'utf8').trim().split('\n');
    expect(manifest.length).toBe(FILES.length - 1);
    for (const line of manifest) {
      const [sum, name] = line.split(/\s+\*?/);
      const actual = createHash('sha256').update(readFileSync(path.join(DIST, name))).digest('hex');
      expect(actual, name).toBe(sum);
    }
  });

  test('pk-table.js stands alone: no relative import survives', () => {
    const js = readFileSync(path.join(DIST, 'pk-table.js'), 'utf8');
    expect(js).not.toMatch(/^\s*import\s.*from\s+['"]\.\.?\//m);
    expect(js).toContain("customElements.define('pk-table'");
    const css = readFileSync(path.join(DIST, 'pk-table.css'), 'utf8');
    expect(css).not.toMatch(/@import|url\(|Grotesk|Plex|Archivo|JetBrains|Marcellus/);
  });

  test('the wasm is under the size gate and the licenses are the two', () => {
    expect(readFileSync(path.join(DIST, 'pkarena0_web_bg.wasm')).length).toBeLessThan(2_300_000);
    expect(readFileSync(path.join(DIST, 'LICENSE-MIT'), 'utf8')).toContain('MIT License');
    expect(readFileSync(path.join(DIST, 'LICENSE-APACHE'), 'utf8')).toContain('Apache License');
  });

  test('refuses a tag that is not the crate version', () => {
    const out = path.join(ROOT, 'www', 'dist-wrong-tag');
    rmSync(out, { recursive: true, force: true });
    let failed = false;
    try {
      execFileSync('node', ['scripts/embed-bundle.mjs', out, '--tag', 'v9.9.9'], { cwd: ROOT, stdio: 'pipe' });
    } catch (e: any) {
      failed = true;
      expect(String(e.stderr)).toContain('9.9.9');
    }
    expect(failed).toBe(true);
    expect(existsSync(out)).toBe(false);
  });

  test('a page with two lines plays a hand from the flat layout', async ({ page }) => {
    const urls: string[] = [];
    page.on('request', r => urls.push(r.url()));
    await page.goto('/dist/test.html');
    await page.evaluate(() => { const el = document.querySelector('pk-table')!; el.setAttribute('delay', '0'); });
    await page.locator('pk-table button.pk-deal').click();
    await page.waitForFunction(() => (document.querySelector('pk-table') as HTMLElement).dataset.phase === 'WaitingForHuman', null, { timeout: 20_000 });
    await expect(page.locator('pk-table .pk-badge')).toHaveText(`pkarena0-web v${version}`);
    const fetched = urls.filter(u => /pkarena0_web|pk-table\.css/.test(u)).map(u => new URL(u).pathname);
    expect(fetched.sort()).toEqual(['/dist/pk-table.css', '/dist/pkarena0_web.js', '/dist/pkarena0_web_bg.wasm']);
  });
});
