/**
 * English: the reference dictionary. Its shape is the `Dictionary` type, so
 * every other language must provide exactly the same keys.
 */
export const en = {
  site: {
    name: 'DeckPilot Plugin Marketplace',
    shortName: 'Marketplace',
    tagline: 'Plugins for DeckPilot, the open-source Stream Deck controller.',
    description:
      'The DeckPilot Plugin Marketplace: independently installable plugins that extend DeckPilot, the open-source, local-first Stream Deck controller. Signed registry, sandboxed plugins, explicit permissions.',
  },

  a11y: {
    skipToContent: 'Skip to content',
    mainNav: 'Main',
    footerNav: 'Footer',
    openMenu: 'Open menu',
    closeMenu: 'Close menu',
    homeLink: 'DeckPilot Plugin Marketplace, home',
    languageSwitcher: 'Language',
    switchToDark: 'Switch to dark theme',
    switchToLight: 'Switch to light theme',
    externalLink: '(opens on GitHub)',
    breadcrumb: 'Breadcrumb',
    table: 'Table',
    copyCode: 'Copy code',
    copied: 'Copied',
    copyFailed: 'Copy failed',
  },

  nav: {
    plugins: 'Plugins',
    developers: 'Developers',
    docs: 'Docs',
    community: 'Community',
    github: 'GitHub',
  },

  status: {
    badge: 'Registry live',
    short: 'The signed plugin registry is live. Installing plugins needs the next DeckPilot release: DeckPilot 0.6.0, the current release, does not include plugin support.',
  },

  footer: {
    about:
      'The community plugin registry of DeckPilot, the open-source, local-first Stream Deck controller and automation app.',
    marketplace: 'Marketplace',
    developers: 'Developers',
    project: 'Project',
    explore: 'Explore plugins',
    howItWorks: 'How it works',
    securityModel: 'Security and permissions',
    faq: 'FAQ',
    portal: 'Developer portal',
    docs: 'Documentation',
    gettingStarted: 'Getting started',
    submit: 'Submit a plugin',
    website: 'DeckPilot website',
    deckpilot: 'DeckPilot on GitHub',
    registryRepo: 'Registry repository',
    contributing: 'Contributing guide',
    securityPolicy: 'Security policy',
    codeOfConduct: 'Code of Conduct',
    license: 'License',
    legal:
      'This site and the registry tooling are licensed under Apache-2.0. Each plugin carries its own license.',
    trademark:
      'Stream Deck is a trademark of its respective owner. DeckPilot is an independent open-source project and is not affiliated with or endorsed by Elgato or Corsair.',
  },

  home: {
    title: 'DeckPilot Plugin Marketplace: make your Stream Deck work your way',
    description:
      'Extend DeckPilot with independently installable plugins: a signed registry, sandboxed WebAssembly plugins, explicit permissions. Coming with the next DeckPilot release.',
    hero: {
      headline: 'Make your Stream Deck work your way.',
      lead: 'The Marketplace extends DeckPilot with independently installable plugins: new actions for the services and tools you use, each one sandboxed and limited to the permissions you approve.',
      explore: 'Explore plugins',
      build: 'Build a plugin',
      github: 'View DeckPilot on GitHub',
      deckAlt: 'Illustration of a Stream Deck key grid with plugin actions on its keys',
    },
    search: {
      title: 'Find a plugin',
      lead: 'Search the registry by name or description.',
      label: 'Search plugins',
      placeholder: 'Search plugins by name or description',
      submit: 'Search',
      emptyHint: 'This build of the site lists no plugins, so search has nothing to return.',
    },
    featured: {
      eyebrow: 'Integrations',
      title: 'Featured integrations',
      lead: 'The plugins most recently published to the registry.',
      viewAll: 'View all plugins',
      emptyTitle: 'No plugins in this build',
      emptyText:
        'The catalog is generated from the signed registry when the site is deployed. This build contains no plugins, and nothing here is invented to fill the space.',
      emptyPrimary: 'Build a plugin',
      emptySecondary: 'Follow the registry on GitHub',
    },
    categories: {
      eyebrow: 'Categories',
      title: 'Plugin categories',
      lead: 'Every plugin declares up to three categories from this fixed list, so the directory stays easy to browse.',
      count: {
        one: '{count} plugin',
        few: '{count} plugins',
        many: '{count} plugins',
        other: '{count} plugins',
      },
    },
    how: {
      eyebrow: 'How it works',
      title: 'From a pull request to a key on your deck',
      lead: 'The Marketplace is a signed, static registry. Nothing reaches DeckPilot without a review, a rebuild from source and a signature the app checks itself.',
      steps: [
        {
          title: 'An author submits',
          text: 'A plugin is proposed as a pull request that names the exact tag and commit of its public source repository.',
        },
        {
          title: 'CI rebuilds, the owner reviews',
          text: 'The registry rebuilds the package from that source and checks it byte for byte. The owner reviews the source, the permissions and the listing.',
        },
        {
          title: 'The registry is signed',
          text: 'Publication signs a new index that pins the sha256 and size of every package. Merging a pull request alone publishes nothing.',
        },
        {
          title: 'DeckPilot verifies and asks you',
          text: 'The app verifies the signatures against keys built into it and shows the permissions a plugin requests. A plugin stays disabled until you grant them.',
        },
      ],
      more: 'Read the trust model',
    },
    security: {
      eyebrow: 'Security and permissions',
      title: 'Plugins get only what you grant',
      lead: 'A plugin is a WebAssembly module in a sandboxed process. It has no file system, network, clock or environment of its own.',
      points: [
        {
          title: 'Sandboxed by default',
          text: 'Each plugin runs in a separate, OS-sandboxed host process without WASI. Everything it reaches outside itself is a request that DeckPilot checks on every call.',
        },
        {
          title: 'Explicit permissions',
          text: 'Network hosts, credentials, notifications and more are declared in the manifest, each with a justification. You see them before a plugin is enabled, and you are asked again when an update adds one.',
        },
        {
          title: 'Secrets stay with DeckPilot',
          text: 'A plugin refers to a credential by a handle. DeckPilot attaches the secret to the approved request; the plugin never receives its value.',
        },
        {
          title: 'Signed and revocable',
          text: 'DeckPilot trusts the registry only through signatures it verifies itself. A plugin found to be harmful can be revoked, and clients stop it at their next refresh.',
        },
      ],
      caveat:
        'Review and sandboxing reduce risk; they are not a guarantee. The registry has not had an independent security review yet.',
      more: 'How permissions work',
      report: 'Report a security issue',
    },
    build: {
      eyebrow: 'For developers',
      title: 'Build your own plugin',
      lead: 'Plugins are written in Rust with the DeckPilot plugin SDK and compiled to one WebAssembly module. This is a complete plugin with one action:',
      codeTitle: 'src/lib.rs',
      points: [
        'Typed actions, parameters and outputs',
        'Unit tests against a mock host with plain cargo test',
        'Brokered HTTPS, credentials, storage, variables and notifications',
      ],
      note: 'The SDK is not published to crates.io yet. You build against a checkout of the DeckPilot repository.',
      primary: 'Start building',
      secondary: 'Browse the examples',
    },
    community: {
      eyebrow: 'Open source',
      title: 'Built in the open',
      lead: 'DeckPilot and its plugin registry are open source under Apache-2.0. The format, the checks and the publishing pipeline are public, and so is every review.',
      cards: [
        {
          title: 'Submit a plugin',
          text: 'Prepare and validate a submission today. The registry publishes DeckPilot’s own plugins for now; other submissions are reviewed and published once it opens to other publishers.',
          link: 'How submission works',
        },
        {
          title: 'Improve the registry',
          text: 'Documentation fixes are welcome as pull requests. Tooling problems are tracked as issues.',
          link: 'Read the contributing guide',
        },
        {
          title: 'Report a problem',
          text: 'Report a malicious plugin or a vulnerability privately through GitHub. There is no other channel.',
          link: 'Read the security policy',
        },
      ],
    },
    faq: {
      eyebrow: 'FAQ',
      title: 'Questions and answers',
      items: [
        {
          q: 'What is the DeckPilot Plugin Marketplace?',
          a: 'It is the community plugin registry of DeckPilot. A plugin adds new actions to DeckPilot and is installed independently of the application, from a signed registry that the app verifies itself.',
        },
        {
          q: 'Can I install plugins today?',
          a: 'Not yet. The signed registry is live and the plugins listed on this site are published, but installing them needs the next DeckPilot release, which brings the Marketplace screen and plugin support. DeckPilot 0.6.0, the current release and the first stable one, does not include plugin support.',
        },
        {
          q: 'How will installing a plugin work?',
          a: 'From the Marketplace screen inside DeckPilot, starting with the next DeckPilot release. You pick a plugin, review the permissions it requests and approve the install. This website is a catalog and documentation: it has no install button, and DeckPilot has no install links.',
        },
        {
          q: 'How are plugins reviewed?',
          a: 'Every submission is a pull request. Automated checks rebuild the package from the author’s tagged source and compare it byte for byte, and the owner reviews the publisher identity, the source, every permission and its justification, the build and the license. A green check run is not an approval, and merging does not publish.',
        },
        {
          q: 'What can a plugin access on my computer?',
          a: 'Only what its manifest declares and you approve. A plugin has no direct access to files, the network or your secrets: it asks DeckPilot, which checks each request against the granted permissions. In the current plugin ABI these are HTTPS requests to named hosts, credentials used as handles, opening approved links in your browser, notifications, and the plugin’s own settings, variables and storage.',
        },
        {
          q: 'Which language are plugins written in?',
          a: 'Rust, compiled to a WebAssembly module with the DeckPilot plugin SDK. A JavaScript SDK exists only as type declarations: JavaScript plugins cannot be built or run yet.',
        },
        {
          q: 'What license do plugins use?',
          a: 'Each plugin carries its own license, declared in its manifest as an SPDX expression and shipped in its package. The registry and this site are licensed under Apache-2.0, which does not apply to the plugins listed in it.',
        },
        {
          q: 'How do I report a malicious plugin?',
          a: 'Privately, through GitHub’s private vulnerability reporting on the registry repository. Please do not open a public issue for a suspected malicious plugin or a vulnerability.',
        },
      ],
    },
  },

  categories: {
    'developer-tools': {
      name: 'Developer tools',
      text: 'Repositories, builds, pull requests and the tools around your code.',
    },
    productivity: {
      name: 'Productivity',
      text: 'Tasks, notes, timers and everyday shortcuts.',
    },
    communication: {
      name: 'Communication',
      text: 'Chat, calls, status and notifications.',
    },
    monitoring: {
      name: 'Monitoring',
      text: 'Service health, metrics and alerts at a glance.',
    },
    media: {
      name: 'Media',
      text: 'Audio, video, streaming and playback control.',
    },
    'smart-home': {
      name: 'Smart home',
      text: 'Lights, scenes, sensors and connected devices.',
    },
    other: {
      name: 'Other',
      text: 'Everything that does not fit a category above.',
    },
  },

  fixture: {
    banner: 'Test build with fixture data. These plugins are not real and are not published.',
  },

  directory: {
    title: 'Plugins',
    metaTitle: 'Plugin directory',
    description:
      'Browse the plugins published in the DeckPilot registry. Search by name or description and filter by category, platform and DeckPilot version.',
    lead: 'Every plugin published in the signed DeckPilot registry. This list is generated from the registry itself.',
    searchLabel: 'Search',
    searchPlaceholder: 'Name or description',
    category: 'Category',
    platform: 'Platform',
    compatibility: 'DeckPilot version',
    anyCategory: 'All categories',
    anyPlatform: 'All platforms',
    anyVersion: 'Any DeckPilot version',
    versionOption: 'DeckPilot {version}',
    clear: 'Clear filters',
    results: {
      one: '{count} plugin',
      few: '{count} plugins',
      many: '{count} plugins',
      other: '{count} plugins',
    },
    resultsOf: 'Showing {shown} of {total}',
    listLabel: 'Plugins',
    noResultsTitle: 'No plugins match these filters',
    noResultsText: 'Try a different search term or remove a filter.',
    emptyTitle: 'No plugins in this build',
    emptyText:
      'The catalog is generated from the signed registry when the site is deployed, and this build contains no plugins. On the published site, every plugin the registry offers is listed here.',
    emptyPrimary: 'Build a plugin',
    emptySecondary: 'How submission works',
    emptyStatusTitle: 'Where things stand',
    emptyStatus: [
      'The signed production registry is live, and its timestamp is re-signed daily.',
      'It publishes DeckPilot’s own plugins. It is not open to other publishers yet.',
      'DeckPilot 0.6.0 does not include plugin support. Plugin support comes with the next DeckPilot release.',
    ],
    emptyStatusLink: 'What remains before the registry opens to other publishers',
    errorTitle: 'Search is unavailable',
    errorText: 'The search index could not be loaded, so every plugin is listed. Reload the page to try again.',
    registryInfo: 'Registry index {seq}, generated {date}.',
  },

  plugin: {
    metaTitle: '{name}: DeckPilot plugin',
    verifiedPublisher: 'Verified publisher',
    prerelease: 'Pre-release',
    deprecated: 'Deprecated',
    revoked: 'Revoked',
    yanked: 'Yanked',
    by: 'by {publisher}',
    allPlugins: 'Plugins',
    about: 'About this plugin',
    untranslatedNote: 'Texts provided by the publisher are shown in English.',
    actions: 'Actions',
    actionsLead: 'The actions this plugin adds to DeckPilot.',
    actionsUnavailable:
      'The list of actions is stored inside the plugin package, and no reviewed copy matches the published version, so it is not shown here. DeckPilot shows the actions before you install.',
    noActions: 'This version declares no actions.',
    parameters: 'Parameters',
    outputs: 'Outputs',
    required: 'required',
    optional: 'optional',
    deprecatedAction: 'deprecated',
    permissions: 'Requested permissions',
    permissionsLead:
      'DeckPilot shows these before the plugin is enabled, and the plugin stays disabled until you grant them. You can revoke a permission at any time.',
    noPermissions: 'This plugin requests no permissions. Its actions run entirely inside the sandbox.',
    install: 'How to install',
    installSteps: [
      'Install the next DeckPilot release, the first with plugin support. It is not published yet, and DeckPilot 0.6.0 cannot install plugins.',
      'Open the Marketplace screen in DeckPilot and find {name}.',
      'Review the requested permissions and install. DeckPilot verifies the registry’s signatures and the package’s sha256 and size before installing.',
      'Enable the plugin and grant its permissions. A plugin is installed disabled and can do nothing until then.',
    ],
    installNote:
      'This website has no install button: DeckPilot does not support install links, and plugins are installed only from inside the app.',
    installRevoked: 'This plugin has been revoked. DeckPilot does not offer it for installation.',
    history: 'Version history',
    historyLead:
      'Every published version, newest first. The registry records versions and their source, not release notes; for those, see the source repository.',
    version: 'Version',
    published: 'Published',
    source: 'Source',
    changes: 'Changes',
    compare: 'Compare with {version}',
    firstVersion: 'First version',
    sourceAt: '{tag} at {commit}',
    details: 'Details',
    latestVersion: 'Latest version',
    platforms: 'Platforms',
    allPlatforms: 'macOS, Windows and Linux',
    compatibility: 'DeckPilot versions',
    compatibleWith: 'Includes the current release {version}, which does not include plugin support yet. Plugins need the next DeckPilot release.',
    notCompatibleWith: 'Does not include the current release {version}.',
    pluginApi: 'Plugin API',
    license: 'License',
    publisher: 'Publisher',
    sourceRepository: 'Source repository',
    homepage: 'Homepage',
    package: 'Package',
    id: 'Plugin ID',
    revokedTitle: 'This plugin has been revoked',
    revokedDisable: 'DeckPilot stops installed copies and refuses to load them.',
    revokedYank: 'It is no longer offered for installation or updates. Installed copies keep running.',
    revokedVersions: 'Revoked versions: {versions}.',
    reason: 'Reason given by the registry:',
    advisory: 'Read the advisory',
    deprecatedTitle: 'This plugin is deprecated',
    replacement: 'Suggested replacement:',
    report: 'Report a problem with this plugin',
  },

  platforms: { macos: 'macOS', windows: 'Windows', linux: 'Linux' },

  permissions: {
    network: {
      name: 'Network access',
      text: 'Sends HTTPS requests through DeckPilot to the hosts listed below, and to no other server.',
    },
    credentials: {
      name: 'Credentials',
      text: 'Uses secrets you enter in DeckPilot (for example an API token) for requests to its approved hosts. The plugin never sees the secret itself.',
    },
    notifications: {
      name: 'Notifications',
      text: 'Shows system notifications through DeckPilot.',
    },
    storage: {
      name: 'Larger storage',
      text: 'Raises the limit of the plugin’s own private storage above the default 256 KiB. It gives no access to your files.',
    },
    openUrl: {
      name: 'Open links',
      text: 'Opens HTTPS pages on the hosts listed below in your browser, and only in a run you started.',
    },
    profile: {
      name: 'Profile information',
      text: 'Declared for reading the active profile or project. The current plugin ABI has no capability that uses it.',
    },
    variables: {
      name: 'Read variables',
      text: 'Declared for reading variables of other owners. The current plugin ABI has no capability that uses it.',
    },
    background: {
      name: 'Background work',
      text: 'Declared for running on a schedule. The current plugin ABI has no background work.',
    },
    device: {
      name: 'Device',
      text: 'Declared for device state or key icons. The current plugin ABI has no capability that uses it.',
    },
    unknown: {
      name: 'Other permission',
      text: 'A permission kind this website does not know. DeckPilot explains it before you grant it.',
    },
    justificationLabel: 'Why the plugin asks, in the publisher’s words:',
    hosts: 'Hosts',
    methods: 'Methods',
    slots: 'Credential slots',
    quota: 'Quota',
    quotaValue: '{kb} KiB',
    userOrigins: 'Also a server address you enter yourself, after you confirm it.',
    privateNetwork: 'Declares private network addresses.',
    reads: 'Reads',
    interval: 'Minimum interval',
    seconds: '{count} s',
  },

  docs: {
    title: 'Documentation',
    metaTitle: 'Plugin developer documentation',
    description:
      'Documentation for DeckPilot plugin developers: SDK, manifest, actions, permissions, storage, testing, packaging and submission to the registry.',
    lead: 'Everything needed to write, test and submit a DeckPilot plugin, written against what the DeckPilot repository implements today.',
    groups: {
      start: 'Start here',
      build: 'Build',
      capabilities: 'Capabilities',
      ship: 'Test and ship',
      reference: 'Reference',
    },
    sidebarLabel: 'Documentation pages',
    menu: 'Documentation menu',
    onThisPage: 'On this page',
    previous: 'Previous',
    next: 'Next',
    pagination: 'Previous and next page',
    statusTitle: 'Plugins come with the next DeckPilot release',
    status:
      'DeckPilot 0.6.0, the current release, does not include plugin support. The signed registry is live and publishes DeckPilot’s own plugins; it is not open to other publishers yet. The SDK is 0.x and not published as a package, so its Rust API can still change.',
    verified: 'Checked against DeckPilot at commit {commit}.',
    source: 'Canonical SDK documentation in the DeckPilot repository',
  },

  developers: {
    title: 'Build plugins for DeckPilot',
    metaTitle: 'Developer portal',
    description:
      'Write DeckPilot plugins in Rust, compile them to WebAssembly, test them against a mock host and submit them to the signed registry. Guides, SDK and examples.',
    lead: 'A plugin adds actions to DeckPilot. You write it in Rust with the plugin SDK, compile it to one WebAssembly module, and describe it in a manifest. DeckPilot runs it in a sandbox with only the permissions its user granted.',
    primary: 'Get started',
    secondary: 'Browse the examples',
    facts: [
      {
        title: 'One WebAssembly module',
        text: 'A plugin is a single plugin.wasm implementing guest ABI 1, next to a manifest.json, a LICENSE and an icon. No WASI: no files, network or clock of its own.',
      },
      {
        title: 'A typed Rust SDK',
        text: 'deckpilot-plugin-sdk gives you actions, parameters, outputs and a Context for HTTPS, credentials, settings, variables, storage and notifications.',
      },
      {
        title: 'Tested like any crate',
        text: 'Handlers run natively under cargo test against a mock host, and the SDK checks that your code and your manifest agree.',
      },
    ],
    obtainTitle: 'How you get the SDK',
    obtain:
      'The SDK is not published to crates.io, and there is no npm package. It lives in the DeckPilot repository: build inside a checkout, or depend on the repository as a git dependency pinned to a commit. The JavaScript SDK in the repository is type declarations only; JavaScript plugins cannot be built or run yet.',
    obtainLink: 'Getting started',
    pathTitle: 'From idea to registry',
    pathLead: 'The route every plugin takes. Each step links to its guide.',
    path: [
      { title: 'Write', text: 'Declare actions and parameters in code and in the manifest.', doc: 'actions' },
      { title: 'Ask for less', text: 'Declare only the permissions you call, each with a justification.', doc: 'permissions' },
      { title: 'Test', text: 'Run unit tests against the mock host, then against a real agent.', doc: 'testing' },
      { title: 'Package', text: 'Build for wasm32-unknown-unknown and pack a .dpplugin.', doc: 'packaging' },
      { title: 'Submit', text: 'Open a pull request to the registry with your version record.', doc: 'submission' },
    ],
    topicsTitle: 'Documentation',
    topicsLead: 'All guides, grouped the way you will need them.',
    examplesTitle: 'Examples',
    examplesLead: 'Three example plugins live in the DeckPilot repository. They are built, packaged and run through the real agent in its CI. They are examples, not published plugins.',
    examples: [
      { name: 'hello-world', text: 'The minimum: one pure action with one parameter. No permissions.' },
      { name: 'developer-tools', text: 'HTTPS to one origin, a credential used as a handle, settings, storage, variables, notifications, typed errors.' },
      { name: 'hello-weather', text: 'A pure action, one brokered request and an init hook.' },
    ],
    helpTitle: 'Questions and problems',
    help: 'For a bug in the registry tooling, open an issue in the registry repository. For the SDK, the sandbox or the app, use the DeckPilot repository. Report vulnerabilities privately.',
    helpIssues: 'Registry issues',
    helpSecurity: 'Security policy',
  },

  community: {
    title: 'Community',
    metaTitle: 'Community and contributing',
    description:
      'How to submit a plugin to the DeckPilot registry, how review works, licensing, versioning, updates, revocation and how to report a vulnerability.',
    lead: 'The registry is run in the open, in one GitHub repository. This page summarises its rules. The documents it links to are authoritative; if this page and a document disagree, the document wins.',
    statusTitle: 'Not open to other publishers yet',
    status:
      'The signed registry is live and publishes DeckPilot’s own plugins. Plugins from other publishers are not published yet. You can already prepare and validate a submission; according to the contributing guide, it will be reviewed and published once the registry opens.',
    toc: 'On this page',
    submit: {
      title: 'Submit a plugin',
      lead: 'Your source stays in your own public GitHub repository. The registry records which tag and commit to build; it never holds your source.',
      steps: [
        {
          title: 'Tag your source',
          text: 'Commit Cargo.lock, pin the Rust toolchain to an exact version in rust-toolchain.toml and push a tag. Never move or delete a tag that a registry record names.',
        },
        {
          title: 'Fork the registry and add your files',
          text: 'A publisher record (first plugin only), the listing, the build recipe, a copy of your manifest and the version record generated with the pinned tools.',
        },
        {
          title: 'Run the checks',
          text: 'Locally if you can, with tools/build-cli.sh and tools/validate.sh. The same checks run on your pull request, without secrets.',
        },
        {
          title: 'Open a pull request',
          text: 'Use the template and fill in the plugin submission checklist.',
        },
      ],
      namespace:
        'Your plugin id is <namespace>.<name>. The namespace becomes yours when your publisher record is merged, and it is bound to your numeric GitHub user id. Ids that DeckPilot reserves, such as deckpilot, knittingcode, core, system or the names of built-in modules, cannot be a namespace.',
      guide: 'Step-by-step submission guide',
      contributing: 'CONTRIBUTING.md',
    },
    review: {
      title: 'How review works',
      lead: 'Every submission is reviewed by the owner before it is merged. Nothing is merged automatically, and a green CI run is necessary but not an approval.',
      checks: [
        { title: 'Publisher identity', text: 'The GitHub account exists, its numeric id matches the record, and the namespace does not impersonate another project, a company or DeckPilot.' },
        { title: 'Source', text: 'The repository and tag are the author’s, the tag resolves to the recorded commit, and the diff since the previous version contains nothing unexplained.' },
        { title: 'Permissions', text: 'Each permission has a justification that matches what the plugin does, and network hosts belong to the service the plugin claims. A new permission in an update is reviewed as carefully as a new plugin.' },
        { title: 'Build', text: 'An exact toolchain, a locked Cargo workspace and a plain cargo build of one package. CI rebuilt the package, and it matches the record byte for byte.' },
        { title: 'License', text: 'The manifest declares an SPDX license and the package ships its LICENSE.' },
        { title: 'Listing', text: 'Name, summary, description and categories are accurate and not misleading.' },
      ],
      publish:
        'Merging does not publish. The owner publishes a version by creating its release tag. Contributors never get publish rights automatically: not by having a plugin merged, not by being a verified publisher, and not by contributing regularly.',
      times: 'There are no guaranteed response times. Security reports are handled first.',
      policy: 'Review policy',
    },
    security: {
      title: 'Security requirements',
      lead: 'What a submission has to satisfy, beyond passing the automated checks.',
      items: [
        'Declare only the permissions the plugin calls. Each one costs the user a consent decision and is reviewed.',
        'Network hosts must belong to the service the plugin is for. Hosts for opening links must be pages the plugin’s actions open for the user, never a broad wildcard.',
        'Credential slots must match the plugin’s network hosts.',
        'Keep builds hermetic: no network access beyond Cargo’s locked dependencies, and no build-time downloads.',
        'No obfuscation, generated blobs, vendored binaries, or build scripts that fetch or execute anything.',
        'Never commit a private key, a key file, a package or generated output. The hygiene check refuses them.',
      ],
      guide: 'Security guidelines for plugin authors',
    },
    licenses: {
      title: 'Licenses',
      items: [
        'Choose a license for your plugin, declare it in manifest.json as an SPDX expression and ship the LICENSE file in the plugin directory.',
        'Your plugin keeps its own license. Listing it in the registry does not relicense it, and the registry’s Apache-2.0 license does not apply to plugin source or packages.',
        'Contributions to the registry’s own files (documentation, tooling) are under its Apache-2.0 license.',
        'A DCO sign-off (git commit -s) is welcome but optional. There is no CLA.',
      ],
      note: 'The registry does not publish a list of accepted licenses; the license is checked during review.',
      link: 'LICENSE',
    },
    versioning: {
      title: 'Versioning and updates',
      items: [
        'Versions are semantic versions, and a new version must be greater than every recorded one.',
        'Recorded versions are immutable: never edit or delete one. To change something, publish a new version.',
        'To update a plugin, add a new version record, replace the manifest copy with the new manifest, and change the listing or the build recipe if needed.',
        'If an update adds permissions, say why in the pull request. Users are asked to approve added permissions before the update applies.',
        'The release tag of a published version is always <id>--v<version>, created by the owner.',
      ],
      link: 'Publishing process',
    },
    revocation: {
      title: 'Revocation',
      lead: 'A published plugin or version can be revoked by the owner in a reviewed pull request, followed by a new signed index. Clients apply a revocation at their next successful refresh and enforce it offline from then on.',
      yankTitle: 'Yank',
      yank: 'The version is no longer offered for installation or updates. Installed copies keep running, with a notice.',
      disableTitle: 'Disable',
      disable: 'DeckPilot stops the installed plugin and refuses to load it again. Used for malicious or dangerous plugins.',
      cumulative: 'Revocations are cumulative: a new index must keep every published revocation at least as strong. Lifting one requires an offline root key ceremony.',
      link: 'Trust model',
    },
    reporting: {
      title: 'Reporting a vulnerability',
      lead: 'Report a malicious plugin, a registry or pipeline vulnerability, or an exposed key privately, through GitHub’s private vulnerability reporting on the registry repository. There is no separate email address or other channel.',
      dont: 'Do not open a public issue or pull request for a suspected malicious plugin or vulnerability, do not post exploit details publicly, and do not test against other users’ installations.',
      scope: 'Vulnerabilities in the DeckPilot app or agent, the plugin sandbox and host, the SDK or the client-side verification code belong to the DeckPilot repository instead. For a non-security problem with a plugin, use the Plugin report issue form.',
      after: 'There are no guaranteed response times; reports are handled before any other work. Reporters are credited in the advisory unless they prefer otherwise.',
      caveat: 'The registry has not had an independent security review.',
      primary: 'Report a vulnerability',
      policy: 'SECURITY.md',
      appPolicy: 'Report a DeckPilot vulnerability',
    },
    conduct: {
      title: 'Code of Conduct',
      text: 'By participating you agree to the Code of Conduct.',
      link: 'CODE_OF_CONDUCT.md',
    },
    other: {
      title: 'Other ways to contribute',
      text: 'Typos, clarifications and broken links in the documentation are welcome as ordinary pull requests. For a problem with the tooling or a workflow, open a bug report issue rather than a pull request: those paths are maintained by the owner.',
      issues: 'Open an issue',
    },
  },

  notFound: {
    title: 'Page not found',
    description: 'The page you are looking for does not exist on the DeckPilot Plugin Marketplace.',
    heading: 'This page does not exist',
    text: 'The address may be mistyped, or the page may have moved. The registry files that DeckPilot reads are not affected.',
    home: 'Back to the homepage',
    plugins: 'Explore plugins',
  },

  root: {
    title: 'DeckPilot Plugin Marketplace',
    description: 'Choose a language to continue to the DeckPilot Plugin Marketplace.',
    heading: 'Choose your language',
  },
};
