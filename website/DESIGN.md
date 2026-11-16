# DeckPilot design system

The visual identity shared by DeckPilot's websites: this Marketplace site
today, the DeckPilot application site later. It is three plain CSS files
with no build dependency, plus Astro components that wrap them.

| File | Contents | Depends on |
| --- | --- | --- |
| [`src/styles/tokens.css`](src/styles/tokens.css) | Every token: colour (light and dark), type, spacing, layout, shape, motion | nothing |
| [`src/styles/base.css`](src/styles/base.css) | Element defaults: body, headings, links, code, focus, reduced motion | tokens |
| [`src/styles/components.css`](src/styles/components.css) | Component classes (`.btn`, `.card`, `.badge`, ...) | tokens |
| [`src/styles/global.css`](src/styles/global.css) | Fonts, and Tailwind utilities mapped onto the tokens (optional) | all of the above |

**To reuse it in another site** copy `tokens.css`, `base.css` and
`components.css`, load the two fonts, and add the theme script from
`src/layouts/Base.astro`. Tailwind is not required: `global.css` only maps
utility names (`bg-surface`, `text-muted`, `border-line`) to the same
tokens. Change a value in `tokens.css` and every site follows; never write
a colour, a font size or a duration anywhere else.

## Brand

| Asset | File | Use |
| --- | --- | --- |
| Icon | `src/assets/brand/icon.svg`, `icon.png` | Favicon, touch icon |
| Wordmark | `src/assets/brand/wordmark.svg` | Header and footer on light backgrounds |
| Wordmark, dark | `src/assets/brand/wordmark-dark.svg` | Header and footer on dark backgrounds |

These are byte-identical copies of the DeckPilot project's files
(`.github/assets/` here, `.github/assets/branding/` in DeckPilot). Never
edit, recolour or redraw them; give them clear space of at least the height
of the icon's keys. The two brand colours are constants:

| Token | Value | |
| --- | --- | --- |
| `--dp-navy` | `#07112f` | The logo's navy; the dark theme's background |
| `--dp-blue` | `#20a5ff` | The logo's blue; the accent fill |

## Colour

Semantic tokens, each with a light and a dark value. Components use only
these, never the brand constants directly (except artwork that is dark in
both themes, such as the hero device and plugin monograms).

| Token | Light | Dark | Use |
| --- | --- | --- | --- |
| `--color-bg` | `#ffffff` | `#07112f` | Page background |
| `--color-bg-subtle` | `#f5f7fc` | `#0a1638` | Alternate sections, hovers, table heads |
| `--color-surface` | `#ffffff` | `#0c1a40` | Cards, inputs |
| `--color-surface-raised` | `#ffffff` | `#11214d` | Selected segment, popovers |
| `--color-surface-sunken` | `#eef2fa` | `#050c22` | Wells |
| `--color-border` | `#dde3f0` | `#1d2b57` | Hairlines |
| `--color-border-strong` | `#c3cce0` | `#2e3f72` | Inputs, secondary buttons |
| `--color-text` | `#07112f` | `#eef2fb` | Headings, primary text |
| `--color-text-muted` | `#414d74` | `#b1bcd9` | Body text |
| `--color-text-subtle` | `#5b678c` | `#8f9cc0` | Captions, metadata |
| `--color-accent` | `#20a5ff` | `#20a5ff` | Fills only: primary button, highlights |
| `--color-on-accent` | `#07112f` | `#07112f` | Text on an accent fill |
| `--color-accent-text` | `#0a63b3` | `#6cc4ff` | Links, accent text and icons |
| `--color-accent-soft` | `#e6f4ff` | `#0d2a55` | Accent backgrounds (badges, icon tiles) |
| `--color-success` / `-soft` | `#0f6b46` / `#e3f6ec` | `#5ad6a0` / `#0b2f2d` | Positive state |
| `--color-warning` / `-soft` | `#8a4b00` / `#fff3db` | `#ffc266` / `#33270f` | Caution, pre-release, deprecated |
| `--color-danger` / `-soft` | `#b3261e` / `#fdecea` | `#ff8a80` / `#3a1620` | Errors, revoked |
| `--color-focus` | `#0a63b3` | `#6cc4ff` | Focus ring |
| `--color-code-bg` | `#f5f7fc` | `#050c22` | Code blocks |

