import { Blocks, Clapperboard, Code, Gauge, Lightbulb, ListChecks, MessagesSquare } from '@lucide/astro';

/**
 * The categories a plugin may declare: the `categories` enum of DeckPilot's
 * manifest schema (plugins/schemas/plugin-manifest.schema.json). A category
 * the registry lists that is not here is shown under its raw id.
 */
export const CATEGORIES = [
  { id: 'developer-tools', icon: Code },
  { id: 'productivity', icon: ListChecks },
  { id: 'communication', icon: MessagesSquare },
  { id: 'monitoring', icon: Gauge },
  { id: 'media', icon: Clapperboard },
  { id: 'smart-home', icon: Lightbulb },
  { id: 'other', icon: Blocks },
] as const;

export type CategoryId = (typeof CATEGORIES)[number]['id'];
