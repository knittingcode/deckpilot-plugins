// Marketplace Studio UI: edits editorial/editorial.json and listings through
// studio.py, which checks and writes them. No dependency, no bundler.

import { h, clear, announce } from './dom.js';
import { api } from './api.js';
import {
  localized, text, number, select, checkbox, windowField, platforms, pluginPicker, mediaField, LOCALES, PLATFORMS,
} from './fields.js';

const KINDS = ['banner_carousel', 'collection_row', 'spotlight', 'recently_added', 'category_highlights', 'catalog'];
const STATUSES = ['draft', 'published', 'archived'];
const state = { data: null, editorial: null, dirty: false, selected: null, listingId: null, listing: null };

const $ = (id) => document.getElementById(id);

function setDirty(value = true) {
  state.dirty = value;
  const button = $('save-editorial');
  if (button) button.textContent = value ? 'Save editorial.json *' : 'Save editorial.json';
}

window.addEventListener('beforeunload', (event) => {
  if (state.dirty) event.preventDefault();
});

// --------------------------------------------------------------------------
// Tabs (arrow keys move between them)
// --------------------------------------------------------------------------

function setupTabs() {
  const tabs = [...document.querySelectorAll('[role="tab"]')];
  const activate = (tab) => {
    for (const t of tabs) {
      const on = t === tab;
      t.setAttribute('aria-selected', String(on));
      t.tabIndex = on ? 0 : -1;
      $(t.getAttribute('aria-controls')).hidden = !on;
    }
    tab.focus();
    const panel = tab.id.replace('tab-', '');
    if (panel === 'preview') renderPreview();
  };
  tabs.forEach((tab, i) => {
    tab.addEventListener('click', () => activate(tab));
    tab.addEventListener('keydown', (event) => {
      if (event.key === 'ArrowRight') activate(tabs[(i + 1) % tabs.length]);
      if (event.key === 'ArrowLeft') activate(tabs[(i + tabs.length - 1) % tabs.length]);
    });
  });
}

// --------------------------------------------------------------------------
// State
// --------------------------------------------------------------------------

async function load() {
  state.data = await api('GET', '/api/state');
  state.editorial = structuredClone(state.data.editorial);
  state.editorial.sections ||= [];
  state.editorial.collections ||= [];
  const live = state.data.live;
  const liveText = live
    ? `live index seq ${live.seq ?? '?'}${live.verified === true ? ', verified' : live.verified === false ? ', NOT verified' : ', not verified (CLI not built)'}`
    : 'live index not mirrored (offline)';
  $('status').textContent = `${state.data.root} · branch ${state.data.branch} · ${liveText} · ` +
    `Rust storefront check: ${state.data.cli.storefrontCheck ? 'available' : 'not in the pinned CLI (header-level checks only)'}`;
  setDirty(false);
}

// --------------------------------------------------------------------------
// Editorial: sections and collections
// --------------------------------------------------------------------------

function renumber(list) {
  list.forEach((entry, i) => { entry.order = (i + 1) * 10; });
}

function summary(entry) {
  const w = entry.window || {};
  const window = w.starts_at || w.ends_at ? ` · ${w.starts_at || '…'} → ${w.ends_at || '…'}` : '';
  const plats = entry.platforms ? ` · ${entry.platforms.join(', ')}` : '';
  return `${entry.kind ? `${entry.kind} · ` : ''}order ${entry.order ?? '?'}${window}${plats}`;
}

