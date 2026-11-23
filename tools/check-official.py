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
  knittingcode/deckpilot at `rev` = --commit, the SDK pin
  (DECKPILOT_SDK_COMMIT of tools/deckpilot.env), never a branch or tag; no path dependency leaves official/; Cargo.lock
  exists and names no other DeckPilot revision.
* Permission policy: the manifest's permission kinds equal the plugin's
  `permissions` in plugins.json (a new kind is a reviewed policy change).
  While the privileged-kinds gate is closed (DeckPilot
  docs/marketplace/publisher-review-request.md) no plugin may declare any
  plugin API 1.9 privileged kind (privileged_usages, exactly what the
  Rust validator reports) unless plugins.json marks it `held` (the reason,
  as text): its source may then be on main, built, tested and compared,
  but it is never packaged, signed, released or given a Marketplace entry
  (official.sh, official-entry.py). `held` is refused on a fixture,
  without a reason and on a plugin with no privileged usage; it stays
  valid with the gate open (a plugin is released one at a time, by a
  reviewed change that removes its `held`). Process execution (`tools`) is
  in any case only ever for deckpilot.git (git) and deckpilot.docker
  (docker).
* The reviewed privileged policy: official/privileged-policy.json, a byte
  copy of DeckPilot's crates/deckpilot-plugin-registry/publisher/
  privileged-policy.json (compared with --deckpilot), must be valid
  (version 1, minThreshold at least 2, only the five privileged plugins,
  each once, process execution only as PROCESS_ALLOW_LIST allows). With
  the gate open, a plugin with privileged usages that is not held must be
  EXACTLY its entry: an approved review, the same id and version, the same
  `permissions` (JSON equality), the same privileged usages with the same
  values (reviewed_reasons; DeckPilot's privileged_policy::check is the
  same rule). Anything else fails: a new construct, a broader permission,
  an extra executable, another plugin, another version.
* Compatibility: engines.pluginApi is satisfied by the pinned DeckPilot's
  plugin API (--plugin-api) and engines.deckpilot accepts its version
  (--app-version), so `release verify` will accept the host.
