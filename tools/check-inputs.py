#!/usr/bin/env python3
"""Checks the registry inputs of one registry root (stdlib only).

    check-inputs.py --root DIR --recipes DIR --schema-check BIN --schemas DIR
                    [--list-versions] [--manifest-of PACKAGE]

* Every JSON file under DIR/registry and every build recipe is validated
  against its schema with the pinned schema-check tool.
* The storefront listing (listing.json and media/ beside plugin.json) is
  allowed here and checked by tools/check-storefront.py.
* Layout and cross-field rules the schemas cannot express: file names
  match their content, every plugin's namespace has a publisher, every
  plugin has a build recipe and vice versa, namespaces are unique, the
  version history is a plain list of <semver>.json files.
* An official plugin (one DeckPilot bundles, released from official/,
  docs/official-marketplace.md) has no build recipe: it is indexed from its
  verified release, so each recorded version needs its committed candidate
  entry DIR/official/marketplace/<id>/versions/<v>.json instead, and a
  recipe for it is refused (tools/official-entry.py gate checks the rest).
* --list-versions prints "<id> <version> <path>" per recorded version,
  in ascending semver order per plugin (for tools/validate.sh).
* --manifest-of PACKAGE prints manifest.json of a .dpplugin (tar.gz).

Run with `python3 -I`. Exit status 1 on any finding.
"""

import argparse
import json
import re
import subprocess
import sys
import tarfile
from pathlib import Path

SCHEMA_BASE = "https://knittingcode.github.io/deckpilot-plugins/v1/schemas/"
INPUT_BASE = "https://github.com/knittingcode/deckpilot-plugins/schemas/inputs/"
SCHEMAS = {
    "publisher": SCHEMA_BASE + "registry-index.schema.json#/properties/publishers/items",
    "revocations": SCHEMA_BASE + "registry-index.schema.json#/properties/revocations",
    "version": SCHEMA_BASE + "registry-plugin.schema.json#/$defs/version",
    "plugin": INPUT_BASE + "plugin-info.schema.json",
    "recipe": INPUT_BASE + "build-recipe.schema.json",
    "manifest": SCHEMA_BASE + "plugin-manifest.schema.json",
}
SEMVER = re.compile(
    r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)"
    r"(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?"
    r"(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$"
)

findings = []


def finding(message):
    findings.append(message)
    print(f"error: {message}", file=sys.stderr)


def semver_key(text):
    """Sort key following semver precedence (build metadata ignored)."""
    m = SEMVER.match(text)
    if not m:
        return ((0, 0, 0), 0, ())
    core = tuple(int(x) for x in m.group(1, 2, 3))
    pre = m.group(4)
    if pre is None:
        return (core, 1, ())
    parts = tuple((0, int(p), "") if p.isdigit() else (1, 0, p) for p in pre.split("."))
    return (core, 0, parts)


def load(path):
    try:
        with open(path, "rb") as f:
            return json.loads(f.read().decode("utf-8"))
    except (OSError, ValueError) as e:
        finding(f"{path}: {e}")
        return None


def schema_check(args, kind, files):
    if not files:
        return
    result = subprocess.run(
        [args.schema_check, "--schemas", args.schemas, SCHEMAS[kind], *map(str, files)],
        check=False,
        stdout=sys.stderr,
    )
    if result.returncode != 0:
        finding(f"{kind} files do not match {SCHEMAS[kind]} (see above)")


def manifest_of(package):
    with tarfile.open(package, "r:gz") as tar:
        member = tar.getmember("manifest.json")
        if not member.isfile():
            raise ValueError("manifest.json is not a file")
        return tar.extractfile(member).read()


