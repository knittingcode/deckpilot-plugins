#!/usr/bin/env python3
"""Marketplace entries of the official plugins, made only from verified releases
(stdlib only; run with python3 -I). See docs/official-marketplace.md.

    official-entry.py fetch --tag TAG --out DIR
    official-entry.py generate --bundle DIR [--dry-run]
    official-entry.py generate --bundle DIR --test-keys KEYS.json --official DIR --out DIR
                      [--app-version V] [--plugin-api M.N]
    official-entry.py check [--official DIR] [--marketplace DIR]
    official-entry.py matrix [--check] [--out FILE]

`fetch` (network, read-only token is enough) writes a release bundle: the
GitHub Release of TAG (`deckpilot.<name>--v<version>`) as `release.json`,
its commit and whether that commit is on main, and every asset under
`assets/`. Nothing else ever reads the network.

`generate` turns ONE bundle into a candidate entry under
official/marketplace/<id>/ (the listing `plugin.json`, the version record
`versions/<v>.json` exactly as `deckpilot-registry add-version` writes it,
and `versions/<v>.release.json`, the release and its verification). It
refuses, before writing anything:

* an id that is not one DeckPilot bundles and official/plugins.json
  releases (test mode: a fixture of the given official/ only);
* a release that is a draft, not published, of another tag, of a commit not
  on main, or whose pre-release flag contradicts the version;
* assets other than exactly the package and its two signatures,
  `<id>-<version>.dpplugin.<KEYID>.minisig` (KEYID: the minisign key id
  the file carries, 16 uppercase hex) by two distinct keys; an asset whose
  downloaded size or sha256 differs from what GitHub reports for it;
* anything `deckpilot-registry release verify` refuses, run for every
  platform with both signatures and the publisher keys pinned in the
  pinned DeckPilot (never `--keys` outside test mode), which must pin a
  threshold of at least two, DeckPilot's released-pins.json as rollback
  floor and its artifacts.json revocations; a verification not by two
  distinct signers;
* a manifest that is not the claimed id/version, an engines range the
  pinned DeckPilot (tools/deckpilot.env) does not satisfy, permission kinds
  other than the policy of plugins.json, any plugin API 1.9 privileged kind
  while the gate is closed, process execution outside the allow-list;
* a dependency that is not a built-in module or an official plugin, or a
  required one that neither a candidate entry nor the plugin DeckPilot
  bundles at the pin satisfies;
* a version not above every candidate version of the plugin (downgrade or
  rewrite; the same record again is "unchanged").

Test mode (--test-keys): the keys are a throwaway publisher key set, the
id must be a fixture, and --out must lie outside official/marketplace/;
the release record says `"keys": "test"`. `check` refuses such a record.

The release record names both signature files and both signer key ids
(`verification.signerKeyIds`).

`check` re-checks every committed candidate (ids, gate, records against
their release, both signatures and signer key ids, dependencies,
append-only order) and that
docs/official-compatibility-matrix.md is current. `matrix` writes it.

No command signs, tags, uploads, publishes or touches registry/: the
candidates are never copied into the signed index here
(MARKETPLACE_SERVES_BUNDLED). DeckPilot's agent takes an official plugin,
a bundled one included, from a registry only as a release signed by the
first-party publisher keys compiled into it, verified before install, and
refuses it outright while no publisher key is pinned (knittingcode/
deckpilot#378, apps/agent/src/plugins/official.rs).
"""

import argparse
import hashlib
import importlib.util
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REPOSITORY = "knittingcode/deckpilot-plugins"
DOWNLOAD = f"https://github.com/{REPOSITORY}/releases/download"
WORK = Path(os.environ.get("DECKPILOT_PLUGINS_WORK", ROOT / ".work"))
CLI = WORK / "bin/deckpilot-registry"
DECKPILOT_DIR = WORK / "deckpilot"
MARKETPLACE = ROOT / "official/marketplace"
MATRIX = ROOT / "docs/official-compatibility-matrix.md"
PLATFORMS = ("macos", "windows", "linux")
BUILDER = "1.0.0"
# Whether this repository copies candidate entries of the bundled plugins
# into registry/ (the signed index). It does not. DeckPilot's agent
# (apps/agent/src/plugins/service.rs `fetch`/`pins`, official.rs) accepts an
# official plugin from a registry only as a first-party release signed by
# the publisher keys compiled into it, verified before install, and refuses
# it before any download while no publisher key is pinned (none is). Serving
# them is a reviewed change here once a key is pinned and released.
MARKETPLACE_SERVES_BUNDLED = False

TAG = re.compile(r"(deckpilot\.[a-z0-9][a-z0-9-]{0,62})--v((?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
                 r"(?:-[0-9A-Za-z.-]+)?)")
SHA = re.compile(r"[0-9a-f]{64}")
COMMIT = re.compile(r"[0-9a-f]{40}")


