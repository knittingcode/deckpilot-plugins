# Storefront: listings, editorial and media

How a plugin gets screenshots, an icon and promo text in the DeckPilot
Marketplace, and how the owner places plugins on the Discover page
(banners, collection rows, spotlights). For authors and for the owner.

> **Status: pipeline dark.** The files below are accepted and checked, and
> the publish pipeline can sign a storefront, but it does so only once the
> owner sets the repository variable `REGISTRY_STOREFRONT_ENABLED` to `true`
> (rollout stage 4 below). Unset or `false`, publication signs exactly what
> it signed before storefronts. Released DeckPilot versions (up to v0.12)
> never request a storefront. Until stage 4 a listing has no effect on any
> client.

## Who writes what

| File | Who | What |
| --- | --- | --- |
| `registry/plugins/<id>/listing.json` | the plugin's author (their namespace) | promo line, description, highlights, icon, screenshots, promo art, links, release notes |
| `registry/plugins/<id>/media/*.png\|webp\|jpg` | the plugin's author | the images `listing.json` names |
| `editorial/editorial.json` | **owner only** | sections of the Discover page, banners, collections, ordering, schedules |
| `editorial/media/*.png\|webp\|jpg` | **owner only** | banner art |

**Authors cannot place their plugins.** `listing.json` has no placement
field of any kind (`featured`, `order`, `collection`, badges): the schema
refuses unknown fields. Placement lives only in `editorial/`, which
*Submission rules* refuses for everyone but the owner and CODEOWNERS
assigns to the owner. Badges (official, verified, bundled) are never
written by anyone: the client derives them from the signed index and its
own knowledge. Listing text of a community plugin that says "official",
"verified" or "DeckPilot team" is flagged in the job summary for review.

Editorial never overrides trust data. The client builds every card from
the signed index and detail; editorial supplies only plugin ids and copy.
An id that is unknown, revoked, without an installable version, or
incompatible with the user's host or platform is dropped from placements
(it stays in the catalog with its normal state).

## Listing (`registry/plugins/<id>/listing.json`)

Schema: [`schemas/inputs/listing.schema.json`](../schemas/inputs/listing.schema.json).

```json
{
  "$schema": "../../../schemas/inputs/listing.schema.json",
  "listing_version": 1,
  "id": "acme.clock",
  "promo": { "en": "A world clock on your keys", "pl": "Zegar świata na Twoich klawiszach" },
  "description_md": { "en": "Shows the time in **any city**.\n\n- One city per key\n- 12 or 24 hours" },
  "highlights": [
    { "title": { "en": "Any city" }, "text": { "en": "Pick a city per key from the settings." } }
  ],
  "icon": { "file": "media/icon.png" },
  "screenshots": [
    { "file": "media/clock-page.png",
      "alt": { "en": "A deck page with four keys showing the time in four cities" },
      "caption": { "en": "Four cities at a glance" },
      "platforms": ["macos", "windows"] }
  ],
  "promo_art": { "file": "media/promo.webp", "alt": { "en": "Clock faces over a map" } },
  "links": {
    "documentation": "https://github.com/acme/clock#readme",
    "support": "https://github.com/acme/clock/issues",
    "changelog": "https://github.com/acme/clock/blob/main/CHANGELOG.md"
  },
  "release_notes": { "1.0.0": { "en": "First release." } }
}
```

Rules (the schema plus `tools/check-storefront.py`):

- `listing_version` is `1` and `id` equals the directory name. Only
  `listing_version` and `id` are required; every other field is optional.
- Every localized text is a map with `en` required and `pl` optional.
  These are the only languages (a closed set; adding one is a schema
  change by the owner). Clients fall back from the user's locale to its
  language, then to `en`.
- Text limits (characters): promo 1-80, description 1-6000, at most 6
  highlights (title 1-40, text 1-160), alt 1-250, caption 1-120, release
  note 1-2000 per version.
