import { test } from '@playwright/test';
import { expectAccessible } from '../axe';

const PAGES = [
  'en/',
  'pl/',
  'en/plugins/',
  'pl/plugins/',
  'en/developers/',
  'pl/developers/',
  'en/docs/',
  'pl/docs/',
  'en/docs/getting-started/',
  'pl/docs/permissions/',
  'en/docs/manifest/',
  'en/community/',
  'pl/community/',
  'en/404/',
  'pl/404/',
];

for (const scheme of ['light', 'dark'] as const) {
  for (const path of PAGES) {
    test(`axe: ${path} (${scheme})`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: scheme, reducedMotion: 'reduce' });
      await page.goto(path);
      await expectAccessible(page);
    });
  }
}

test('axe: mobile menu open', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('en/');
  await page.getByRole('button', { name: 'Open menu' }).click();
  await expectAccessible(page);
});

test('axe: FAQ answers open', async ({ page }) => {
  await page.goto('pl/');
  for (const summary of await page.locator('.faq summary').all()) await summary.click();
  await expectAccessible(page);
});