Rules:

- **Contrast is WCAG AA at least**: 4.5:1 for text, 3:1 for large text and
  for the boundaries of controls. The brand blue is 2.7:1 on white, so it is
  **never text on a light background**; use `--color-accent-text`. On an
  accent fill the text is navy (7.3:1), in both themes.
- The CI accessibility tests (axe) run on every page type in both themes;
  a new colour pair has to pass them.
- Colour never carries meaning alone: states also have a label or an icon.

### Themes

Light is the default sheet. Dark applies when the system asks for it and
the visitor has made no choice; `data-theme="light"` or `"dark"` on
`<html>` overrides both ways. The choice is stored in `localStorage` under
`dp-theme` and applied by an inline script in the document head before
first paint. The two blocks that hold the dark values in `tokens.css` (the media query
and the attribute selector) must stay identical.

Helpers: `.only-light` and `.only-dark` show an element in one theme (used
for the two wordmarks).

## Typography

| Token | Value | Use |
| --- | --- | --- |
| `--font-sans` | Inter Variable, system fallbacks | Everything |
| `--font-mono` | JetBrains Mono Variable, system fallbacks | Code, versions, identifiers |
| `--text-xs` … `--text-xl` | 13, 14, 16, 18, 20 px | Captions to lead paragraphs |
| `--text-2xl` … `--text-5xl` | fluid (`clamp`) | Headings: section h3, h2, page title, hero |
| `--leading-tight` / `-snug` / `-normal` | 1.1 / 1.3 / 1.6 | Display / headings / body |
| `--tracking-tight` / `-display` / `-caps` | -0.022em / -0.035em / 0.08em | Headings / large display / uppercase labels |

Body text is 16px at 1.6; documentation prose is at most 46rem wide.
Headings are weight 650 with negative tracking, balanced with `text-wrap`.
Uppercase is only for the small `.eyebrow` and metadata labels. Both fonts
cover Polish (`latin-ext`); the browser downloads only the subsets a page
uses.

## Space, layout, shape, motion

| Token | Value |
| --- | --- |
| `--space-1` … `--space-24` | A 4px grid: 4, 8, 12, 16, 20, 24, 32, 40, 48, 64, 80, 96 px |
| `--space-section` | Fluid vertical padding of a page section (64 to 120 px) |
| `--container` / `--container-narrow` | 72rem / 46rem content widths |
| `--gutter` | Fluid side padding (20 to 40 px) |
| `--header-height` | 4rem |
| `--radius-sm` / `-md` / `-lg` / `-xl` / `-full` | 6 / 10 / 14 / 20 px / pill |
| `--shadow-sm` / `-md` / `-lg` | Resting card / hover / floating |
| `--duration-fast` / `-base` / `-slow` | 120 / 200 / 360 ms |
| `--ease-out` | The one easing curve |

- Mobile first. Breakpoints: 48rem (two columns), 60rem (desktop
  navigation, docs sidebar), 72rem and 80rem (wide grids, table of contents).
- Sections alternate `--color-bg` and `--color-bg-subtle` (`.section-alt`).
- Gradients are restrained: one soft blue glow and a faint grid behind a
  hero (`.hero`), nothing else.
- Motion is small (a 2px lift, a colour change) and is switched off under
  `prefers-reduced-motion`.

## Components

Each has a class in `components.css` and, where it helps, an Astro wrapper
in `src/components/`.

