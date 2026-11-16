# Review policy

Every change to this repository is a pull request that the owner
(`@knittingcode`, the code owner of every path) reviews and approves. No
submission is ever merged automatically, and a passing CI run is necessary
but not sufficient.

## Plugin submissions

The reviewer checks, with the job summary of *Validate registry* and
*Submission rules* open:

1. **Publisher identity.** For a new publisher: the GitHub account exists,
   its numeric id matches `github_id`, and the namespace is not misleading
   (no impersonation of another project, company or DeckPilot). The owner
   alone sets `"verified": true`.
2. **Source.** The repository and tag are the author's; the tag resolves to
   the recorded commit (CI checks it); the source diff since the previous
   version (compare link in the summary) contains nothing unexplained:
   obfuscation, generated blobs, build scripts that fetch or execute
   anything, vendored binaries.
3. **Permissions.** Each permission has a justification that matches what
   the plugin does; network hosts belong to the service the plugin claims;
   credential slots match those hosts; `openUrl` hosts are the pages the
   plugin's actions open for the user (the plugin's own service, its
   documentation), never a broad wildcard; a `userOrigins` slot is for a
   service the user hosts themselves (the address is theirs to enter and
   to consent to), never a way around naming a fixed service in `hosts`,
   and `privateNetwork` is set only when such a server can be on a private
   network. Any new permission in an update is
   an escalation: it is reviewed as carefully as a new plugin, and users
   are asked again before it applies.
4. **Build.** The recipe uses an exact toolchain, a locked Cargo workspace
   and a plain `cargo build` of one package; the package was rebuilt by CI
   and matches the record byte for byte.
5. **License.** The manifest declares an SPDX license, and the package
   ships its `LICENSE`. The plugin's license is the author's choice; this
   repository's Apache-2.0 license does not apply to it.
6. **Listing.** Name, summary, description and categories are accurate and
   not misleading.

Approval of a submission does not publish it. The owner publishes by
creating the release tag ([publishing.md](publishing.md)).

## Changes to tooling, workflows, schemas, keys

These are owner-only (the *Submission rules* path guard refuses them from
other authors, and CODEOWNERS requires the owner's review). In particular:

- `tools/deckpilot.env`, `schemas/` and `rust-toolchain.toml` move only
  together, to a reviewed DeckPilot commit; *Schemas match DeckPilot*
  checks they agree.
- `keys/` changes only after an offline root ceremony.
- `registry/revocations.json` changes only by the owner, and only by
  adding or strengthening revocations.

## Single maintainer (U-8)

There is one maintainer today. The owner's own pull requests are merged
through the ruleset's pull-request-only admin bypass after CI passes; the
design recommends a second maintainer before the registry opens to the
public (DeckPilot #191, section 19.2).

## Response times

There are no guaranteed response times. Security reports are handled
first ([SECURITY.md](../SECURITY.md)).
