import { DECKPILOT_VERSIONS } from '@/data/site';
import { platformsOf, satisfies, type CatalogPlugin } from './registry';

/** One plugin in the build-time search index (plugins/search.json). */
export type SearchEntry = {
  id: string;
  /** Lower-cased, diacritics removed: name, id, publisher, summary, description. */
  text: string;
  categories: string[];
  platforms: string[];
  /** The public DeckPilot versions inside the plugin's `engines.deckpilot` range. */
  deckpilot: string[];
};

/** The same folding the client applies to the query. */
export function fold(text: string): string {
  return text
    .normalize('NFD')
    .replace(/[̀-ͯ]/g, '')
    .replace(/ł/g, 'l')
    .replace(/Ł/g, 'L')
    .toLowerCase();
}

export function searchEntry(plugin: CatalogPlugin): SearchEntry {
  return {
    id: plugin.id,
    text: fold(
      [plugin.name, plugin.id, plugin.publisher.displayName, plugin.publisher.login, plugin.summary, plugin.description].join(' '),
    ),
    categories: plugin.categories,
    platforms: [...platformsOf(plugin.latest)],
    deckpilot: DECKPILOT_VERSIONS.filter((v) => satisfies(v.version, plugin.latest.engines.deckpilot)).map((v) => v.version),
  };
}