- All text is Unicode NFC, without control characters (line breaks only in
  the description and release notes) and without invisible or format
  characters: soft hyphen (U+00AD), CGJ (U+034F), Arabic letter mark
  (U+061C), Hangul fillers (U+115F, U+1160, U+3164, U+FFA0), Khmer
  inherent vowels (U+17B4, U+17B5), Mongolian vowel separator (U+180E),
  zero-width and directional marks (U+200B-200F), line and paragraph
  separators (U+2028, U+2029), bidi embeddings, overrides and isolates
  (U+202A-202E, U+2066-2069), word joiner and invisible operators
  (U+2060-2064), deprecated format characters (U+206A-206F), variation
  selectors (U+FE00-FE0F, U+E0100-E01EF), BOM (U+FEFF), interlinear
  annotations (U+FFF9-FFFB), shorthand format controls (U+1BCA0-1BCA3),
  musical symbol format characters (U+1D173-1D17A) and tag characters
  (U+E0000-E007F). The schemas refuse the ones in the Basic Multilingual
  Plane (their patterns stay portable to every regex dialect);
  `tools/check-storefront.py` refuses all of them.
- `description_md` and release notes use a **safe markdown subset**:
  paragraphs, line breaks, `-` or `*` lists one level deep, `**bold**`,
  `_emphasis_`, `` `code` `` and links written exactly as
  `[text](https://…)` (no title, no spaces or parentheses in the URL, the
  URL rules below). No headings, images, tables, raw HTML (`<` followed by
  a letter, `/`, `!` or `?`), autolinks (`<https://…>`), reference links or
  definitions; any other `](` is refused. Anything else is shown as
  literal text.
- A `release_notes` key must be a recorded version
  (`registry/plugins/<id>/versions/<v>.json`). Release notes can be added
  after the version is published, without a new version.
- `links` (`documentation`, `support`, `changelog`): `https:` only, at most
  512 characters, no user information, no port, no IP address in any
  notation (dotted, integer, octal, hex, a numeric last label), a lowercase
  ASCII host, no punycode (`xn--`), no dot segments in the path (`/./`,
  `/../`, `%2e`). A `deckpilot.*` plugin links only to
  `github.com/knittingcode/…` or `knittingcode.github.io`. Clients show the
  full URL before opening it in the browser.
- `file` is `media/<name>` with `<name>` matching `[a-z0-9-]{1,48}` and the
  extension `.png`, `.webp` or `.jpg`. Every file in `media/` must be
  referenced, and every referenced file must exist.
- Screenshots may name `platforms` (`macos`, `windows`, `linux`); clients
  show only the ones for their platform (no `platforms`: all).

## Editorial (`editorial/editorial.json`, owner only)

Schema: [`schemas/inputs/editorial.schema.json`](../schemas/inputs/editorial.schema.json).
Normally edited with [Marketplace Studio](studio.md) (`tools/studio/`), but
it is a plain JSON file.

```json
{
  "$schema": "../schemas/inputs/editorial.schema.json",
  "editorial_version": 1,
  "sections": [
    { "id": "launch", "status": "published", "kind": "banner_carousel", "order": 10,
      "window": { "starts_at": "2026-12-01T00:00:00Z", "ends_at": "2027-01-15T00:00:00Z" },
      "platforms": ["macos", "windows", "linux"],
      "items": [
        { "id": "weather-banner",
          "title": { "en": "Know before you go", "pl": "Sprawdź, zanim wyjdziesz" },
          "body": { "en": "Weather and air quality on your keys." },
          "artwork": { "file": "media/weather-banner.webp", "alt": { "en": "Rain clouds over a deck" } },
          "target": { "type": "plugin", "id": "deckpilot.weather" },
          "cta": { "en": "See plugin" } } ] },
    { "id": "focus", "status": "published", "kind": "collection_row", "order": 20, "layout": "cards",
      "title": { "en": "Stay focused" }, "description": { "en": "Timers and reminders." },
      "plugins": ["deckpilot.pomodoro", "deckpilot.break-reminder"] },
    { "id": "spot-memory", "status": "draft", "kind": "spotlight", "order": 30,
      "plugin": "deckpilot.memory", "version": "0.1.0",
      "headline": { "en": "New: Memory" }, "body": { "en": "Notes on a key." } },
    { "id": "new", "status": "published", "kind": "recently_added", "order": 40, "limit": 8,
      "title": { "en": "Recently added" } },
    { "id": "cats", "status": "published", "kind": "category_highlights", "order": 50,
      "categories": [ { "id": "monitoring", "blurb": { "en": "Watch what matters." } } ] },
    { "id": "all", "status": "published", "kind": "catalog", "order": 100, "title": { "en": "All plugins" } }
  ],
  "collections": [
    { "id": "productivity", "status": "published", "order": 10,
      "title": { "en": "Productivity" }, "description": { "en": "Plugins for focused work." },
      "plugins": ["deckpilot.pomodoro", "deckpilot.memory"] }
  ]
}
```