* With --deckpilot DIR (a checkout of the pinned commit): every fixture is
  byte-identical to DeckPilot's plugins/first-party/test-fixtures/<name>,
  Cargo.toml excepted; and every plugin keeps its contract, the files but
  the module of the release DeckPilot bundles,
  plugins/first-party/contracts/<id>/ (DeckPilot no longer has the
  sources): the same manifest but for `version` (never older than the
  contract's) and `repository`, the same locales and assets. A plugin
  without a contract there fails: the comparison is never skipped.

* The reviewed code of the privileged plugins (official/reviewed-sources.json,
  REVIEWED_SOURCES): for every plugin of official/privileged-policy.json,
  the sha256 of its Cargo.toml, manifest.json and every file under src/
  (recursively), and its version. Any other file there (a new module, a
  build.rs), a missing one or other bytes fail: changing the plugins'
  code means changing that file, which is reviewed like the policy.
  `check-official.py --root official --reviewed-sources` runs this check
  alone (stdlib only, no DeckPilot; validate.yml's required *Repository
  hygiene* job); `--print-reviewed-sources` prints the file for the tree
  as it is, for a reviewed update.

--list prints `<path> <id> <version> <package> <kind>` per member, kind
`plugin`, `held` (a plugin marked held) or `fixture`. Without --list, each
held plugin is printed with its reason.
--release ID fails unless ID is a released plugin (not held, not a fixture).
`check-official.py --gate` (alone) prints `open` or `closed`
(PRIVILEGED_GATE_OPEN).
"""

import argparse
import filecmp
import hashlib
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
# Plugin API 1.9's privileged permission kinds
# (PermissionKind::is_first_party_only). Every privileged usage, permissions
# and the rest, is privileged_usages() below.
PRIVILEGED_KINDS = frozenset({"tools", "resources", "projectContext", "hostViews"})
# ViewKind::is_host_view: the app's own screens.
HOST_VIEW_KINDS = frozenset({"workflows", "projects"})
# The privileged-kinds gate of DeckPilot's publisher signature. Opening it is
# a reviewed change here AND in DeckPilot (this one, pending the owner's
# approval after the independent security review). Open, it still admits
# nothing until official/privileged-policy.json records the review as
# approved (reviewed_reasons), and only once the pinned DeckPilot carries
# that same record (check --deckpilot).
PRIVILEGED_GATE_OPEN = True
# Process execution, ever: only these plugins, only these executables.
PROCESS_ALLOW_LIST = {"deckpilot.git": ["git"], "deckpilot.docker": ["docker"]}
# The reviewed privileged policy (DeckPilot privileged_policy.rs): its copy
# here, DeckPilot's at the pinned commit, and its rules.
PRIVILEGED_POLICY = "privileged-policy.json"
DECKPILOT_PRIVILEGED_POLICY = "crates/deckpilot-plugin-registry/publisher/privileged-policy.json"
PRIVILEGED_PLUGIN_IDS = ("deckpilot.docker", "deckpilot.git", "deckpilot.projects", "deckpilot.workflows",
                         "deckpilot.workspace")
MIN_THRESHOLD = 2
# The policy read by main() (None: not read; official-entry.py never admits
# a privileged plugin whatever it holds).
REVIEWED_POLICY = None

problems = []


def problem(message):
    problems.append(message)
    print(f"error: {message}", file=sys.stderr)


def no_duplicate_keys(pairs):
    """object_pairs_hook: a JSON object naming a key twice is refused (a
    later value would silently win)."""
    keys = [k for k, _ in pairs]
    duplicates = sorted({k for k in keys if keys.count(k) > 1})
    if duplicates:
        raise ValueError(f"duplicate key(s) {', '.join(duplicates)}")
    return dict(pairs)


def load(path):
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=no_duplicate_keys)
    except (OSError, ValueError) as e:
        problem(f"{path}: {e}")
        return None


def strict_equal(a, b):
    """JSON equality without Python's coercions: true is not 1, 1 is not
    1.0 (DeckPilot compares serde_json values, which keep the types)."""
    if type(a) is not type(b):
        return False
    if isinstance(a, dict):
        return a.keys() == b.keys() and all(strict_equal(a[k], b[k]) for k in a)
    if isinstance(a, list):
        return len(a) == len(b) and all(strict_equal(x, y) for x, y in zip(a, b))
    return a == b


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


def privileged_usages(manifest):
    """Every use of a plugin API 1.9 privileged kind, as (JSON path, what).

    Mirrors `usages` in DeckPilot's crates/deckpilot-plugin-manifest/src/
    privileged.rs at the pinned commit exactly (each usage there reports
    IssueCode::FirstPartyOnly): privileged permission kinds,
    `openUrl.fromParams`, `node.changesContext`, `resource` action
    parameters, variable host sources, host views, `statusItems` and
    `dashboardCards` (present at all, even empty). The package step's
    `deckpilot-registry package --first-party` runs the Rust validator on
    the same manifest; tools/tests/test_check_official.py holds this mirror
    to DeckPilot's own manifests.
    """
    found = []
    m = manifest if isinstance(manifest, dict) else {}
    for i, p in enumerate(m.get("permissions") or []):
        if not isinstance(p, dict):
            continue
        if p.get("kind") in PRIVILEGED_KINDS:
            found.append((f"permissions[{i}]", f"the `{p['kind']}` permission"))
        elif p.get("kind") == "openUrl" and p.get("fromParams") is True:
            found.append((f"permissions[{i}].fromParams", "`openUrl.fromParams`"))
    contributes = m.get("contributes") if isinstance(m.get("contributes"), dict) else {}
    for a, action in enumerate(contributes.get("actions") or []):
        if not isinstance(action, dict):
            continue
        node = action.get("node") if isinstance(action.get("node"), dict) else {}
        if node.get("changesContext") is True:
            found.append((f"contributes.actions[{a}].node.changesContext", "`node.changesContext`"))
        for q, parameter in enumerate(action.get("parameters") or []):
            kind = parameter.get("kind") if isinstance(parameter, dict) else None
            # ParameterKind is tagged by `type`: {"type": "resource", "kinds": [...]}.
            if isinstance(kind, dict) and kind.get("type") == "resource":
                found.append((f"contributes.actions[{a}].parameters[{q}].kind", "a `resource` parameter"))
    for i, variable in enumerate(contributes.get("variables") or []):
        if isinstance(variable, dict) and variable.get("source") is not None:
            found.append((f"contributes.variables[{i}].source", f"the host source `{variable['source']}`"))
    for i, view in enumerate(contributes.get("views") or []):
        if isinstance(view, dict) and view.get("kind") in HOST_VIEW_KINDS:
            found.append((f"contributes.views[{i}].kind", f"the `{view['kind']}` view"))
    if contributes.get("statusItems") is not None:
        found.append(("contributes.statusItems", "`statusItems`"))
    if contributes.get("dashboardCards") is not None:
        found.append(("contributes.dashboardCards", "`dashboardCards`"))
    return found


def permission_index(path):
    m = re.fullmatch(r"permissions\[(\d+)\](\..*)?", path)
    return int(m.group(1)) if m else None


def value_at(doc, path):
    """The JSON at a usage path (`a.b[0].c`), or KeyError."""
    for segment in path.split("."):
        m = re.fullmatch(r"([A-Za-z]+)((?:\[\d+\])*)", segment)
        if not m or not isinstance(doc, dict) or m.group(1) not in doc:
            raise KeyError(path)
        doc = doc[m.group(1)]
        for i in re.findall(r"\[(\d+)\]", m.group(2)):
            if not isinstance(doc, list) or int(i) >= len(doc):
                raise KeyError(path)
            doc = doc[int(i)]
    return doc


def policy_problems(policy):
    """The reviewed privileged policy's rules (PrivilegedPolicy::validate)."""
    out = []
    if not isinstance(policy, dict):
        return ["not a JSON object"]
    allowed = {"$comment", "privilegedPolicyVersion", "review", "minThreshold", "plugins"}
    if set(policy) - allowed or not {"privilegedPolicyVersion", "review", "minThreshold", "plugins"} <= set(policy):
        out.append(f"fields {sorted(policy)}; exactly {sorted(allowed - {'$comment'})} (and $comment)")
    if policy.get("privilegedPolicyVersion") != 1:
        out.append("privilegedPolicyVersion must be 1")
    threshold = policy.get("minThreshold")
    if not isinstance(threshold, int) or isinstance(threshold, bool) or threshold < MIN_THRESHOLD:
        out.append(f"minThreshold must be at least {MIN_THRESHOLD}")
    review = policy.get("review")
    if review is not None:
        if not isinstance(review, dict) or set(review) != {"reviewer", "date", "outcome", "record"}:
            out.append("review: exactly reviewer, date, outcome and record")
        else:
            if not str(review["reviewer"]).strip() or not str(review["record"]).strip():
                out.append("review: reviewer and record must be given")
            if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", str(review["date"])):
                out.append("review: date is not YYYY-MM-DD")
            if review["outcome"] not in ("pending", "approved"):
                out.append("review: outcome is pending or approved")
    seen = set()
    for plugin in policy.get("plugins") or []:
        pid = plugin.get("id") if isinstance(plugin, dict) else None
        if pid not in PRIVILEGED_PLUGIN_IDS:
            out.append(f"{pid} is not one of DeckPilot's privileged plugins")
            continue
        if set(plugin) != {"id", "version", "permissions", "constructs"}:
            out.append(f"{pid}: exactly id, version, permissions and constructs")
            continue
        if pid in seen:
            out.append(f"{pid} is listed twice")
        seen.add(pid)
        if not version_key(str(plugin["version"])):
            out.append(f"{pid}: version {plugin['version']} is not MAJOR.MINOR.PATCH")
        permissions = plugin["permissions"] if isinstance(plugin["permissions"], list) else []
        constructs = plugin["constructs"] if isinstance(plugin["constructs"], list) else []
        if not constructs:
            out.append(f"{pid}: no privileged construct is listed")
        paths = set()
        for c in constructs:
            if not isinstance(c, dict) or not {"path", "what"} <= set(c) or set(c) - {"path", "what", "value"}:
                out.append(f"{pid}: a construct is path, what and value")
                continue
            if c["path"] in paths:
                out.append(f"{pid}: {c['path']} is listed twice")
            paths.add(c["path"])
            index = permission_index(c["path"])
            if index is not None and ("value" in c or index >= len(permissions)):
                out.append(f"{pid}: {c['path']}: a permission's construct names a listed permission, no value")
            if index is None and "value" not in c:
                out.append(f"{pid}: {c['path']}: value is missing")
        for p in permissions:
            if isinstance(p, dict) and p.get("kind") == "tools":
                names = [t.get("executable") for t in p.get("tools", []) if isinstance(t, dict)]
                if names != PROCESS_ALLOW_LIST.get(pid):
                    out.append(f"{pid} may not run {names}: process execution is only for "
                               f"{', '.join(f'{k} ({v[0]})' for k, v in PROCESS_ALLOW_LIST.items())}")
    return out


def reviewed_reasons(policy, manifest):
    """Why the open gate refuses this privileged manifest (empty: it is
    exactly the policy's entry for its id and version)."""
    if policy is None:
        return ["no reviewed privileged policy was read"]
    found = policy_problems(policy)
    if found:
        return [f"the policy is invalid: {'; '.join(found)}"]
    if (policy.get("review") or {}).get("outcome") != "approved":
        return ["the policy records no approved security review"]
    pid, version = manifest.get("id"), manifest.get("version")
    entry = next((p for p in policy["plugins"] if p["id"] == pid), None)
    if entry is None:
        return [f"the review does not cover {pid}"]
    if entry["version"] != version:
        return [f"the review covers {pid} {entry['version']} only, not {version}"]
    reasons = []
    if not strict_equal(manifest.get("permissions") or [], entry["permissions"]):
        reasons.append("the permissions differ from the reviewed ones")
    reviewed = {c["path"]: c for c in entry["constructs"]}
    usages = privileged_usages(manifest)
    for path, what in usages:
        c = reviewed.get(path)
        if c is None:
            reasons.append(f"{path} ({what}) is not reviewed")
        elif c["what"] != what:
            reasons.append(f"{path} is {what}, the review has {c['what']}")
        elif "value" in c:
            try:
                if not strict_equal(value_at(manifest, path), c["value"]):
                    reasons.append(f"{path} ({what}) differs from the reviewed value")
            except KeyError:
                reasons.append(f"{path} ({what}) differs from the reviewed value")
    for path in sorted(set(reviewed) - {p for p, _ in usages}):
        reasons.append(f"{path} is reviewed but not declared")
    return reasons


def check_permissions(where, entry, manifest, held=False):
    """The permission policy. `held` (only check-official.py's own source
    check passes it, for a plugin plugins.json marks held) allows the
    privileged usages in source; every other caller (the Marketplace entry
    rules) refuses them while the gate is closed."""
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
    if not PRIVILEGED_GATE_OPEN and not held:
        for path, what in privileged_usages(manifest):
            problem(f"{where}: {path}: {what} is a plugin API 1.9 privileged kind; a signed release "
                    "artifact cannot carry it while the gate is closed (DeckPilot "
                    "docs/marketplace/publisher-review-request.md); its source may be on main only "
                    "as a `held` plugin (plugins.json)")
    elif not held and privileged_usages(manifest):
        # The gate is open: exactly the reviewed policy, nothing more.
        usages = ", ".join(f"{path}: {what}" for path, what in privileged_usages(manifest))
        for reason in reviewed_reasons(REVIEWED_POLICY, manifest):
            problem(f"{where}: {usages}: the reviewed privileged policy (official/{PRIVILEGED_POLICY}) "
                    f"does not admit it: {reason}")


REVIEWED_SOURCES = "reviewed-sources.json"
# The privileged plugins whose code is frozen, fixed here and not taken from
# the files the check reads: a pull request that drops a plugin from both
# privileged-policy.json and reviewed-sources.json still fails. Changing
# this list is a change of this checker, reviewed as tooling.
REVIEWED_PRIVILEGED_IDS = ("deckpilot.docker", "deckpilot.git", "deckpilot.projects",
                           "deckpilot.workflows", "deckpilot.workspace")


def reviewed_files(directory):
    """The files of a privileged plugin its review covers: Cargo.toml,
    manifest.json, everything under src/, and a build.rs if there is one
    (never reviewed, so always a difference)."""
    names = [n for n in ("Cargo.toml", "manifest.json", "build.rs") if (directory / n).is_file()]
    src = directory / "src"
    if src.is_dir():
        names += [str(p.relative_to(directory)).replace("\\", "/") for p in src.rglob("*") if p.is_file()]
    return {n: hashlib.sha256((directory / n).read_bytes()).hexdigest() for n in sorted(names)}


def privileged_paths(root):
    """id -> (path, manifest version) of every plugin the reviewed
    privileged policy covers."""
    policy = load(root / PRIVILEGED_POLICY) or {}
    members = {e.get("id"): e.get("path") for e in (load(root / "plugins.json") or {}).get("plugins", [])}
    out = {}
    for entry in policy.get("plugins", []):
        pid = entry.get("id")
        path = members.get(pid)
        if path is None:
            problem(f"official/{PRIVILEGED_POLICY} covers {pid}, which is not a plugin of official/plugins.json")
            continue
        manifest = load(root / path / "manifest.json") or {}
        out[pid] = (path, manifest.get("version"))
    return out


def reviewed_sources_document(root):
    plugins = [{"id": pid, "path": path, "version": version, "files": reviewed_files(root / path)}
               for pid, (path, version) in sorted(privileged_paths(root).items())]
    return {"reviewedSourcesVersion": 1, "plugins": plugins}


def check_reviewed_sources(root):
    """official/reviewed-sources.json is exactly the privileged plugins'
    reviewed code as it is in the tree, for at least REVIEWED_PRIVILEGED_IDS."""
    covered = {e.get("id") for e in (load(root / PRIVILEGED_POLICY) or {}).get("plugins", []) if isinstance(e, dict)}
    for pid in REVIEWED_PRIVILEGED_IDS:
        if pid not in covered:
            problem(f"official/{PRIVILEGED_POLICY} does not cover {pid}, one of the privileged plugins "
                    "tools/check-official.py freezes (REVIEWED_PRIVILEGED_IDS)")
    frozen = load(root / REVIEWED_SOURCES)
    if frozen is None:
        return
    listed = {e.get("id") for e in frozen.get("plugins", []) if isinstance(e, dict)}
    for pid in REVIEWED_PRIVILEGED_IDS:
        if pid not in listed:
            problem(f"official/{REVIEWED_SOURCES} does not cover {pid}, one of the privileged plugins "
                    "tools/check-official.py freezes (REVIEWED_PRIVILEGED_IDS)")
    if frozen.get("reviewedSourcesVersion") != 1:
        problem(f"official/{REVIEWED_SOURCES}: reviewedSourcesVersion must be 1")
    want = {e["id"]: e for e in reviewed_sources_document(root)["plugins"]}
    have = {}
    for e in frozen.get("plugins", []):
        if not isinstance(e, dict) or not isinstance(e.get("id"), str):
            problem(f"official/{REVIEWED_SOURCES}: an entry without an id")
            continue
        if e["id"] in have:
            problem(f"official/{REVIEWED_SOURCES}: {e['id']} is listed twice")
        have[e["id"]] = e
    for pid in sorted(set(want) - set(have)):
        problem(f"official/{REVIEWED_SOURCES} does not cover {pid}, a plugin of the reviewed privileged policy")
    for pid in sorted(set(have) - set(want)):
        problem(f"official/{REVIEWED_SOURCES} covers {pid}, which the reviewed privileged policy does not")
    for pid in sorted(set(want) & set(have)):
        w, h = want[pid], have[pid]
        where = f"official/{w['path']}"
        for key in ("path", "version"):
            if h.get(key) != w[key]:
                problem(f"{where}: {key} {w[key]!r}, but official/{REVIEWED_SOURCES} reviewed {h.get(key)!r}")
        files = h.get("files") if isinstance(h.get("files"), dict) else {}
        for name in sorted(set(w["files"]) - set(files)):
            problem(f"{where}/{name} is not reviewed code (official/{REVIEWED_SOURCES}): a new file of a "
                    "privileged plugin is a reviewed change")
        for name in sorted(set(files) - set(w["files"])):
            problem(f"{where}/{name} is reviewed (official/{REVIEWED_SOURCES}) but missing")
        for name in sorted(set(files) & set(w["files"])):
            if files[name] != w["files"][name]:
                problem(f"{where}/{name} differs from its reviewed bytes (official/{REVIEWED_SOURCES}: "
                        f"sha256 {files[name]}, now {w['files'][name]})")


def main():
    if len(sys.argv) == 4 and sys.argv[1] == "--root" and sys.argv[3] in ("--reviewed-sources", "--print-reviewed-sources"):
        root = Path(sys.argv[2])
        if sys.argv[3] == "--print-reviewed-sources":
            print(json.dumps(reviewed_sources_document(root), indent=2))
            return 1 if problems else 0
        check_reviewed_sources(root)
        if problems:
            return 1
        print(f"official: the reviewed code of {len(privileged_paths(root))} privileged plugin(s) is unchanged "
              f"(official/{REVIEWED_SOURCES}): {', '.join(sorted(privileged_paths(root)))}")
        return 0
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", required=True, type=Path)
    ap.add_argument("--commit", required=True)
    ap.add_argument("--app-version", required=True)
    ap.add_argument("--plugin-api", required=True)
    ap.add_argument("--deckpilot", type=Path)
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--release")
    if sys.argv[1:] == ["--gate"]:
        print("open" if PRIVILEGED_GATE_OPEN else "closed")
        return 0
    args = ap.parse_args()
    root = args.root
    if not re.fullmatch(r"[0-9a-f]{40}", args.commit):
        problem(f"--commit {args.commit} is not a full commit")
        return 1

    global REVIEWED_POLICY
    REVIEWED_POLICY = load(root / PRIVILEGED_POLICY)
    for found in policy_problems(REVIEWED_POLICY) if REVIEWED_POLICY is not None else []:
        problem(f"official/{PRIVILEGED_POLICY}: {found}")
    if args.deckpilot:
        theirs = args.deckpilot / DECKPILOT_PRIVILEGED_POLICY
        if theirs.is_file():
            if not filecmp.cmp(root / PRIVILEGED_POLICY, theirs, shallow=False):
                problem(f"official/{PRIVILEGED_POLICY} is not byte-identical to DeckPilot's "
                        f"{DECKPILOT_PRIVILEGED_POLICY} at the pinned commit")
        elif PRIVILEGED_GATE_OPEN:
            problem(f"the pinned DeckPilot has no {DECKPILOT_PRIVILEGED_POLICY}: the gate cannot be open")
    check_reviewed_sources(root)
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
    held_reasons = []
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
        if held is not None:
            if kind != "plugin" or not isinstance(held, str) or not held.strip():
                problem(f"{where}: `held` is a reason (text), for a plugin only")
            elif not privileged_usages(manifest):
                problem(f"{where}: held, but nothing holds it (no privileged usage); remove `held`")
            else:
                kind = "held"
                held_reasons.append((mid, manifest.get("version", ""), held.strip()))
        check_permissions(where, entry, manifest, held=kind == "held")
        if kind == "fixture" and args.deckpilot:
            theirs = args.deckpilot / "plugins/first-party/test-fixtures" / Path(path).name
            compare_tree(where, directory, theirs)
        if kind != "fixture" and args.deckpilot:
            compare_contract(where, directory, manifest, args.deckpilot / CONTRACTS / mid)
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
        for mid, version, reason in held_reasons:
            print(f"held: {mid} {version}: {reason}")
    return 0


# Fields a moved plugin's manifest may change: its version (the bundled
# release's or a newer one, official/README.md) and where its source lives.
MOVED_FIELDS = ("version", "repository")
# Where the pinned DeckPilot keeps the contract of each bundled release:
# its files but the module (DeckPilot #392).
CONTRACTS = Path("plugins/first-party/contracts")


def compare_contract(where, ours, manifest, theirs):
    """A moved plugin keeps the contract of the release DeckPilot bundles."""
    if not (theirs / "manifest.json").is_file():
        problem(f"{where}: the pinned DeckPilot has no contract {CONTRACTS}/{theirs.name}/manifest.json; "
                "every official plugin is compared with the release DeckPilot bundles")
        return
    original = load(theirs / "manifest.json")
    if original is None:
        return
    ours_v, theirs_v = version_key(manifest.get("version", "")), version_key(original.get("version", ""))
    if ours_v is None or theirs_v is None or ours_v < theirs_v:
        problem(f"{where}/manifest.json: version {manifest.get('version')} is older than DeckPilot's contract "
                f"{original.get('version')} (the bundled release); a source here is that release or a newer one")
    for key in sorted(set(manifest) | set(original)):
        if key not in MOVED_FIELDS and manifest.get(key) != original.get(key):
            problem(f"{where}/manifest.json: `{key}` differs from DeckPilot's {theirs.name} at the pinned "
                    "commit; ids, actions, settings, permissions and views are its contract")

    def files(base):
        return sorted(str(p.relative_to(base)) for p in base.rglob("*") if p.is_file()) if base.is_dir() else []

    for sub in ("locales", "assets"):
        a, b = ours / sub, theirs / sub
        if files(a) != files(b):
            problem(f"{where}/{sub}: files differ from DeckPilot's: {sorted(set(files(a)) ^ set(files(b)))}")
            continue
        for rel in files(a):
            if not filecmp.cmp(a / rel, b / rel, shallow=False):
                problem(f"{where}/{sub}/{rel} differs from DeckPilot's at the pinned commit")
    if not (ours / "LICENSE").is_file() or not (theirs / "LICENSE").is_file() or \
            not filecmp.cmp(ours / "LICENSE", theirs / "LICENSE", shallow=False):
        problem(f"{where}/LICENSE differs from DeckPilot's contract at the pinned commit")


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