| Component | Class | Astro | Notes |
| --- | --- | --- | --- |
| Container | `.container`, `.container-narrow` | | Page width and gutters |
| Section | `.section`, `.section-alt`, `.section-head`, `.eyebrow`, `.section-title`, `.section-lead` | `Section.astro` | Heading block plus rhythm; sets `aria-labelledby` |
| Header | `.site-header`, `.brand` | `Header.astro` | Sticky, translucent; contains navigation and both switchers |
| Navigation | `.nav-list`, `.nav-link` | `Navigation.astro` | Marks the current section with `aria-current="page"`; a disclosure below 60rem |
| Language switcher | `.lang-switch` | `LanguageSwitcher.astro` | Links to the same page in each language |
| Theme switcher | `.theme-toggle` | `ThemeSwitcher.astro` | One button; shows the theme it switches to |
| Footer | `.site-footer`, `.footer-list` | `Footer.astro` | |
| Button | `.btn` + `.btn-primary` / `.btn-secondary` / `.btn-ghost`, `.btn-sm`, `.btn-icon` | `Button.astro` | One primary per view. A link when it navigates, a `<button>` when it acts. 44px tall |
| Text link with arrow | `.link-arrow` | | "Read more" style links |
| Card | `.card`, `.card-title`, `.card-text`, `.card-link`, `.icon-tile` | `Card.astro` | With `href` the whole card is one link (the title carries it) |
| Plugin card, plugin icon | `.plugin-icon` | `PluginCard.astro`, `PluginIcon.astro` | A monogram: the registry has no icon files |
| Badge | `.badge` + `-accent` / `-success` / `-warning` / `-danger`, `.badge-mono` | `Badge.astro` | Short, not interactive |
| Notice | `.notice` + `-warning` / `-danger` | `Notice.astro` | Inline status with an icon |
| Search field | `.field`, `.input`, `.input-lg` | `SearchField.astro` | Always has a label (visible or `sr-only`) |
| Filters | `.filter-bar`, `.select`, `.label`, `.result-bar` | | Native `<select>`; result count is a live region |
| Empty state | `.state` | `EmptyState.astro` | What is missing, why, and what to do next |
| Error state | `.state.state-error` | `ErrorState.astro` | Same anatomy; `live` announces it |
| Code block | `.code-block`, `.code-copy` | `CodeBlock.astro` | Shiki, both themes, copy button. Markdown fences get the button from `scripts/ui.ts` |
| Docs sidebar, table of contents | `.docs-layout`, `.docs-sidebar`, `.side-link`, `.docs-toc`, `.toc-list` | `DocsSidebar.astro` | Sticky on desktop, a disclosure on mobile |
| Prose | `.prose` | | Typography for Markdown: headings, lists, tables, inline code, blockquote notes |
| Table | `.table`, `.table-wrap` | | Scrolls inside itself on small screens |
| FAQ | `.faq` | | Native `<details>` |
| Steps | `.steps`, `.step` | | Numbered cards from an `<ol>` |
| Breadcrumb | `.breadcrumb` | | |
| Metadata list | `.meta-list` | | `<dl>` of label and value |
| Hero | `.hero`, `.hero-title`, `.hero-lead`, `.deck` | `home/Deck.astro` | The key grid is decorative (`aria-hidden`) |

## Accessibility rules

- Semantic elements first: one `<h1>` per page, headings never skip a
  level, lists are lists, navigation is `<nav>` with a label.
- Everything works with the keyboard; the focus ring (`:focus-visible`,
  2px, `--color-focus`) is never removed. The first stop is the skip link.
- Interactive targets are at least 36px, primary ones 44px.
- Icons are decorative (`aria-hidden`) next to text, or have a text
  alternative when they stand alone. A visible label is part of the
  accessible name.
- Every page declares its language; text in another language is marked
  with `lang`.
- Nothing depends on JavaScript to be readable: the scripts add the theme
  toggle, the mobile menu, copy buttons and directory filtering.

## Voice

Plain, precise and calm. Say what is true today, including what does not
exist yet: no invented numbers, testimonials or plugins. Sentence case for
headings and buttons. Polish is written as Polish, with full diacritics,
not translated word for word.