def _official_rules():
    spec = importlib.util.spec_from_file_location("check_official", ROOT / "tools/check-official.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


RULES = _official_rules()


def _keyid_module():
    spec = importlib.util.spec_from_file_location("minisign_keyid", ROOT / "tools/lib/minisign_keyid.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


KEYID = _keyid_module()
# An official release is signed by two distinct publisher keys (threshold 2).
SIGNATURES = 2
KEY_ID = re.compile(r"[0-9A-F]{16}")
BUNDLED_PLUGIN_IDS = RULES.BUNDLED_PLUGIN_IDS


class Refused(Exception):
    pass


def env():
    return dict(re.findall(r"^([A-Z_]+)=(\S+)$", (ROOT / "tools/deckpilot.env").read_text(), re.M))


def load(path):
    try:
        return json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, ValueError) as e:
        raise Refused(f"{path}: {e}") from None


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 16), b""):
            h.update(chunk)
    return h.hexdigest()


def dump(value):
    return json.dumps(value, indent=2, ensure_ascii=False) + "\n"


# ---------------------------------------------------------------- versions

def semver(text):
    m = re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z.-]+))?", text or "")
    if not m:
        return None
    pre = m.group(4)
    # Precedence: a pre-release sorts below its release; identifiers compare
    # numerically when numeric, otherwise as text, numeric below text.
    pre_key = (1,) if pre is None else (0,) + tuple((0, int(p), "") if p.isdigit() else (1, 0, p)
                                                    for p in pre.split("."))
    return (int(m.group(1)), int(m.group(2)), int(m.group(3)), pre_key)


def in_range(text, version):
    """A VersionRange of engines.deckpilot / dependencies[].version."""
    v = semver(version)
    if v is None or not text:
        return False
    parts = text.split(" ")
    if len(parts) > 2:
        return False
    for part in parts:
        m = re.fullmatch(r"(>=|<=|>|<|=)?(\S+)", part)
        bound = semver(m.group(2)) if m else None
        if bound is None:
            return False
        op = m.group(1) or "="
        if not {">=": v >= bound, ">": v > bound, "<=": v <= bound, "<": v < bound, "=": v == bound}[op]:
            return False
    return True


# ---------------------------------------------------------------- policy

def official_policy(official):
    policy = load(Path(official) / "plugins.json")
    out = {}
    for kind in ("plugins", "fixtures"):
        for entry in policy.get(kind, []):
            out[entry.get("id")] = dict(entry, kind=kind[:-1])
    return out


def check_id(pid, policy, test):
    entry = policy.get(pid)
    if test:
        if not entry or entry["kind"] != "fixture":
            raise Refused(f"{pid}: test mode takes only a fixture of official/plugins.json")
        return entry
    if pid not in BUNDLED_PLUGIN_IDS:
        raise Refused(f"{pid} is not a plugin DeckPilot bundles (BUNDLED_PLUGIN_IDS); "
                      "only those have official entries")
    if not entry or entry["kind"] != "plugin":
        raise Refused(f"{pid} is not released from official/ (official/plugins.json `plugins`)")
    return entry


def check_release(bundle, tag):
    """release.json of a bundle: the published GitHub Release of tag."""
    m = TAG.fullmatch(tag or "")
    if not m:
        raise Refused(f"tag {tag!r} is not deckpilot.<name>--v<version>")
    pid, version = m.groups()
    rel = bundle.get("release") or {}
    if bundle.get("repository") != REPOSITORY:
        raise Refused(f"the release is not of {REPOSITORY}")
    if rel.get("tag_name") != tag:
        raise Refused(f"the release is of tag {rel.get('tag_name')!r}, not {tag}")
    if rel.get("draft") is not False:
        raise Refused(f"{tag}: the release is a draft (or its state is unknown); only a published release counts")
    if not rel.get("published_at"):
        raise Refused(f"{tag}: the release is not published")
    if bool(rel.get("prerelease")) != ("-" in version):
        raise Refused(f"{tag}: the release's pre-release flag ({rel.get('prerelease')}) contradicts version {version}")
    if not COMMIT.fullmatch(bundle.get("commit") or ""):
        raise Refused(f"{tag}: the tag's commit is unknown")
    if bundle.get("onMain") is not True:
        raise Refused(f"{tag}: commit {bundle.get('commit')} is not on main")
    return pid, version


def check_assets(bundle, assets_dir, pid, version):
    """Exactly the package and its two signatures, each as GitHub reports it;
    returns the package path and [(signature path, key id)]."""
    package = f"{pid}-{version}.dpplugin"
    signature = re.compile(re.escape(package) + r"\.([0-9A-F]{16})\.minisig")
    listed = {a.get("name"): a for a in (bundle.get("release") or {}).get("assets", [])}
    if package not in listed:
        raise Refused(f"the release has no asset {package}")
    sigs = sorted(n for n in listed if signature.fullmatch(n or ""))
    if not sigs:
        raise Refused(f"the release has no signature {package}.<KEYID>.minisig")
    extra = sorted(set(listed) - {package, *sigs}, key=str)
    if extra:
        raise Refused(f"the release has assets other than the package and its signatures {package}.<KEYID>.minisig: {extra}")
    if len(sigs) != SIGNATURES:
        raise Refused(f"the release has {len(sigs)} signature(s); an official release has exactly {SIGNATURES}, "
                      "by two distinct publisher keys")
    on_disk = sorted(p.name for p in Path(assets_dir).iterdir()) if Path(assets_dir).is_dir() else []
    if on_disk != sorted(listed):
        raise Refused(f"the downloaded assets {on_disk} are not the release's {sorted(listed)}")
    for name, asset in listed.items():
        path = Path(assets_dir) / name
        if asset.get("state", "uploaded") != "uploaded":
            raise Refused(f"{name}: asset state {asset.get('state')}")
        size = path.stat().st_size
        if size != asset.get("size"):
            raise Refused(f"{name}: {size} bytes downloaded, the release says {asset.get('size')}")
        digest = asset.get("digest")
        if digest is not None and digest != "sha256:" + sha256(path):
            raise Refused(f"{name}: sha256 {sha256(path)} differs from the release's {digest}")
    signatures = []
    for name in sigs:
        named = signature.fullmatch(name).group(1)
        try:
            carried = KEYID.key_id((Path(assets_dir) / name).read_bytes())
        except KEYID.NotMinisign as e:
            raise Refused(f"{name}: not a minisign signature ({e})") from None
        if carried != named:
            raise Refused(f"{name} carries key id {carried}, not {named}")
        signatures.append((Path(assets_dir) / name, named))
    if len({k for _, k in signatures}) != SIGNATURES:
        raise Refused("the two signatures are by the same key; two distinct publisher keys sign an official release")
    return Path(assets_dir) / package, signatures


