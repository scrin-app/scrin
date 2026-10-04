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

for (const mode of ['light', 'dark'] as const) {
  test(`home renders with no axe violations (${mode})`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: mode, reducedMotion: 'reduce' });
    await page.goto('/');
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
    // Wait for the mock engine's ID and code so skeletons are gone.
    await expect(
      page.getByRole('status', { name: /your id/i }).or(page.locator('output').first()),
    ).toBeVisible();
    await expect(page.locator('output')).toHaveCount(2);
    expect(await axe(page)).toEqual([]);
  });
}

test('primary navigation reaches every screen', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/');
  const nav = page.getByRole('navigation', { name: 'Primary' }).locator('visible=true');
  await nav.getByRole('link', { name: 'Devices' }).click();
  await expect(page.getByRole('heading', { level: 1, name: 'Devices' })).toBeVisible();
  await nav.getByRole('link', { name: 'Settings' }).click();
  await expect(page.getByRole('heading', { level: 1, name: 'Settings' })).toBeVisible();
  expect(await axe(page)).toEqual([]);
});

test('connect flow reaches SAS verification and the session', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/');
  await page.getByLabel('Partner ID').fill('123456789');
  await page.getByLabel('One-time code', { exact: true }).last().fill('ACDE2345');
  await page.getByRole('button', { name: 'Connect', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Verify these emoji' })).toBeVisible({
    timeout: 10_000,
  });
  expect(await axe(page)).toEqual([]);
  await page.getByRole('button', { name: 'They match' }).click();
  await expect(page.getByRole('toolbar', { name: 'Session toolbar' })).toBeVisible({
    timeout: 10_000,
  });
});

test('unknown routes show the 404 page', async ({ page }) => {
  await page.goto('/does-not-exist');
  await expect(page.getByRole('heading', { name: 'Page not found' })).toBeVisible();
});

test('no horizontal overflow', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('output')).toHaveCount(2);
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - window.innerWidth,
  );
  expect(overflow).toBeLessThanOrEqual(0);
});
