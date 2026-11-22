# Marketplace Studio

A local editor for the storefront ([storefront.md](storefront.md)):
the Discover page's sections, banners and collections in
`editorial/editorial.json`, and listings in
`registry/plugins/<id>/listing.json`, with their media. **Maintainer-only**:
`editorial/` is owner-only, and Studio is the owner's tool. It is Python 3
standard library plus vanilla ES modules (`tools/studio/`): no dependency,
no bundler, no npm.

## Launch

```sh
python3 -I tools/studio/studio.py            # mirrors the live registry first
python3 -I tools/studio/studio.py serve --offline --no-browser
```

It prints `http://127.0.0.1:<random port>/?token=<per-launch token>` and
opens it. Stop it with Ctrl-C; the token dies with it. For the full
validation and the live index verification, build the pinned tools first
(`tools/build-cli.sh`); without them Studio still works and says what it
skipped.

Other commands:

| Command | Does |
| --- | --- |
| `studio.py render-preview --out DIR [--as-of T] [--drafts]` | static HTML wireframes of Discover per platform and locale, their media, `summary.md` (CI uploads these as the `storefront-preview` artifact of *Validate registry*) |
| `studio.py export-fixture [--platform P] [--locale L] [--as-of T] [--drafts]` | `.work/studio/storefront-fixture.json` and `.work/studio/media/` for the DeckPilot desktop dev build (below) |
| `studio.py check` | the storefront checks, plus `deckpilot-registry storefront check` when the pinned CLI has it |

## What it does

- **Index**: the plugins come from `registry/` at `HEAD`, marked with their
  state in the live signed index (mirrored by `tools/mirror-site.sh` into
  `.work/studio/mirror` and verified with the pinned `verify-site` against
  `keys/roots.json` when the CLI is built): live, recorded but not live
  yet, or never published. Pickers offer only these ids.
- **Edit**: sections, banners, collections; localized text with `en`
  required and `pl` beside it; reordering with ↑/↓ buttons or Alt+↑/↓ on a
  focused row (the `order` values are renumbered 10, 20, …); status draft,
  published or archived; UTC window pickers; platform toggles; listings with
  highlights, screenshots, links and release notes of recorded versions.
- **Media import**: the file is checked before it is written: format
  sniffed from the bytes (must match `.png`, `.webp` or `.jpg`; SVG
  refused), dimensions from the headers, aspect, bytes, pixels, animation
  and metadata chunks. Python's standard library cannot decode images, so
  this is a header-level check; CI's `deckpilot-registry storefront check`
  decodes every file and is authoritative.
- **Save** writes the canonical JSON: the schema's key order, 2-space
  indent, a trailing newline, UTF-8 without escapes. A file that does not
  match the schema yet is saved with its errors listed.
- **Validate**: the schemas, the cross-file rules, text, URLs, markdown and
  media (`tools/check-storefront.py`), and the Rust check when available.
- **Preview**: a wireframe of Discover with the client's resolver rules:
  status (drafts on request, archived never), the UTC window as of a chosen
  time (start inclusive, end exclusive), platform, locale fallback (exact
  tag, language, `en`), order then id, placements of unknown, revoked,
  yanked-only, unpublished or platform-incompatible plugins dropped, empty
  sections dropped, unknown section kinds skipped, and the 48 KiB cap. It
  cannot know the user's DeckPilot version, so `engines` is not applied.
  The dropped entries are listed under the preview.
- **True preview**: *Export fixture* (or `export-fixture`) writes
  `.work/studio/storefront-fixture.json`, the `StorefrontFixture` that
  DeckPilot's `fixtureFromJson()` loads (`storefront-mocks.ts`, frozen
  contract `storefront-types.ts` at knittingcode/deckpilot `97bceac7`):
  `storefront` is the `StorefrontView` the agent would answer, `listings`
  the `ListingView`s, `media` each image as a `data:image/…;base64,` URL by
  sha256. The desktop dev build loads it with
  `VITE_STOREFRONT_FIXTURE=<path>`, so the real components render it.
  The editorial source keeps its own names (`banner_carousel`, a spotlight's
  `headline` and `body`); the view uses the contract's (`bannerCarousel`,
  `title` and `description`). `recentlyAdded` and `catalog` carry no plugin
  ids: the UI derives them, as it does from the agent.
- **Prepare PR**: 1. show the changed storefront files (other changes are
  listed and never committed); 2. `git switch -c editorial/<yyyy-mm-dd>-<slug>`;
  3. `git add` of exactly those files; 4. `git commit` of exactly those
  files; 5. `git push -u origin HEAD:refs/heads/<that branch>`;
  6. `gh pr create --fill --base main`. Each step asks for confirmation
  first, and push and pull request only run from an `editorial/` branch.
  Studio never tags and never publishes: after review and merge the owner
  publishes with a `registry--v<date>.<n>` tag ([storefront.md](storefront.md#workflow)).

## Security

- Binds `127.0.0.1` on a random port.
- A random token per launch, checked on every request: the URL's
  `?token=` once, which sets an `HttpOnly; SameSite=Strict` cookie; every
  write (`POST`, `PUT`, `DELETE`) must also carry it in the
  `X-Studio-Token` header and come from this server's `Origin`.
- `Host` must be `127.0.0.1:<port>` or `localhost:<port>` (DNS rebinding).
- Its own CSP on every answer: `default-src 'self'; img-src 'self' blob:;
  style-src 'self'; script-src 'self'; …`, plus `nosniff`, `no-referrer`
  and `no-store`. The UI never assigns `innerHTML`; the preview HTML is
  escaped server-side and has no script.
- Writes only `editorial/editorial.json`, `editorial/media/<name>`,
  `registry/plugins/<id>/listing.json` and `registry/plugins/<id>/media/<name>`
  (an existing plugin of `registry/plugins/`). Paths with `..`, `.`, empty
  parts, backslashes or absolute forms are refused, and so is a symlink
  anywhere on the way (directory or file); writes are atomic.
- Never reads or handles keys: the public `keys/roots.json` is only passed
  by path to `verify-site`. Network only through `tools/mirror-site.sh`,
  `git push` and `gh`.

Tests: `tools/tests/test_studio.py` (also in *Repository hygiene*).