def localized(manifest, files, key, locale):
    value = manifest.get(key, "")
    if not isinstance(value, str) or not value.startswith("t:"):
        return value
    loc = files / "locales" / f"{locale}.json"
    if not loc.is_file():
        return None
    node = load(loc)
    for part in value[2:].split("."):
        node = node.get(part) if isinstance(node, dict) else None
    return node if isinstance(node, str) else None


def check_manifest(manifest, pid, version, entry, host):
    if manifest.get("id") != pid or manifest.get("version") != version:
        raise Refused(f"the package is {manifest.get('id')} {manifest.get('version')}, not {pid} {version}")
    if manifest.get("publisher", {}).get("id") != "deckpilot":
        raise Refused(f"{pid}: publisher.id must be deckpilot")
    engines = manifest.get("engines", {})
    if not RULES.api_ok(engines.get("pluginApi"), host["pluginApi"]):
        raise Refused(f"{pid} {version}: engines.pluginApi {engines.get('pluginApi')} is not satisfied by "
                      f"DeckPilot's plugin API {host['pluginApi']}: incompatible host")
    if not in_range(engines.get("deckpilot", ""), host["version"]):
        raise Refused(f"{pid} {version}: engines.deckpilot {engines.get('deckpilot')} does not accept "
                      f"DeckPilot {host['version']}: incompatible host")
    before = len(RULES.problems)
    RULES.check_permissions(f"{pid} {version}", entry, manifest)
    found = RULES.problems[before:]
    del RULES.problems[before:]
    if found:
        raise Refused("; ".join(found))


def resolve_dependencies(manifest, candidates, bundled, host, policy_ids):
    """Each dependency: (id, range, optional, satisfied by)."""
    out = []
    for dep in manifest.get("dependencies", []) or []:
        did, rng, optional = dep.get("id", ""), dep.get("version", ""), bool(dep.get("optional"))
        if re.fullmatch(r"[a-z][a-z0-9-]*", did):  # a built-in module, matched against DeckPilot's version
            if not in_range(rng, host["version"]):
                raise Refused(f"{manifest['id']}: built-in module {did} {rng} is not DeckPilot {host['version']}")
            out.append((did, rng, optional, f"built-in module (DeckPilot {host['version']})"))
            continue
        if not did.startswith("deckpilot.") or (did not in BUNDLED_PLUGIN_IDS and did not in policy_ids):
            raise Refused(f"{manifest['id']}: dependency {did} is not an official plugin")
        offers = [(v, "Marketplace candidate") for v in candidates.get(did, [])]
        if did in bundled:
            offers.append((bundled[did], "bundled with DeckPilot"))
        fits = sorted((o for o in offers if in_range(rng, o[0])), key=lambda o: semver(o[0]), reverse=True)
        if fits:
            out.append((did, rng, optional, f"{fits[0][0]} ({fits[0][1]})"))
        elif optional:
            out.append((did, rng, optional, "not available (optional: its actions stay unavailable)"))
        else:
            have = ", ".join(f"{v} ({w})" for v, w in offers) or "none"
            raise Refused(f"{manifest['id']}: required dependency {did} {rng} is unmet (available: {have})")
    return out


def check_order(pid, version, existing):
    """Append-only, strictly increasing versions per plugin."""
    newer = [v for v in existing if semver(v) >= semver(version)]
    if version in existing:
        return "same"
    if newer:
        raise Refused(f"{pid} {version} is a downgrade: the candidate entries already hold "
                      f"{', '.join(sorted(newer, key=semver))}")
    return "new"


def candidate_versions(marketplace):
    out = {}
    base = Path(marketplace)
    if not base.is_dir():
        return out
    for pdir in sorted(p for p in base.iterdir() if p.is_dir()):
        vdir = pdir / "versions"
        out[pdir.name] = sorted((p.name[:-5] for p in vdir.glob("*.json") if not p.name.endswith(".release.json")),
                                key=semver) if vdir.is_dir() else []
    return out


def bundled_versions(deckpilot):
    """What the pinned DeckPilot bundles: id -> version of its source."""
    out = {}
    base = Path(deckpilot) / "plugins/first-party"
    if not (base / "bundled.json").is_file():
        return out
    for entry in load(base / "bundled.json").get("plugins", []):
        manifest = base / entry["path"] / "manifest.json"
        if manifest.is_file():
            m = load(manifest)
            out[m["id"]] = m["version"]
    return out