function reorderList(listName, label) {
  const list = state.editorial[listName];
  const ol = h('ol', { class: 'reorder', 'aria-label': `${label}, in display order. Alt+Arrow keys move the focused entry.` });
  const move = (i, delta) => {
    const j = i + delta;
    if (j < 0 || j >= list.length) return;
    [list[i], list[j]] = [list[j], list[i]];
    renumber(list);
    setDirty();
    renderEditorial();
    announce(`${list[j].id} moved to position ${j + 1} of ${list.length}`);
    const row = document.querySelector(`[data-list="${listName}"][data-index="${j}"]`);
    if (row) row.focus();
  };
  list.forEach((entry, i) => {
    const selected = state.selected && state.selected.list === listName && state.selected.index === i;
    const row = h('li', {
      tabindex: 0, 'data-list': listName, 'data-index': i, class: selected ? 'selected' : '',
      'aria-current': selected ? 'true' : undefined,
      onkeydown: (event) => {
        if (event.altKey && event.key === 'ArrowUp') { event.preventDefault(); move(i, -1); }
        if (event.altKey && event.key === 'ArrowDown') { event.preventDefault(); move(i, 1); }
        if (event.key === 'Enter') { state.selected = { list: listName, index: i }; renderEditorial(); }
      },
    },
    h('span', { class: `badge ${entry.status}` }, entry.status || '?'),
    h('strong', {}, entry.id || '(no id)'),
    h('span', { class: 'meta' }, summary(entry)),
    h('span', { class: 'actions' },
      h('button', { type: 'button', 'aria-label': `Move ${entry.id} up`, disabled: i === 0, onclick: () => move(i, -1) }, '↑'),
      h('button', { type: 'button', 'aria-label': `Move ${entry.id} down`, disabled: i === list.length - 1, onclick: () => move(i, 1) }, '↓'),
      h('button', { type: 'button', onclick: () => { state.selected = { list: listName, index: i }; renderEditorial(); } }, 'Edit'),
      h('button', {
        type: 'button', 'aria-label': `Delete ${entry.id}`,
        onclick: () => {
          if (!window.confirm(`Delete ${entry.id}? (Archiving keeps it in the file instead.)`)) return;
          list.splice(i, 1);
          state.selected = null;
          setDirty();
          renderEditorial();
        },
      }, 'Delete')));
    ol.append(row);
  });
  return ol;
}

function newSection(kind) {
  const base = { id: `${kind.replace(/_/g, '-')}-${state.editorial.sections.length + 1}`, status: 'draft', kind, order: 0 };
  if (kind === 'banner_carousel') base.items = [];
  if (kind === 'collection_row') Object.assign(base, { layout: 'cards', title: { en: '' }, plugins: [] });
  if (kind === 'spotlight') Object.assign(base, { plugin: '', headline: { en: '' } });
  if (kind === 'recently_added') Object.assign(base, { limit: 8, title: { en: 'Recently added' } });
  if (kind === 'category_highlights') base.categories = [];
  return base;
}

function sectionEditor(entry) {
  const changed = () => { setDirty(); refreshList(); };
  const plugins = state.data.plugins;
  const common = [
    text(entry, 'id', { label: 'Id', pattern: '[a-z0-9][a-z0-9-]{0,47}', help: '[a-z0-9-], unique' }, changed),
    select(entry, 'status', { label: 'Status', options: STATUSES }, changed),
    number(entry, 'order', { label: 'Order (ties: by id)', min: 0, max: 1000000 }, changed),
    windowField(entry, changed),
    platforms(entry, changed),
  ];
  const fields = [];
  const kind = entry.kind;
  if (kind !== 'spotlight') fields.push(localized(entry, 'title', { label: 'Title', max: 60, required: ['collection_row', 'recently_added'].includes(kind) }, changed));
  if (kind === 'banner_carousel') fields.push(bannerItems(entry, changed));
  if (kind === 'collection_row') {
    fields.push(select(entry, 'layout', { label: 'Layout', options: ['cards', 'compact'] }, changed));
    fields.push(localized(entry, 'description', { label: 'Description', max: 200 }, changed));
    fields.push(pluginPicker(entry, 'plugins', { label: 'Plugins (1-24)', plugins }, changed));
  }
  if (kind === 'spotlight') {
    fields.push(pluginPicker(entry, 'plugin', { label: 'Plugin', plugins, single: true }, changed));
    const versions = ((plugins[entry.plugin] || {}).versions || []).map((v) => v.version);
    fields.push(select(entry, 'version', { label: 'Release notes of version', options: versions, empty: '— none —' }, changed));
    fields.push(localized(entry, 'headline', { label: 'Headline', max: 60, required: true }, changed));
    fields.push(localized(entry, 'body', { label: 'Body', max: 200 }, changed));
  }
  if (kind === 'recently_added') fields.push(number(entry, 'limit', { label: 'Limit (4-24)', min: 4, max: 24 }, changed));
  if (kind === 'category_highlights') fields.push(categories(entry, changed));
  return [h('h3', {}, `Section ${entry.id} (${kind})`), common, fields];
}

