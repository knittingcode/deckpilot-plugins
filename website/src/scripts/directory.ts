/**
 * Plugin directory: client-side search and filters over the build-time
 * index (search.json). The cards are already in the page; this only hides
 * the ones that do not match, keeps the state in the URL, and reports the
 * count to assistive technology. Without JavaScript every plugin is listed.
 */
type Entry = { id: string; text: string; categories: string[]; platforms: string[]; deckpilot: string[] };

function fold(text: string): string {
  return text
    .normalize('NFD')
    .replace(/[̀-ͯ]/g, '')
    .replace(/ł/g, 'l')
    .replace(/Ł/g, 'L')
    .toLowerCase();
}

const rootEl = document.querySelector<HTMLElement>('[data-directory]');

async function init(root: HTMLElement): Promise<void> {
  const form = root.querySelector<HTMLFormElement>('[data-filters]');
  const list = root.querySelector<HTMLElement>('[data-results]');
  const count = root.querySelector<HTMLElement>('[data-count]');
  const none = root.querySelector<HTMLElement>('[data-no-results]');
  const failed = root.querySelector<HTMLElement>('[data-search-error]');
  if (!form || !list || !count || !none) return;

  const q = form.elements.namedItem('q') as HTMLInputElement;
  const category = form.elements.namedItem('category') as HTMLSelectElement;
  const platform = form.elements.namedItem('platform') as HTMLSelectElement;
  const deckpilot = form.elements.namedItem('deckpilot') as HTMLSelectElement;
  const clearButtons = root.querySelectorAll<HTMLButtonElement>('[data-clear]');
  const items = [...list.querySelectorAll<HTMLElement>('[data-plugin-item]')];

  // The URL is the state: a filtered view can be linked and reloaded.
  const params = new URLSearchParams(location.search);
  q.value = params.get('q') ?? '';
  for (const select of [category, platform, deckpilot]) {
    const wanted = params.get(select.name);
    if (wanted && [...select.options].some((o) => o.value === wanted)) select.value = wanted;
  }

  let entries: Map<string, Entry>;
  try {
    const response = await fetch(root.dataset.index ?? '');
    if (!response.ok) throw new Error(String(response.status));
    const data = (await response.json()) as { plugins: Entry[] };
    entries = new Map(data.plugins.map((entry) => [entry.id, entry]));
  } catch {
    if (failed) failed.hidden = false;
    return;
  }

  form.hidden = false;
  const total = items.length;
  const forms = JSON.parse(root.dataset.plural ?? '{}') as Record<string, string>;
  const rules = new Intl.PluralRules(document.documentElement.lang);

  const apply = (): void => {
    const words = fold(q.value).split(/\s+/).filter(Boolean);
    let shown = 0;
    for (const item of items) {
      const entry = entries.get(item.dataset.pluginItem ?? '');
      const match =
        !!entry &&
        words.every((word) => entry.text.includes(word)) &&
        (!category.value || entry.categories.includes(category.value)) &&
        (!platform.value || entry.platforms.includes(platform.value)) &&
        (!deckpilot.value || entry.deckpilot.includes(deckpilot.value));
      item.hidden = !match;
      if (match) shown++;
    }
    const filtered = Boolean(q.value.trim() || category.value || platform.value || deckpilot.value);
    const label = (forms[rules.select(shown)] ?? forms.other ?? '{count}').replace('{count}', String(shown));
    count.textContent = filtered
      ? (root.dataset.resultsOf ?? '').replace('{shown}', String(shown)).replace('{total}', String(total))
      : label;
    none.hidden = shown !== 0;
    list.hidden = shown === 0;
    for (const button of clearButtons) button.hidden = !filtered && button.dataset.clear !== 'always';

    const next = new URLSearchParams();
    if (q.value.trim()) next.set('q', q.value.trim());
    for (const select of [category, platform, deckpilot]) if (select.value) next.set(select.name, select.value);
    const query = next.toString();
    history.replaceState(null, '', `${location.pathname}${query ? `?${query}` : ''}${location.hash}`);
  };

  form.addEventListener('input', apply);
  form.addEventListener('submit', (event) => event.preventDefault());
  for (const button of clearButtons) {
    button.addEventListener('click', () => {
      form.reset();
      q.value = '';
      apply();
      q.focus();
    });
  }
  apply();
}

if (rootEl) void init(rootEl);