# ---------------------------------------------------------------- fetch

def gh(*args):
    p = subprocess.run(["gh", *args], capture_output=True, text=True)
    if p.returncode != 0:
        raise Refused(f"gh {' '.join(args)}: {p.stderr.strip()}")
    return p.stdout


def cmd_fetch(args):
    if not TAG.fullmatch(args.tag):
        raise Refused(f"tag {args.tag!r} is not deckpilot.<name>--v<version>")
    out = Path(args.out)
    if out.exists() and any(out.iterdir()):
        raise Refused(f"{out} must be a new, empty directory")
    release = json.loads(gh("api", f"repos/{REPOSITORY}/releases/tags/{args.tag}"))
    ref = json.loads(gh("api", f"repos/{REPOSITORY}/git/ref/tags/{args.tag}"))["object"]
    if ref["type"] == "tag":
        ref = json.loads(gh("api", f"repos/{REPOSITORY}/git/tags/{ref['sha']}"))["object"]
    commit = ref["sha"]
    status = json.loads(gh("api", f"repos/{REPOSITORY}/compare/main...{commit}"))["status"]
    (out / "assets").mkdir(parents=True)
    gh("release", "download", args.tag, "-R", REPOSITORY, "-D", str(out / "assets"))
    keep = ("tag_name", "draft", "prerelease", "published_at", "html_url")
    bundle = {
        "repository": REPOSITORY,
        "commit": commit,
        "onMain": status in ("behind", "identical"),
        "release": {**{k: release.get(k) for k in keep},
                    "assets": [{k: a.get(k) for k in ("name", "size", "digest", "state")}
                               for a in release.get("assets", [])]},
    }
    (out / "release.json").write_text(dump(bundle), encoding="utf-8")
    print(f"fetched {args.tag} ({commit[:12]}, {len(bundle['release']['assets'])} asset(s)) to {out}")
    return 0


# ---------------------------------------------------------------- generate

def run_cli(cli, *args):
    p = subprocess.run([str(cli), *args], capture_output=True, text=True)
    return p.returncode, (p.stdout + p.stderr).strip()


def verify(cli, package, signatures, pid, version, host, platforms, keys, deckpilot, scratch, test):
    """release verify for every platform; returns (unpacked dir, signers)."""
    pins = Path(deckpilot) / "plugins/first-party/released-pins.json"
    lock = Path(deckpilot) / "plugins/first-party/artifacts.json"
    if not test and not (pins.is_file() and lock.is_file()):
        raise Refused(f"no pinned DeckPilot checkout at {deckpilot} (released-pins.json, artifacts.json); "
                      "run tools/build-cli.sh")
    audit = scratch / "audit.jsonl"
    claim = ["--package", str(package), "--id", pid, "--version", version,
             "--sha256", sha256(package), "--size", str(package.stat().st_size),
             "--app-version", host["version"], "--plugin-api", host["pluginApi"], "--audit", str(audit)]
    for s in signatures:
        claim += ["--signature", str(s)]
    if pins.is_file():
        claim += ["--released-pins", str(pins)]
    if lock.is_file():
        claim += ["--revocations", str(lock)]
    keyset = Path(keys) if keys else Path(deckpilot) / "crates/deckpilot-plugin-registry/publisher/first-party.json"
    try:
        threshold = load(keyset).get("threshold")
    except Refused:
        threshold = None
    if not isinstance(threshold, int) or threshold < SIGNATURES:
        raise Refused(f"the publisher keys of {keyset} have threshold {threshold!r}; an official release needs "
                      f"a threshold of at least {SIGNATURES} distinct keys")
    if keys:
        claim += ["--keys", str(keys)]
    unpacked = scratch / "unpacked"
    for i, platform in enumerate(platforms):
        extra = ["--unpack", str(unpacked)] if i == 0 else []
        rc, out = run_cli(cli, "release", "verify", *claim, "--platform", platform, *extra)
        if rc != 0:
            raise Refused(f"release verify ({platform}) refused it: {out}")
        if not test and "not the pinned keys" in out:
            raise Refused("release verify did not use the pinned keys")
    signers = set()
    for line in audit.read_text(encoding="utf-8").splitlines():
        record = json.loads(line)
        if record.get("outcome") != "verified":
            raise Refused(f"audit: {record}")
        signers.update(record.get("signers", []))
    if len(signers) < SIGNATURES:
        raise Refused(f"release verify names {len(signers)} signer(s) ({', '.join(sorted(signers))}); "
                      f"an official release is signed by {SIGNATURES} distinct keys")
    return unpacked, sorted(signers)


def listing(manifest, files, pid, path):
    names = {loc: localized(manifest, files, "name", loc) for loc in ("en", "pl")}
    descriptions = {loc: localized(manifest, files, "description", loc) for loc in ("en", "pl")}
    if not names["en"] or not descriptions["en"]:
        raise Refused(f"{pid}: the package has no English name or description")
    if len(names["en"]) > 48 or len(descriptions["en"]) > 300:
        raise Refused(f"{pid}: the name (48) or description (300 characters) is too long for a listing")
    info = {
        "id": pid,
        "name": names["en"],
        "summary": descriptions["en"],
        "description": descriptions["en"],
        "license": manifest.get("license", ""),
        "homepage": f"https://github.com/{REPOSITORY}/tree/main/official/{path}",
        "source_repository": REPOSITORY,
    }
    if manifest.get("categories"):
        info["categories"] = manifest["categories"][:3]
    return info, {k: v for k, v in names.items() if v}, {k: v for k, v in descriptions.items() if v}


