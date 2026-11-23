/**
 * Build-time reader of the published plugin registry.
 *
 * The website is a read-only consumer. It is given a registry tree (the
 * `v1/` directory DeckPilot clients read: timestamp.json, index/<seq>.json,
 * plugins/<id>/<sha256>.json) through environment variables and turns it
 * into the catalog the pages render. It never writes to that tree, never
 * signs anything and never downloads or opens a plugin package.
 *
 *   REGISTRY_DIR     the v1/ directory; unset or without timestamp.json
 *                    means no registry is published: the catalog is empty.
 *   REGISTRY_TRUST   required when a registry is present:
 *                    `verified`  the caller ran DeckPilot's own client
 *                                checks on this exact tree
 *                                (`deckpilot-registry verify-site`, as the
 *                                deploy workflow does) before the build;
 *                    `fixture`   test data. The build marks every page and
 *                                the deploy pipeline refuses the output.
 *   MANIFESTS_DIR    directory with <id>/manifest.json copies (the
 *                    repository's plugins/). A manifest is used only when
 *                    its sha256 equals the `manifest_sha256` the signed
 *                    registry pins for the plugin's latest version.
 *
 * Signatures are verified by the caller, with DeckPilot's client code. This
 * module re-checks the hash chain a client checks after that (timestamp ->
 * index -> details: length and sha256), so a tree that was altered between
 * verification and the build fails the build instead of being shown.
 */
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

export type Trust = 'verified' | 'fixture';

export type Permission = {
  kind: string;
  justification: string;
  hosts?: string[];
  methods?: string[];
  privateNetwork?: boolean;
  userOrigins?: unknown[];
  refs?: string[];
  read?: string[];
  quotaKb?: number;
  minIntervalSeconds?: number;
  state?: boolean;
  icons?: boolean;
};

export type PluginVersion = {
  version: string;
  publishedAt: string;
  engines: { deckpilot: string; pluginApi: string; sdk?: string };
  /** Empty means every platform (the manifest omits `platforms`). */
  platforms: Platform[];
  permissions: Permission[];
  manifestSha256: string;
  source: { repository: string; tag: string; commit: string; path?: string };
  package: { url: string; sha256: string; size: number };
  yanked?: { at: string; reason: string };
};

export type ManifestAction = {
  id: string;
  name: string;
  description: string;
  parameters: { key: string; label: string; required: boolean; type: string }[];
  outputs: { key: string; label: string }[];
  deprecated: boolean;
};

export type Revocation = { versions: '*' | string[]; action: 'yank' | 'disable'; reason: string; advisory?: string; at: string };

export type Platform = 'macos' | 'windows' | 'linux';
export const PLATFORMS: readonly Platform[] = ['macos', 'windows', 'linux'];

export type CatalogPlugin = {
  id: string;
  name: string;
  summary: string;
  description: string;
  license: string;
  homepage?: string;
  sourceRepository?: string;
  categories: string[];
  publisher: { namespace: string; login: string; displayName: string; verified: boolean };
  /** The version the registry offers: latest stable, else latest pre-release. */
  latest: PluginVersion;
  latestIsPrerelease: boolean;
  /** Newest first. */
  versions: PluginVersion[];
  deprecated?: { at: string; reason: string; replacement?: string };
  /** Revocations that apply to this plugin (any versions). */
  revocations: Revocation[];
  /** True when every version is revoked: the plugin is no longer offered. */
  revoked: boolean;
  /** From the reviewed manifest copy, only when its digest matches the registry's pin. */
  actions: ManifestAction[] | null;
};

/**
 * A plugin no longer offered: every version is withdrawn. It keeps its page,
 * a notice with the signed revocations, and is never in discovery.
 */
export type RevokedPlugin = {
  id: string;
  /** The index name when the index still lists the plugin; otherwise the id. */
  name: string;
  summary?: string;
  /** The publisher of the namespace, when the index has it. */
  publisher?: CatalogPlugin['publisher'];
  /** Its signed revocations, newest first. */
  revocations: Revocation[];
  /**
   * The listing and detail, when the index still lists the plugin (a
   * wildcard revocation of a listed plugin). DeckPilot's publisher omits a
   * plugin with no installable version from `index.plugins` and keeps its
   * revocations, so usually there is none: the page is built from the
   * revocations alone.
   */
  plugin?: CatalogPlugin;
};

