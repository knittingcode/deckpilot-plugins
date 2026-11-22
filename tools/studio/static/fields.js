// Form fields that edit a JSON object in place. Each calls onChange()
// after a change; the caller decides when to save.

import { h, uid, announce } from './dom.js';
import { api, fileUrl, toBase64 } from './api.js';

export const LOCALES = ['en', 'pl'];
export const PLATFORMS = ['macos', 'windows', 'linux'];

function setOrDelete(obj, key, value) {
  if (value === undefined || value === null || value === '' ||
      (typeof value === 'object' && !Array.isArray(value) && Object.keys(value).length === 0)) {
    delete obj[key];
  } else {
    obj[key] = value;
  }
}

// A localized text: `en` (required when the field is) and `pl` alongside.
export function localized(obj, key, { label, max, multiline = false, required = false }, onChange) {
  const value = obj[key] && typeof obj[key] === 'object' ? obj[key] : {};
  const rows = LOCALES.map((locale) => {
    const id = uid(key);
    const attrs = {
      id, value: value[locale] || '', maxLength: max || undefined, lang: locale,
      'aria-required': locale === 'en' && required ? 'true' : undefined,
      oninput: (event) => {
        const next = { ...(obj[key] || {}) };
        if (event.target.value) next[locale] = event.target.value; else delete next[locale];
        setOrDelete(obj, key, next);
        counter.textContent = max ? `${event.target.value.length}/${max}` : '';
        onChange();
      },
    };
    const input = multiline ? h('textarea', { ...attrs, rows: 5 }) : h('input', { ...attrs, type: 'text' });
    const counter = h('span', { class: 'meta' }, max ? `${(value[locale] || '').length}/${max}` : '');
    return h('div', { class: 'locale-row' },
      h('label', { for: id }, `${label} (${locale})`,
        locale === 'en' ? h('span', { class: required ? 'req' : 'meta' }, required ? ' required' : ' needed if pl is set') : ''),
      input, counter);
  });
  return h('fieldset', { class: 'localized' }, h('legend', {}, label), rows);
}

export function text(obj, key, { label, pattern, help }, onChange) {
  const id = uid(key);
  return h('div', { class: 'field' },
    h('label', { for: id }, label),
    h('input', {
      id, type: 'text', value: obj[key] ?? '', pattern: pattern || undefined,
      oninput: (event) => { setOrDelete(obj, key, event.target.value); onChange(); },
    }),
    help ? h('p', { class: 'meta' }, help) : '');
}

export function number(obj, key, { label, min, max }, onChange) {
  const id = uid(key);
  return h('div', { class: 'field' },
    h('label', { for: id }, label),
    h('input', {
      id, type: 'number', value: obj[key] ?? '', min, max, step: 1,
      oninput: (event) => {
        const v = event.target.value === '' ? undefined : Number(event.target.value);
        setOrDelete(obj, key, Number.isInteger(v) ? v : undefined);
        onChange();
      },
    }));
}

export function select(obj, key, { label, options, empty }, onChange) {
  const id = uid(key);
  const choices = (empty ? [['', empty]] : []).concat(options.map((o) => (Array.isArray(o) ? o : [o, o])));
  return h('div', { class: 'field' },
    h('label', { for: id }, label),
    h('select', {
      id,
      onchange: (event) => { setOrDelete(obj, key, event.target.value || undefined); onChange(); },
    }, choices.map(([value, name]) => h('option', { value, selected: (obj[key] ?? '') === value }, name))));
}

export function checkbox(obj, key, { label }, onChange) {
  const id = uid(key);
  return h('div', { class: 'field inline' },
    h('input', {
      id, type: 'checkbox', checked: obj[key] === true,
      onchange: (event) => { setOrDelete(obj, key, event.target.checked || undefined); onChange(); },
    }),
    h('label', { for: id }, label));
}

// UTC window: datetime-local inputs read and written as UTC ("…Z").
export function windowField(obj, onChange) {
  const w = obj.window || {};
  const input = (bound, label) => {
    const id = uid(bound);
    return h('div', { class: 'field' },
      h('label', { for: id }, `${label} (UTC)`),
      h('input', {
        id, type: 'datetime-local', step: 1, value: (w[bound] || '').replace(/Z$/, ''),
        onchange: (event) => {
          const next = { ...(obj.window || {}) };
          const v = event.target.value;
          if (v) next[bound] = (v.length === 16 ? `${v}:00` : v) + 'Z'; else delete next[bound];
          setOrDelete(obj, 'window', next);
          onChange();
        },
      }));
  };
  return h('fieldset', { class: 'window' }, h('legend', {}, 'Window (UTC; empty: always)'),
    input('starts_at', 'Starts at'), input('ends_at', 'Ends at'));
}

