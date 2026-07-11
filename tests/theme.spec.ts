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
});