export type Catalog = {
  /** `none`: no registry is published. `live`: a registry tree was read. */
  state: 'none' | 'live';
  trust: Trust | null;
  seq: number | null;
  generatedAt: string | null;
  /** Plugins the registry offers, newest publication first. */
  plugins: CatalogPlugin[];
  /** Plugins whose every version is revoked; they keep a page with the notice. */
  revoked: RevokedPlugin[];
};

class RegistryError extends Error {
  constructor(message: string) {
    super(`registry: ${message}`);
  }
}

function sha256(bytes: Uint8Array): string {
  return createHash('sha256').update(bytes).digest('hex');
}

type Json = Record<string, unknown>;

function parse(bytes: Uint8Array, what: string): Json {
  let value: unknown;
  try {
    value = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
  } catch {
    throw new RegistryError(`${what} is not valid UTF-8 JSON`);
  }
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw new RegistryError(`${what} is not a JSON object`);
  }
  return value as Json;
}

function str(value: unknown, what: string): string {
  if (typeof value !== 'string' || value === '') throw new RegistryError(`${what} must be a non-empty string`);
  return value;
}

function optStr(value: unknown, what: string): string | undefined {
  return value === undefined ? undefined : str(value, what);
}

function arr(value: unknown, what: string): unknown[] {
  if (!Array.isArray(value)) throw new RegistryError(`${what} must be an array`);
  return value;
}

function obj(value: unknown, what: string): Json {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw new RegistryError(`${what} must be an object`);
  }
  return value as Json;
}

const ID = /^(?=[^.]+\.[^.]+)[a-z][a-z0-9.-]{3,56}$/;
const DETAIL_PATH = /^plugins\/([a-z][a-z0-9.-]{3,56})\/([0-9a-f]{64})\.json$/;
const INDEX_PATH = /^index\/[1-9][0-9]*\.json$/;
const SEMVER = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$/;

/** Reads a file the previous document pinned by length and sha256. */
function readPinned(dir: string, path: string, pin: Json, what: string): Uint8Array {
  const file = join(dir, path);
  if (!existsSync(file)) throw new RegistryError(`${what} (${path}) is missing`);
  const bytes = readFileSync(file);
  if (bytes.length !== pin.length) throw new RegistryError(`${what} (${path}) does not have the pinned length`);
  if (sha256(bytes) !== pin.sha256) throw new RegistryError(`${what} (${path}) does not have the pinned sha256`);
  return bytes;
}

/** Semantic-version precedence, as DeckPilot orders versions. */
export function compareVersions(a: string, b: string): number {
  const ma = SEMVER.exec(a);
  const mb = SEMVER.exec(b);
  if (!ma || !mb) return a < b ? -1 : a > b ? 1 : 0;
  for (const i of [1, 2, 3]) {
    const d = Number(ma[i]) - Number(mb[i]);
    if (d !== 0) return d;
  }
  const pa = ma[4];
  const pb = mb[4];
  if (pa === pb) return 0;
  if (pa === undefined) return 1;
  if (pb === undefined) return -1;
  const xs = pa.split('.');
  const ys = pb.split('.');
  for (let i = 0; i < Math.max(xs.length, ys.length); i++) {
    const x = xs[i];
    const y = ys[i];
    if (x === undefined) return -1;
    if (y === undefined) return 1;
    const nx = /^\d+$/.test(x);
    const ny = /^\d+$/.test(y);
    if (nx && ny) {
      if (Number(x) !== Number(y)) return Number(x) - Number(y);
    } else if (nx !== ny) {
      return nx ? -1 : 1;
    } else if (x !== y) {
      return x < y ? -1 : 1;
    }
  }
  return 0;
}

/**
 * Whether a DeckPilot version satisfies `engines.deckpilot` (one or two
 * comparators), with semantic-version precedence including pre-releases,
 * as DeckPilot's `VersionRange::matches` does: 0.6.0-alpha.3 satisfies
 * ">=0.5.0 <1.0.0", and 0.5.0-beta.1 does not satisfy ">=0.5.0".
 */