def add_version(cli, package, info, pid, version, tag, commit, path, published_at, scratch):
    """The record exactly as add-version writes it, in a scratch workspace
    whose THROWAWAY key set authorizes deckpilot (as validate.sh does)."""
    ws, keys = scratch / "ws", scratch / "keys"
    (ws / "registry/publishers").mkdir(parents=True)
    keys.mkdir()
    shutil.copy(ROOT / "registry/publishers/knittingcode.json", ws / "registry/publishers/")
    for role in ("root", "index", "timestamp"):
        rc, out = run_cli(cli, "keys", "generate", "--role", role, "--out", str(keys / f"{role}.key"), "--no-password")
        if rc:
            raise Refused(out)
    steps = [
        ["keyset", "init", "--dir", str(ws), "--root", str(keys / "root.key.pub"), "--index",
         str(keys / "index.key.pub"), "--timestamp", str(keys / "timestamp.key.pub"), "--first-party"],
        ["keyset", "sign", "--dir", str(ws), "--key", str(keys / "root.key")],
    ]
    detail = scratch / "plugin.json"
    detail.write_text(dump(info), encoding="utf-8")
    steps.append(["add-version", "--dir", str(ws), "--package", str(package), "--plugin-detail", str(detail),
                  "--source-repository", REPOSITORY, "--tag", tag, "--commit", commit, "--source-path",
                  f"official/{path}", "--builder", BUILDER, "--first-party", "--now", published_at])
    for step in steps:
        rc, out = run_cli(cli, *step)
        if rc:
            raise Refused(f"deckpilot-registry {step[0]}: {out}")
    shutil.rmtree(keys)
    record = load(ws / f"registry/plugins/{pid}/versions/{version}.json")
    expected = f"{DOWNLOAD}/{tag}/{pid}-{version}.dpplugin"
    if record["package"]["url"] != expected:
        raise Refused(f"the record names {record['package']['url']}, not the release asset {expected}")
    return (ws / f"registry/plugins/{pid}/versions/{version}.json").read_text(encoding="utf-8")


def cmd_generate(args):
    test = args.test_keys is not None
    official = Path(args.official or ROOT / "official")
    out = Path(args.out or MARKETPLACE)
    if not test and (args.official or args.out or args.app_version or args.plugin_api or args.cli or args.deckpilot):
        raise Refused("--official, --out, --app-version, --plugin-api, --cli and --deckpilot are for test mode "
                      "(--test-keys) only")
    if test and (out.resolve() == MARKETPLACE.resolve() or MARKETPLACE.resolve() in out.resolve().parents):
        raise Refused("test mode never writes into official/marketplace/")
    e = env()
    host = {"version": args.app_version or e["DECKPILOT_VERSION"],
            "pluginApi": args.plugin_api or e["DECKPILOT_PLUGIN_API"], "commit": e["DECKPILOT_COMMIT"]}
    bundle_dir = Path(args.bundle)
    bundle = load(bundle_dir / "release.json")
    tag = (bundle.get("release") or {}).get("tag_name") or ""
    pid, version = check_release(bundle, tag)
    policy = official_policy(official)
    entry = check_id(pid, policy, test)
    package, signed = check_assets(bundle, bundle_dir / "assets", pid, version)
    signatures = [path for path, _ in signed]
    cli = Path(args.cli or CLI)
    if not cli.is_file():
        raise Refused(f"{cli} is missing; run tools/build-cli.sh")
    deckpilot = Path(args.deckpilot or DECKPILOT_DIR)
    if not test:
        head = subprocess.run(["git", "-C", str(deckpilot), "rev-parse", "HEAD"], capture_output=True, text=True)
        if head.stdout.strip() != host["commit"]:
            raise Refused(f"{deckpilot} is not DeckPilot at {host['commit']} (tools/deckpilot.env); "
                          "run tools/build-cli.sh so the verifier and its pinned keys are that commit's")
    existing = candidate_versions(out)
    order = check_order(pid, version, existing.get(pid, []))
    with tempfile.TemporaryDirectory() as tmp:
        scratch = Path(tmp)
        unpacked, signers = verify(cli, package, signatures, pid, version, host, PLATFORMS, args.test_keys,
                                   deckpilot, scratch, test)
        manifest = load(unpacked / "manifest.json")
        platforms = manifest.get("platforms") or list(PLATFORMS)
        check_manifest(manifest, pid, version, entry, host)
        candidates = {k: [v for v in vs if k != pid] for k, vs in existing.items()}
        deps = resolve_dependencies(manifest, candidates, bundled_versions(deckpilot), host, set(policy))
        info, names, descriptions = listing(manifest, unpacked, pid, entry["path"])
        record = add_version(cli, package, info, pid, version, tag, bundle["commit"], entry["path"],
                             bundle["release"]["published_at"], scratch)
    release = {
        "tag": tag,
        "release": bundle["release"].get("html_url") or f"https://github.com/{REPOSITORY}/releases/tag/{tag}",
        "commit": bundle["commit"],
        "publishedAt": bundle["release"]["published_at"],
        "package": {"name": package.name, "sha256": sha256(package), "size": package.stat().st_size},
        "signatures": [{"name": s.name, "keyId": k, "sha256": sha256(s)} for s, k in signed],
        "verification": {
            "keys": "test" if test else "pinned",
            "signers": signers,
            "signerKeyIds": sorted(k for _, k in signed),
            "deckpilot": host["version"],
            "pluginApi": host["pluginApi"],
            "deckpilotCommit": host["commit"],
            "platforms": platforms,
        },
        "names": names,
        "descriptions": descriptions,
        "dependencies": [{"id": d, "version": r, "optional": o, "satisfiedBy": s} for d, r, o, s in deps],
    }
    target = out / pid
    rec_path = target / "versions" / f"{version}.json"
    rel_path = target / "versions" / f"{version}.release.json"
    if order == "same":
        old = load(rel_path) if rel_path.is_file() else {}
        if rec_path.read_text(encoding="utf-8") != record or {**old, "verification": None} != {**release, "verification": None}:
            raise Refused(f"{pid} {version} already has a different candidate entry; versions are append-only")
        print(f"{pid} {version}: unchanged")
        return 0
    if args.dry_run:
        sys.stdout.write(record)
        return 0
    (target / "versions").mkdir(parents=True, exist_ok=True)
    (target / "plugin.json").write_text(dump(info), encoding="utf-8")
    rec_path.write_text(record, encoding="utf-8")
    rel_path.write_text(dump(release), encoding="utf-8")
    print(f"{pid} {version}: candidate entry written to {target} "
          f"({'THROWAWAY test keys' if test else 'verified with the pinned publisher keys'})")
    return 0


