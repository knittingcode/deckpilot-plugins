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
WORKFLOWS_HELD = {"id": "deckpilot.workflows", "path": "workflows", "kind": "plugin", "permissions": ["hostViews"],
                  "held": "hostViews is privileged"}
case("a held plugin has no entry", lambda: E.check_id("deckpilot.workflows", {**policy, "deckpilot.workflows": WORKFLOWS_HELD}, False), "is held")
case("a held plugin's manifest is still refused by the entry rules",
     lambda: E.check_manifest(manifest(pid="deckpilot.workflows", version="0.1.1", perms=[{"kind": "hostViews"}]),
                              "deckpilot.workflows", "0.1.1", WORKFLOWS_HELD, HOST), "privileged")
# The five privileged plugins are released (no longer held) with the gate
# open; each still has no Marketplace entry: its committed manifest is
# refused by the entry rules whatever the gate.
OFFICIAL = E.official_policy(ROOT / "official")
for pid in ("deckpilot.docker", "deckpilot.git", "deckpilot.projects", "deckpilot.workflows", "deckpilot.workspace"):
    entry = OFFICIAL[pid]
    committed = json.loads((ROOT / "official" / entry["path"] / "manifest.json").read_text())
    case(f"the committed {pid} is a released plugin (not held)", lambda e=entry: None if e.get("held") is None
         else (_ for _ in ()).throw(E.Refused("held")))
    case(f"the committed {pid} {committed['version']} is refused a Marketplace entry",
         lambda pid=pid, m=committed: E.check_manifest(m, pid, m["version"], E.check_id(pid, OFFICIAL, False),
                                                                {**HOST, "version": "0.11.0"}),
         "never as a Marketplace entry")

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

# Release assets: the package and exactly two signatures named by their key ids.
import base64

KA, KB = "0123456789ABCDEF", "FEDCBA9876543210"


def signature_bytes(key_id):
    raw = b"ED" + int(key_id, 16).to_bytes(8, "little") + bytes(64)
    text = ("untrusted comment: signature from deckpilot-registry\n" + base64.b64encode(raw).decode() +
            "\ntrusted comment: x\n" + base64.b64encode(bytes(64)).decode() + "\n")
    return base64.b64encode(text.encode()) + b"\n"


PKG = "deckpilot.github-0.2.1.dpplugin"


def assets(files):
    """check_assets on a bundle of FILES {name: bytes}, as GitHub lists them."""
    def run():
        with tempfile.TemporaryDirectory() as tmp:
            d = Path(tmp)
            listed = []
            for name, data in files.items():
                (d / name).write_bytes(data)
                listed.append({"name": name, "size": len(data), "digest": "sha256:" + E.sha256(d / name), "state": "uploaded"})
            _, signed = E.check_assets({"release": {"assets": listed}}, d, "deckpilot.github", "0.2.1")
            if sorted(k for _, k in signed) != sorted({KA, KB}):
                raise E.Refused(f"key ids {signed}")
    return run


case("assets: the package and two signatures by distinct keys",
     assets({PKG: b"p", f"{PKG}.{KA}.minisig": signature_bytes(KA), f"{PKG}.{KB}.minisig": signature_bytes(KB)}))
case("assets: one signature (one key missing)",
     assets({PKG: b"p", f"{PKG}.{KA}.minisig": signature_bytes(KA)}), "1 signature(s)")
case("assets: no signature", assets({PKG: b"p"}), "no signature")
case("assets: the single-key name <package>.minisig",
     assets({PKG: b"p", f"{PKG}.minisig": signature_bytes(KA), f"{PKG}.{KB}.minisig": signature_bytes(KB)}), "other than the package")
case("assets: a third signature",
     assets({PKG: b"p", f"{PKG}.{KA}.minisig": signature_bytes(KA), f"{PKG}.{KB}.minisig": signature_bytes(KB),
             f"{PKG}.1111111111111111.minisig": signature_bytes("1111111111111111")}), "3 signature(s)")
case("assets: a duplicated signature under the other key's name",
     assets({PKG: b"p", f"{PKG}.{KA}.minisig": signature_bytes(KA), f"{PKG}.{KB}.minisig": signature_bytes(KA)}), f"carries key id {KA}, not {KB}")
case("assets: a key id in lower case",
     assets({PKG: b"p", f"{PKG}.{KA.lower()}.minisig": signature_bytes(KA), f"{PKG}.{KB}.minisig": signature_bytes(KB)}), "other than the package")
case("assets: a signature file that is not minisign",
     assets({PKG: b"p", f"{PKG}.{KA}.minisig": b"junk", f"{PKG}.{KB}.minisig": signature_bytes(KB)}), "not a minisign signature")

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
            pkg = "deckpilot.github-0.2.1.dpplugin"
            release = {"tag": tag, "commit": "a" * 40, "package": {"sha256": "b" * 64, "size": 10},
                       "signatures": [{"name": f"{pkg}.{k}.minisig", "keyId": k, "sha256": "c" * 64} for k in (KA, KB)],
                       "verification": {"keys": "pinned", "signers": ["k1", "k2"], "signerKeyIds": [KA, KB]}}
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
case("committed: one signature",
     committed(lambda r, rel, mp: rel.update(signatures=rel["signatures"][:1])), "1 signature(s)")
case("committed: two signatures by the same key",
     committed(lambda r, rel, mp: rel.update(signatures=[rel["signatures"][0]] * 2)), "by distinct keys")
case("committed: a signature named otherwise than its key id",
     committed(lambda r, rel, mp: rel["signatures"][0].update(name=f"{PKG}.minisig")), "is not")