| Section `kind` | Holds |
| --- | --- |
| `banner_carousel` | 1-6 banner `items`, each with art, a title, an optional body and call to action, and a `target` |
| `collection_row` | `layout` `cards` or `compact`, a title, 1-24 plugin ids |
| `spotlight` | one `plugin`, a headline, an optional body and `version` (which release notes to show; install always takes the normal update path) |
| `recently_added` | a title and a `limit` of 4-24; the client derives the list from the signed `published_at` |
| `category_highlights` | 1-12 categories, each with an optional blurb |
| `catalog` | the full catalog, with an optional title |

Rules:

- At most 12 sections and 16 collections. Section, collection and banner
  ids match `[a-z0-9][a-z0-9-]{0,47}` and are unique.
- `status` is `draft`, `published` or `archived`. Only `published` entries
  are signed; drafts and archived entries stay in this repository.
- `order` is an integer (0-1000000); ties are broken by `id`.
- `window` (optional) is UTC with whole seconds; shown from `starts_at`
  (inclusive) until `ends_at` (exclusive); either bound may be left out.
  `ends_at` must be after `starts_at`.
- `platforms` (optional) limits a section or collection to `macos`,
  `windows` and/or `linux`.
- `target.type` is `plugin`, `collection` or `category`. **There are no
  external URLs in editorial.**
- Text limits (characters): title and headline 1-60, body and description
  1-200, call to action 1-24, category blurb 1-160, alt 1-250. The same
  text rules as listings (NFC, no controls, bidi or zero-width).
- Every plugin id must exist in `registry/plugins/`; one without a recorded
  version yet is a warning (clients drop it until it is published). A
  spotlight `version` must be recorded and not yanked; a collection target
  must exist.
- Banner art needs `alt`, unless it is marked `"decorative": true`, which
  then has no `alt`.

## Asset rules

| Asset | Formats | Size (px) and aspect | Max bytes | Count | Alt text |
| --- | --- | --- | --- | --- | --- |
| Icon (listing) | PNG, WebP | 1:1, 256 to 1024 (512 recommended) | 256 KiB | 0-1 | none; the UI uses the plugin's name |
| Screenshot (listing) | PNG, WebP, JPEG | 16:10 or 16:9 (±1%), width 1280-2560 | 1 MiB | 0-8 | required, `en` required |
| Promo art (listing) | PNG, WebP, JPEG | 2:1 (±1%), width 1600-3200 | 1 MiB | 0-1 | required |
| Banner art (editorial) | PNG, WebP, JPEG | 3:1 (±1%), width 1200-3600 | 1 MiB | 1 per banner item | required, or `"decorative": true` without alt |

Totals: at most 8 MiB of media per plugin and 16 MiB for editorial.

For every file:

- **SVG is refused**, as are GIF, AVIF and every other format. Export PNG
  or WebP.
- The format is sniffed from the bytes and must match the extension
  (`.png`, `.webp`, `.jpg`; not `.jpeg`).
- At most 1 MiB per file, 8,388,608 pixels, no empty side; 8-bit channels
  (no 16-bit PNG, no 12-bit JPEG); no animation (APNG, animated WebP).
- **No metadata**, for privacy: PNG `tEXt`, `zTXt`, `iTXt` and `eXIf`, JPEG
  APP1 (Exif, XMP), APP13 (IPTC) and comments, and WebP `EXIF` and `XMP`
  chunks or their VP8X flags are refused. Most export dialogs have a
  "strip metadata" option; `exiftool -all= file` or `oxipng --strip safe`
  also work.
