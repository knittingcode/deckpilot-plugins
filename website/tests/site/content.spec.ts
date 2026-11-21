import { expect, test, type APIRequestContext } from '@playwright/test';

/** The ids of the plugins this build was made with (empty without a registry tree). */
async function catalogIds(request: APIRequestContext): Promise<string[]> {
  const index = await (await request.get('en/plugins/search.json')).json();
  return index.plugins.map((plugin: { id: string }) => plugin.id);
}

test.describe('homepage', () => {
  test('has the sections in order, in both languages', async ({ page }) => {
    await page.goto('en/');
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Make your Stream Deck work your way.');
    const ids = await page.locator('main > section[id], main > section[aria-labelledby="hero-title"]').evaluateAll((els) =>
      els.map((el) => el.id || 'hero'),
    );
    expect(ids).toEqual(['hero', 'search', 'featured', 'categories', 'how-it-works', 'security', 'build', 'community', 'faq']);
    await page.goto('pl/');
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Twój Stream Deck. Twoje możliwości.');
  });

  test('hero actions lead to the directory, the developer portal and GitHub', async ({ page }) => {
    await page.goto('en/');
    await expect(page.getByRole('link', { name: 'Explore plugins' }).first()).toHaveAttribute('href', /\/en\/plugins\/$/);
    await expect(page.getByRole('link', { name: 'Build a plugin' }).first()).toHaveAttribute('href', /\/en\/developers\/$/);
    await expect(page.getByRole('link', { name: 'View DeckPilot on GitHub' })).toHaveAttribute(
      'href',
      'https://github.com/knittingcode/deckpilot',
    );
  });

  test('states the release status and shows no invented plugins', async ({ page, request }) => {
    const ids = await catalogIds(request);
    await page.goto('en/');
    await expect(page.locator('#status')).toContainText('DeckPilot 0.10.0, the current release, installs plugins from its Marketplace; 0.7.0 was the first release with plugin support');
    await expect(page.locator('#status')).not.toContainText('next DeckPilot release');
    if (ids.length === 0) {
      await expect(page.locator('#featured')).toContainText('No plugins in this build');
      await expect(page.locator('[data-plugin]')).toHaveCount(0);
    } else {
      // Built with the published registry (a deployment of main): the
      // featured plugins (at most six) all come from this build's catalog.
      await expect(page.locator('#featured')).not.toContainText('No plugins in this build');
      const shown = await page.locator('#featured [data-plugin]').evaluateAll((cards) => cards.map((card) => card.getAttribute('data-plugin')));
      expect(shown).toHaveLength(Math.min(6, ids.length));
      for (const id of shown) expect(ids).toContain(id);
    }
  });

  test('search hands the query to the directory', async ({ page }) => {
    await page.goto('en/');
    await page.getByRole('searchbox', { name: 'Search plugins' }).fill('weather');
    await page.keyboard.press('Enter');
    await expect(page).toHaveURL(/\/en\/plugins\/\?q=weather$/);
  });
});

test.describe('the directory', () => {
  test('lists exactly the plugins of this build, or explains that there are none', async ({ page, request }) => {
    const ids = await catalogIds(request);
    await page.goto('en/plugins/');
    await expect(page.locator('[data-fixture-banner]')).toHaveCount(0);
    if (ids.length === 0) {
      // Built without a registry tree (pull requests).
      await expect(page.locator('#empty-registry')).toContainText('No plugins in this build');
      await expect(page.locator('[data-plugin-item]')).toHaveCount(0);
      await page.goto('pl/plugins/');
      await expect(page.locator('#empty-registry')).toContainText('Ta wersja witryny nie zawiera wtyczek');
    } else {
      // Built with the published registry: every listed plugin comes from it.
      await expect(page.locator('#empty-registry')).toHaveCount(0);
      const listed = await page.locator('[data-plugin-item]').evaluateAll((items) => items.map((item) => item.getAttribute('data-plugin-item')));
      expect([...listed].sort()).toEqual([...ids].sort());
      await page.goto('pl/plugins/');
      await expect(page.locator('#empty-registry')).toHaveCount(0);
      await expect(page.locator('[data-plugin-item]')).toHaveCount(ids.length);
    }
  });
});

test.describe('documentation', () => {
  test('sidebar lists every page and marks the current one', async ({ page }) => {
    await page.goto('en/docs/manifest/');
    const sidebar = page.getByRole('navigation', { name: 'Documentation pages' });
    await expect(sidebar.getByRole('link')).toHaveCount(17);
    await expect(sidebar.getByRole('link', { name: 'Manifest format' })).toHaveAttribute('aria-current', 'page');
    await sidebar.getByRole('link', { name: 'Permissions' }).click();
    await expect(page).toHaveURL(/\/en\/docs\/permissions\/$/);
  });

  test('code is highlighted in both themes and can be copied', async ({ page, context }) => {
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
    await page.goto('en/docs/getting-started/');
    const block = page.locator('.prose .code-block').first();
    await expect(block.locator('pre.astro-code')).toBeVisible();
    const colours = await block.locator('pre span[style]').evaluateAll((spans) => new Set(spans.map((s) => getComputedStyle(s).color)).size);
    expect(colours).toBeGreaterThan(1);
    await block.hover();
    await block.getByRole('button', { name: 'Copy code' }).click();
    await expect(block.getByRole('button')).toHaveText('Copied');
    const copied = await page.evaluate(() => navigator.clipboard.readText());
    expect(copied).toBe(await block.locator('pre').textContent());
  });

  test('the homepage sample has a working copy button', async ({ page, context }) => {
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
    await page.goto('pl/#build');
    await page.locator('#build').getByRole('button', { name: 'Kopiuj kod' }).click();
    expect(await page.evaluate(() => navigator.clipboard.readText())).toContain('deckpilot_plugin_sdk::plugin!(plugin);');
  });

  test('previous and next links walk the pages in order', async ({ page }) => {
    await page.goto('en/docs/getting-started/');
    await page.getByRole('navigation', { name: 'Previous and next page' }).getByRole('link').click();
    await expect(page).toHaveURL(/\/en\/docs\/architecture\/$/);
  });
});
