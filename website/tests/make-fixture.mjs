// Writes a TEST-ONLY registry tree to .fixture/ (git-ignored):
//
//   .fixture/v1/timestamp.json, index/1.json, plugins/<id>/<sha256>.json
//   .fixture/manifests/<id>/manifest.json
//
// It has the shape of a published registry (same documents, same hash
// chain) but every plugin in it is invented, nothing is signed, and it is
// built only with REGISTRY_TRUST=fixture, which marks every page and makes
// the deploy pipeline refuse the output. It exists so the catalog, search,
// filters and plugin pages can be tested while the real registry is empty.
import { createHash } from 'node:crypto';
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const out = join(dirname(fileURLToPath(import.meta.url)), '..', '.fixture');
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
const write = (path, text) => {
  mkdirSync(dirname(join(out, path)), { recursive: true });
  writeFileSync(join(out, path), text);
};
const PREFIX = 'https://github.com/knittingcode/deckpilot-plugins/releases/download/';
const hex = (seed, length) => sha256(Buffer.from(seed)).repeat(2).slice(0, length);

function version(id, v, date, extra = {}) {
  return {
    version: v,
    published_at: date,
    engines: { deckpilot: '>=0.5.0 <1.0.0', pluginApi: '^1.0', sdk: '0.1.0' },
    permissions: [],
    manifest_sha256: hex(`${id}@${v}:manifest`, 64),
    source: { repository: `example/${id.split('.')[1]}`, tag: `v${v}`, commit: hex(`${id}@${v}:commit`, 40) },
    package: { url: `${PREFIX}${id}--v${v}/${id}-${v}.dpplugin`, sha256: hex(`${id}@${v}:package`, 64), size: 80000 + id.length * 1500 },
    builder: '1.0.0',
    ...extra,
  };
}

const github = [
  { kind: 'network', hosts: ['api.github.com'], methods: ['GET'], justification: 'Reads the latest workflow run of the repository you name from the GitHub API.' },
  { kind: 'credentials', refs: ['githubToken'], justification: 'Sends your optional GitHub token to api.github.com for private repositories.' },
  { kind: 'notifications', justification: 'Tells you when the build status of a repository changes.' },
];

const buildStatusManifest = {
  manifestVersion: 1,
  id: 'fixture.build-status',
  name: 'Build Status',
  version: '1.2.0',
  contributes: {
    actions: [
      {
        id: 'show-status',
        name: 'Show build status',
        description: 'Shows the result of the latest workflow run of a repository.',
        category: 'Build Status',
        parameters: [
          { key: 'repository', label: 'Repository', required: true, kind: { type: 'text', maxLength: 140 } },
          { key: 'branch', label: 'Branch', required: false, kind: { type: 'text', maxLength: 100 } },
        ],
        node: { outputs: [{ key: 'status', label: 'Status' }, { key: 'url', label: 'Run URL' }] },
      },
      {
        id: 'open-run',
        name: 'Open latest run',
        description: 'Opens the latest workflow run in your browser.',
        category: 'Build Status',
        parameters: [{ key: 'repository', label: 'Repository', required: true, kind: { type: 'text' } }],
        deprecated: true,
      },
    ],
  },
};
const focusTimerManifest = {
  manifestVersion: 1,
  id: 'fixture.focus-timer',
  name: 'Focus Timer',
  version: '0.3.1',
  contributes: {
    actions: [
      {
        id: 'start',
        name: 'Start a focus session',
        description: 'Counts down a focus session on the key.',
        category: 'Focus Timer',
        parameters: [{ key: 'minutes', label: 'Minutes', required: false, kind: { type: 'integer', min: 1, max: 180 } }],
      },
    ],
  },
};
const manifests = { 'fixture.build-status': buildStatusManifest, 'fixture.focus-timer': focusTimerManifest };
const manifestDigest = {};
for (const [id, manifest] of Object.entries(manifests)) {
  const text = `${JSON.stringify(manifest, null, 2)}\n`;
  write(`manifests/${id}/manifest.json`, text);
  manifestDigest[id] = sha256(Buffer.from(text));
}