# ---------------------------------------------------------------- check

def git_manifest(commit, path):
    """official/<path>/manifest.json at `commit` (fetched from origin when
    the checkout lacks it)."""
    if not COMMIT.fullmatch(commit or ""):
        raise Refused(f"{commit!r} is not a commit")
    spec = f"{commit}:official/{path}/manifest.json"
    show = subprocess.run(["git", "-C", str(ROOT), "show", spec], capture_output=True)
    if show.returncode != 0:
        subprocess.run(["git", "-C", str(ROOT), "fetch", "-q", "--depth", "1", "origin", commit],
                       capture_output=True)
        show = subprocess.run(["git", "-C", str(ROOT), "show", spec], capture_output=True)
    if show.returncode != 0:
        raise Refused(f"official/{path}/manifest.json at {commit[:12]} cannot be read")
    return show.stdout


# Where `check` reads the signed manifest (replaced in the unit tests).
SIGNED_MANIFEST = git_manifest


def check_signatures(release, pid, version):
    """A committed release record: two signature files named by two distinct
    key ids, the signer key ids recorded, two signers."""
    package = f"{pid}-{version}.dpplugin"
    sigs = release.get("signatures") or []
    ids = [s.get("keyId") for s in sigs]
    if len(sigs) != SIGNATURES or len(set(ids)) != SIGNATURES or not all(KEY_ID.fullmatch(k or "") for k in ids):
        raise Refused(f"the release record names {len(sigs)} signature(s) with key ids {ids}; "
                      f"an official release has {SIGNATURES}, by distinct keys")
    for s in sigs:
        if s.get("name") != f"{package}.{s['keyId']}.minisig" or not SHA.fullmatch(s.get("sha256") or ""):
            raise Refused(f"signature {s.get('name')} is not {package}.{s['keyId']}.minisig with its sha256")
    verification = release.get("verification", {})
    if verification.get("signerKeyIds") != sorted(ids):
        raise Refused(f"verification.signerKeyIds {verification.get('signerKeyIds')} are not the signatures' {sorted(ids)}")
    if len(set(verification.get("signers") or [])) != SIGNATURES:
        raise Refused(f"the verification names {len(set(verification.get('signers') or []))} signer(s), not {SIGNATURES}")


