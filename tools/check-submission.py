#!/usr/bin/env python3
"""Submission rules for one pull request (stdlib only; run with python3 -I).

    check-submission.py --base SHA --head SHA --author LOGIN --author-id ID

Reads both trees with git; never runs anything from the pull request.

* Maintainers: a pull request whose author's login and numeric GitHub id
  both equal an entry of MAINTAINERS (the repository owner). Everything else
  is a contributor.
* Path guard: a contributor who is not a maintainer may change only
  registry/publishers/<their login>.json, registry/plugins/<ns>.*/** and
  plugins/<ns>.*/** for namespaces their publisher record owns (at the base,
  or added by this pull request), and documentation (docs/*.md, README
  files). Everything else (workflows, tools, schemas, keys/, revocations,
  other publishers, policy documents) is maintainer-only.
* Publisher records: github_id and login must be the pull request author's;
  a contributor cannot set "verified": true or take an existing namespace.
* Reserved publishers: a contributor can never use a publisher id DeckPilot
  reserves (RESERVED_PUBLISHERS, mirrored from deckpilot-plugin-manifest):
  not as a namespace, not in a plugin path, not as a manifest's
  publisher.id.
* First-party plugins (DeckPilot B6): `deckpilot` is DeckPilot's own
  publisher, served by this registry because its root-signed key set
  authorizes the namespace. Only a maintainer changes `deckpilot.*` records,
  and the publisher record of `deckpilot` must be FIRST_PARTY_OWNER's,
  verified. Every other reserved publisher stays refused for everyone (no
  client installs its plugins), and so do the plugin ids the application
  bundles (BUNDLED_PLUGIN_IDS: the agent never takes them from a registry).
* Versions are append-only: a recorded registry/plugins/<id>/versions/<v>.json
  is never changed or deleted (a maintainer may only add "yanked"), and a new
  version must be greater than every recorded one.
* Writes the permission diff of every new version against the previous one
  to the job summary.

Advisory: a pull request can edit this file and the workflow. CODEOWNERS
review and the checks on main are authoritative.
"""

import argparse
import json
import os
import re
import subprocess
import sys

# login -> numeric GitHub id. Both must match the pull request author.
MAINTAINERS = {"knittingcode": 109468044}
# The only publisher record that may own the namespace `deckpilot`.
FIRST_PARTY_OWNER = ("knittingcode", 109468044)
# Documentation anyone may propose (still code-owner reviewed). The policy
# documents (SECURITY.md, CONTRIBUTING.md, CODE_OF_CONDUCT.md) are not here.
DOC_PATHS = {"README.md", "plugins/README.md", "registry/README.md"}
# DeckPilot's RESERVED_PUBLISHERS (crates/deckpilot-plugin-manifest/src/
# identity.rs at the pinned commit): Manifest::validate refuses all of them;
# FIRST_PARTY_PUBLISHER is accepted only from the application bundle and,
# since B6, from a registry whose root-signed key set authorizes it. Keep in
# step when tools/deckpilot.env moves (tools/check-schemas.sh compares).
FIRST_PARTY_PUBLISHER = "deckpilot"
RESERVED_PUBLISHERS = frozenset({
    "deckpilot", "knittingcode", "core", "system", "plugin", "plugins", "builtin",
    "dev", "local", "test", "user",
    # Built-in modules.
    "general", "projects", "git", "docker", "http", "notifications", "github",
    "gitlab", "jira", "workspace", "work-session",
})
# DeckPilot's BUNDLED_PLUGIN_IDS (crates/deckpilot-plugin-bundled/src/lib.rs
# at the pinned commit): the agent refuses them from any registry.
BUNDLED_PLUGIN_IDS = frozenset({
    "deckpilot.github", "deckpilot.gitlab", "deckpilot.jira", "deckpilot.notifications",
})
PLUGIN_PATH = re.compile(r"^(?:registry/plugins|plugins)/([^/]+)/")
SEMVER_CORE = re.compile(r"^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$")
VERSION_PATH = re.compile(r"^registry/plugins/([^/]+)/versions/([^/]+)\.json$")

problems = []


def problem(message):
    problems.append(message)
    print(f"error: {message}", file=sys.stderr)


def git(*args):
    return subprocess.run(["git", *args], check=True, capture_output=True).stdout


def show(rev, path):
    try:
        return json.loads(git("show", f"{rev}:{path}"))
    except subprocess.CalledProcessError:
        return None
    except ValueError:
        problem(f"{path} at {rev[:12]} is not valid JSON")
        return None


