import { AxeBuilder } from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';

async function axe(page: Page) {
  const result = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21aa', 'wcag22aa'])
    .analyze();
  return result.violations.map(
    (v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`,
  );
}

/** 360 px phone, a laptop, and a 32:9 ultrawide. */
const VIEWPORTS = [
  { name: 'phone', width: 360, height: 780 },
  { name: 'laptop', width: 1440, height: 900 },
  { name: 'ultrawide', width: 5120, height: 1440 },
] as const;

for (const vp of VIEWPORTS) {
  test(`settings: every section renders without overflow or axe violations (${vp.name})`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: vp.width, height: vp.height });
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.goto('/settings');
    await expect(page.getByRole('heading', { level: 1, name: 'Settings' })).toBeVisible();
    const tabs = page.getByRole('tab');
    await expect(tabs).toHaveCount(10);
    for (const name of ['General', 'Security', 'Network', 'Appearance', 'Updates', 'About']) {
      await page.getByRole('tab', { name }).click();
      await expect(page.getByRole('heading', { level: 2, name })).toBeVisible();
    }
    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth - window.innerWidth,
    );
    expect(overflow).toBeLessThanOrEqual(0);
    expect(await axe(page)).toEqual([]);
  });
}

test('settings: deep link to a section through the hash', async ({ page }) => {
  await page.goto('/settings#network');
  await expect(page.getByRole('heading', { level: 2, name: 'Network' })).toBeVisible();
});

test('session toolbar: keyboard, morph, edges', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/');
  await page.getByLabel('Partner ID').fill('123456789');
  await page.getByLabel('One-time code', { exact: true }).last().fill('ACDE2345');
  await page.getByRole('button', { name: 'Connect', exact: true }).click();
  await page.getByRole('button', { name: 'They match' }).click({ timeout: 10_000 });
  const toolbar = page.getByRole('toolbar', { name: 'Session toolbar' });
  await expect(toolbar).toBeVisible({ timeout: 10_000 });

  await toolbar.getByRole('button', { name: 'Quality' }).focus();
  await page.keyboard.press('ArrowRight');
  await expect(toolbar.getByRole('button', { name: 'Display' })).toBeFocused();

  await page.getByRole('button', { name: 'Collapse toolbar' }).click();
  await expect(toolbar.getByRole('button', { name: 'Quality' })).toHaveCount(0);
  await page.getByRole('button', { name: 'More tools' }).click();
  await expect(toolbar.getByRole('button', { name: 'Quality' })).toBeVisible();

  await page.getByRole('button', { name: /Move the toolbar/ }).focus();
  await page.keyboard.press('ArrowDown');
  await expect(page.locator('[data-edge="bottom"]')).toBeVisible();
  expect(await axe(page)).toEqual([]);
});
