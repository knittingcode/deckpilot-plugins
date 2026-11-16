import { defineCollection } from 'astro:content';
import { glob } from 'astro/loaders';
import { z } from 'astro/zod';

/**
 * Developer documentation: src/content/docs/<lang>/<slug>.md. Both languages
 * must have the same slugs (tests/i18n.test.mjs).
 */
const docs = defineCollection({
  loader: glob({ pattern: '{en,pl}/*.md', base: './src/content/docs' }),
  schema: z.object({
    title: z.string().min(1).max(60),
    description: z.string().min(1).max(160),
    group: z.enum(['start', 'build', 'capabilities', 'ship', 'reference']),
    order: z.number().int().positive(),
  }),
});

export const collections = { docs };