export function satisfies(version: string, range: string): boolean {
  return range.split(' ').every((part) => {
    const m = /^(>=|>|<=|<|=)?(\d+\.\d+\.\d+)$/.exec(part);
    if (!m) return false;
    const c = compareVersions(version.replace(/\+.*$/, ''), m[2] ?? '');
    switch (m[1] ?? '=') {
      case '>=':
        return c >= 0;
      case '>':
        return c > 0;
      case '<=':
        return c <= 0;
      case '<':
        return c < 0;
      default:
        return c === 0;
    }
  });
}

function toVersion(raw: unknown, what: string): PluginVersion {
  const v = obj(raw, what);
  const engines = obj(v.engines, `${what}.engines`);
  const source = obj(v.source, `${what}.source`);
  const pkg = obj(v.package, `${what}.package`);
  const platforms = v.platforms === undefined ? [] : arr(v.platforms, `${what}.platforms`).map((p) => str(p, 'platform'));
  const yanked = v.yanked === undefined ? undefined : obj(v.yanked, `${what}.yanked`);
  return {
    version: str(v.version, `${what}.version`),
    publishedAt: str(v.published_at, `${what}.published_at`),
    engines: {
      deckpilot: str(engines.deckpilot, `${what}.engines.deckpilot`),
      pluginApi: str(engines.pluginApi, `${what}.engines.pluginApi`),
      sdk: optStr(engines.sdk, `${what}.engines.sdk`),
    },
    platforms: PLATFORMS.filter((p) => platforms.includes(p)),
    permissions: arr(v.permissions, `${what}.permissions`).map((p, i) => {
      const permission = obj(p, `${what}.permissions[${i}]`);
      str(permission.kind, `${what}.permissions[${i}].kind`);
      str(permission.justification, `${what}.permissions[${i}].justification`);
      return permission as unknown as Permission;
    }),
    manifestSha256: str(v.manifest_sha256, `${what}.manifest_sha256`),
    source: {
      repository: str(source.repository, `${what}.source.repository`),
      tag: str(source.tag, `${what}.source.tag`),
      commit: str(source.commit, `${what}.source.commit`),
      path: optStr(source.path, `${what}.source.path`),
    },
    package: {
      url: str(pkg.url, `${what}.package.url`),
      sha256: str(pkg.sha256, `${what}.package.sha256`),
      size: Number(pkg.size),
    },
    yanked: yanked && { at: str(yanked.at, 'yanked.at'), reason: str(yanked.reason, 'yanked.reason') },
  };
}

function readActions(manifestsDir: string | undefined, id: string, digest: string): ManifestAction[] | null {
  if (!manifestsDir) return null;
  const file = join(manifestsDir, id, 'manifest.json');
  if (!existsSync(file)) return null;
  const bytes = readFileSync(file);
  // Only the manifest the signed registry pins is shown; a copy that moved
  // ahead of the published version (or anything else) is ignored.
  if (sha256(bytes) !== digest) return null;
  const manifest = parse(bytes, `${id}/manifest.json`);
  const contributes = manifest.contributes === undefined ? {} : obj(manifest.contributes, 'contributes');
  return arr(contributes.actions ?? [], 'contributes.actions').map((raw) => {
    const action = obj(raw, 'action');
    const node = action.node === undefined ? {} : obj(action.node, 'node');
    return {
      id: str(action.id, 'action.id'),
      name: str(action.name, 'action.name'),
      description: str(action.description, 'action.description'),
      parameters: arr(action.parameters ?? [], 'action.parameters').map((p) => {
        const parameter = obj(p, 'parameter');
        return {
          key: str(parameter.key, 'parameter.key'),
          label: str(parameter.label, 'parameter.label'),
          required: parameter.required === true,
          type: str(obj(parameter.kind, 'parameter.kind').type, 'parameter.kind.type'),
        };
      }),
      outputs: arr(node.outputs ?? [], 'node.outputs').map((o) => {
        const output = obj(o, 'output');
        return { key: str(output.key, 'output.key'), label: str(output.label, 'output.label') };
      }),
      deprecated: action.deprecated === true,
    };
  });
}

