// Calls to studio.py. Every write carries the per-launch token in a header
// (the server refuses a write that only has the cookie).

const token = document.querySelector('meta[name="studio-token"]').content;

export async function api(method, path, body) {
  const init = { method, headers: {}, credentials: 'same-origin' };
  if (method !== 'GET') {
    init.headers['Content-Type'] = 'application/json';
    init.headers['X-Studio-Token'] = token;
    init.body = JSON.stringify(body || {});
  }
  const response = await fetch(path, init);
  const data = await response.json().catch(() => ({ error: `HTTP ${response.status}` }));
  if (!response.ok) throw new Error(data.error || `HTTP ${response.status}`);
  return data;
}

export function fileUrl(path) {
  return `/api/file?path=${encodeURIComponent(path)}`;
}

export async function toBase64(file) {
  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = '';
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(binary);
}
