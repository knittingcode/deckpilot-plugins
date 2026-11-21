#!/usr/bin/env python3
"""Layout rules of first-party/ (stdlib only; run with python3 -I).

    check-first-party.py --root first-party --commit DECKPILOT_COMMIT [--list]

first-party/ is one Cargo workspace; every directory in it that has a
manifest.json is one plugin. For each, this checks what neither cargo nor
the manifest schema does:

* the id is not one the application bundles (BUNDLED_PLUGIN_IDS);
* the directory is the plugin's name: id `deckpilot.<dir>`, publisher
  `deckpilot`, Cargo package `deckpilot-<dir>`, a workspace member, the same
  version in Cargo.toml and manifest.json, `repository` pointing here;
* LICENSE, README.md, README.pl.md, the icon and locales/pl.json exist;
* locales/pl.json has a Polish text for exactly the texts of manifest.json
  (name, description, permission justifications, actions with their
  parameters and outputs, variables, settings, credential slots, error
  codes, navigation labels), each within the manifest's own length limit. English is the
  manifest itself. Plugin API 1.0 has no localized metadata, so the file is
  a source convention and is not packaged (docs/plugin-factory.md).

Also: the SDK `rev` of first-party/Cargo.toml must be the pinned DeckPilot
commit. --list prints `<dir> <id> <version> <package>` per plugin.
"""

import argparse
import json
import re
import sys
from pathlib import Path

REPOSITORY = "https://github.com/knittingcode/deckpilot-plugins"
PUBLISHER = "deckpilot"
# DeckPilot's BUNDLED_PLUGIN_IDS (crates/deckpilot-plugin-bundled/src/lib.rs
# at the pinned commit; tools/check-schemas.sh compares): the agent never
# takes these from a registry, so they have no source here.
BUNDLED_PLUGIN_IDS = frozenset({
    "deckpilot.docker", "deckpilot.git", "deckpilot.github", "deckpilot.gitlab", "deckpilot.jira",
    "deckpilot.notifications", "deckpilot.projects", "deckpilot.workflows", "deckpilot.workspace",
})
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
    """The first `key = "value"` of a small, hand-written Cargo.toml."""
    m = re.search(rf'^{re.escape(key)}\s*=\s*"([^"]*)"', text, re.M)
    return m.group(1) if m else None


def by(items, key):
    return {item[key]: item for item in items or [] if isinstance(item, dict) and key in item}


def expected_texts(manifest):
    """The shape locales/<lang>.json must have: {path: (min, max) length}."""
    texts = {("name",): (1, 48), ("description",): (1, 300)}
    for permission in manifest.get("permissions", []):
        texts[("permissions", permission["kind"])] = (10, 200)
    contributes = manifest.get("contributes", {})
    for aid, action in by(contributes.get("actions"), "id").items():
        base = ("actions", aid)
        texts[base + ("name",)] = (1, 48)
        texts[base + ("description",)] = (1, 300)
        texts[base + ("category",)] = (1, 32)
        for key, parameter in by(action.get("parameters"), "key").items():
            texts[base + ("parameters", key, "label")] = (1, 48)
            if "description" in parameter:
                texts[base + ("parameters", key, "description")] = (1, 300)
        for key in by(action.get("node", {}).get("outputs"), "key"):
            texts[base + ("outputs", key)] = (1, 48)
    for vid, variable in by(contributes.get("variables"), "id").items():
        texts[("variables", vid, "name")] = (1, 48)
        if "description" in variable:
            texts[("variables", vid, "description")] = (1, 300)
    for key, field in by(contributes.get("settings", {}).get("fields"), "key").items():
        texts[("settings", key, "label")] = (1, 48)
        if "description" in field:
            texts[("settings", key, "description")] = (1, 300)
    for key, slot in by(contributes.get("credentials"), "key").items():
        texts[("credentials", key, "label")] = (1, 48)
        if "description" in slot:
            texts[("credentials", key, "description")] = (1, 300)
    for code in contributes.get("errorCodes", {}):
        texts[("errorCodes", code)] = (1, 200)
    for nid in by(contributes.get("navigation"), "id"):
        texts[("navigation", nid, "label")] = (1, 24)
    return texts


def flatten(value, prefix=()):
    if isinstance(value, dict):
        out = {}
        for key, child in value.items():
            out.update(flatten(child, prefix + (key,)))
        return out
    return {prefix: value}