function load(): Catalog {
  const empty: Catalog = { state: 'none', trust: null, seq: null, generatedAt: null, plugins: [], revoked: [] };
  const configured = process.env.REGISTRY_DIR;
  if (!configured) return empty;
  const dir = resolve(configured);
  const timestampFile = join(dir, 'timestamp.json');
  // No timestamp: no registry was ever published (what clients conclude too).
  if (!existsSync(timestampFile)) return empty;

  const trust = process.env.REGISTRY_TRUST;
  if (trust !== 'verified' && trust !== 'fixture') {
    throw new RegistryError(
      'REGISTRY_DIR holds a registry but REGISTRY_TRUST is not set. Set it to `verified` only after ' +
        '`deckpilot-registry verify-site` accepted this tree, or to `fixture` for test data.',
    );
  }

  const timestamp = parse(readFileSync(timestampFile), 'timestamp.json');
  if (timestamp._type !== 'deckpilot-registry-timestamp' || timestamp.spec_version !== 1) {
    throw new RegistryError('timestamp.json is not a v1 registry timestamp');
  }
  const indexPin = obj(timestamp.index, 'timestamp.index');
  const indexPath = str(indexPin.path, 'timestamp.index.path');
  if (!INDEX_PATH.test(indexPath)) throw new RegistryError(`unexpected index path ${indexPath}`);
  const index = parse(readPinned(dir, indexPath, indexPin, 'the index'), indexPath);
  if (index._type !== 'deckpilot-registry-index' || index.spec_version !== 1 || index.seq !== timestamp.seq) {
    throw new RegistryError(`${indexPath} is not the v1 index the timestamp names`);
  }

  const publishers = new Map(
    arr(index.publishers, 'index.publishers').map((raw) => {
      const p = obj(raw, 'publisher');
      const namespace = str(p.namespace, 'publisher.namespace');
      const login = str(p.login, 'publisher.login');
      return [
        namespace,
        { namespace, login, displayName: optStr(p.display_name, 'publisher.display_name') ?? login, verified: p.verified === true },
      ] as const;
    }),
  );

  const revocations = arr(index.revocations, 'index.revocations').map((raw) => {
    const r = obj(raw, 'revocation');
    const versions = r.versions === '*' ? ('*' as const) : arr(r.versions, 'revocation.versions').map((v) => str(v, 'version'));
    const action = r.action === 'disable' ? ('disable' as const) : ('yank' as const);
    return {
      id: str(r.id, 'revocation.id'),
      versions,
      action,
      reason: str(r.reason, 'revocation.reason'),
      advisory: optStr(r.advisory, 'revocation.advisory'),
      at: str(r.at, 'revocation.at'),
    };
  });

  const manifestsDir = process.env.MANIFESTS_DIR ? resolve(process.env.MANIFESTS_DIR) : undefined;
  const all: CatalogPlugin[] = arr(index.plugins, 'index.plugins').map((raw) => {
    const entry = obj(raw, 'index plugin');
    const id = str(entry.id, 'plugin.id');
    if (!ID.test(id)) throw new RegistryError(`unexpected plugin id ${id}`);
    const pin = obj(entry.detail, `${id}.detail`);
    const path = str(pin.path, `${id}.detail.path`);
    const match = DETAIL_PATH.exec(path);
    if (!match || match[1] !== id || match[2] !== pin.sha256) {
      throw new RegistryError(`${id}: the detail path is not content-addressed under the plugin id`);
    }
    const detail = parse(readPinned(dir, path, pin, `the detail of ${id}`), path);
    if (detail._type !== 'deckpilot-registry-plugin' || detail.id !== id) {
      throw new RegistryError(`${path} is not the detail of ${id}`);
    }

    const versions = arr(detail.versions, `${id}.versions`)
      .map((v, i) => toVersion(v, `${id}.versions[${i}]`))
      .sort((a, b) => compareVersions(b.version, a.version));
    const latestRef = obj(entry.latest, `${id}.latest`);
    const stable = optStr(latestRef.stable, `${id}.latest.stable`);
    const wanted = stable ?? str(latestRef.prerelease, `${id}.latest.prerelease`);
    const latest = versions.find((v) => v.version === wanted);
    if (!latest) throw new RegistryError(`${id}: the index names version ${wanted}, which the detail does not have`);

    const publisherId = str(entry.publisher, `${id}.publisher`);
    const publisher = publishers.get(publisherId);
    if (!publisher) throw new RegistryError(`${id}: publisher ${publisherId} is not in the index`);
    const own = revocations.filter((r) => r.id === id).map(({ id: _id, ...rest }) => rest);
    const deprecated = entry.deprecated === undefined ? undefined : obj(entry.deprecated, `${id}.deprecated`);

    return {
      id,
      name: str(entry.name, `${id}.name`),
      summary: str(entry.summary, `${id}.summary`),
      description: str(detail.description, `${id}.description`),
      license: str(detail.license, `${id}.license`),
      homepage: optStr(detail.homepage, `${id}.homepage`),
      sourceRepository: optStr(detail.source_repository, `${id}.source_repository`),
      categories: entry.categories === undefined ? [] : arr(entry.categories, `${id}.categories`).map((c) => str(c, 'category')),
      publisher,
      latest,
      latestIsPrerelease: stable === undefined,
      versions,
      deprecated: deprecated && {
        at: str(deprecated.at, 'deprecated.at'),
        reason: str(deprecated.reason, 'deprecated.reason'),
        replacement: optStr(deprecated.replacement, 'deprecated.replacement'),
      },
      revocations: own,
      revoked: own.some((r) => r.versions === '*'),
      actions: readActions(manifestsDir, id, latest.manifestSha256),
    };
  });

  const newestFirst = (a: CatalogPlugin, b: CatalogPlugin) =>
    b.latest.publishedAt.localeCompare(a.latest.publishedAt) || a.id.localeCompare(b.id);
  const byDateDesc = (a: Revocation, b: Revocation) => b.at.localeCompare(a.at);

  // Listed but wildcard-revoked: the detail is pinned by the index, use it.
  const revokedListed: RevokedPlugin[] = all
    .filter((p) => p.revoked)
    .sort(newestFirst)
    .map((plugin) => ({
      id: plugin.id,
      name: plugin.name,
      summary: plugin.summary,
      publisher: plugin.publisher,
      revocations: [...plugin.revocations].sort(byDateDesc),
      plugin,
    }));
  // Revoked and not listed: the publisher leaves out a plugin with no
  // installable version and keeps its revocations, so every version is
  // withdrawn (a wildcard, or individual revocations that exhaust it). The
  // signed index is the only trustworthy evidence here; no unpinned detail
  // is read.
  const listedIds = new Set(all.map((p) => p.id));
  const withdrawn = new Map<string, Revocation[]>();
  for (const { id, ...rest } of revocations) {
    if (listedIds.has(id)) continue;
    if (!ID.test(id)) throw new RegistryError(`unexpected revoked plugin id ${id}`);
    withdrawn.set(id, [...(withdrawn.get(id) ?? []), rest]);
  }
  const newestRevocation = (r: RevokedPlugin) => r.revocations[0]?.at ?? '';
  const revokedUnlisted: RevokedPlugin[] = [...withdrawn]
    .map(([id, own]) => ({
      id,
      name: id,
      publisher: publishers.get(id.slice(0, id.indexOf('.'))),
      revocations: own.sort(byDateDesc),
    }))
    .sort((a, b) => newestRevocation(b).localeCompare(newestRevocation(a)) || a.id.localeCompare(b.id));

  return {
    state: 'live',
    trust,
    seq: Number(index.seq),
    generatedAt: str(index.generated_at, 'index.generated_at'),
    plugins: all.filter((p) => !p.revoked).sort(newestFirst),
    revoked: [...revokedListed, ...revokedUnlisted],
  };
}

let cached: Catalog | undefined;

/** The catalog of this build (read once). */
export function getCatalog(): Catalog {
  cached ??= load();
  return cached;
}

/** True for a build from test data: pages say so and are kept out of indexes. */
export function isFixtureBuild(): boolean {
  return getCatalog().trust === 'fixture';
}

/** The platforms a version runs on, with "all" expanded. */
export function platformsOf(version: PluginVersion): readonly Platform[] {
  return version.platforms.length === 0 ? PLATFORMS : version.platforms;
}
