import type { APIRoute } from 'astro';
import { langPaths } from '@/i18n';
import { getCatalog } from '@/lib/registry';
import { searchEntry } from '@/lib/search';

/**
 * The search index of the plugin directory, generated from the registry at
 * build time. The directory page filters its server-rendered cards with it.
 */
export const getStaticPaths = langPaths;

export const GET: APIRoute = () =>
  new Response(JSON.stringify({ version: 1, plugins: getCatalog().plugins.map(searchEntry) }), {
    headers: { 'Content-Type': 'application/json; charset=utf-8' },
  });
