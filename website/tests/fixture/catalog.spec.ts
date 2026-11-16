import { expect, test } from '@playwright/test';
import { expectAccessible } from '../axe';

/**
 * The catalog with plugins in it. This build uses invented test data
 * (tests/make-fixture.mjs), is marked on every page and is never deployed.
 */
const visible = (page: import('@playwright/test').Page) =>
  page.locator('[data-plugin-item]:visible').evaluateAll((items) => items.map((item) => item.getAttribute('data-plugin-item')));

test.describe('fixture build', () => {
  test('every page says it is test data and is kept out of indexes', async ({ page, request }) => {
    for (const path of ['en/', 'pl/plugins/', 'en/plugins/fixture.build-status/']) {
      await page.goto(path);
      await expect(page.locator('[data-fixture-banner]')).toBeVisible();
      await expect(page.locator('html')).toHaveAttribute('data-fixture-build', '');
      await expect(page.locator('meta[name="robots"]')).toHaveAttribute('content', 'noindex');
    }
    expect(await (await request.get('robots.txt')).text()).toContain('Disallow: /');
  });
});

test.describe('directory', () => {
  test('lists the offered plugins and leaves out the revoked one', async ({ page }) => {
    await page.goto('en/plugins/');
    await expect(page.locator('[data-filters]')).toBeVisible();
    expect((await visible(page)).sort()).toEqual([
      'fixture.build-status',
      'fixture.focus-timer',
      'sample.chat-status',
      'sample.media-keys',
      'sample.room-lights',
    ]);
    await expect(page.locator('[data-count]')).toHaveText('5 plugins');
    const card = page.locator('[data-plugin="fixture.build-status"]');
    await expect(card).toContainText('Build Status');
    await expect(card).toContainText('Fixture Labs');
    await expect(card).toContainText('v1.2.0');
    await expect(card).toContainText('Verified publisher');
  });

  test('search matches name, description and publisher, ignoring case and diacritics', async ({ page }) => {
    await page.goto('pl/plugins/');
    const search = page.getByRole('searchbox', { name: 'Szukaj' });
    await search.fill('TIMER');
    expect(await visible(page)).toEqual(['fixture.focus-timer']);
    await expect(page.locator('[data-count]')).toHaveText('Wyświetlono 1 z 5');
    await search.fill('workflow run');
    expect(await visible(page)).toEqual(['fixture.build-status']);
    await search.fill('fixture labs');
    expect((await visible(page)).sort()).toEqual(['fixture.build-status', 'fixture.focus-timer']);
    await search.fill('łights');
    expect(await visible(page)).toEqual(['sample.room-lights']);
  });

  test('filters combine, and the state lives in the URL', async ({ page }) => {
    await page.goto('en/plugins/');
    await page.getByLabel('Category').selectOption('monitoring');
    expect(await visible(page)).toEqual(['fixture.build-status']);
    await expect(page).toHaveURL(/\?category=monitoring$/);
    await page.getByLabel('Category').selectOption('');
    await page.getByLabel('Platform').selectOption('windows');
    expect((await visible(page)).sort()).toEqual(['fixture.build-status', 'fixture.focus-timer', 'sample.chat-status', 'sample.media-keys']);
    await page.getByLabel('Platform').selectOption('linux');
    expect(await visible(page)).not.toContain('sample.chat-status');
    await page.getByLabel('Works with').selectOption('0.6.0-alpha.3');
    expect(await visible(page)).not.toContain('sample.room-lights');
    await expect(page).toHaveURL(/platform=linux&deckpilot=0\.6\.0-alpha\.3$/);
    await page.reload();
    expect((await visible(page)).sort()).toEqual(['fixture.build-status', 'fixture.focus-timer', 'sample.media-keys']);
  });

  test('a link with filters opens filtered (homepage search and categories)', async ({ page }) => {
    await page.goto('en/plugins/?q=media');
    expect(await visible(page)).toEqual(['sample.media-keys']);
    await page.goto('en/');
    await page.locator('#categories').getByRole('link', { name: 'Smart home' }).click();
    await expect(page).toHaveURL(/\/en\/plugins\/\?category=smart-home$/);
    expect(await visible(page)).toEqual(['sample.room-lights']);
  });

  test('no results shows an empty state, and clearing restores the list', async ({ page }) => {
    await page.goto('en/plugins/');
    await page.getByRole('searchbox', { name: 'Search' }).fill('no such plugin anywhere');
    await expect(page.locator('[data-no-results]')).toBeVisible();
    await expect(page.locator('[data-no-results]')).toContainText('No plugins match these filters');
    await expectAccessible(page);
    await page.locator('[data-no-results]').getByRole('button', { name: 'Clear filters' }).click();
    expect((await visible(page)).length).toBe(5);
    await expect(page).toHaveURL(/\/en\/plugins\/$/);
    await expect(page.getByRole('searchbox', { name: 'Search' })).toBeFocused();
  });

  test('works with the keyboard alone', async ({ page }) => {
    await page.goto('en/plugins/');
    await page.getByRole('searchbox', { name: 'Search' }).focus();
    await page.keyboard.type('focus');
    expect(await visible(page)).toEqual(['fixture.focus-timer']);
    await page.keyboard.press('Tab');
    await expect(page.getByLabel('Category')).toBeFocused();
    await page.keyboard.press('Tab');
    await page.keyboard.press('Tab');
    await page.keyboard.press('Tab');
    await expect(page.getByRole('button', { name: 'Clear filters' })).toBeFocused();
    await page.keyboard.press('Tab');
    await expect(page.getByRole('link', { name: 'Focus Timer' })).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(page).toHaveURL(/\/en\/plugins\/fixture\.focus-timer\/$/);
  });

  test('shows an error state and all plugins when the index cannot be loaded', async ({ page }) => {
    await page.route('**/search.json', (route) => route.abort());
    await page.goto('en/plugins/');
    await expect(page.locator('[data-search-error]')).toBeVisible();
    await expect(page.locator('[data-search-error]')).toContainText('Search is unavailable');
    expect((await visible(page)).length).toBe(5);
  });

  test('lists every plugin without JavaScript', async ({ browser }) => {
    const context = await browser.newContext({ javaScriptEnabled: false });
    const page = await context.newPage();
    await page.goto('en/plugins/');
    await expect(page.locator('[data-plugin-item]')).toHaveCount(5);
    await expect(page.locator('[data-filters]')).toBeHidden();
    await context.close();
  });

  test('is accessible in both themes and has no sideways scroll on a phone', async ({ page }) => {
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      await page.goto('pl/plugins/');
      await expectAccessible(page);
    }
    await page.setViewportSize({ width: 360, height: 800 });
    await page.goto('pl/plugins/');
    expect(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)).toBeLessThanOrEqual(0);
  });

  test('the homepage features the newest plugins and counts categories', async ({ page }) => {
    await page.goto('en/');
    await expect(page.locator('#featured [data-plugin]')).toHaveCount(5);
    await expect(page.locator('#featured [data-plugin]').first()).toContainText('Room Lights');
    await expect(page.locator('#categories li').filter({ hasText: 'Monitoring' })).toContainText('1 plugin');
    await page.goto('pl/');
    await expect(page.locator('#categories li').filter({ hasText: 'Produktywność' })).toContainText('1 wtyczka');
    await expect(page.locator('#categories li').filter({ hasText: 'Narzędzia programistyczne' })).toContainText('1 wtyczka');
  });
});