const plugins = [
  {
    listing: { id: 'fixture.build-status', publisher: 'fixture', name: 'Build Status', summary: 'Fixture plugin: the result of the latest CI run of a repository on a key.', categories: ['developer-tools', 'monitoring'], latest: { stable: '1.2.0' } },
    detail: {
      description: 'An invented plugin for testing the website.\n\nIt shows the result of the latest workflow run of a repository and can notify you when it changes.',
      license: 'Apache-2.0',
      homepage: 'https://example.com/build-status',
      source_repository: 'example/build-status',
      versions: [
        version('fixture.build-status', '1.0.0', '2026-01-10T09:00:00Z', { permissions: github.slice(0, 1) }),
        version('fixture.build-status', '1.1.0', '2026-03-02T09:00:00Z', { permissions: github.slice(0, 2) }),
        version('fixture.build-status', '1.2.0', '2026-06-18T09:00:00Z', { permissions: github, manifest_sha256: manifestDigest['fixture.build-status'] }),
      ],
    },
  },
  {
    listing: { id: 'fixture.focus-timer', publisher: 'fixture', name: 'Focus Timer', summary: 'Fixture plugin: a focus session countdown that needs no permissions.', categories: ['productivity'], latest: { stable: '0.3.1' } },
    detail: {
      description: 'An invented plugin for testing the website. It runs entirely inside the sandbox.',
      license: 'MIT',
      source_repository: 'example/focus-timer',
      versions: [version('fixture.focus-timer', '0.3.1', '2026-05-04T12:00:00Z', { manifest_sha256: manifestDigest['fixture.focus-timer'] })],
    },
  },
  {
    listing: { id: 'sample.room-lights', publisher: 'sample', name: 'Room Lights', summary: 'Fixture plugin: scenes and brightness for a lighting bridge.', categories: ['smart-home'], latest: { prerelease: '0.9.0-beta.2' } },
    detail: {
      description: 'An invented plugin for testing the website: a pre-release only, for two platforms, that needs a newer DeckPilot.',
      license: 'MPL-2.0',
      source_repository: 'example/room-lights',
      versions: [
        version('sample.room-lights', '0.9.0-beta.2', '2026-07-21T15:30:00Z', {
          engines: { deckpilot: '>=0.8.0 <1.0.0', pluginApi: '^1.0' },
          platforms: ['macos', 'linux'],
          permissions: [
            { kind: 'network', hosts: [], methods: ['GET', 'PUT'], userOrigins: [{ setting: 'bridgeUrl' }], justification: 'Talks to the lighting bridge whose address you enter in the settings.' },
            { kind: 'openUrl', hosts: ['lights.example.com'], justification: 'Opens the bridge documentation when you ask for help.' },
            { kind: 'storage', quotaKb: 1024, justification: 'Keeps the list of your scenes so keys render without a request.' },
            { kind: 'telepathy', justification: 'A permission kind this website does not know, to test the fallback.' },
          ],
        }),
      ],
    },
  },
  {
    listing: { id: 'sample.chat-status', publisher: 'sample', name: 'Chat Status', summary: 'Fixture plugin: sets your chat presence from a key.', categories: ['communication'], latest: { stable: '2.0.1' }, deprecated: { at: '2026-08-01T00:00:00Z', reason: 'The service this plugin talks to was shut down.', replacement: 'fixture.focus-timer' } },
    detail: {
      description: 'An invented plugin for testing the website: deprecated, with one yanked version and no reviewed manifest copy.',
      license: 'Apache-2.0',
      source_repository: 'example/chat-status',
      versions: [
        version('sample.chat-status', '2.0.0', '2026-02-11T08:00:00Z', { yanked: { at: '2026-02-12T08:00:00Z', reason: 'Crashed on start.' } }),
        version('sample.chat-status', '2.0.1', '2026-02-12T10:00:00Z', { platforms: ['windows'] }),
      ],
    },
  },
  {
    listing: { id: 'sample.media-keys', publisher: 'sample', name: 'Media Keys', summary: 'Fixture plugin: playback control for a network media player.', categories: ['media', 'other'], latest: { stable: '1.0.0' } },
    detail: {
      description: 'An invented plugin for testing the website.',
      license: 'BSD-3-Clause',
      source_repository: 'example/media-keys',
      versions: [version('sample.media-keys', '1.0.0', '2026-04-01T00:00:00Z', { permissions: [{ kind: 'network', hosts: ['player.example.com'], methods: ['GET', 'POST'], justification: 'Sends playback commands to your media player service.' }] })],
    },
  },
  {
    listing: { id: 'sample.old-monitor', publisher: 'sample', name: 'Old Monitor', summary: 'Fixture plugin: a revoked plugin.', categories: ['monitoring'], latest: { stable: '0.1.0' } },
    detail: {
      description: 'An invented plugin for testing the website: every version is revoked.',
      license: 'Apache-2.0',
      source_repository: 'example/old-monitor',
      versions: [version('sample.old-monitor', '0.1.0', '2026-01-05T00:00:00Z')],
    },
  },
];

rmSync(join(out, 'v1'), { recursive: true, force: true });
const listed = plugins.map(({ listing, detail }) => {
  const text = `${JSON.stringify({ _type: 'deckpilot-registry-plugin', spec_version: 1, id: listing.id, publisher: listing.publisher, name: listing.name, ...detail }, null, 2)}\n`;
  const digest = sha256(Buffer.from(text));
  const path = `plugins/${listing.id}/${digest}.json`;
  write(`v1/${path}`, text);
  return { ...listing, detail: { path, sha256: digest, length: Buffer.byteLength(text) } };
});
const index = `${JSON.stringify(
  {
    _type: 'deckpilot-registry-index',
    spec_version: 1,
    seq: 1,
    keys_version: 1,
    generated_at: '2026-08-02T00:00:00Z',
    expires: '2026-10-31T00:00:00Z',
    source_commit: hex('fixture', 40),
    download_prefix: PREFIX,
    publishers: [
      { namespace: 'fixture', github_id: 1, login: 'fixture-labs', display_name: 'Fixture Labs', verified: true },
      { namespace: 'sample', github_id: 2, login: 'sample-dev', verified: false },
    ],
    plugins: listed,
    revocations: [
      { id: 'sample.old-monitor', versions: '*', action: 'disable', reason: 'Sent data to a host unrelated to its purpose.', advisory: 'https://example.com/advisory', at: '2026-03-01T00:00:00Z' },
      { id: 'fixture.build-status', versions: ['1.0.0'], action: 'yank', reason: 'Showed the wrong branch.', at: '2026-01-20T00:00:00Z' },
    ],
  },
  null,
  2,
)}\n`;
write('v1/index/1.json', index);
write(
  'v1/timestamp.json',
  `${JSON.stringify({ _type: 'deckpilot-registry-timestamp', spec_version: 1, seq: 1, keys_version: 1, signed_at: '2026-08-02T00:00:00Z', expires: '2026-08-09T00:00:00Z', index: { path: 'index/1.json', sha256: sha256(Buffer.from(index)), length: Buffer.byteLength(index) } }, null, 2)}\n`,
);
console.log(`fixture registry: ${plugins.length} invented plugins in ${out}`);