def check_candidates(official, marketplace, deckpilot, host):
    problems = []
    policy = official_policy(official)
    existing = candidate_versions(marketplace)
    bundled = bundled_versions(deckpilot)
    for pid, versions in existing.items():
        where = f"official/marketplace/{pid}"
        try:
            entry = check_id(pid, policy, test=False)
        except Refused as e:
            problems.append(f"{where}: {e}")
            continue
        base = Path(marketplace) / pid
        info = load(base / "plugin.json") if (base / "plugin.json").is_file() else None
        if not info or info.get("id") != pid:
            problems.append(f"{where}/plugin.json is missing or names another id")
        for version in versions:
            try:
                record = load(base / f"versions/{version}.json")
                release = load(base / f"versions/{version}.release.json")
                tag = f"{pid}--v{version}"
                if record.get("version") != version or release.get("tag") != tag:
                    raise Refused("record, release and file name disagree")
                if release.get("verification", {}).get("keys") != "pinned":
                    raise Refused("verified with test keys; only pinned-key verifications are committed")
                check_signatures(release, pid, version)
                pkg = record.get("package", {})
                if pkg.get("url") != f"{DOWNLOAD}/{tag}/{pid}-{version}.dpplugin":
                    raise Refused(f"package.url {pkg.get('url')} is not the release asset")
                if (pkg.get("sha256"), pkg.get("size")) != (release["package"]["sha256"], release["package"]["size"]):
                    raise Refused("package sha256/size differ from the verified release")
                if record.get("source", {}).get("tag") != tag or record["source"].get("commit") != release.get("commit"):
                    raise Refused("source tag/commit differ from the release")
                if record["source"].get("path") != f"official/{entry['path']}":
                    raise Refused(f"source.path is not official/{entry['path']}")
                manifest = {"id": pid, "version": version, "publisher": {"id": "deckpilot"},
                            "engines": record.get("engines", {}), "permissions": record.get("permissions", []),
                            "dependencies": record.get("dependencies", [])}
                # The record against the manifest that was signed: the
                # source manifest at the release commit, whose sha256 the
                # record pins (the package carries it byte for byte).
                signed = SIGNED_MANIFEST(release["commit"], entry["path"])
                if hashlib.sha256(signed).hexdigest() != record.get("manifest_sha256"):
                    raise Refused("manifest_sha256 is not the signed manifest's (official/"
                                  f"{entry['path']}/manifest.json at {release['commit'][:12]})")
                signed_manifest = json.loads(signed)
                for field in ("permissions", "engines"):
                    if record.get(field) != signed_manifest.get(field):
                        raise Refused(f"the record's {field} differ from the signed manifest's")
                if (record.get("dependencies") or []) != (signed_manifest.get("dependencies") or []):
                    raise Refused("the record's dependencies differ from the signed manifest's")
                check_manifest(signed_manifest, pid, version, entry, host)
                manifest = {**manifest, **signed_manifest}
                candidates = {k: [v for v in vs if k != pid] for k, vs in existing.items()}
                resolve_dependencies(manifest, candidates, bundled, host, set(policy))
            except (Refused, KeyError, TypeError) as e:
                problems.append(f"{where}/versions/{version}: {e}")
    return problems


def cmd_check(args):
    e = env()
    host = {"version": e["DECKPILOT_VERSION"], "pluginApi": e["DECKPILOT_PLUGIN_API"]}
    official = Path(args.official or ROOT / "official")
    problems = check_candidates(official, Path(args.marketplace or MARKETPLACE), Path(args.deckpilot or DECKPILOT_DIR), host)
    if not args.marketplace:
        try:
            want = matrix(Path(args.deckpilot or DECKPILOT_DIR))
            have = MATRIX.read_text(encoding="utf-8") if MATRIX.is_file() else ""
            if have != want:
                import difflib
                sys.stderr.writelines(difflib.unified_diff(have.splitlines(True), want.splitlines(True),
                                                           "committed", "generated"))
                problems.append(f"{MATRIX.relative_to(ROOT)} is out of date: run tools/official-entry.py matrix")
        except (OSError, Refused) as err:
            problems.append(f"matrix: {err}")
    for p in problems:
        print(f"error: {p}", file=sys.stderr)
    if problems:
        return 1
    n = sum(len(v) for v in candidate_versions(args.marketplace or MARKETPLACE).values())
    print(f"official marketplace: {n} candidate version(s) checked; matrix current")
    return 0


# ---------------------------------------------------------------- matrix

def sdk_pin():
    cargo = (ROOT / "official/Cargo.toml").read_text(encoding="utf-8")
    for line in cargo.splitlines():
        if line.startswith("deckpilot-plugin-sdk"):
            rev = re.search(r'rev\s*=\s*"([0-9a-f]{40})"', line)
            return rev.group(1) if rev else "?"
    return "?"


def fmt_permissions(manifest):
    kinds = []
    for p in manifest.get("permissions", []):
        kind = p.get("kind", "?")
        if kind == "tools":
            kind += "(" + ",".join(t.get("executable", "?") for t in p.get("tools", [])) + ")"
        if kind in RULES.PRIVILEGED_KINDS:
            kind += "*"
        kinds.append(f"`{kind}`")
    return ", ".join(kinds) or "none"


def fmt_dependencies(manifest):
    deps = manifest.get("dependencies") or []
    return "<br>".join(f"`{d['id']}` `{d['version']}`{' (optional)' if d.get('optional') else ''}" for d in deps) or "none"


def privileged(manifest):
    return bool(RULES.privileged_usages(manifest))