test.describe('plugin page', () => {
  test('shows everything the registry knows about the plugin', async ({ page }) => {
    await page.goto('en/plugins/fixture.build-status/');
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Build Status');
    const details = page.getByRole('complementary');
    await expect(details).toContainText('1.2.0');
    await expect(details).toContainText('Jun 18, 2026');
    await expect(details).toContainText('macOS, Windows and Linux');
    await expect(details).toContainText('>=0.5.0 <1.0.0');
    await expect(details).toContainText('Includes the current pre-release v0.6.0-alpha.3');
    await expect(details).toContainText('Apache-2.0');
    await expect(details.getByRole('link', { name: 'example/build-status' })).toHaveAttribute('href', 'https://github.com/example/build-status');
    await expect(details).toContainText('fixture.build-status');

    await expect(page.locator('[data-action]')).toHaveCount(2);
    await expect(page.locator('[data-action="show-status"]')).toContainText('Repository (text, required)');
    await expect(page.locator('[data-action="show-status"]')).toContainText('Run URL');

    await expect(page.locator('[data-permission]')).toHaveCount(3);
    const network = page.locator('[data-permission="network"]');
    await expect(network).toContainText('Network access');
    await expect(network).toContainText('api.github.com');
    await expect(network).toContainText('Reads the latest workflow run');
    await expect(page.locator('[data-permission="credentials"]')).toContainText('never sees the secret');

    const history = page.getByRole('table');
    await expect(history.getByRole('row')).toHaveCount(4);
    await expect(history.getByRole('link', { name: 'Compare with 1.1.0' })).toHaveAttribute('href', /github\.com\/example\/build-status\/compare\/[0-9a-f]{40}\.\.\.[0-9a-f]{40}$/);

    await expect(page.locator('#install-title')).toHaveText('How to install');
    await expect(page.getByRole('main')).toContainText('switched off in every released build');
    await expect(page.getByRole('main')).toContainText('This website has no install button');
    await expect(page.getByRole('main').getByRole('link', { name: /^install/i })).toHaveCount(0);
    await expect(page.getByRole('main').getByRole('button', { name: /install/i })).toHaveCount(0);
    await expect(page.locator('a[href^="deckpilot:"]')).toHaveCount(0);
  });

  test('explains unknown, pre-release and incompatible cases honestly', async ({ page }) => {
    await page.goto('en/plugins/sample.room-lights/');
    await expect(page.getByRole('main')).toContainText('Pre-release');
    const details = page.getByRole('complementary');
    await expect(details).toContainText('macOS, Linux');
    await expect(details).toContainText('Does not include the current pre-release v0.6.0-alpha.3');
    await expect(page.locator('[data-permission="telepathy"]')).toContainText('Other permission');
    await expect(page.locator('[data-permission="network"]')).toContainText('a server address you enter yourself');
    await expect(page.locator('#actions-title + p')).toContainText('not shown here');
  });

  test('a plugin without permissions says so', async ({ page }) => {
    await page.goto('en/plugins/fixture.focus-timer/');
    await expect(page.getByRole('main')).toContainText('This plugin requests no permissions');
    await expect(page.locator('[data-action]')).toHaveCount(1);
  });

  test('deprecated and yanked states are shown', async ({ page }) => {
    await page.goto('en/plugins/sample.chat-status/');
    await expect(page.getByRole('main')).toContainText('This plugin is deprecated');
    await expect(page.getByRole('main').getByRole('link', { name: 'fixture.focus-timer' })).toHaveAttribute('href', /\/en\/plugins\/fixture\.focus-timer\/$/);
    await expect(page.getByRole('table')).toContainText('Yanked');
    await expect(page.getByRole('table')).toContainText('Crashed on start.');
  });

  test('a revoked plugin keeps its page, with the reason and no install steps', async ({ page }) => {
    await page.goto('en/plugins/sample.old-monitor/');
    await expect(page.getByRole('main')).toContainText('This plugin has been revoked');
    await expect(page.getByRole('main')).toContainText('Sent data to a host unrelated to its purpose.');
    await expect(page.getByRole('main')).toContainText('DeckPilot does not offer it for installation');
    await expect(page.getByRole('link', { name: 'Read the advisory' })).toHaveAttribute('href', 'https://example.com/advisory');
  });

  test('Polish page: Polish interface, publisher text marked as English', async ({ page }) => {
    await page.goto('pl/plugins/fixture.build-status/');
    await expect(page.locator('html')).toHaveAttribute('lang', 'pl');
    await expect(page.locator('#permissions-title')).toHaveText('Wymagane uprawnienia');
    await expect(page.getByRole('complementary')).toContainText('18 cze 2026');
    await expect(page.locator('[data-permission="network"] blockquote')).toHaveAttribute('lang', 'en');
    await expect(page.getByRole('heading', { level: 1 })).toHaveAttribute('lang', 'en');
    await page.getByRole('link', { name: /English/ }).click();
    await expect(page).toHaveURL(/\/en\/plugins\/fixture\.build-status\/$/);
  });

  test('plugin pages are accessible in both themes', async ({ page }) => {
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      for (const path of ['en/plugins/fixture.build-status/', 'pl/plugins/sample.room-lights/', 'en/plugins/sample.old-monitor/', 'en/plugins/sample.chat-status/']) {
        await page.goto(path);
        await expectAccessible(page);
      }
    }
  });
});
