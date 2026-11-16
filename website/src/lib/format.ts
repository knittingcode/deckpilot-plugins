import type { Lang } from '@/i18n';

/** A date of the registry (UTC), in the page's language. */
export function formatDate(lang: Lang, iso: string): string {
  return new Intl.DateTimeFormat(lang, { dateStyle: 'medium', timeZone: 'UTC' }).format(new Date(iso));
}

/** The date for <time datetime>. */
export function isoDate(iso: string): string {
  return new Date(iso).toISOString().slice(0, 10);
}

export function formatSize(lang: Lang, bytes: number): string {
  const kib = bytes / 1024;
  const number = new Intl.NumberFormat(lang, { maximumFractionDigits: kib < 100 ? 1 : 0 });
  return kib < 1024 ? `${number.format(kib)} KiB` : `${number.format(kib / 1024)} MiB`;
}