- **ICC profiles** are allowed: in JPEG as APP2 `ICC_PROFILE` segments of
  at most 64 KiB + 1024 bytes together (any other APP2 is refused), in
  WebP as one `ICCP` chunk of at most 64 KiB right after `VP8X`, with the
  VP8X ICC flag set, and in PNG as an `iCCP` chunk of at most 16 KiB
  compressed (its zlib data is not inflated by the header check), before
  `PLTE` and `IDAT`.
- PNG: only the chunks `IHDR` (first, once, 13 bytes; a defined colour
  type with a valid bit depth: 1, 2, 4 or 8 for grayscale and palette, 8
  otherwise; compression and filter method 0, interlace 0 or 1), `PLTE`
  (not in grayscale images, once, before `IDAT`, 3-768 bytes in steps of 3;
  required for palette images), `tRNS` (before `IDAT`), `IDAT`
  (consecutive), `IEND` (empty, last), `sRGB`, `gAMA`, `cHRM`, `pHYs`
  (before `IDAT`) and `iCCP`; valid CRCs; nothing after `IEND`.
- JPEG: exactly one frame header, baseline (SOF0) or progressive (SOF2),
  before the first scan, 8-bit, grayscale or YCbCr (1 or 3 components;
  no CMYK), its length 6 + 3 × components, the height not deferred to a
  DNL marker. Besides the tables (DHT, DAC, DQT, DRI, DNL) and the scans,
  only one plain JFIF APP0 (14 bytes, without a thumbnail), the 12-byte
  Adobe APP14 and APP2 ICC profiles; every other APPn, COM and marker is
  refused, and nothing may follow the end marker (EOI).
- WebP: the RIFF size equals the file length (chunks padded to even). A
  simple file is exactly one `VP8 ` key frame or one `VP8L` image
  (version 0). An extended file has a 10-byte `VP8X` without reserved
  bits, then the optional `ICCP` (above), an optional `ALPH` before the
  image, and exactly one image whose size equals the canvas.

The header checks are the same in `tools/lib/storefront.py` and in
DeckPilot's `crates/deckpilot-plugin-registry/src/media.rs`, which is
authoritative.

Where the rules are enforced:

| Check | Where | Runs on |
| --- | --- | --- |
| Paths: images only in `registry/plugins/<id>/media/` and `editorial/media/`, `.png`/`.webp`/`.jpg` only, no SVG | `tools/check-hygiene.py` | every push and pull request (*Repository hygiene*) |
| Contributor media only in their own namespace's `media/`; `media/` holds only regular image files; no symlinks; `editorial/` owner only; the listing schema of the base commit | `tools/check-submission.py` | pull requests (*Submission rules*) |
| Schemas, cross-file rules, text, URL and markdown rules, header-level media checks (format, dimensions, aspect, bytes, chunks, metadata, animation) | `tools/check-storefront.py` (`tools/validate.sh storefront`) | *Validate registry* |
| The same schemas with the pinned `schema-check` | `tools/validate.sh storefront` | *Validate registry* |
| **Authoritative**: every rule, including a full decode of every image | `deckpilot-registry storefront check` (DeckPilot) | *Validate registry*, once `tools/deckpilot.env` pins a DeckPilot commit that has it; until then the step says that it was skipped |

The header-level check never decodes pixels, so a file with valid headers
and corrupt image data passes it; the Rust check and the client's decoder
refuse it.

These checks run the pull request's own code (a pull request can change
`tools/` and the workflow), so for a pull request from outside they are
advisory; the gate is the owner's code-owner review that the ruleset on
`main` requires.

## Workflow