case("committed: signer key ids differ from the signatures'",
     committed(lambda r, rel, mp: rel["verification"].update(signerKeyIds=[KA])), "signerKeyIds")
case("committed: one signer verified",
     committed(lambda r, rel, mp: rel["verification"].update(signers=["k1"])), "1 signer(s)")
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


def expect(value, want):
    if value != want:
        raise E.Refused(f"got {value!r}, want {want!r}")


# Promotion into registry/ (MARKETPLACE_SERVES_BUNDLED since DeckPilot 0.11.0).
case("the registry serves official candidates", lambda: expect(E.MARKETPLACE_SERVES_BUNDLED, True))
case("host floor is DeckPilot 0.11.0", lambda: expect(E.OFFICIAL_REGISTRY_MIN_HOST, "0.11.0"))
case("host floor: lower bound raised", lambda: expect(E.narrow_host(">=0.6.0 <1.0.0"), ">=0.11.0 <1.0.0"))
case("host floor: open range", lambda: expect(E.narrow_host(">=0.6.0"), ">=0.11.0"))
case("host floor: already above", lambda: expect(E.narrow_host(">=0.12.0 <1.0.0"), ">=0.12.0 <1.0.0"))
case("host floor: a range without 0.11.0", lambda: E.narrow_host(">=0.6.0 <0.11.0"), "does not accept DeckPilot 0.11.0")
case("host floor: exact version", lambda: E.narrow_host("=0.11.0"), "cannot be set")
case("host floor: upper bound only", lambda: E.narrow_host("<1.0.0"), "cannot be set")
case("host floor: older hosts are not offered it",
     lambda: expect([E.in_range(E.narrow_host(">=0.6.0 <1.0.0"), v) for v in ("0.6.0", "0.10.0", "0.10.9", "0.11.0", "0.99.0")],
                    [False, False, False, True, True]))
REC = {"version": "0.2.1", "engines": {"deckpilot": ">=0.6.0 <1.0.0", "pluginApi": "^1.0"}, "permissions": [{"kind": "network"}]}
case("registry record: only engines.deckpilot changes",
     lambda: expect(json.loads(E.registry_record(E.dump(REC))),
                    {**REC, "engines": {"deckpilot": ">=0.11.0 <1.0.0", "pluginApi": "^1.0"}}))
case("registry record: not add-version's format", lambda: E.registry_record(json.dumps(REC)), "format")
for pid in sorted(E.BUNDLED_ONLY_IDS):
    case(f"never promoted: {pid}", lambda pid=pid: E.check_eligible(pid, OFFICIAL), "bundled in DeckPilot only")
case("bundled-only ids are exactly the privileged policies",
     lambda: expect({p for p, v in OFFICIAL.items() if v["kind"] == "plugin" and set(v["permissions"]) & E.RULES.PRIVILEGED_KINDS},
                    set(E.BUNDLED_ONLY_IDS)))
for pid in ("deckpilot.github", "deckpilot.gitlab", "deckpilot.jira", "deckpilot.notifications"):
    case(f"eligible: {pid}", lambda pid=pid: E.check_eligible(pid, OFFICIAL))
case("eligible: a privileged kind in the policy",
     lambda: E.check_eligible("deckpilot.github", {"deckpilot.github": {**GITHUB, "permissions": ["network", "tools"]}}),
     "privileged kind")
case("eligible: a community plugin", lambda: E.check_eligible("deckpilot.pomodoro", OFFICIAL), "not a plugin DeckPilot bundles")


def registry_case(mutate, pid="deckpilot.github"):
    def run():
        with tempfile.TemporaryDirectory() as tmp:
            mp, reg = Path(tmp) / "mp", Path(tmp) / "registry"
            (mp / pid / "versions").mkdir(parents=True)
            (mp / pid / "plugin.json").write_text(E.dump({"id": pid}))
            (mp / pid / "versions/0.2.1.json").write_text(E.dump(REC))
            (mp / pid / "versions/0.2.1.release.json").write_text("{}\n")
            (reg / "plugins" / pid / "versions").mkdir(parents=True)
            (reg / "plugins" / pid / "plugin.json").write_text(E.dump({"id": pid}))
            (reg / "plugins" / pid / "versions/0.2.1.json").write_text(E.registry_record(E.dump(REC)))
            (reg / "plugins/deckpilot.pomodoro/versions").mkdir(parents=True)  # community: not checked here
            mutate(mp, reg)
            problems = E.registry_problems(ROOT / "official", mp, reg)
            if problems:
                raise E.Refused("; ".join(problems))
    return run


case("gate: the promoted record", registry_case(lambda mp, reg: None))
case("gate: the candidate's record unchanged (no host floor)",
     registry_case(lambda mp, reg: (reg / "plugins/deckpilot.github/versions/0.2.1.json").write_text(E.dump(REC))),
     "not the registry record")
case("gate: a version without a candidate",
     registry_case(lambda mp, reg: (reg / "plugins/deckpilot.github/versions/0.3.0.json").write_text(E.dump(REC))),
     "no committed candidate")
case("gate: another listing",
     registry_case(lambda mp, reg: (reg / "plugins/deckpilot.github/plugin.json").write_text(E.dump({"id": "x"}))),
     "listing")
case("gate: a privileged plugin in the registry",
     registry_case(lambda mp, reg: (reg / "plugins/deckpilot.workflows").mkdir()), "bundled in DeckPilot only")

print(f"official-entry: {passed} passed, {failed} failed")
sys.exit(1 if failed else 0)
