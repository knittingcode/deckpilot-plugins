/**
 * Facts about the project that pages state. Each one is checked against its
 * source before it changes; the source is named next to it.
 */

export const REPO_PLUGINS = 'https://github.com/knittingcode/deckpilot-plugins';
export const REPO_APP = 'https://github.com/knittingcode/deckpilot';

export const LINKS = {
  app: REPO_APP,
  /** The product website; append the language (`en/`, `pl/`). */
  appSite: 'https://knittingcode.github.io/deckpilot/',
  appReleases: `${REPO_APP}/releases`,
  appSecurity: `${REPO_APP}/security/advisories/new`,
  appSdkDocs: `${REPO_APP}/tree/main/docs/marketplace/sdk`,
  appExamples: `${REPO_APP}/tree/main/plugins/examples`,
  appDesign: `${REPO_APP}/pull/191`,
  plugins: REPO_PLUGINS,
  contributing: `${REPO_PLUGINS}/blob/main/CONTRIBUTING.md`,
  security: `${REPO_PLUGINS}/blob/main/SECURITY.md`,
  codeOfConduct: `${REPO_PLUGINS}/blob/main/CODE_OF_CONDUCT.md`,
  license: `${REPO_PLUGINS}/blob/main/LICENSE`,
  reviewPolicy: `${REPO_PLUGINS}/blob/main/docs/review-policy.md`,
  trustModel: `${REPO_PLUGINS}/blob/main/docs/trust-model.md`,
  publishing: `${REPO_PLUGINS}/blob/main/docs/publishing.md`,
  readiness: `${REPO_PLUGINS}/blob/main/docs/production-readiness.md`,
  reportVulnerability: `${REPO_PLUGINS}/security/advisories/new`,
  issues: `${REPO_PLUGINS}/issues/new/choose`,
  discussions: `${REPO_PLUGINS}/pulls`,
} as const;

/**
 * DeckPilot's public versions, newest first (DeckPilot README.md: "The first
 * public pre-release is v0.6.0-alpha.3; there is no stable release yet").
 * The compatibility filter of the plugin directory offers exactly these.
 */
export const DECKPILOT_VERSIONS = [{ version: '0.6.0-alpha.3', prerelease: true }] as const;

export const DECKPILOT_RELEASE_URL = `${REPO_APP}/releases/tag/v0.6.0-alpha.3`;

/**
 * The DeckPilot commit the developer documentation (src/content/docs) was
 * checked against: every Rust example was compiled and tested with the SDK
 * at this commit. Update it together with the documentation.
 */
export const DOCS_VERIFIED_COMMIT = '7f614b521a7b858d26b3f018da946e6a515cbcd4';