function collectionEditor(entry) {
  const changed = () => { setDirty(); refreshList(); };
  return [h('h3', {}, `Collection ${entry.id}`),
    text(entry, 'id', { label: 'Id', pattern: '[a-z0-9][a-z0-9-]{0,47}' }, changed),
    select(entry, 'status', { label: 'Status', options: STATUSES }, changed),
    number(entry, 'order', { label: 'Order', min: 0, max: 1000000 }, changed),
    windowField(entry, changed), platforms(entry, changed),
    localized(entry, 'title', { label: 'Title', max: 60, required: true }, changed),
    localized(entry, 'description', { label: 'Description', max: 200 }, changed),
    pluginPicker(entry, 'plugins', { label: 'Plugins (1-24)', plugins: state.data.plugins }, changed)];
}

function bannerItems(entry, changed) {
  entry.items ||= [];
  const box = h('fieldset', { class: 'items' }, h('legend', {}, 'Banners (1-6)'));
  const render = () => {
    const rows = entry.items.map((item, i) => {
      item.artwork ||= {};
      item.target ||= { type: 'plugin', id: '' };
      const targetIds = item.target.type === 'collection'
        ? state.editorial.collections.map((c) => c.id)
        : item.target.type === 'category'
          ? [...new Set(Object.values(state.data.plugins).flatMap((p) => p.categories))].sort()
          : Object.keys(state.data.plugins).sort();
      return h('div', { class: 'item' },
        h('h4', {}, `Banner ${i + 1}: ${item.id || ''}`),
        text(item, 'id', { label: 'Id' }, changed),
        localized(item, 'title', { label: 'Title', max: 60, required: true }, changed),
        localized(item, 'body', { label: 'Body', max: 200 }, changed),
        localized(item, 'cta', { label: 'Call to action', max: 24 }, changed),
        mediaField(item.artwork, 'file', { label: 'Banner art 3:1', dir: 'editorial/media', kind: 'banner' }, changed),
        checkbox(item.artwork, 'decorative', { label: 'Decorative (no alt text)' }, () => {
          if (item.artwork.decorative) delete item.artwork.alt;
          changed();
          render();
        }),
        item.artwork.decorative ? '' : localized(item.artwork, 'alt', { label: 'Alt text', max: 250, required: true }, changed),
        select(item.target, 'type', { label: 'Target', options: ['plugin', 'collection', 'category'] }, () => { item.target.id = ''; changed(); render(); }),
        select(item.target, 'id', { label: 'Target id', options: targetIds, empty: '— choose —' }, changed),
        h('div', { class: 'row' },
          h('button', { type: 'button', disabled: i === 0, onclick: () => { [entry.items[i - 1], entry.items[i]] = [entry.items[i], entry.items[i - 1]]; changed(); render(); } }, 'Move up'),
          h('button', { type: 'button', disabled: i === entry.items.length - 1, onclick: () => { [entry.items[i + 1], entry.items[i]] = [entry.items[i], entry.items[i + 1]]; changed(); render(); } }, 'Move down'),
          h('button', { type: 'button', onclick: () => { entry.items.splice(i, 1); changed(); render(); } }, 'Remove banner')));
    });
    box.replaceChildren(h('legend', {}, 'Banners (1-6)'), rows,
      h('button', { type: 'button', disabled: entry.items.length >= 6, onclick: () => {
        entry.items.push({ id: `banner-${entry.items.length + 1}`, title: { en: '' }, artwork: {}, target: { type: 'plugin', id: '' } });
        changed();
        render();
      } }, 'Add banner'));
  };
  render();
  return box;
}