def ls(rev, prefix):
    out = git("ls-tree", "-r", "--name-only", rev, "--", prefix).decode()
    return [line for line in out.splitlines() if line]


def semver_key(text):
    m = SEMVER_CORE.match(text)
    if not m:
        return None
    core = tuple(int(x) for x in m.group(1, 2, 3))
    pre = m.group(4)
    if pre is None:
        return (core, 1, ())
    return (core, 0, tuple((0, int(p), "") if p.isdigit() else (1, 0, p) for p in pre.split(".")))


def reserved_why(publisher):
    if publisher == FIRST_PARTY_PUBLISHER:
        return (f"the publisher {publisher!r} is reserved for DeckPilot's first-party plugins; "
                "community submissions cannot use it")
    return f"the publisher {publisher!r} is reserved by DeckPilot (Manifest::validate refuses it)"


def summary(*lines):
    path = os.environ.get("GITHUB_STEP_SUMMARY")
    if path:
        with open(path, "a", encoding="utf-8") as f:
            f.write("\n".join(lines) + "\n")
    for line in lines:
        print(line)


def perm_key(p):
    return json.dumps(p, sort_keys=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--author", required=True)
    parser.add_argument("--author-id", required=True, type=int)
    args = parser.parse_args()
    base, head = args.base, args.head
    maintainer = MAINTAINERS.get(args.author) == args.author_id

    changes = []
    for line in git("diff", "--name-status", "--no-renames", f"{base}...{head}").decode().splitlines():
        status, path = line.split("\t", 1)
        changes.append((status[0], path))

    # Namespaces the author owns: their publisher record at the base or head.
    own = set()
    for rev in (base, head):
        rec = show(rev, f"registry/publishers/{args.author}.json")
        if isinstance(rec, dict) and rec.get("github_id") == args.author_id:
            own.add(rec.get("namespace"))

    for status, path in changes:
        if maintainer:
            continue
        m = re.match(r"^(registry/plugins|plugins)/([a-z0-9-]+)\.[^/]+/", path)
        if path == f"registry/publishers/{args.author}.json":
            continue
        if path in DOC_PATHS or (path.startswith("docs/") and path.endswith(".md")):
            continue
        if m and m.group(2) in own:
            continue
        problem(f"{path}: only a maintainer may change this (see CONTRIBUTING.md, 'What you may change')")

    # Publisher records.
    for status, path in changes:
        if not re.match(r"^registry/publishers/[^/]+\.json$", path) or status == "D":
            if status == "D" and path.startswith("registry/publishers/") and not maintainer:
                problem(f"{path}: publisher records are never deleted by contributors")
            continue
        rec = show(head, path)
        old = show(base, path)
        if not isinstance(rec, dict) or maintainer:
            continue
        if rec.get("github_id") != args.author_id or rec.get("login") != args.author:
            problem(f"{path}: github_id and login must be yours ({args.author_id}, {args.author})")
        if rec.get("verified") and not (isinstance(old, dict) and old.get("verified")):
            problem(f"{path}: only a maintainer marks a publisher verified")
        for other in ls(base, "registry/publishers/"):
            orec = show(base, other)
            if other != path and isinstance(orec, dict) and orec.get("namespace") == rec.get("namespace"):
                problem(f"{path}: namespace {rec.get('namespace')!r} belongs to {orec.get('login')}")

    # Reserved publishers: never in a community pull request.
    if not maintainer:
        for status, path in changes:
            if status == "D":
                continue
            m = re.match(r"^(?:registry/plugins|plugins)/([^/.]+)\.[^/]+/", path)
            if m and m.group(1) in RESERVED_PUBLISHERS:
                problem(f"{path}: {reserved_why(m.group(1))}")
            if re.match(r"^registry/publishers/[^/]+\.json$", path):
                rec = show(head, path)
                if isinstance(rec, dict) and rec.get("namespace") in RESERVED_PUBLISHERS:
                    problem(f"{path}: {reserved_why(rec.get('namespace'))}")
            if path.endswith("/manifest.json") and path.startswith("plugins/"):
                doc = show(head, path)
                pub = doc.get("publisher") if isinstance(doc, dict) else None
                pub_id = pub.get("id") if isinstance(pub, dict) else None
                if pub_id in RESERVED_PUBLISHERS:
                    problem(f"{path}: {reserved_why(pub_id)}")

    # Reserved publishers in maintainer pull requests: only `deckpilot`, and
    # only with the owner's verified record.
    if maintainer:
        for status, path in changes:
            if status == "D":
                continue
            m = re.match(r"^(?:registry/plugins|plugins)/([^/.]+)\.[^/]+/", path)
            if m and m.group(1) in RESERVED_PUBLISHERS and m.group(1) != FIRST_PARTY_PUBLISHER:
                problem(f"{path}: {reserved_why(m.group(1))}")
            if re.match(r"^registry/publishers/[^/]+\.json$", path):
                rec = show(head, path)
                ns = rec.get("namespace") if isinstance(rec, dict) else None
                if ns == FIRST_PARTY_PUBLISHER:
                    owner = (rec.get("login"), rec.get("github_id"))
                    if owner != FIRST_PARTY_OWNER or rec.get("verified") is not True:
                        problem(f"{path}: the publisher {ns!r} must be {FIRST_PARTY_OWNER[0]} "
                                f"({FIRST_PARTY_OWNER[1]}), verified")
                elif ns in RESERVED_PUBLISHERS:
                    problem(f"{path}: {reserved_why(ns)}")

    # Bundled plugins: never from the registry, whoever proposes them.
    for status, path in changes:
        m = PLUGIN_PATH.match(path)
        if status != "D" and m and m.group(1) in BUNDLED_PLUGIN_IDS:
            problem(f"{path}: {m.group(1)} is bundled with DeckPilot; the agent never installs it "
                    "from a registry")

    # Version history: append-only.
    summary("### Permission changes", "")
    any_new = False
    for status, path in changes:
        m = VERSION_PATH.match(path)
        if not m:
            continue
        pid, version = m.groups()
        old = show(base, path)
        new = show(head, path)
        if old is not None:
            if status == "D":
                problem(f"{path}: recorded versions are never deleted (revoke or yank instead)")
                continue
            if not maintainer:
                problem(f"{path}: recorded versions are immutable; publish a new version instead")
                continue
            if isinstance(old, dict) and isinstance(new, dict):
                stripped = {k: v for k, v in new.items() if k != "yanked"}
                if stripped != {k: v for k, v in old.items() if k != "yanked"}:
                    problem(f"{path}: a recorded version may only gain 'yanked'")
            continue
        if status == "D" or not isinstance(new, dict):
            continue
        any_new = True
        key = semver_key(version)
        previous = []
        for other in ls(base, f"registry/plugins/{pid}/versions/"):
            om = VERSION_PATH.match(other)
            if om and semver_key(om.group(2)) is not None:
                previous.append((semver_key(om.group(2)), om.group(2), other))
        previous.sort()
        if key is None:
            problem(f"{path}: {version} is not a semantic version")
        elif previous and previous[-1][0] >= key:
            problem(f"{path}: {version} is not greater than the recorded {previous[-1][1]}")
        before = show(base, previous[-1][2]) if previous else None
        old_perms = {perm_key(p): p for p in (before or {}).get("permissions", [])}
        new_perms = {perm_key(p): p for p in new.get("permissions", [])}
        added = [new_perms[k] for k in new_perms if k not in old_perms]
        removed = [old_perms[k] for k in old_perms if k not in new_perms]
        label = f"`{pid}` {previous[-1][1]} → {version}" if previous else f"`{pid}` {version} (first version)"
        summary(f"**{label}**", "")
        for p in added:
            summary(f"- added: `{json.dumps(p, sort_keys=True)}`")
        for p in removed:
            summary(f"- removed: `{json.dumps(p, sort_keys=True)}`")
        if not added and not removed:
            summary("- no permission changes")
        src = new.get("source", {})
        if before and before.get("source", {}).get("repository") == src.get("repository"):
            summary(
                "",
                f"Source diff: https://github.com/{src.get('repository')}/compare/"
                f"{before['source'].get('commit')}...{src.get('commit')}",
            )
        elif src.get("repository"):
            summary("", f"Source: https://github.com/{src.get('repository')}/tree/{src.get('commit')}")
        summary("")
    if not any_new:
        summary("No new versions in this pull request.")

    if problems:
        summary("", f"**{len(problems)} submission rule violation(s)**; see the log.")
        return 1
    summary("", f"Submission rules: passed (author `{args.author}`{', maintainer' if maintainer else ''}).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
