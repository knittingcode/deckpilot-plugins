#!/usr/bin/env python3
"""Rules of official/, the plugins DeckPilot bundles (stdlib only; run with python3 -I).

    check-official.py --root official --commit SHA --app-version V --plugin-api M.N
                      [--deckpilot DIR] [--list] [--release ID]

official/ is one Cargo workspace (official/README.md). This checks what
neither cargo nor the manifest schema does:

* Layout: every workspace member is listed in official/plugins.json, once,
  as a plugin (released) or a fixture (never released), and every listed
  path is a member with manifest.json, LICENSE and Cargo.toml; a plugin's
  id is one DeckPilot bundles (BUNDLED_PLUGIN_IDS), a fixture's is
  `deckpilot.*` and not one; the same version in Cargo.toml and
  manifest.json; publisher `deckpilot`.
* Pinned SDK: every git dependency of official/Cargo.toml is
  knittingcode/deckpilot at `rev` = the pinned commit (tools/deckpilot.env),
  never a branch or tag; no path dependency leaves official/; Cargo.lock
  exists and names no other DeckPilot revision.
* Permission policy: the manifest's permission kinds equal the plugin's
  `permissions` in plugins.json (a new kind is a reviewed policy change).
  While the privileged-kinds gate is closed (DeckPilot
  docs/marketplace/publisher-review-request.md) no plugin may declare any
  plugin API 1.9 privileged kind unless plugins.json marks it `held` (a
  reason): built, tested, packaged and compared, never released; process
  execution (`tools`) is in any case only ever for deckpilot.git (git) and
  deckpilot.docker (docker).
* Compatibility: engines.pluginApi is satisfied by the pinned DeckPilot's
  plugin API (--plugin-api) and engines.deckpilot accepts its version
  (--app-version), so `release verify` will accept the host.
* With --deckpilot DIR (a checkout of the pinned commit): every fixture is
  byte-identical to DeckPilot's plugins/first-party/test-fixtures/<name>,
  Cargo.toml excepted; and every plugin that DeckPilot still has in
  plugins/first-party/<name> keeps its contract: the same manifest but for
  `version` and `repository`, the same locales and assets.

--list prints `<path> <id> <version> <package> <kind>` per member, kind
`plugin`, `held` (a plugin marked held) or `fixture`.
--release ID fails unless ID is a released plugin (not held, not a fixture).
"""

import argparse
import filecmp
import json
import re
import sys
from pathlib import Path

DECKPILOT_GIT = "https://github.com/knittingcode/deckpilot"
PUBLISHER = "deckpilot"
# DeckPilot's BUNDLED_PLUGIN_IDS (crates/deckpilot-plugin-bundled/src/lib.rs).
BUNDLED_PLUGIN_IDS = frozenset({
    "deckpilot.docker", "deckpilot.git", "deckpilot.github", "deckpilot.gitlab", "deckpilot.jira",
    "deckpilot.notifications", "deckpilot.projects", "deckpilot.workflows", "deckpilot.workspace",
})
# Plugin API 1.9's privileged kinds (PermissionKind::is_first_party_only and
# the other usages of IssueCode::FirstPartyOnly).
PRIVILEGED_KINDS = frozenset({"tools", "resources", "projectContext", "hostViews"})
# The privileged-kinds gate of DeckPilot's publisher signature: closed until
# the security review. Opening it is a reviewed change here AND in DeckPilot.
PRIVILEGED_GATE_OPEN = False
# Process execution, ever: only these plugins, only these executables.
PROCESS_ALLOW_LIST = {"deckpilot.git": ["git"], "deckpilot.docker": ["docker"]}

problems = []


def problem(message):
    problems.append(message)
    print(f"error: {message}", file=sys.stderr)


def load(path):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as e:
        problem(f"{path}: {e}")
        return None


def toml_value(text, key):
    m = re.search(rf'^{re.escape(key)}\s*=\s*"([^"]*)"', text, re.M)
    return m.group(1) if m else None