function categories(entry, changed) {
  entry.categories ||= [];
  const all = [...new Set(Object.values(state.data.plugins).flatMap((p) => p.categories))].sort();
  const box = h('fieldset', {}, h('legend', {}, 'Categories (1-12)'));
  const render = () => {
    box.replaceChildren(h('legend', {}, 'Categories (1-12)'),
      entry.categories.map((c, i) => h('div', { class: 'item' },
        select(c, 'id', { label: 'Category', options: all }, changed),
        localized(c, 'blurb', { label: 'Blurb', max: 160 }, changed),
        h('button', { type: 'button', onclick: () => { entry.categories.splice(i, 1); changed(); render(); } }, 'Remove'))),
      h('button', { type: 'button', disabled: entry.categories.length >= 12, onclick: () => {
        entry.categories.push({ id: all[0] || 'monitoring' });
        changed();
        render();
      } }, 'Add category'));
  };
  render();
  return box;
}

function refreshList() {
  for (const name of ['sections', 'collections']) {
    const holder = document.querySelector(`[data-holder="${name}"]`);
    if (holder) holder.replaceChildren(reorderList(name, name === 'sections' ? 'Sections' : 'Collections'));
  }
}

function renderEditorial() {
  const panel = $('panel-editorial');
  const kindSelect = h('select', { 'aria-label': 'Kind of the new section' }, KINDS.map((k) => h('option', { value: k }, k)));
  const errors = h('div', { id: 'editorial-errors', 'aria-live': 'polite' });
  let editor = h('p', { class: 'meta' }, 'Choose an entry to edit (Edit, or Enter on a focused row).');
  if (state.selected) {
    const entry = state.editorial[state.selected.list][state.selected.index];
    if (entry) editor = h('div', {}, state.selected.list === 'sections' ? sectionEditor(entry) : collectionEditor(entry));
  }
  clear(panel,
    h('div', { class: 'toolbar' },
      h('button', { id: 'save-editorial', type: 'button', onclick: saveEditorial }, 'Save editorial.json'),
      h('button', { type: 'button', onclick: async () => {
        if (state.dirty && !window.confirm('Discard unsaved changes?')) return;
        await load();
        state.selected = null;
        renderEditorial();
      } }, 'Reload from disk'),
      state.data.editorialExists ? '' : h('span', { class: 'meta' }, 'editorial/editorial.json does not exist yet; saving creates it.')),
    errors,
    h('div', { class: 'split' },
      h('div', { class: 'lists' },
        h('h2', {}, 'Sections'),
        h('div', { 'data-holder': 'sections' }, reorderList('sections', 'Sections')),
        h('div', { class: 'row' }, kindSelect, h('button', {
          type: 'button', disabled: state.editorial.sections.length >= 12,
          onclick: () => {
            state.editorial.sections.push(newSection(kindSelect.value));
            renumber(state.editorial.sections);
            state.selected = { list: 'sections', index: state.editorial.sections.length - 1 };
            setDirty();
            renderEditorial();
          },
        }, 'Add section')),
        h('h2', {}, 'Collections'),
        h('div', { 'data-holder': 'collections' }, reorderList('collections', 'Collections')),
        h('button', {
          type: 'button', disabled: state.editorial.collections.length >= 16,
          onclick: () => {
            state.editorial.collections.push({ id: `collection-${state.editorial.collections.length + 1}`, status: 'draft', order: 0, title: { en: '' }, plugins: [] });
            renumber(state.editorial.collections);
            state.selected = { list: 'collections', index: state.editorial.collections.length - 1 };
            setDirty();
            renderEditorial();
          },
        }, 'Add collection')),
      h('div', { class: 'editor' }, editor)));
  setDirty(state.dirty);
}

function showErrors(holder, errors, ok) {
  clear(holder, errors.length
    ? h('div', { class: 'errors' }, h('p', {}, `Saved; ${errors.length} schema error(s) to fix before a pull request:`),
      h('ul', {}, errors.map((e) => h('li', {}, e))))
    : h('p', { class: 'ok' }, ok));
}