def check_locale(path, manifest):
    doc = load(path)
    if not isinstance(doc, dict):
        return
    expected = expected_texts(manifest)
    found = flatten(doc)
    for where in sorted(set(expected) - set(found)):
        problem(f"{path}: {'.'.join(where)} is missing")
    for where in sorted(set(found) - set(expected)):
        problem(f"{path}: {'.'.join(where)} is not a text of manifest.json")
    for where, text in found.items():
        if where not in expected:
            continue
        low, high = expected[where]
        if not isinstance(text, str) or not low <= len(text.strip()) or len(text) > high:
            problem(f"{path}: {'.'.join(where)} must be a text of {low}-{high} characters")


def check_plugin(root, pdir, members):
    name = pdir.name
    manifest = load(pdir / "manifest.json")
    if not isinstance(manifest, dict):
        return None
    pid = manifest.get("id")
    if pid != f"{PUBLISHER}.{name}":
        problem(f"{pdir}/manifest.json: id {pid!r} is not {PUBLISHER}.{name}")
    if pid in BUNDLED_PLUGIN_IDS:
        problem(f"{pdir}: {pid} is bundled with the application; DeckPilot never installs it from a registry")
    if manifest.get("publisher", {}).get("id") != PUBLISHER:
        problem(f"{pdir}/manifest.json: publisher.id is not {PUBLISHER!r}")
    repository = manifest.get("repository", {})
    if repository.get("url") != REPOSITORY or repository.get("directory") != f"{root.name}/{name}":
        problem(f"{pdir}/manifest.json: repository must be {REPOSITORY}, directory {root.name}/{name}")
    cargo_path = pdir / "Cargo.toml"
    cargo = cargo_path.read_text(encoding="utf-8") if cargo_path.is_file() else ""
    package = toml_value(cargo, "name")
    if package != f"{PUBLISHER}-{name}":
        problem(f"{cargo_path}: package name {package!r} is not {PUBLISHER}-{name}")
    if toml_value(cargo, "version") != manifest.get("version"):
        problem(f"{cargo_path}: version differs from manifest.json ({manifest.get('version')})")
    if name not in members:
        problem(f"{root}/Cargo.toml: {name} is not a workspace member")
    required = ["LICENSE", "README.md", "README.pl.md", "locales/pl.json", "src/lib.rs"]
    required += list(manifest.get("icons", {}).values())
    for rel in required:
        if not (pdir / rel).is_file():
            problem(f"{pdir}/{rel} is missing")
    if (pdir / "locales/pl.json").is_file():
        check_locale(pdir / "locales/pl.json", manifest)
    return name, pid, manifest.get("version"), package


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--list", action="store_true")
    args = parser.parse_args()
    root = Path(args.root)
    workspace = (root / "Cargo.toml").read_text(encoding="utf-8")
    m = re.search(r"^members\s*=\s*\[([^\]]*)\]", workspace, re.M)
    members = set(re.findall(r'"([^"]+)"', m.group(1))) if m else set()
    rev = re.search(r'^deckpilot-plugin-sdk\s*=.*\brev\s*=\s*"([0-9a-f]+)"', workspace, re.M)
    if not rev or rev.group(1) != args.commit:
        problem(
            f"{root}/Cargo.toml: deckpilot-plugin-sdk must be pinned to rev {args.commit} "
            f"(tools/deckpilot.env), found {rev.group(1) if rev else 'no rev'}"
        )
    if not (root / "Cargo.lock").is_file():
        problem(f"{root}/Cargo.lock is missing (commit it; builds use --locked)")
    plugins = []
    for pdir in sorted(p for p in root.iterdir() if p.is_dir()):
        if not (pdir / "manifest.json").is_file():
            if pdir.name in members:
                problem(f"{pdir}: a workspace member without manifest.json")
            continue
        if not re.fullmatch(r"[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*", pdir.name):
            problem(f"{pdir}: the directory name is not a plugin name")
            continue
        entry = check_plugin(root, pdir, members)
        if entry:
            plugins.append(entry)
    if problems:
        print(f"{len(problems)} problem(s) in {root}/", file=sys.stderr)
        return 1
    if args.list:
        for entry in plugins:
            print(*entry)
    else:
        print(f"first-party: {len(plugins)} plugin(s) checked")
    return 0


if __name__ == "__main__":
    sys.exit(main())
