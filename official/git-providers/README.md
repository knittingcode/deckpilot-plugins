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

## Pipelines

GitLab pipelines and GitHub Actions workflow runs are both "pipelines"
here, with one closed set of states (`pending`, `running`, `success`,
`failed`, `cancelled`, `skipped`, `manual`, `unknown`) and the provider's
own word in `providerStatus` (`completed/success` on GitHub).

- **`pipeline-status`** reads the latest pipeline of a change request
  (`changeRequest`, which wins over `ref`), of a branch, tag or commit
  (`ref`), or of the default branch when neither is given. With none,
  `state` is `none`. A key bound to it shows the last run's state (green,
  red, a running mark) with the branch or `#number` as its second line;
  the key reads what the last press or workflow node stored, and nothing
  polls the server in the background.
- **`trigger-pipeline`** starts one. On GitLab it can pass `variables`
  (names `[A-Z_][A-Z0-9_]{0,63}`, not `CI_*` or `GITLAB_*`, values of at
  most 1 KiB without control characters or anything that looks like a
  token, at most 50). On GitHub it dispatches a workflow file
  (`workflow`, picked from `list-workflows`) with optional `inputs`;
  `variables` are refused there. Everything that would refuse a trigger is
  refused before you are asked and before any request. It is never
  retried, by the engine or by the plugin.
  - GitHub answers a dispatch without the run it started. The plugin then
    looks for the oldest run of that same workflow, on that ref, by the
    same account, created since the dispatch (a few seconds of clock skew
    allowed): never an older run, another account's or another workflow's.
    It looks three times, two seconds apart. If the run has not shown up,
    the action still succeeds (the dispatch happened), with no
    `pipelineId` and `url` the workflow's page of runs.
- **`wait-pipeline`** reads the pipeline every `pollSeconds` (10 to 120,
  default 15) in sleeps of at most 30 s, and ends at once when it has
  ended or is waiting for a manual step. Cancelling the run ends it at once
  with `cancelled`. After 540 s it ends with `timedOut: true` and the
  last `state` it read (`running`), as a success, so a Condition on
  `state` or a Loop around the node goes on; the node's own limit is
  600 s.
- **`list-workflows`** (internal: the `workflow` picker) lists the
  `.github/workflows` files of a GitHub repository. On GitLab it is
  refused with `capability-unsupported`: a GitLab pipeline runs the
  project's `.gitlab-ci.yml`.

A typical workflow is three nodes: `create-change-request`, then
`trigger-pipeline` with `ref` mapped from its source branch, then
`wait-pipeline` with `pipelineId` mapped from the trigger, and a
Condition on `state == "success"`.

## Self-managed servers

GitHub Enterprise Server and self-managed GitLab are connected in the app
with the web address you know, on any port and under any path prefix:
`https://ghe.example.com:8443` (API `…/api/v3`),
`https://code.example.com:8443/gitlab` (API `…/gitlab/api/v4`).

- Every URL the plugin sends or opens is built from the connection's API
  and web bases that the host hands over (`connections.get`), segment by
  segment: never `/api/v40`, never a double slash. A cloud connection has
  exactly the fixed `api.github.com`/`github.com` or `gitlab.com` bases,
  and any other connection has its pages on the API's own host and port;
  anything else is not used.
- A link in an answer (a pipeline's or repository's page) is kept only
  when it is an `https` page under the connection's web base; pages the
  plugin opens are built on the server, never taken from an answer. A
  next-page link outside the API base (another host or port, a sibling
  path, a dot segment, `http`) fails with `invalid-response` and is never
  followed.
- A redirect is never followed: the host refuses one off the connection
  with `redirect-refused`, and a redirect answer that reaches the plugin
  fails the same way. Nothing is retried somewhere else.
- The plugin never builds a credential, an auth header or a token in a
  URL: the host adds the connection's secret. TLS is the system's (add
  your company's certificate authority to the system); there is no way to
  skip a certificate check.
- The unit tests run all of this against scripted answers only
  (`src/tests_base_urls.rs`, `src/tests_pipelines.rs`), never a network.

## How it is built

The SDK pinned here predates plugin API 1.14, so the connection operations
(`connections.get`, `connections.reportStatus`, `time.sleep`, and
`http.fetch` with `connection`) go through the raw host call of guest
ABI 1 (`src/host.rs`). The module imports only `deckpilot_v1.host_call`.
The adapters reach the provider through a small transport trait
(`src/transport.rs`); the unit tests use a scripted transport, never a
real GitHub, GitLab or company server.

`cargo test -p deckpilot-git-providers` runs them.