async function saveEditorial() {
  const doc = structuredClone(state.editorial);
  if (!doc.collections.length) delete doc.collections;
  doc.$schema ||= '../schemas/inputs/editorial.schema.json';
  doc.editorial_version ||= 1;
  try {
    const result = await api('PUT', '/api/file', { path: 'editorial/editorial.json', json: doc });
    setDirty(false);
    state.data.editorialExists = true;
    showErrors($('editorial-errors'), result.errors, 'Saved editorial/editorial.json; it matches the schema.');
    announce('Saved');
  } catch (error) {
    clear($('editorial-errors'), h('p', { class: 'errors' }, `Not saved: ${error.message}`));
  }
}

// --------------------------------------------------------------------------
// Listings
// --------------------------------------------------------------------------

function renderListings() {
  const panel = $('panel-listings');
  const plugins = state.data.plugins;
  const chooser = h('select', { 'aria-label': 'Plugin' },
    h('option', { value: '' }, '— choose a plugin —'),
    Object.keys(plugins).sort().map((pid) => h('option', { value: pid, selected: pid === state.listingId },
      `${plugins[pid].name} · ${pid}${state.data.listings[pid] ? ' · has listing' : ''}`)));
  chooser.addEventListener('change', () => {
    state.listingId = chooser.value || null;
    state.listing = state.listingId
      ? structuredClone(state.data.listings[state.listingId] || { $schema: '../../../schemas/inputs/listing.schema.json', listing_version: 1, id: state.listingId })
      : null;
    renderListings();
  });
  const body = [];
  const errors = h('div', { 'aria-live': 'polite' });
  if (state.listing) {
    const l = state.listing;
    const pid = state.listingId;
    const dir = `registry/plugins/${pid}/media`;
    const changed = () => {};
    l.highlights ||= [];
    l.screenshots ||= [];
    l.links ||= {};
    l.release_notes ||= {};
    const shots = h('fieldset', {}, h('legend', {}, 'Screenshots (0-8)'));
    const renderShots = () => {
      shots.replaceChildren(h('legend', {}, 'Screenshots (0-8)'),
        l.screenshots.map((s, i) => h('div', { class: 'item' },
          mediaField(s, 'file', { label: `Screenshot ${i + 1} 16:10 or 16:9`, dir, kind: 'screenshot' }, changed),
          localized(s, 'alt', { label: 'Alt text', max: 250, required: true }, changed),
          localized(s, 'caption', { label: 'Caption', max: 120 }, changed),
          platforms(s, changed),
          h('button', { type: 'button', onclick: () => { l.screenshots.splice(i, 1); renderShots(); } }, 'Remove screenshot'))),
        h('button', { type: 'button', disabled: l.screenshots.length >= 8, onclick: () => { l.screenshots.push({ alt: { en: '' } }); renderShots(); } }, 'Add screenshot'));
    };
    renderShots();
    const highlights = h('fieldset', {}, h('legend', {}, 'Highlights (0-6)'));
    const renderHighlights = () => {
      highlights.replaceChildren(h('legend', {}, 'Highlights (0-6)'),
        l.highlights.map((x, i) => h('div', { class: 'item' },
          localized(x, 'title', { label: 'Title', max: 40, required: true }, changed),
          localized(x, 'text', { label: 'Text', max: 160, required: true }, changed),
          h('button', { type: 'button', onclick: () => { l.highlights.splice(i, 1); renderHighlights(); } }, 'Remove highlight'))),
        h('button', { type: 'button', disabled: l.highlights.length >= 6, onclick: () => { l.highlights.push({ title: { en: '' }, text: { en: '' } }); renderHighlights(); } }, 'Add highlight'));
    };
    renderHighlights();
    l.icon ||= {};
    l.promo_art ||= {};
    const versions = (plugins[pid].versions || []).map((v) => v.version);
    body.push(
      h('p', { class: 'meta' }, plugins[pid].live ? 'Live in the signed index.' : 'Not in the live index yet.'),
      localized(l, 'promo', { label: 'Promo line', max: 80 }, changed),
      localized(l, 'description_md', { label: 'Description (safe markdown)', max: 6000, multiline: true }, changed),
      highlights,
      mediaField(l.icon, 'file', { label: 'Icon 1:1', dir, kind: 'icon' }, changed),
      shots,
      mediaField(l.promo_art, 'file', { label: 'Promo art 2:1', dir, kind: 'promo_art' }, changed),
      localized(l.promo_art, 'alt', { label: 'Promo art alt text', max: 250 }, changed),
      h('fieldset', {}, h('legend', {}, 'Links (https)'),
        ['documentation', 'support', 'changelog'].map((k) => text(l.links, k, { label: k }, changed))),
      h('fieldset', {}, h('legend', {}, 'Release notes (recorded versions only)'),
        versions.length ? versions.map((v) => localized(l.release_notes, v, { label: v, max: 2000, multiline: true }, changed))
          : h('p', { class: 'meta' }, 'No recorded version yet.')),
      h('button', { type: 'button', onclick: () => saveListing(errors) }, `Save registry/plugins/${pid}/listing.json`),
      errors);
  }
  clear(panel, h('h2', {}, 'Listings'),
    h('p', { class: 'meta' }, 'An author normally submits their own listing; the owner edits deckpilot.* listings here. A listing never places a plugin.'),
    chooser, body);
}

