#!/usr/bin/env python3
"""Unit tests of tools/official-entry.py's rules (stdlib only; run with
python3 -I). No network, no cargo, no deckpilot-registry: the end-to-end
cases with real signed artifacts are tools/tests/test-official-marketplace.sh.

    python3 -I tools/tests/test_official_entry.py
"""

import importlib.util
import json
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("official_entry", ROOT / "tools/official-entry.py")
E = importlib.util.module_from_spec(spec)
spec.loader.exec_module(E)

HOST = {"version": "0.10.0", "pluginApi": "1.9", "commit": "0" * 40}
passed = failed = 0


def case(name, fn, refused=None):
    global passed, failed
    try:
        fn()
        ok = refused is None
        detail = "accepted"
    except E.Refused as e:
        ok = refused is not None and refused in str(e)
        detail = str(e)
    if ok:
        passed += 1
        print(f"ok   {name}")
    else:
        failed += 1
        print(f"FAIL {name}: {detail}", file=sys.stderr)


def manifest(pid="deckpilot.github", version="0.2.1", perms=None, deps=None, engines=None):
    m = {"id": pid, "version": version, "publisher": {"id": "deckpilot"},
         "engines": engines or {"deckpilot": ">=0.6.0 <1.0.0", "pluginApi": "^1.0"},
         "permissions": perms if perms is not None else [{"kind": "network"}, {"kind": "credentials"},
                                                         {"kind": "openUrl"}]}
    if deps is not None:
        m["dependencies"] = deps
    return m


GITHUB = {"id": "deckpilot.github", "path": "github", "kind": "plugin",
          "permissions": ["network", "credentials", "openUrl"]}

# Versions and ranges.
case("semver: pre-release below release", lambda: E.semver("1.0.0-rc.1") < E.semver("1.0.0") or (_ for _ in ()).throw(E.Refused("order")))
case("semver: numeric identifiers", lambda: E.semver("1.0.0-rc.2") < E.semver("1.0.0-rc.10") or (_ for _ in ()).throw(E.Refused("order")))
case("range: two bounds", lambda: E.in_range(">=0.1.0 <1.0.0", "0.9.9") or (_ for _ in ()).throw(E.Refused("range")))
case("range: upper bound excluded", lambda: (not E.in_range(">=0.1.0 <1.0.0", "1.0.0")) or (_ for _ in ()).throw(E.Refused("range")))

# Ids.
policy = {"deckpilot.github": GITHUB,
          "deckpilot.example-default": {"id": "deckpilot.example-default", "path": "x", "kind": "fixture"}}
case("bundled, released id", lambda: E.check_id("deckpilot.github", policy, False))
case("bundled id not released from official/", lambda: E.check_id("deckpilot.jira", policy, False), "not released from official/")
case("id DeckPilot does not bundle", lambda: E.check_id("deckpilot.pomodoro", policy, False), "not a plugin DeckPilot bundles")
case("community id", lambda: E.check_id("acme.weather", policy, False), "not a plugin DeckPilot bundles")
case("fixture outside test mode", lambda: E.check_id("deckpilot.example-default", policy, False), "not a plugin DeckPilot bundles")
case("bundled id in test mode", lambda: E.check_id("deckpilot.github", policy, True), "only a fixture")

# Releases.
def bundle(**release):
    r = {"tag_name": "deckpilot.github--v0.2.1", "draft": False, "prerelease": False,
         "published_at": "2026-11-21T10:00:00Z", "assets": []}
    r.update(release)
    return {"repository": "knittingcode/deckpilot-plugins", "commit": "a" * 40, "onMain": True, "release": r}

case("published release", lambda: E.check_release(bundle(), "deckpilot.github--v0.2.1"))
case("tag not deckpilot.*", lambda: E.check_release(bundle(tag_name="acme.x--v1.0.0"), "acme.x--v1.0.0"), "is not deckpilot")
case("registry tag", lambda: E.check_release(bundle(), "registry--v2026.1"), "is not deckpilot")
case("draft", lambda: E.check_release(bundle(draft=True), "deckpilot.github--v0.2.1"), "draft")
case("draft state unknown", lambda: E.check_release(bundle(draft=None), "deckpilot.github--v0.2.1"), "draft")
case("other repository", lambda: E.check_release({**bundle(), "repository": "acme/fork"}, "deckpilot.github--v0.2.1"), "is not of")
case("pre-release of a stable version", lambda: E.check_release(bundle(prerelease=True), "deckpilot.github--v0.2.1"), "pre-release")