def matrix(deckpilot):
    e = env()
    deckpilot = Path(deckpilot)
    keys_file = deckpilot / "crates/deckpilot-plugin-registry/publisher/first-party.json"
    if not keys_file.is_file():
        raise Refused(f"no pinned DeckPilot checkout at {deckpilot}; run tools/build-cli.sh")
    keys = load(keys_file)
    policy = official_policy(ROOT / "official")
    shipped = {}
    for release in load(deckpilot / "plugins/first-party/released-pins.json").get("releases", []):
        for p in release.get("plugins", []):
            if semver(p["version"]) > semver(shipped.get(p["id"], "0.0.0")):
                shipped[p["id"]] = p["version"]
    candidates = candidate_versions(MARKETPLACE)
    bundled_dir = deckpilot / "plugins/first-party"
    rows = []
    for pid in sorted(BUNDLED_PLUGIN_IDS):
        name = pid.split(".", 1)[1]
        entry = policy.get(pid)
        if entry and entry["kind"] == "plugin" and (ROOT / "official" / entry["path"] / "manifest.json").is_file():
            manifest = load(ROOT / "official" / entry["path"] / "manifest.json")
            source = f"`official/{entry['path']}`"
        elif (bundled_dir / name / "manifest.json").is_file():
            manifest = load(bundled_dir / name / "manifest.json")
            source = "DeckPilot (not moved)"
        else:
            manifest, source = {}, "artifact (DeckPilot)"
        engines = manifest.get("engines", {})
        latest = candidates.get(pid, [])[-1:] or []
        if latest:
            rel = load(MARKETPLACE / pid / "versions" / f"{latest[0]}.release.json")
            signature = f"{latest[0]}: verified, pinned keys ({', '.join(rel['verification']['signers'])})"
        else:
            signature = "no signed release"
        if privileged(manifest) and not RULES.PRIVILEGED_GATE_OPEN:
            market = "no: privileged-kinds gate closed"
        elif not latest:
            market = "no: no signed release"
        elif not MARKETPLACE_SERVES_BUNDLED:
            market = f"candidate {latest[0]}; not served yet (DeckPilot takes it only as a publisher-verified release)"
        else:
            market = f"yes, {latest[0]}"
        rows.append(f"| `{pid}` | {source} | {manifest.get('version', '?')} | {shipped.get(pid, '—')} | "
                    f"`{engines.get('deckpilot', '?')}` | `{engines.get('pluginApi', '?')}` | {fmt_permissions(manifest)} | "
                    f"{fmt_dependencies(manifest)} | {signature} | {market} |")
    provisioned = f"{len(keys.get('keys', []))} (threshold {keys.get('threshold')})" if keys.get("keys") else "none (not provisioned)"
    return "\n".join([
        "# Official plugins: compatibility matrix",
        "",
        "<!-- Generated by tools/official-entry.py matrix from tools/deckpilot.env, official/,",
        "     official/marketplace/ and the pinned DeckPilot checkout. Do not edit by hand;",
        "     `tools/official-entry.py check` (official.yml) fails when it is out of date. -->",
        "",
        "The nine plugins DeckPilot bundles, as this repository builds, signs and",
        "(once allowed) lists them. Procedure and gates: [official-marketplace.md](official-marketplace.md).",
        "",
        "| Host | |",
        "| --- | --- |",
        f"| DeckPilot version (pin) | {e['DECKPILOT_VERSION']} |",
        f"| Plugin API | {e['DECKPILOT_PLUGIN_API']} |",
        f"| DeckPilot commit (`tools/deckpilot.env`) | `{e['DECKPILOT_COMMIT']}` |",
        f"| SDK pin (`official/Cargo.toml` rev) | `{sdk_pin()}` |",
        f"| Publisher keys pinned in DeckPilot | {provisioned} |",
        f"| Privileged-kinds gate | {'open' if RULES.PRIVILEGED_GATE_OPEN else 'closed'} |",
        f"| Registry serves bundled ids | {'yes' if MARKETPLACE_SERVES_BUNDLED else 'no (not copied into registry/; DeckPilot accepts them only as publisher-verified releases, and none while no key is pinned)'} |",
        "",
        "| Plugin | Source | Version | Shipped in DeckPilot | `engines.deckpilot` | `engines.pluginApi` | Permissions | Dependencies | Signature | Marketplace |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
        *rows,
        "",
        "`*` a plugin API 1.9 privileged kind: no signed artifact may carry it while the gate is",
        "closed, so such a plugin has neither a release nor an entry. *Shipped in DeckPilot*: the",
        "highest version DeckPilot's `released-pins.json` records (the rollback floor of",
        "`release verify`). *Version*: the source (`official/<name>` once moved, else DeckPilot's",
        "at the pin).",
        "",
    ])


def cmd_matrix(args):
    text = matrix(Path(args.deckpilot or DECKPILOT_DIR))
    target = Path(args.out or MATRIX)
    if args.check:
        if not target.is_file() or target.read_text(encoding="utf-8") != text:
            print(f"error: {target} is out of date: run tools/official-entry.py matrix", file=sys.stderr)
            return 1
        print(f"{target.name}: current")
        return 0
    target.write_text(text, encoding="utf-8")
    print(f"wrote {target}")
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="command", required=True)
    f = sub.add_parser("fetch")
    f.add_argument("--tag", required=True)
    f.add_argument("--out", required=True)
    g = sub.add_parser("generate")
    g.add_argument("--bundle", required=True)
    g.add_argument("--dry-run", action="store_true")
    g.add_argument("--test-keys", type=Path)
    g.add_argument("--official")
    g.add_argument("--out")
    g.add_argument("--app-version")
    g.add_argument("--plugin-api")
    g.add_argument("--cli")
    g.add_argument("--deckpilot")
    c = sub.add_parser("check")
    c.add_argument("--official")
    c.add_argument("--marketplace")
    c.add_argument("--deckpilot")
    m = sub.add_parser("matrix")
    m.add_argument("--check", action="store_true")
    m.add_argument("--out")
    m.add_argument("--deckpilot")
    args = ap.parse_args()
    try:
        return {"fetch": cmd_fetch, "generate": cmd_generate, "check": cmd_check, "matrix": cmd_matrix}[args.command](args)
    except Refused as e:
        print(f"refused: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