function pruneEmpty(value) {
  if (Array.isArray(value)) return value.map(pruneEmpty).filter((v) => v !== undefined);
  if (value && typeof value === 'object') {
    const out = {};
    for (const [k, v] of Object.entries(value)) {
      const p = pruneEmpty(v);
      if (p === undefined || p === '' || (typeof p === 'object' && !Array.isArray(p) && !Object.keys(p).length) ||
          (Array.isArray(p) && !p.length)) continue;
      out[k] = p;
    }
    return out;
  }
  return value;
}

async function saveListing(holder) {
  const doc = pruneEmpty(structuredClone(state.listing));
  doc.listing_version = 1;
  doc.id = state.listingId;
  try {
    const result = await api('PUT', '/api/file', { path: `registry/plugins/${state.listingId}/listing.json`, json: doc });
    state.data.listings[state.listingId] = doc;
    showErrors(holder, result.errors, 'Saved; it matches the schema.');
  } catch (error) {
    clear(holder, h('p', { class: 'errors' }, `Not saved: ${error.message}`));
  }
}

// --------------------------------------------------------------------------
// Preview, validate, fixture
// --------------------------------------------------------------------------

const preview = { as_of: '', platform: 'macos', locale: 'en', drafts: false };

function renderPreview() {
  const panel = $('panel-preview');
  const frame = h('iframe', { title: 'Discover wireframe', class: 'preview', sandbox: 'allow-same-origin' });
  const reload = () => {
    const q = new URLSearchParams({ platform: preview.platform, locale: preview.locale, drafts: preview.drafts ? '1' : '0' });
    if (preview.as_of) q.set('as_of', preview.as_of);
    frame.src = `/preview?${q}`;
  };
  const out = h('p', { class: 'meta', 'aria-live': 'polite' });
  clear(panel, h('h2', {}, 'Preview'),
    h('p', { class: 'meta' }, 'Saved files only (save first). The same rules as the client: status, UTC window, platform, locale fallback (exact, language, en), order then id, unknown or uninstallable plugins dropped.'),
    h('div', { class: 'row' },
      text(preview, 'as_of', { label: 'As of (UTC, e.g. 2026-12-01T00:00:00Z; empty: now)' }, () => {}),
      select(preview, 'platform', { label: 'Platform', options: PLATFORMS }, reload),
      select(preview, 'locale', { label: 'Locale', options: [...LOCALES, 'pl-PL', 'de'] }, reload),
      checkbox(preview, 'drafts', { label: 'Show drafts' }, reload),
      h('button', { type: 'button', onclick: reload }, 'Refresh'),
      h('button', { type: 'button', onclick: async () => {
        try {
          const r = await api('POST', '/api/export-fixture', preview);
          out.textContent = `Wrote ${r.path} (${r.sections} section(s); provisional shape) for VITE_STOREFRONT_FIXTURE.`;
        } catch (error) { out.textContent = error.message; }
      } }, 'Export fixture')),
    out, frame);
  reload();
}

