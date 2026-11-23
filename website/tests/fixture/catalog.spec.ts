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
    await page.getByLabel('DeckPilot version').selectOption('0.7.0');
    expect(await visible(page)).not.toContain('sample.room-lights');
    await expect(page).toHaveURL(/platform=linux&deckpilot=0\.7\.0$/);
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
    await expect(details).toContainText('Includes the current release v0.10.0.');
    await expect(details).not.toContainText('does not include plugin support');
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
    await expect(page.getByRole('main')).toContainText('Install DeckPilot 0.7.0 or later; the current release is 0.10.0. 0.7.0 is the first release with plugin support');
    await expect(page.getByRole('main')).toContainText('Open the Marketplace in DeckPilot and find Build Status. Since DeckPilot 0.8.0 the Marketplace opens from the toolbar; in 0.7.0 it is under Settings › Extensions › Plugins.');
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
    await expect(details).toContainText('Does not include the current release v0.10.0');
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

  // Fully revoked plugins are absent from index.plugins (the publisher's
  // shape); their notice pages come from the signed revocations alone.
  test('a fully revoked plugin keeps its page, with the action, reason and advisory and no install controls', async ({ page }) => {
    await page.goto('en/plugins/sample.old-monitor/');
    const main = page.getByRole('main');
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('sample.old-monitor');
    await expect(main).toContainText('This plugin has been revoked');
    await expect(main).toContainText('DeckPilot stops installed copies and refuses to load them.');
    await expect(main).toContainText('Sent data to a host unrelated to its purpose.');
    await expect(main).toContainText('DeckPilot does not offer it for installation');
    await expect(main.getByRole('link', { name: 'Read the advisory' })).toHaveAttribute('href', 'https://example.com/advisory');
    await expect(page.locator('#install-title')).toHaveCount(0);
    await expect(main.getByRole('listitem').filter({ hasText: 'Open the Marketplace' })).toHaveCount(0);
    await expect(page.locator('[data-revocation-notice]')).toHaveCount(1);

    await page.goto('pl/plugins/sample.old-monitor/');
    await expect(page.locator('html')).toHaveAttribute('lang', 'pl');
    await expect(page.getByRole('main')).toContainText('Ta wtyczka została odwołana');
    await expect(page.getByRole('main')).toContainText('DeckPilot zatrzymuje zainstalowane kopie i odmawia ich wczytania.');
    await expect(page.getByRole('main').locator('[lang="en"]', { hasText: 'Sent data to a host unrelated to its purpose.' })).toHaveCount(1);
    await expect(page.getByRole('main').getByRole('link', { name: 'Przeczytaj komunikat bezpieczeństwa' })).toHaveAttribute('href', 'https://example.com/advisory');
  });

  test('individual revocations that exhaust every version also keep a notice page', async ({ page }) => {
    for (const [path, title, disable, yank] of [
      ['en/plugins/sample.legacy-sync/', 'This plugin has been revoked', 'DeckPilot stops installed copies and refuses to load them.', 'It is no longer offered for installation or updates.'],
      ['pl/plugins/sample.legacy-sync/', 'Ta wtyczka została odwołana', 'DeckPilot zatrzymuje zainstalowane kopie i odmawia ich wczytania.', 'Nie jest już oferowana do instalacji ani aktualizacji.'],
    ] as const) {
      await page.goto(path);
      const main = page.getByRole('main');
      await expect(main).toContainText(title);
      await expect(main).toContainText(disable);
      await expect(main).toContainText(yank);
      await expect(main).toContainText('Uploaded the settings file to an unrelated host.');
      await expect(main).toContainText('Lost settings on update.');
      await expect(main).toContainText('1.1.0');
      await expect(page.locator('#install-title')).toHaveCount(0);
    }
  });

  test('fully revoked plugins stay out of discovery; a partly revoked one stays in', async ({ page, request }) => {
    for (const lang of ['en', 'pl']) {
      const ids = (await (await request.get(`${lang}/plugins/search.json`)).json()).plugins.map((p: { id: string }) => p.id);
      expect(ids).not.toContain('sample.old-monitor');
      expect(ids).not.toContain('sample.legacy-sync');
      expect(ids).toContain('fixture.build-status');
      await page.goto(`${lang}/plugins/`);
      await expect(page.locator('[data-plugin-item="sample.old-monitor"], [data-plugin-item="sample.legacy-sync"]')).toHaveCount(0);
      await page.goto(`${lang}/`);
      await expect(page.locator('[data-plugin="sample.old-monitor"], [data-plugin="sample.legacy-sync"]')).toHaveCount(0);
    }
    await page.goto('en/plugins/fixture.build-status/');
    await expect(page.getByRole('main')).toContainText('Revoked versions: 1.0.0.');
    await expect(page.locator('#install-title')).toHaveText('How to install');
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
      for (const path of ['en/plugins/fixture.build-status/', 'pl/plugins/sample.room-lights/', 'en/plugins/sample.old-monitor/', 'pl/plugins/sample.legacy-sync/', 'en/plugins/sample.chat-status/']) {
        await page.goto(path);
        await expectAccessible(page);
      }
    }
  });
});