// Platforms: all three checked means "all" (the field is left out).
export function platforms(obj, onChange) {
  const current = Array.isArray(obj.platforms) ? obj.platforms : PLATFORMS;
  return h('fieldset', { class: 'platforms' }, h('legend', {}, 'Platforms'),
    PLATFORMS.map((p) => {
      const id = uid(p);
      return h('span', { class: 'inline' },
        h('input', {
          id, type: 'checkbox', checked: current.includes(p),
          onchange: (event) => {
            const set = new Set(Array.isArray(obj.platforms) ? obj.platforms : PLATFORMS);
            if (event.target.checked) set.add(p); else set.delete(p);
            const list = PLATFORMS.filter((x) => set.has(x));
            if (list.length === PLATFORMS.length) delete obj.platforms; else obj.platforms = list;
            onChange();
          },
        }),
        h('label', { for: id }, p));
    }));
}

function pluginLabel(plugins, id) {
  const p = plugins[id];
  if (!p) return `${id} (not in registry/)`;
  const state = p.live === true ? 'live'
    : p.live === null ? 'live state unknown (no mirror)'
      : (p.versions.length ? 'recorded, not live yet' : 'not published');
  return `${p.name} · ${id} · ${state}`;
}

export function pluginPicker(obj, key, { label, plugins, single = false }, onChange) {
  const id = uid(key);
  const options = Object.keys(plugins).sort().map((pid) => [pid, pluginLabel(plugins, pid)]);
  if (single) return select(obj, key, { label, options, empty: '— choose a plugin —' }, onChange);
  const wrap = h('fieldset', { class: 'plugins' }, h('legend', {}, label));
  const render = () => {
    const items = (obj[key] || []).map((pid, i) => h('li', {},
      h('span', {}, pluginLabel(plugins, pid)),
      h('button', { type: 'button', 'aria-label': `Move ${pid} up`, disabled: i === 0,
        onclick: () => { move(i, -1); } }, '↑'),
      h('button', { type: 'button', 'aria-label': `Move ${pid} down`, disabled: i === obj[key].length - 1,
        onclick: () => { move(i, 1); } }, '↓'),
      h('button', { type: 'button', 'aria-label': `Remove ${pid}`,
        onclick: () => { obj[key].splice(i, 1); if (!obj[key].length) delete obj[key]; onChange(); render(); } }, 'Remove')));
    const add = h('select', { id, 'aria-label': `Add a plugin to ${label}` },
      h('option', { value: '' }, '— add a plugin —'),
      options.filter(([pid]) => !(obj[key] || []).includes(pid)).map(([value, name]) => h('option', { value }, name)));
    add.addEventListener('change', () => {
      if (!add.value) return;
      obj[key] = [...(obj[key] || []), add.value];
      onChange();
      render();
      announce(`Added ${add.value}`);
    });
    wrap.replaceChildren(h('legend', {}, label), h('ol', {}, items), add);
  };
  const move = (i, delta) => {
    const arr = obj[key];
    [arr[i], arr[i + delta]] = [arr[i + delta], arr[i]];
    onChange();
    render();
    announce(`Moved ${arr[i + delta]} to position ${i + delta + 1}`);
  };
  render();
  return wrap;
}

// One media file: shows it, imports a new one with immediate checks.
// `dir` is the repository path of the media/ directory; obj[key] is "media/<name>".
export function mediaField(obj, key, { label, dir, kind }, onChange) {
  const id = uid(key);
  const out = h('p', { class: 'meta', 'aria-live': 'polite' });
  const preview = h('div', { class: 'thumb' });
  const show = () => {
    preview.replaceChildren(obj[key]
      ? h('img', { src: fileUrl(`${dir}/${obj[key].replace(/^media\//, '')}`), alt: '', loading: 'lazy' })
      : h('span', { class: 'meta' }, 'none'));
  };
  show();
  const input = h('input', { id, type: 'file', accept: 'image/png,image/webp,image/jpeg' });
  input.addEventListener('change', async () => {
    const file = input.files[0];
    if (!file) return;
    let base = file.name.toLowerCase().replace(/\.[a-z0-9]+$/, '').replace(/[^a-z0-9-]+/g, '-')
      .replace(/^-+|-+$/g, '').slice(0, 48) || kind;
    let ext = (file.name.toLowerCase().match(/\.([a-z0-9]+)$/) || [])[1] || '';
    if (ext === 'jpeg') ext = 'jpg';
    if (!['png', 'webp', 'jpg'].includes(ext)) {
      out.textContent = `Refused: ${file.name} is not .png, .webp or .jpg (SVG is refused).`;
      return;
    }
    out.textContent = 'Checking…';
    try {
      const result = await api('POST', '/api/media', {
        path: `${dir}/${base}.${ext}`, kind, data: await toBase64(file),
      });
      if (!result.written) {
        out.textContent = `Refused: ${result.problems.join('; ')}`;
        return;
      }
      obj[key] = `media/${base}.${ext}`;
      const i = result.info;
      out.textContent = `Imported ${base}.${ext}: ${i.format.toUpperCase()} ${i.width}×${i.height}, ` +
        `${file.size} bytes. ${result.note}.`;
      onChange();
      show();
    } catch (error) {
      out.textContent = `Refused: ${error.message}`;
    }
  });
  return h('div', { class: 'field media' },
    h('label', { for: id }, `${label} (${kind})`), preview, input, out);
}
