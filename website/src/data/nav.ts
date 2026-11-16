import type { Dictionary } from '@/i18n';

/** Main navigation: route (after the language) and its label key. */
export const NAV: { route: string; key: keyof Dictionary['nav'] }[] = [
  { route: 'plugins', key: 'plugins' },
  { route: 'developers', key: 'developers' },
  { route: 'docs', key: 'docs' },
  { route: 'community', key: 'community' },
];