# Manifest rules.
case("compatible manifest", lambda: E.check_manifest(manifest(), "deckpilot.github", "0.2.1", GITHUB, HOST))
case("other version in the package", lambda: E.check_manifest(manifest(version="0.2.2"), "deckpilot.github", "0.2.1", GITHUB, HOST), "not deckpilot.github 0.2.1")
case("host too old", lambda: E.check_manifest(manifest(engines={"deckpilot": ">=0.11.0 <1.0.0", "pluginApi": "^1.0"}), "deckpilot.github", "0.2.1", GITHUB, HOST), "incompatible host")
case("plugin API too new", lambda: E.check_manifest(manifest(engines={"deckpilot": ">=0.6.0 <1.0.0", "pluginApi": "^1.10"}), "deckpilot.github", "0.2.1", GITHUB, HOST), "incompatible host")
case("permission outside the policy", lambda: E.check_manifest(manifest(perms=[{"kind": "network"}]), "deckpilot.github", "0.2.1", GITHUB, HOST), "policy")
for kind in ("tools", "resources", "projectContext", "hostViews"):
    entry = dict(GITHUB, permissions=["network", "credentials", "openUrl", kind])
    perms = [{"kind": "network"}, {"kind": "credentials"}, {"kind": "openUrl"}, {"kind": kind}]
    case(f"privileged `{kind}` while the gate is closed",
         lambda e=entry, p=perms: E.check_manifest(manifest(perms=p), "deckpilot.github", "0.2.1", e, HOST), "privileged")
case("openUrl.fromParams while the gate is closed",
     lambda: E.check_manifest(manifest(perms=[{"kind": "network"}, {"kind": "credentials"}, {"kind": "openUrl", "fromParams": True}]),
                              "deckpilot.github", "0.2.1", GITHUB, HOST), "fromParams")

# Dependencies.
dep = lambda did, rng, optional=False: {"id": did, "version": rng, "optional": optional}
ids = set(policy)
case("required dep met by a candidate", lambda: E.resolve_dependencies(manifest(deps=[dep("deckpilot.projects", ">=0.2.0 <1.0.0")]), {"deckpilot.projects": ["0.2.0"]}, {}, HOST, ids))
case("required dep met by the bundle", lambda: E.resolve_dependencies(manifest(deps=[dep("deckpilot.projects", ">=0.1.0 <1.0.0")]), {}, {"deckpilot.projects": "0.1.0"}, HOST, ids))
case("required dep unmet", lambda: E.resolve_dependencies(manifest(deps=[dep("deckpilot.projects", ">=0.2.0 <1.0.0")]), {}, {"deckpilot.projects": "0.1.0"}, HOST, ids), "unmet")
case("optional dep unmet is recorded", lambda: E.resolve_dependencies(manifest(deps=[dep("deckpilot.projects", ">=0.2.0 <1.0.0", True)]), {}, {}, HOST, ids))
case("community dependency", lambda: E.resolve_dependencies(manifest(deps=[dep("acme.calendar", ">=1.0.0")]), {}, {}, HOST, ids), "not an official plugin")
case("built-in module matching DeckPilot", lambda: E.resolve_dependencies(manifest(deps=[dep("projects", ">=0.8.0")]), {}, {}, HOST, ids))
case("built-in module DeckPilot is too old for", lambda: E.resolve_dependencies(manifest(deps=[dep("projects", ">=0.11.0")]), {}, {}, HOST, ids), "is not DeckPilot")
case("satisfied by the highest fitting version", lambda: (
    E.resolve_dependencies(manifest(deps=[dep("deckpilot.projects", ">=0.1.0 <1.0.0")]), {"deckpilot.projects": ["0.1.1", "0.3.0"]}, {"deckpilot.projects": "0.1.0"}, HOST, ids)[0][3] == "0.3.0 (Marketplace candidate)"
    or (_ for _ in ()).throw(E.Refused("not the highest"))))

# Order.
case("first version", lambda: E.check_order("deckpilot.github", "0.2.1", []))
case("newer version", lambda: E.check_order("deckpilot.github", "0.2.1", ["0.2.0"]))
case("downgrade", lambda: E.check_order("deckpilot.github", "0.2.0", ["0.2.1"]), "downgrade")
case("pre-release below a release", lambda: E.check_order("deckpilot.github", "0.3.0-rc.1", ["0.3.0"]), "downgrade")