function renderValidate() {
  const panel = $('panel-validate');
  const out = h('div', { 'aria-live': 'polite' });
  clear(panel, h('h2', {}, 'Validate'),
    h('p', { class: 'meta' }, 'Checks the saved files: schemas, cross-file rules, text, URLs, markdown, header-level media checks, and the pinned deckpilot-registry storefront check when it exists.'),
    h('button', { type: 'button', onclick: async () => {
      out.textContent = 'Checking…';
      try {
        const r = await api('POST', '/api/validate', {});
        clear(out,
          h('p', { class: r.errors.length ? 'errors' : 'ok' }, `${r.errors.length} error(s), ${r.warnings.length} warning(s)`),
          h('ul', {}, r.errors.map((e) => h('li', { class: 'error' }, e)), r.warnings.map((w) => h('li', { class: 'warning' }, w))),
          h('p', { class: 'meta' }, r.rust.ran ? `Rust storefront check: ${r.rust.ok ? 'passed' : 'FAILED'}` : `Rust storefront check skipped: ${r.rust.why}. CI's check is authoritative.`),
          r.rust.ran ? h('pre', {}, r.rust.output) : '');
      } catch (error) { out.textContent = error.message; }
    } }, 'Validate'),
    h('button', { type: 'button', onclick: async () => {
      out.textContent = 'Mirroring the live registry…';
      try {
        const r = await api('POST', '/api/mirror', {});
        out.textContent = `Live registry: ${r.state || 'unavailable'}${r.verified ? ', verified' : ''}.`;
        await load();
      } catch (error) { out.textContent = error.message; }
    } }, 'Refresh the live index'),
    out);
}

// --------------------------------------------------------------------------
// Prepare PR: every step asks first
// --------------------------------------------------------------------------

function renderPr() {
  const panel = $('panel-pr');
  const log = h('pre', { 'aria-live': 'polite' });
  const form = { slug: '', message: 'feat(editorial): ' };
  const step = async (name, question, extra = {}) => {
    if (!window.confirm(question)) return;
    try {
      const r = await api('POST', '/api/pr', { step: name, confirm: true, ...extra });
      log.textContent += `$ ${r.command.join(' ')}\n${r.output}\n${r.ok ? '' : '(failed)\n'}`;
    } catch (error) { log.textContent += `${name}: ${error.message}\n`; }
  };
  clear(panel, h('h2', {}, 'Prepare PR'),
    h('p', { class: 'meta' }, 'Studio commits only editorial/ and listing/media changes, never tags and never publishes. Publication stays the owner\'s registry--v… tag after review.'),
    h('button', { type: 'button', onclick: async () => {
      const r = await api('POST', '/api/pr', { step: 'status' });
      log.textContent = `Branch ${r.branch}\nChanged inputs:\n${r.changed.map((p) => `  ${p}`).join('\n') || '  (none)'}\n` +
        (r.other.length ? `Other changes (never committed by Studio):\n${r.other.map((p) => `  ${p}`).join('\n')}\n` : '');
    } }, '1. Show changes'),
    text(form, 'slug', { label: 'Branch slug (editorial/<date>-<slug>)', pattern: '[a-z0-9][a-z0-9-]{0,40}' }, () => {}),
    h('button', { type: 'button', onclick: () => step('branch', `Create and switch to editorial/<today>-${form.slug}?`, { slug: form.slug }) }, '2. Create branch'),
    h('button', { type: 'button', onclick: () => step('add', 'Stage the changed editorial and listing files?') }, '3. Add'),
    text(form, 'message', { label: 'Commit message (one line)' }, () => {}),
    h('button', { type: 'button', onclick: () => step('commit', `Commit with "${form.message}"?`, { message: form.message }) }, '4. Commit'),
    h('button', { type: 'button', onclick: () => step('push', 'Push this editorial branch to origin?') }, '5. Push'),
    h('button', { type: 'button', onclick: () => step('pr', 'Open a pull request with gh pr create --fill?') }, '6. Open pull request'),
    log);
}

// --------------------------------------------------------------------------

async function main() {
  setupTabs();
  try {
    await load();
  } catch (error) {
    $('status').textContent = `Cannot load: ${error.message}`;
    return;
  }
  renderEditorial();
  renderListings();
  renderValidate();
  renderPr();
}

main();
