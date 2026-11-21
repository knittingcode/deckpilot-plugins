# Release governance of the official plugins

Who may authorize a release of an official plugin (the plugins DeckPilot
bundles), how the two publisher keys are used, and what each control
does and does not prove. The DeckPilot side is in
`docs/marketplace/publisher-signature.md` of `knittingcode/deckpilot`.

In short: the **default policy** is team-managed, with two people. DeckPilot
currently has **one maintainer**, so releases run under a **documented
single-maintainer exception** (the owner's decision of 2026-11-21). The
exception changes who approves a release. It does **not** lower the
signature threshold: every package still needs two valid signatures by two
distinct keys, from two separate signing jobs.

## Human approval and cryptographic signing

These are two different controls:

- **Human release approval** decides *who may start or authorize a
  release*: who can create the release tag, who approves the deployment
  to the environment `official-release`, and which settings and checks
  must hold before a key is used. It is enforced by GitHub (rulesets,
  environment protection) and by the unprivileged `gate` job. It is
  checked once, when the release runs. A user's DeckPilot never sees it.
- **Cryptographic package signing** proves *what a package is and where it
  came from*: that two pinned publisher keys signed exactly this file
  (id, version, sha256 and size in the trusted comment). Every DeckPilot
  build and the agent check it, offline, on every install and update,
  against the keys compiled into DeckPilot, with threshold 2, revocations
  and rollback floors.

Approval keeps the pipeline from signing what it should not. The
signatures are what users rely on. The single-maintainer exception
weakens the first control only. The second control is unchanged.

## Default policy (team-managed)

This is the intended production governance:

1. Releases are approved by **two distinct authorized people**.
2. The two signing keys are **independently controlled**: each is held by
   a different person, so neither can sign alone.
3. The person who starts a release **never approves their own release**.
4. All required CI, provenance checks and signature verification must
   pass. Here, provenance means: the tagged commit is on `main`, every
   check *Protect main* requires passed for it, the pinned DeckPilot
   commit is on DeckPilot's `main`, the toolchain and `Cargo.lock` are
   pinned, and two independent builds are byte-identical. There are no
   separate build attestations.

What GitHub enforces natively, and what it does not:

- An environment's **required reviewers** rule needs **one** approval from
  any one of the listed reviewers, however many are listed. GitHub has no
  native setting that requires two approvals for one deployment.
- With **Prevent self-review** on, the person who triggered the run cannot
  approve it, so that one approval comes from someone else.
- Together, these give a two-person rule: one person starts the release
  (creates the tag) and a different person approves it. That is **one
  initiator plus one approver**, not two approvals.
- The gate checks these settings when the repository variables are set.
  `OFFICIAL_RELEASE_REQUIRE_REVIEWERS=N` requires at least N listed
  reviewers. `OFFICIAL_RELEASE_REQUIRE_PREVENT_SELF_REVIEW=true` requires
  Prevent self-review. Neither can be met by a ruleset. Neither makes
  GitHub collect a second approval.
- Requiring two distinct *approvals* (both different from the initiator)
  is **not implemented**. It would need a check of the run's recorded
  approvals (`GET /repos/{repo}/actions/runs/{run_id}/approvals`) before
  any key is used. See [Transition](#transition-to-team-governance).

## Current single-maintainer exception

DeckPilot is a single-maintainer project. The owner (`knittingcode`) is
the only maintainer, administrator and release authority, and there is no
second trusted person. The owner decided on **2026-11-21** that, until a
second trusted maintainer joins:

- **The owner is the sole authorized release approver.** Releases go ahead
  without a second human reviewer. Points 1 and 3 of the default policy are
  suspended.
- **Both publisher keys stay mandatory.** Every package needs two valid,
  distinct signatures, from the separate jobs `sign-a` (key A only) and
  `sign-b` (key B only). `verify` has no secret and refuses anything less.
  The threshold is 2 and stays 2.
- **Required CI, package verification, provenance and security checks
  stay enforced**, unchanged (below).
- **Both signing keys are controlled by one person.** Key A and key B,
  and their passwords, are in the owner's custody, in one 1Password
  account. They do **not** provide independent custody. Point 2 of the
  default policy is not met.

This is a documented single-maintainer exception to the default policy.
It is not a reduction of the signature threshold.

### How a release is authorized under the exception: tag authority

With nobody else to approve a deployment, the authorization of every run
is the **release tag itself**, the same model `registry-release` uses:

- Only the repository administrator can create, move or delete a tag. The
  tag ruleset *Protect tags* covers `~ALL`, has no exclusion, restricts
  creation, update, deletion and non-fast-forward, and only the admin role
  can bypass it. The workflow token cannot bypass it.
- `official-release` deploys only from tags `deckpilot.*--v*` (one
  selected tag policy, no branch), and administrators cannot bypass the
  environment. Nothing that runs on a branch, a pull request, a fork or
  `workflow_run` can reach the keys.
- The unprivileged `gate` job of `official-release.yml`, and of the key
  test `official-test-sign.yml`, runs `tools/check-environment.sh
  official-release` and `tools/check-required-checks.sh` before any job
  that names the environment. It accepts the tag authority above, or a
  required reviewer if the environment lists one.

`tools/check-environment.sh official-release` passes only if all of the
following hold. It fails closed on any error or unexpected answer:

| Requirement | Fails when |
| --- | --- |
| The environment exists. | It is missing or cannot be read. |
| `can_admins_bypass` is `false`. | Administrators can bypass it. |
| The deployment policies are selected ones, exactly `tag:deckpilot.*--v*`. | It deploys from protected branches, every branch, any branch, `*--v*` or `deckpilot.*`, or anything else is added. |
| There is a required reviewer, **or** an active tag ruleset that includes `~ALL`, `refs/tags/*`, `refs/tags/*--v*` or `refs/tags/deckpilot.*--v*`, has no exclusion, restricts creation, update and deletion, and can never be bypassed by the workflow token. | There is no such ruleset. The ruleset only evaluates or is disabled, has any exclusion, leaves creation, update or deletion unrestricted, or can be bypassed by the token (always or for pull requests). The ruleset cannot be read. |
| If `OFFICIAL_RELEASE_REQUIRE_REVIEWERS` or `OFFICIAL_RELEASE_REQUIRE_PREVENT_SELF_REVIEW` is set, the environment meets it. | The environment lists fewer reviewers, or allows self-review. A ruleset never meets these. |

The key test tag `deckpilot.publisher-test--v<n>` matches both the
environment's `deckpilot.*--v*` policy and *Protect tags* (`~ALL`). The key
test is authorized the same way: the administrator creates that tag on a
commit of `main`.

These stay enforced on top of the gate:

- `OFFICIAL_RELEASE_ENABLED` must be `true`, or every release run stops
  in `resolve`.
- The tagged commit must be on `main` (`official.sh resolve` and
  `test-resolve`). Every check *Protect main* requires must have passed
  for it (`check-required-checks.sh`).
- There are two separate signing jobs, `sign-a` with only key A and
  `sign-b` with only key B, and nothing is compiled in them. A `verify`
  job without any secret requires both signatures against the keys pinned
  in DeckPilot (threshold 2). There is no `release` without `verify`, and
  nothing is ever published unsigned.
- The privileged policy record, the privileged-kinds gate (open for
  exactly that record) and the process allow-lists are unchanged by the
  exception.

### Settings of the exception

| Setting | Value |
| --- | --- |
| Environment `official-release`: required reviewers | none |
| Environment `official-release`: deployment policy | selected, one tag policy `deckpilot.*--v*`, no branch |
| Environment `official-release`: administrators can bypass | no |
| Environment `official-release`: secrets | `DECKPILOT_PUBLISHER_KEY_A`, `_A_PASSWORD`, `DECKPILOT_PUBLISHER_KEY_B`, `_B_PASSWORD` (unchanged) |
| Ruleset *Protect tags* | active, `~ALL`, no exclusion, creation, update, deletion and non-fast-forward, bypass for the admin role only (unchanged) |
| Variables `OFFICIAL_RELEASE_REQUIRE_REVIEWERS`, `OFFICIAL_RELEASE_REQUIRE_PREVENT_SELF_REVIEW` | unset |
| Variable `OFFICIAL_RELEASE_ENABLED` | the owner's switch (`true` only when releases should run) |

### What threshold 2 still protects against

- **One leaked key or secret.** A copy of one secret key file, one
  password or one GitHub secret signs nothing a DeckPilot build accepts.
- **One compromised signing job.** `sign-a` and `sign-b` run on separate
  runners with one key each. A compromised job, or a dependency it runs,
  sees one key only. No job ever holds both.
- **Signature substitution.** Each `<package>.<KEYID>.minisig` counts only
  for the one pinned key whose id is both in its file name and inside the
  signature, at most once per key. The trusted comment binds id, version,
  sha256 and size, so a signature cannot be moved to another plugin,
  version or file. A key test signature is never a release signature.
- **A tampered package.** Two independent builds must agree byte for byte
  before anything is signed. Both signing jobs re-check the package
  against the agreed sha256 and size. `verify` and every DeckPilot build
  check the signatures, size and sha256 before parsing.
- **An unauthorized tag.** Without the administrator's rights, nobody can
  create, move or delete a tag, and no other ref can deploy from
  `official-release`.

### What it does not protect against

- **One compromised maintainer account or token.** The owner's GitHub
  account, or a token with its rights, can change the environment, its
  secrets, the rulesets and the workflows. It can also create tags,
  approve deployments and merge pull requests. Both keys are in one
  1Password account of the same person. Whoever controls either account
  can obtain both keys or make the pipeline sign.
- **No independent custody and no separation of duties.** The author, the
  reviewer, the approver and the release authority are one person. Two
  signatures prove two keys, not two people.
- **A listed reviewer adds nothing independent.** If a required reviewer is
  listed, it is the same account: it adds a click, not an independent
  decision.

### Accepted risk

On **2026-11-21** the owner accepted that official releases are
authorized by one maintainer (tag authority, no second reviewer), and that
both keys are in that maintainer's custody, because there is no second
maintainer. Threshold 2 is kept for what it still protects (above). The
compensating controls below are kept for three purposes:

- a mistake cannot produce a release;
- a single leak or a compromised job cannot produce a release;
- a bad release can be stopped and rolled back.

### Compensating controls

- **Protected tags.** Tags are admin only and immutable (*Protect tags*:
  creation, update, deletion, non-fast-forward), and the workflow token
  cannot bypass the ruleset.
- **Required CI on the tagged commit.** Every check *Protect main* requires
  must have passed for the commit the tag names (`check-required-checks.sh`).
- **Main ancestry.** The tagged commit must be on `main`, which takes
  changes only through pull requests (*Protect main*: no direct push, no
  force push, no deletion). The ruleset's code-owner review cannot come
  from a second person. The administrator merges by the ruleset's
  pull-request bypass, which could also skip a failing check. This is why
  the gate checks the required checks again on the tagged commit itself.
  The pinned DeckPilot commit must be on DeckPilot's `main` too
  (`require_on_main`).
- **Independent reproducible builds.** `build-a` and `build-b` run on
  fresh runners and must produce a byte-identical module and package before
  any key is used.
- **Verify against the pinned keys.** `verify` (no secret) checks both
  signatures against the keys compiled into the pinned DeckPilot build
  (never `--keys`), with threshold 2. Each signature alone is refused.
- **Revocations.** `revokedKeys` in `first-party.json` and the revoked
  releases in `artifacts.json` apply in `verify`, in every build and in the
  agent at run time.
- **Rollback floors.** `released-pins.json`, the bundled pins and the
  agent's recorded floors refuse any older release, and the same version
  with other files.
- **Audit trail.** The agent keeps a chained audit log of every
  verification. GitHub keeps the deployment history of `official-release`,
  the workflow runs, the tags and the releases.
- **`OFFICIAL_RELEASE_ENABLED`.** When this switch is not `true`, every
  release run stops in `resolve`.
- **1Password custody of the keys.** Both encrypted secret keys and their
  passwords are kept in 1Password, with offline encrypted backups, outside
  any repository. GitHub holds them only as environment secrets. This is
  custody by one person, not independent custody.

### Earlier review findings under this model

The security reviews of the official-plugin pipeline found these issues
(fixed in #68 and #69 here, and in knittingcode/deckpilot#379 and #380).
Their status for a single maintainer:

| Finding | Status |
| --- | --- |
| 3 (medium): the DeckPilot pin must be on DeckPilot `main` | Fixed. Unchanged. |
| 4 (medium): independent builds | Fixed. Unchanged, and now also a compensating control. |
| **M5** (medium): who approves a signing run (someone other than the person who pushed the tag) | **Accepted risk (single maintainer).** Nobody else exists to approve. The `REQUIRE_REVIEWERS` and `REQUIRE_PREVENT_SELF_REVIEW` checks stay in the gate, unset, for the transition. |
| 10 and 11 (low): the privileged mirror; records checked against the signed manifest | Fixed. Unchanged. |
| 12 and 13 (low): the order of release signing; the guard scripts | Fixed. Unchanged. |
| N1 and N3 (re-review): resource parameters; nothing compiles where a key is | Fixed. Unchanged. |
| M1, M2 and L6 to L9 (DeckPilot, #380): revocation, floors, audit | Fixed. Unchanged, and part of the compensating controls. |
| Key custody (item K of DeckPilot's `publisher-review-request.md`) | Accepted risk (single maintainer). One person controls both keys, so there is no independent custody. |

The privileged-kinds gate is a separate decision. It stays closed, and
this page does not change its review request.

## Transition to team governance

Once a second trusted maintainer joins:

1. **Establish independent control of the second signing key.** The new
   maintainer creates a new key B offline, on their own machine, with
   their own password, and keeps it in their own 1Password account. Pin
   it in DeckPilot's `publisher/first-party.json` in a reviewed pull
   request: the old key B goes into `revokedKeys`, and the threshold stays
   2. Release DeckPilot and move `tools/deckpilot.env` here. Then the new
   maintainer replaces `DECKPILOT_PUBLISHER_KEY_B` and its password. Run
   the key test.
2. **Configure protected release approval by distinct authorized
   maintainers.** In `official-release`, list both maintainers as required
   reviewers and turn on *Prevent self-review*. Keep the tag policy and
   *Protect tags*, and keep administrators unable to bypass the
   environment.
3. **Enforce two-person approval with a mechanism that actually checks
   it.** First set `OFFICIAL_RELEASE_REQUIRE_REVIEWERS=2` and
   `OFFICIAL_RELEASE_REQUIRE_PREVENT_SELF_REVIEW=true`, after step 2 (the
   gate fails closed until the environment meets them). This enforces
   that the person who creates the tag cannot approve its signing run, so
   every release involves two distinct people: one initiator and one
   approver. It does **not** make GitHub collect two approvals. If the
   policy is two approvers besides the initiator, implement and test a
   check before any key is used. The check reads the run's approvals
   (`actions/runs/{run_id}/approvals`) and requires two distinct
   approvers, neither of them the initiator. The workflows do not have
   this check today. Document what the chosen mechanism enforces.
4. **Restore the team-managed policy.** Update this page, the official
   README and DeckPilot's `publisher-signature.md` to describe the default
   policy as in force.
5. **Remove the exception.** Delete the "Current single-maintainer
   exception" section and record the date its accepted risk ended.