# check: committed candidates.
def committed(change, signed_change=None, pin=True):
    """A committed candidate; `signed_change` edits only the manifest that
    was signed (what `check` reads at the release commit); `pin=False`
    leaves the record's manifest_sha256 unset."""
    def run():
        with tempfile.TemporaryDirectory() as tmp:
            off, mp = Path(tmp) / "official", Path(tmp) / "marketplace"
            off.mkdir()
            (off / "plugins.json").write_text(json.dumps({"officialVersion": 1, "plugins": [GITHUB], "fixtures": []}))
            v = mp / "deckpilot.github/versions"
            v.mkdir(parents=True)
            tag = "deckpilot.github--v0.2.1"
            url = f"https://github.com/knittingcode/deckpilot-plugins/releases/download/{tag}/deckpilot.github-0.2.1.dpplugin"
            record = {"version": "0.2.1", "engines": {"deckpilot": ">=0.6.0 <1.0.0", "pluginApi": "^1.0"},
                      "permissions": [{"kind": "network"}, {"kind": "credentials"}, {"kind": "openUrl"}],
                      "source": {"repository": "knittingcode/deckpilot-plugins", "tag": tag, "commit": "a" * 40, "path": "official/github"},
                      "package": {"url": url, "sha256": "b" * 64, "size": 10}}
            release = {"tag": tag, "commit": "a" * 40, "package": {"sha256": "b" * 64, "size": 10},
                       "verification": {"keys": "pinned", "signers": ["k"]}}
            (mp / "deckpilot.github/plugin.json").write_text(json.dumps({"id": "deckpilot.github"}))
            change(record, release, mp)
            signed = {"id": "deckpilot.github", "version": "0.2.1", "publisher": {"id": "deckpilot"},
                      "engines": json.loads(json.dumps(record["engines"])),
                      "permissions": json.loads(json.dumps(record["permissions"]))}
            if "dependencies" in record:
                signed["dependencies"] = record["dependencies"]
            if signed_change:
                signed_change(signed)
            signed_bytes = json.dumps(signed, indent=2).encode()
            if pin:
                record["manifest_sha256"] = E.hashlib.sha256(signed_bytes).hexdigest()
            reads = []
            E.SIGNED_MANIFEST = lambda commit, path: reads.append((commit, path)) or signed_bytes
            (v / "0.2.1.json").write_text(json.dumps(record))
            (v / "0.2.1.release.json").write_text(json.dumps(release))
            problems = E.check_candidates(off, mp, Path(tmp) / "no-deckpilot", HOST)
            if not problems and reads != [("a" * 40, "github")]:
                raise E.Refused(f"the signed manifest was read as {reads}")
            if problems:
                raise E.Refused("; ".join(problems))
    return run

case("committed candidate", committed(lambda r, rel, mp: None))
case("committed: test keys", committed(lambda r, rel, mp: rel["verification"].update(keys="test")), "test keys")
case("committed: sha differs from the release", committed(lambda r, rel, mp: r["package"].update(sha256="c" * 64)), "differ from the verified release")
case("committed: url not the release asset", committed(lambda r, rel, mp: r["package"].update(url="https://example.com/x.dpplugin")), "not the release asset")
case("committed: privileged kind", committed(lambda r, rel, mp: r["permissions"].append({"kind": "hostViews"})), "privileged")
case("committed: unmet dependency", committed(lambda r, rel, mp: r.update(dependencies=[dep("deckpilot.projects", ">=0.1.0")])), "unmet")
case("committed: manifest_sha256 is not the signed manifest's", committed(lambda r, rel, mp: None, pin=False), "manifest_sha256")
case("committed: permissions differ from the signed manifest",
     committed(lambda r, rel, mp: None, lambda m: m["permissions"].append({"kind": "credentials"})), "permissions differ")
case("committed: engines differ from the signed manifest",
     committed(lambda r, rel, mp: None, lambda m: m["engines"].update(pluginApi="^1.1")), "engines differ")
case("committed: the signed manifest uses statusItems",
     committed(lambda r, rel, mp: None, lambda m: m.update(contributes={"statusItems": []})), "privileged")
case("committed: id DeckPilot does not bundle",
     committed(lambda r, rel, mp: (mp / "deckpilot.pomodoro/versions").mkdir(parents=True)), "not a plugin DeckPilot bundles")

E.SIGNED_MANIFEST = E.git_manifest
case("candidates are not copied into registry/ (MARKETPLACE_SERVES_BUNDLED)", lambda: (not E.MARKETPLACE_SERVES_BUNDLED) or (_ for _ in ()).throw(E.Refused("open")))

print(f"official-entry: {passed} passed, {failed} failed")
sys.exit(1 if failed else 0)
