import { test, expect } from '@playwright/test';

test.describe('Theme token layer', () => {
  test('body has theme-midnight and resolvable tokens', async ({ page }) => {
    await page.goto('/');
    const cls = await page.evaluate(() => document.body.className);
    expect(cls).toContain('theme-midnight');
    const bg = await page.evaluate(() =>
      getComputedStyle(document.body).getPropertyValue('--bg').trim());
    expect(bg).toBe('#0B0E13');
    const pip = await page.evaluate(() =>
      getComputedStyle(document.body).getPropertyValue('--pip-diamond').trim());
    expect(pip).toBe('#C63C4C');
  });

  test('theme dropdown switches theme and persists across reload', async ({ page }) => {
    await page.goto('/');
    await page.selectOption('#theme-select', 'terminal');
    expect(await page.evaluate(() => document.body.className)).toContain('theme-terminal');
    const bg = await page.evaluate(() =>
      getComputedStyle(document.body).getPropertyValue('--bg').trim());
    expect(bg).toBe('#F4F2EC');
    await page.reload();
    expect(await page.evaluate(() => document.body.className)).toContain('theme-terminal');
  });

  test('deck toggle switches diamond pip color and persists', async ({ page }) => {
    await page.goto('/');
    const pip = () => page.evaluate(() =>
      getComputedStyle(document.body).getPropertyValue('--pip-diamond').trim());
    expect(await pip()).toBe('#C63C4C');
    await page.click('#deck-toggle');
    expect(await pip()).toBe('#2E5FD0');
    expect(await page.textContent('#deck-toggle')).toContain('4-COLOR');
    await page.reload();
    expect(await pip()).toBe('#2E5FD0');
  });
});