def members(cargo):
    m = re.search(r"^members\s*=\s*\[(.*?)\]", cargo, re.M | re.S)
    if not m:
        return []
    return re.findall(r'"([^"]+)"', m.group(1))


def check_pins(root, commit):
    cargo = (root / "Cargo.toml").read_text(encoding="utf-8")
    for line in cargo.splitlines():
        code = line.split("#", 1)[0]
        if "git" in code and "=" in code and "{" in code:
            git = re.search(r'git\s*=\s*"([^"]+)"', code)
            if not git:
                continue
            name = code.split("=", 1)[0].strip()
            if git.group(1) != DECKPILOT_GIT:
                problem(f"{root}/Cargo.toml: {name} is a git dependency on {git.group(1)}, only {DECKPILOT_GIT}")
            if re.search(r"\b(branch|tag)\s*=", code):
                problem(f"{root}/Cargo.toml: {name} names a branch or tag; pin `rev` to the commit")
            rev = re.search(r'rev\s*=\s*"([0-9a-f]{40})"', code)
            if not rev or rev.group(1) != commit:
                problem(f"{root}/Cargo.toml: {name} must be pinned to rev {commit} (tools/deckpilot.env), "
                        f"found {rev.group(1) if rev else 'no full rev'}")
        for path in re.findall(r'path\s*=\s*"([^"]+)"', code):
            target = (root / path).resolve()
            if root.resolve() not in target.parents and target != root.resolve():
                problem(f"{root}/Cargo.toml: path dependency {path} leaves official/")
    lock = root / "Cargo.lock"
    if not lock.is_file():
        problem(f"{lock} is missing: a build must be locked")
        return
    for source in re.findall(r'^source = "git\+([^"]+)"', lock.read_text(encoding="utf-8"), re.M):
        url, _, resolved = source.partition("#")
        if not url.startswith(DECKPILOT_GIT + "?rev=") or resolved != commit:
            problem(f"{lock}: git source {source} is not DeckPilot at {commit}")


def api_ok(requirement, plugin_api):
    m = re.fullmatch(r"\^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", requirement or "")
    h = re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", plugin_api)
    if not m or not h:
        return False
    return int(m.group(1)) == int(h.group(1)) and int(h.group(2)) >= int(m.group(2))


def version_key(text):
    m = re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", text)
    return tuple(int(x) for x in m.groups()) if m else None


def range_ok(text, version):
    """engines.deckpilot: one or two comparators (>=, >, <=, <, =, bare)."""
    v = version_key(version)
    if v is None or not text:
        return False
    for part in text.split(" "):
        m = re.fullmatch(r"(>=|<=|>|<|=)?(\S+)", part)
        bound = version_key(m.group(2)) if m else None
        if bound is None:
            return False
        op = m.group(1) or "="
        if not {">=": v >= bound, ">": v > bound, "<=": v <= bound, "<": v < bound, "=": v == bound}[op]:
            return False
    return True


