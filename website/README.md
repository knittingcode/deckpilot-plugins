# DeckPilot Plugin Marketplace website

The human-facing site at <https://knittingcode.github.io/deckpilot-plugins/>:
the plugin directory, plugin pages, the developer documentation and the
community pages, in English and Polish.

It is a static [Astro](https://astro.build/) site. It has no backend, no
database, no accounts and no analytics, and it needs no secret to build.

> **It is called `website/`, not `site/`,** because `site/` at the repository
> root is the registry tool's generated output and `tools/check-hygiene.py`
> refuses to have it committed.

## The website and the registry

GitHub Pages serves one tree per repository, so the website shares it with
the registry that DeckPilot clients read (`/v1/...`). The rules:

- **The website is a read-only consumer of the registry.** It never signs,
  re-signs, rewrites or generates registry documents, and its jobs have no
  access to signing secrets or to the `registry-*` environments.
- **The catalog is generated from the published registry**, at build time,
  from the `v1/` tree the deploy pipeline has already verified with
  DeckPilot's own client code (`deckpilot-registry verify-site`). See
  [`src/lib/registry.ts`](src/lib/registry.ts). There is no plugin database
  in this directory, and the browser never downloads or opens a package.
- **While no registry is published the catalog is empty**, and the pages
  say so. Example plugins and test fixtures are never shown as published.
- **The registry always wins a path.** `tools/compose-site.py` copies the
  verified registry tree into the Pages artifact byte for byte, puts the
  website around it, and fails the deployment if the website has a file
  under `v1/` or at any other registry path. After the deployment,
  `tools/verify-live-registry.sh` compares the live registry bytes with the
  hashes taken before composing.

| Variable | Meaning |
| --- | --- |
| `REGISTRY_DIR` | The `v1/` directory to read. Unset, or without `timestamp.json`: no registry, empty catalog |
| `REGISTRY_TRUST` | Required when a registry is present. `verified`: the caller ran `verify-site` on this tree. `fixture`: test data; every page is marked and the build can never be deployed |
| `MANIFESTS_DIR` | Directory with `<id>/manifest.json` (the repository's `plugins/`). A manifest is used for a plugin's action list only if its sha256 equals the `manifest_sha256` the registry pins |
| `SITE_OUT_DIR` | Output directory (default `dist`) |

What the registry does not carry, the site does not invent: plugin icons
live inside the packages, so plugins are shown with a monogram; the registry
records versions and their source, not release notes; there are no
download counts, ratings or reviews.

## Develop

Needs Node.js (the version in [`.node-version`](.node-version)).

```sh
cd website
npm ci
npm run dev             # http://localhost:4321/deckpilot-plugins/en/
npm run build           # dist/: the real site (empty catalog today)
npm run build:fixture   # dist-fixture/: an invented catalog, for tests only
npm run verify          # everything CI runs: check, both builds, all tests
```

| Script | Does |
| --- | --- |
| `npm run check` | Type check (`astro check`), including that Polish has every key English has |
| `npm run test:static` | `tests/check-links.mjs` (every internal link and anchor) and `tests/check-i18n.mjs` (the same routes in both languages, hreflang, canonical, no link into the other language) |
| `npm run test:browser` | Playwright: navigation, language and theme switching, mobile menu, keyboard use, search and filters, plugin pages, SEO metadata, 404 handling, and axe (WCAG 2.2 AA) on every page type in both themes |
| `node tests/check-links.mjs dist --external` | Also requests every external link (needs the network; not part of CI) |

The fixture catalog (`tests/make-fixture.mjs`) exists because the real
registry is empty. Its plugins are invented, every page of that build says
so and carries `noindex`, and `tools/compose-site.py` refuses to deploy it.

## Layout

```text
website/
  astro.config.ts        site URL, base path, sitemap, code highlighting
  DESIGN.md              the design system: tokens, components, rules
  src/
    styles/tokens.css    every design token (the one place)
    styles/base.css      element defaults
    styles/components.css component classes
    styles/global.css    fonts, Tailwind mapped onto the tokens
    components/          Astro components (header, footer, cards, ...)
    layouts/Base.astro   document head, SEO, theme, header and footer
    i18n/                en.ts (reference), pl.ts (same shape), helpers
    data/                project facts, navigation, categories
    lib/registry.ts      the registry reader (hash chain re-checked)
    lib/                 paths, search index, formatting, docs helpers
    content/docs/{en,pl} developer documentation (Markdown)
    pages/               routes: /, /404, /[lang]/...
    scripts/             the little client-side JavaScript there is
  tests/                 static checks, Playwright suites, fixture, server
  tools/make-og.mjs      regenerates the social sharing image
```

## Routes

| Route | Page |
| --- | --- |
| `/` | Forwards to the visitor's language (`/en/` or `/pl/`) |
| `/{en,pl}/` | Homepage |
| `/{en,pl}/plugins/` | Plugin directory: search and filters |
| `/{en,pl}/plugins/<id>/` | Plugin page (stable: the plugin id) |
| `/{en,pl}/plugins/search.json` | The build-time search index |
| `/{en,pl}/developers/` | Developer portal |
| `/{en,pl}/docs/`, `/{en,pl}/docs/<page>/` | Developer documentation |
| `/{en,pl}/community/` | Submitting, review, licensing, revocation, reporting |
| `/404.html`, `/{en,pl}/404/` | Not found |
| `/v1/**`, `/icon.svg` | **Not this site's**: the registry tree |

## Languages

English is the default and the reference. `src/i18n/pl.ts` has the type of
`en.ts`, so a missing or extra Polish key fails the type check. Every page
exists in both languages (checked on the built output). Plugin ids,
versions and other identifiers are never translated. Text that comes from
a plugin's publisher (names, descriptions, permission justifications) is
shown as published, in English, and marked `lang="en"` on Polish pages.

To add a documentation page, add `src/content/docs/en/<slug>.md` and
`src/content/docs/pl/<slug>.md` with the same `group` and `order`.

## Documentation accuracy

The developer documentation describes what DeckPilot implements, not what
is planned. It was written against the DeckPilot commit recorded in
`DOCS_VERIFIED_COMMIT` ([`src/data/site.ts`](src/data/site.ts)): every Rust
listing is an excerpt of code that was compiled and tested against the SDK
at that commit, and every manifest example was validated against the
manifest schema. When the documentation is updated for a newer DeckPilot,
re-verify the examples and move that constant.

## Deployment

Nothing here deploys by itself. `.github/workflows/_website.yml` builds and
tests the site without any privilege; `website.yml` runs it for pull
requests (which never deploy); `_site.yml` (push to `main`, and the daily
timestamp refresh) and `publish.yml` (the owner's release tag) compose it
with the registry tree and deploy to GitHub Pages. See
[`docs/architecture.md`](../docs/architecture.md).

## License

Apache-2.0, like the rest of this repository. The DeckPilot logo files in
`src/assets/brand/` are copied unchanged from the DeckPilot project. Fonts
(Inter, JetBrains Mono) are bundled from Fontsource under the SIL Open Font
License; icons are from Lucide (ISC).
