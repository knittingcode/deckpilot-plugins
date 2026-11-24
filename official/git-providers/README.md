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
(`src/provider.rs`), one adapter per provider (`src/adapters/`). This
version has the GitLab adapter; GitHub follows.

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

## How it is built

The SDK pinned here predates plugin API 1.14, so the connection operations
(`connections.get`, `connections.reportStatus`, `time.sleep`, and
`http.fetch` with `connection`) go through the raw host call of guest
ABI 1 (`src/host.rs`). The module imports only `deckpilot_v1.host_call`.
The adapters reach the provider through a small transport trait
(`src/transport.rs`); the unit tests use a scripted transport, never a
real GitHub, GitLab or company server.

`cargo test -p deckpilot-git-providers` runs them.
