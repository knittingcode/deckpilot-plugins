# deckpilot.git-providers

> **Held ahead of the pin.** This plugin needs plugin API 1.14
> (connections), which no DeckPilot provides yet. `plugins.json` marks it
> `held` with `hostApi: "1.14"`, so it is built and tested here but never
> packaged, signed, released or given a Marketplace entry until its host
> side and bundling land in DeckPilot (`design-git-providers.md` §14,
> PR-2 and PR-6). Paths below that are not relative links are in
> [knittingcode/deckpilot](https://github.com/knittingcode/deckpilot).

Repositories, pull and merge requests, pipelines and issues of the GitHub
and GitLab servers you connect: GitHub.com, GitHub Enterprise Server,
GitLab.com and self-managed GitLab on any port or path prefix
(`https://code.example.com:8443/gitlab`). One plugin, one provider trait
(`src/provider.rs`), one adapter per provider (`src/adapters/`). There are
two adapters: GitLab and GitHub (GitHub Actions runs are its pipelines;
starting one dispatches a workflow file).

## Connections and tokens

Every action takes a `connection`: a server you connect in DeckPilot's Git
Providers screen, with its own token. The host makes every request on that
connection, adds its secret and keeps it inside the connection's origin,
path prefix and methods; the plugin never sees a token. A new connection
is read-only until you allow changes on it.

Repositories are named by the provider's id (`gl:4711`), never by path,
so a rename or a transfer changes nothing.

## Changes from a key

Opening a merge request and starting a pipeline change something. From a
key or a gesture they ask first (the `confirm` parameter, on by default);
a workflow you started runs them without asking; a schedule or an event
runs them only when you turned `confirm` off for that action.

## Legacy GitHub and GitLab actions (migration)

Phase 2 of the move from the old `deckpilot.github` 0.2.1 and
`deckpilot.gitlab` 0.1.2 plugins (DeckPilot `design-git-providers.md`
§11.3) needs a successor for every one of their actions. These are
`legacy-github-{current-user,repository,pull-requests,review-requests,checks,open}`
and
`legacy-gitlab-{current-user,project,merge-requests,review-requests,pipeline,open}`.

- **Hidden.** They are `"deprecated": true`, so the Action Library never
  offers them. They exist only as targets of the old action ids once you
  switch the old plugin's keys to Git Providers: `PLUGIN_SUCCESSION`, with
  the `connection` added by the host (`ParamRule::InjectConnection`).
- **The old contracts, exactly.** Each has the old action's parameters,
  outputs, error codes and node spec, with the `connection` the host
  injects in front. The old error codes (`invalid-api-url`,
  `invalid-project`, `github-error`, `gitlab-error`, `timed-out`) are
  declared here too.
- **The old code.** The code is the old plugins' (`src/legacy/`), with two
  changes:
  - the server is the connection's API and web bases, not the old `apiUrl`
    setting;
  - the time limit is the old default of 20 s, since Git Providers has no
    `timeoutSeconds`. **Open:** `timeoutSeconds` is not carried over; it is
    20 s; the decision is pending (owner, DeckPilot PR-10).
- **Read-only.** `GET` only, even on a connection that allows changes.
- **No credentials in the plugin.** The old plugins kept a token in their
  own credential slot. The host copies it into the connection
  (DeckPilot's migration, PR-10), after you approve it, and keeps the old
  one; the plugin never reads either. The old plugins themselves
  (`official/github`, `official/gitlab`) are unchanged.
- **Tests:**
  - `src/legacy/tests.rs` compares every legacy action with its source
    action in `official/{github,gitlab}/manifest.json` (the contracts
    DeckPilot bundles), and fails on any difference;
  - `src/legacy/parity_{github,gitlab}.rs` run the old plugins' own unit
    tests unchanged through the legacy actions on the scripted transport.
    A coverage test fails when an old test is missing, differs by a byte,
    or is replaced without a stated reason.

## How it is built

The SDK pinned here predates plugin API 1.14, so the connection operations
(`connections.get`, `connections.reportStatus`, `time.sleep`, and
`http.fetch` with `connection`) go through the raw host call of guest
ABI 1 (`src/host.rs`). The module imports only `deckpilot_v1.host_call`.
The adapters reach the provider through a small transport trait
(`src/transport.rs`); the unit tests use a scripted transport, never a
real GitHub, GitLab or company server.

`cargo test -p deckpilot-git-providers` runs them.