def check(args):
    root = Path(args.root)
    registry = root / "registry"
    recipes = Path(args.recipes)
    versions = []

    allowed_top = {"publishers", "plugins", "revocations.json", "README.md"}
    if registry.is_dir():
        for entry in sorted(registry.iterdir()):
            if entry.name not in allowed_top:
                finding(f"{entry}: unexpected in registry/")

    publishers = {}
    files = sorted((registry / "publishers").glob("*")) if (registry / "publishers").is_dir() else []
    for path in files:
        if path.suffix != ".json" or not path.is_file():
            if path.name != ".gitkeep":
                finding(f"{path}: registry/publishers/ holds only <login>.json files")
            continue
        doc = load(path)
        if not isinstance(doc, dict):
            continue
        if doc.get("login") != path.stem:
            finding(f"{path}: the file name must be the login ({doc.get('login')!r})")
        ns = doc.get("namespace")
        if ns in publishers:
            finding(f"{path}: namespace {ns!r} already belongs to {publishers[ns]}")
        publishers[ns] = doc.get("login")
    schema_check(args, "publisher", [p for p in files if p.suffix == ".json"])

    revocations = registry / "revocations.json"
    if revocations.exists():
        if isinstance(load(revocations), list):
            schema_check(args, "revocations", [revocations])
        else:
            finding(f"{revocations}: must be a JSON array")

    plugin_files, version_files = [], []
    plugin_ids = set()
    plugins_dir = registry / "plugins"
    for pdir in sorted(plugins_dir.iterdir()) if plugins_dir.is_dir() else []:
        if not pdir.is_dir():
            finding(f"{pdir}: registry/plugins/ holds only <id>/ directories")
            continue
        pid = pdir.name
        plugin_ids.add(pid)
        for entry in sorted(pdir.iterdir()):
            # listing.json and media/ are the storefront listing
            # (docs/storefront.md), checked by tools/check-storefront.py.
            if entry.name not in {"plugin.json", "versions", "listing.json", "media"}:
                finding(f"{entry}: a plugin directory holds plugin.json, versions/, listing.json and media/ only")
        info_path = pdir / "plugin.json"
        info = load(info_path) if info_path.is_file() else finding(f"{pdir}: plugin.json is missing")
        if isinstance(info, dict):
            plugin_files.append(info_path)
            if info.get("id") != pid:
                finding(f"{info_path}: id {info.get('id')!r} is not the directory name {pid}")
        namespace = pid.split(".", 1)[0]
        if namespace not in publishers:
            finding(f"{pdir}: no registry/publishers/*.json owns the namespace {namespace!r}")
        vdir = pdir / "versions"
        recorded = []
        for vpath in sorted(vdir.iterdir()) if vdir.is_dir() else []:
            if vpath.suffix != ".json" or not SEMVER.match(vpath.stem):
                finding(f"{vpath}: versions/ holds only <semver>.json files")
                continue
            doc = load(vpath)
            if not isinstance(doc, dict):
                continue
            version_files.append(vpath)
            if doc.get("version") != vpath.stem:
                finding(f"{vpath}: holds version {doc.get('version')!r}")
            recorded.append((semver_key(vpath.stem), pid, vpath.stem, vpath))
        recorded.sort()
        versions.extend(recorded)

        candidates = root / "official" / "marketplace" / pid
        if candidates.is_dir():
            if (recipes / pid).exists():
                finding(f"{pid}: an official plugin is indexed from its verified release; "
                        f"{recipes / pid} must not exist")
            for _, _, version, _ in recorded:
                if not (candidates / "versions" / f"{version}.json").is_file():
                    finding(f"{pid} {version}: no candidate entry {candidates}/versions/{version}.json")
            continue
        recipe = recipes / pid / "build.json"
        if not recipe.is_file():
            finding(f"{pid}: {recipe} (the build recipe) is missing")
        if recorded and not (recipes / pid / "manifest.json").is_file():
            finding(f"{pid}: {recipes / pid / 'manifest.json'} (copy of the newest version's manifest) is missing")
    schema_check(args, "plugin", plugin_files)
    schema_check(args, "version", version_files)

    recipe_files, manifests = [], []
    for rdir in sorted(recipes.iterdir()) if recipes.is_dir() else []:
        if not rdir.is_dir() or rdir.name in {"examples"} or rdir.name.startswith("."):
            continue
        if rdir.name not in plugin_ids:
            finding(f"{rdir}: no registry/plugins/{rdir.name}/ for this submission")
        recipe = rdir / "build.json"
        doc = load(recipe) if recipe.is_file() else finding(f"{rdir}: build.json is missing")
        if isinstance(doc, dict):
            recipe_files.append(recipe)
            if doc.get("id") != rdir.name:
                finding(f"{recipe}: id {doc.get('id')!r} is not the directory name {rdir.name}")
        if (rdir / "manifest.json").is_file():
            manifests.append(rdir / "manifest.json")
    schema_check(args, "recipe", recipe_files)
    schema_check(args, "manifest", manifests)
    return versions


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root")
    parser.add_argument("--recipes")
    parser.add_argument("--schema-check")
    parser.add_argument("--schemas")
    parser.add_argument("--list-versions", action="store_true")
    parser.add_argument("--manifest-of")
    args = parser.parse_args()
    if args.manifest_of:
        sys.stdout.buffer.write(manifest_of(args.manifest_of))
        return 0
    versions = check(args)
    if args.list_versions:
        for _, pid, version, path in versions:
            print(pid, version, path)
    if findings:
        print(f"{len(findings)} finding(s)", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