**Authors** add or change `listing.json` and `media/` in an ordinary
submission pull request (together with a new version or on their own; see
[publishing.md](publishing.md#1-submission-contributor)). Check locally
with `python3 -I tools/check-storefront.py`. A listing change is published
with the next signed registry update; it needs no new plugin version.

**The owner** edits `editorial/` (with Marketplace Studio or by hand) in a
pull request, merges it after review, and publishes it with a
`registry--v<date>.<n>` tag, which signs a new index and, from stage 4 on,
a new storefront. To withdraw editorial content: revert the pull request,
merge the revert, tag `registry--v…` again. The new, higher seq no longer
carries the content; seq never goes down, and the branch `registry-state`
is never touched by hand.

What the pipeline does (`tools/publish-site.sh`, `tools/mirror-site.sh`;
the throwaway-key tests are `tools/tests/test-storefront-site.sh` and
`test-registry-freshness.sh --with-cli`):

| Where | Variable unset or `false` | `REGISTRY_STOREFRONT_ENABLED=true` |
| --- | --- | --- |
| *Validate registry* (every pull request) | the pinned `deckpilot-registry storefront check`, then a throwaway-key `storefront build`, `storefront sign`, `verify-site --require-storefront` of the sources | the same |
| *Publish* `sign` (a `registry--v…` or plugin tag) | build-index, sign-index, timestamp: no `storefront/` or `media/` on the new site (turning the variable off withdraws the storefront at the next publication) | `storefront check`; build-index, sign-index; `storefront build` and `storefront sign` (the index key, its own trusted comment `role:storefront`) for the new seq; timestamp; `verify-site --require-storefront`. The live storefronts of the indexes kept (the newest and 10 before it) and their files are carried over; `storefront build` prunes the rest |
| *Publish* `verify-published` | as before | the live `storefront/` and `media/`, fetched anonymously, are the signed bytes; `verify-site --require-storefront` |
| Daily refresh and every Pages deploy (`_site.yml`) | the mirror takes the storefront of every kept index, its documents and images (each against its pinned sha256 and length); a refresh changes `timestamp.json` only (checked: `index/`, `plugins/`, `storefront/`, `media/` byte for byte) and verifies the live seq's storefront as a client would | the same (the refresh never looks at the variable) |
| Freshness monitor (`check-freshness.sh`) | when the live seq has a storefront: its signature, documents and images are all served as pinned | the same |

Whenever `verify-site --require-storefront` runs with a client state
(`--state`), that state must then record the storefront it accepted
(`registry-storefront.json`, beside `registry-trust.json`): the live site's
verification records the live seq's storefront, and the new site is
verified against that state, so a refresh can never sign a different
storefront for the same seq.

## Rollout

Nothing reaches the production registry before stage 2 has passed. Each
stage's exit criteria are its tests passing and the owner's sign-off below.

| Stage | What | Exit criteria |
| --- | --- | --- |
| 0. Schema and tooling | DeckPilot storefront registry model and CLI (WP1) merged on DeckPilot `main`; this repository's inputs and checks (WP3) merged. No client or site change. | WP1's crate tests; this repository's storefront, hygiene and submission tests; *Validate registry* green |
| 1. Pipeline dark | The publish pipeline (WP4) merged with `tools/deckpilot.env` pinned to the WP1 merge commit and `REGISTRY_STOREFRONT_ENABLED=false`. | The next `registry--v…` deploy manifest equals the previous one apart from index and timestamp; `validate.sh storefront` runs the authoritative check |
| 2. Test registry | No public staging site: CI throwaway-key registries (*Validate registry*), Marketplace Studio's local preview and preview site with a debug DeckPilot agent, the compatibility matrix (v0.10.0, v0.11.0, v0.12.0 against a site with a storefront). Security review of the media parser, resolver, safe markdown and the desktop media command. | Matrix green; security review recorded |
| 3. Client support | DeckPilot ships storefront support (v0.13.0). With no production storefront, every user sees today's catalog-only Discover. | DeckPilot release checks |
| 4. Production | `REGISTRY_STOREFRONT_ENABLED=true`. The first editorial pull request is minimal (one collection row, no art); `registry--v…` tag; `verify-published` checks the storefront and media; the freshness monitor passes two daily refreshes with media preserved. Then banners and listings. | `verify-published` green; two refreshes preserve the storefront byte for byte |

### Sign-off

The owner records each stage here when its exit criteria are met (date,
commit or run, and initials). An empty row means the stage has not passed.

| Stage | Date | Evidence (commits, runs) | Signed off |
| --- | --- | --- | --- |
| 0. Schema and tooling | | | |
| 1. Pipeline dark | | | |
| 2. Test registry | | | |
| 3. Client support | | | |
| 4. Production | | | |