def check_permissions(where, entry, manifest, held=False):
    declared = [p.get("kind") for p in manifest.get("permissions", [])]
    allowed = entry.get("permissions")
    if not isinstance(allowed, list):
        problem(f"{where}: plugins.json gives no `permissions` list")
        return
    if sorted(set(declared)) != sorted(set(allowed)):
        problem(f"{where}: declares permission kinds {sorted(set(declared))}, the policy in plugins.json "
                f"is {sorted(set(allowed))}; a change of permissions is a reviewed policy change")
    for p in manifest.get("permissions", []):
        kind = p.get("kind")
        if kind == "tools":
            names = sorted(t.get("executable", "?") for t in p.get("tools", []) if isinstance(t, dict))
            if names != PROCESS_ALLOW_LIST.get(manifest.get("id"), None):
                problem(f"{where}: process execution (`tools`) is only for "
                        f"{', '.join(f'{k} ({v[0]})' for k, v in PROCESS_ALLOW_LIST.items())}; "
                        f"{manifest.get('id')} asks for {names or 'it'}")
        if kind in PRIVILEGED_KINDS and not PRIVILEGED_GATE_OPEN and not held:
            problem(f"{where}: `{kind}` is a plugin API 1.9 privileged kind; a signed release artifact "
                    "cannot carry it while the gate is closed (DeckPilot "
                    "docs/marketplace/publisher-review-request.md); mark the plugin `held` in plugins.json")
        if p.get("fromParams") and not PRIVILEGED_GATE_OPEN and not held:
            problem(f"{where}: `openUrl.fromParams` is privileged; the gate is closed")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", required=True, type=Path)
    ap.add_argument("--commit", required=True)
    ap.add_argument("--app-version", required=True)
    ap.add_argument("--plugin-api", required=True)
    ap.add_argument("--deckpilot", type=Path)
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--release")
    args = ap.parse_args()
    root = args.root
    if not re.fullmatch(r"[0-9a-f]{40}", args.commit):
        problem(f"--commit {args.commit} is not a full commit")
        return 1

    policy = load(root / "plugins.json")
    cargo_text = (root / "Cargo.toml").read_text(encoding="utf-8")
    check_pins(root, args.commit)
    if policy is None:
        return 1
    if policy.get("officialVersion") != 1:
        problem("official/plugins.json: officialVersion must be 1")
    entries = [(e, "plugin") for e in policy.get("plugins", [])] + [(e, "fixture") for e in policy.get("fixtures", [])]
    listed = [e.get("path") for e, _ in entries]
    workspace = members(cargo_text)
    if workspace != sorted(workspace):
        problem("official/Cargo.toml: members must be sorted")
    for path in sorted(set(workspace) - set(listed)):
        problem(f"official/{path} is a workspace member but not in plugins.json")
    for path in sorted({p for p in listed if listed.count(p) > 1}):
        problem(f"official/plugins.json lists {path} twice")

    rows = []
    ids = set()
    for entry, kind in entries:
        path = entry.get("path", "")
        where = f"official/{path}"
        if path not in workspace:
            problem(f"{where} is in plugins.json but not a workspace member")
        directory = root / path
        manifest = load(directory / "manifest.json")
        if manifest is None:
            continue
        for required in ("LICENSE", "Cargo.toml"):
            if not (directory / required).is_file():
                problem(f"{where}/{required} is missing")
        mid = manifest.get("id", "")
        if mid != entry.get("id"):
            problem(f"{where}: manifest id {mid} is not plugins.json's {entry.get('id')}")
        if mid in ids:
            problem(f"{mid} is listed twice")
        ids.add(mid)
        if kind == "plugin" and mid not in BUNDLED_PLUGIN_IDS:
            problem(f"{where}: {mid} is not a plugin DeckPilot bundles; only those are released from official/")
        if kind == "fixture" and (mid in BUNDLED_PLUGIN_IDS or not mid.startswith("deckpilot.")):
            problem(f"{where}: a fixture is a deckpilot.* id that DeckPilot does not bundle, not {mid}")
        if manifest.get("publisher", {}).get("id") != PUBLISHER:
            problem(f"{where}: publisher.id must be {PUBLISHER}")
        cargo = (directory / "Cargo.toml").read_text(encoding="utf-8") if (directory / "Cargo.toml").is_file() else ""
        package = toml_value(cargo, "name") or ""
        cargo_version = toml_value(cargo, "version")
        if cargo_version != manifest.get("version"):
            problem(f"{where}: Cargo.toml version {cargo_version} differs from manifest.json {manifest.get('version')}")
        engines = manifest.get("engines", {})
        if not api_ok(engines.get("pluginApi"), args.plugin_api):
            problem(f"{where}: engines.pluginApi {engines.get('pluginApi')} is not satisfied by the pinned "
                    f"DeckPilot's plugin API {args.plugin_api}")
        if not range_ok(engines.get("deckpilot", ""), args.app_version):
            problem(f"{where}: engines.deckpilot {engines.get('deckpilot')} does not accept the pinned "
                    f"DeckPilot {args.app_version}")
        held = entry.get("held")
        if held is not None and (kind != "plugin" or not isinstance(held, str) or not held.strip()):
            problem(f"{where}: `held` is a reason (text), for a plugin only")
        if held:
            privileged = ({p.get("kind") for p in manifest.get("permissions", [])} & PRIVILEGED_KINDS) | (
                {"openUrl.fromParams"} if any(p.get("fromParams") for p in manifest.get("permissions", [])) else set())
            if not privileged or PRIVILEGED_GATE_OPEN:
                problem(f"{where}: held, but nothing holds it (no privileged kind, or the gate is open); "
                        "remove `held`")
            kind = "held"
        check_permissions(where, entry, manifest, held=bool(held))
        if kind == "fixture" and args.deckpilot:
            theirs = args.deckpilot / "plugins/first-party/test-fixtures" / Path(path).name
            compare_tree(where, directory, theirs)
        if kind != "fixture" and args.deckpilot:
            compare_contract(where, directory, manifest, args.deckpilot / "plugins/first-party" / Path(path).name)
        rows.append((path, mid, manifest.get("version", ""), package, kind))

    if args.release is not None:
        released = {mid for (_, mid, _, _, kind) in rows if kind == "plugin"}
        if args.release not in released:
            problem(f"{args.release} is not a released official plugin (plugins.json `plugins`)")

    if problems:
        return 1
    if args.list:
        for row in rows:
            print(" ".join(row))
    else:
        print(f"official: {len(rows)} member(s) checked, SDK pinned to {args.commit[:12]}")
    return 0


# Fields a moved plugin's manifest may change: its version (a new patch
# version, official/README.md) and where its source lives.
MOVED_FIELDS = ("version", "repository")


def compare_contract(where, ours, manifest, theirs):
    """A moved plugin keeps DeckPilot's contract while DeckPilot still has it."""
    if not (theirs / "manifest.json").is_file():
        return
    original = load(theirs / "manifest.json")
    if original is None:
        return
    for key in sorted(set(manifest) | set(original)):
        if key not in MOVED_FIELDS and manifest.get(key) != original.get(key):
            problem(f"{where}/manifest.json: `{key}` differs from DeckPilot's {theirs.name} at the pinned "
                    "commit; ids, actions, settings, permissions and views are its contract")
    for sub in ("locales", "assets"):
        a, b = ours / sub, theirs / sub
        if not b.is_dir() and not a.is_dir():
            continue
        def files(base):
            return sorted(str(p.relative_to(base)) for p in base.rglob("*") if p.is_file()) if base.is_dir() else []
        if files(a) != files(b):
            problem(f"{where}/{sub}: files differ from DeckPilot's: {sorted(set(files(a)) ^ set(files(b)))}")
            continue
        for rel in files(a):
            if not filecmp.cmp(a / rel, b / rel, shallow=False):
                problem(f"{where}/{sub}/{rel} differs from DeckPilot's at the pinned commit")


def compare_tree(where, ours, theirs):
    if not theirs.is_dir():
        problem(f"{where}: DeckPilot has no {theirs}")
        return
    def files(base):
        return sorted(str(p.relative_to(base)) for p in base.rglob("*") if p.is_file())
    a, b = files(ours), files(theirs)
    if a != b:
        problem(f"{where}: files differ from DeckPilot's: {sorted(set(a) ^ set(b))}")
        return
    for rel in a:
        if rel == "Cargo.toml":
            continue
        if not filecmp.cmp(ours / rel, theirs / rel, shallow=False):
            problem(f"{where}/{rel} differs from DeckPilot's at the pinned commit")


if __name__ == "__main__":
    sys.exit(main())
