#!/usr/bin/env python3
"""Tests of tools/check-official.py on copies of official/ with one change
each (stdlib only; run with python3 -I). No network, no cargo.

    python3 -I tools/tests/test_check_official.py
"""

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CHECK = ROOT / "tools/check-official.py"
ENV = dict(re.findall(r"^([A-Z_]+)=(\S+)$", (ROOT / "tools/deckpilot.env").read_text(), re.M))
COMMIT = ENV["DECKPILOT_COMMIT"]
FIXTURE = "test-fixtures/example-default"

passed = failed = 0


def run(root, *extra):
    args = [sys.executable, "-I", str(CHECK), "--root", str(root), "--commit", COMMIT,
            "--app-version", ENV["DECKPILOT_VERSION"], "--plugin-api", ENV["DECKPILOT_PLUGIN_API"], *extra]
    p = subprocess.run(args, capture_output=True, text=True)
    return p.returncode, p.stdout + p.stderr


def case(name, change, want_ok, message="", extra=()):
    global passed, failed
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp) / "official"
        shutil.copytree(ROOT / "official", root, ignore=shutil.ignore_patterns("target"))
        # The fixture's schema test reads ../../../../schemas: not needed here.
        change(root)
        rc, out = run(root, *extra)
    ok = (rc == 0) == want_ok and re.search(message, out)
    if ok:
        passed += 1
        print(f"ok   {name}")
    else:
        failed += 1
        print(f"FAIL {name}: exit {rc}: {out.strip()}")


def edit_json(path, fn):
    data = json.loads(path.read_text())
    fn(data)
    path.write_text(json.dumps(data, indent=2) + "\n")


def manifest(fn):
    return lambda root: edit_json(root / FIXTURE / "manifest.json", fn)


def policy(fn):
    return lambda root: edit_json(root / "plugins.json", fn)


def replace(rel, old, new):
    def change(root):
        p = root / rel
        p.write_text(p.read_text().replace(old, new))
    return change


case("the committed tree passes", lambda r: None, True, r"[0-9]+ member\(s\) checked")
case("--list", lambda r: None, True, r"test-fixtures/example-default deckpilot.example-default 0\.1\.0 deckpilot-example-default fixture", extra=["--list"])
case("another SDK rev", replace("Cargo.toml", COMMIT, "0" * 40), False, "must be pinned to rev")
case("a branch instead of a rev", replace("Cargo.toml", f'rev = "{COMMIT}"', 'branch = "main"'), False, "branch or tag")
case("another git repository", replace("Cargo.toml", "https://github.com/knittingcode/deckpilot\"", "https://github.com/acme/deckpilot\""), False, "only https://github.com/knittingcode/deckpilot")
case("a path dependency outside official/", replace("Cargo.toml", "[workspace.lints.rust]", 'x = { path = "../../deckpilot" }\n\n[workspace.lints.rust]'), False, "leaves official/")
case("no Cargo.lock", lambda r: (r / "Cargo.lock").unlink(), False, "Cargo.lock is missing")
case("a lock on another DeckPilot rev", replace("Cargo.lock", COMMIT, "1" * 40), False, "is not DeckPilot at")
case("a member not in plugins.json", policy(lambda d: d.update(fixtures=[])), False, "not in plugins.json")
case("a permission outside the policy", manifest(lambda m: m["permissions"].append({"kind": "storage", "justification": "x"})), False, "reviewed policy change")
case("process execution", manifest(lambda m: m["permissions"].append({"kind": "tools", "tools": [{"executable": "git"}], "justification": "x"})), False, r"process execution|privileged kind")
case("a privileged kind", manifest(lambda m: m["permissions"].append({"kind": "hostViews", "justification": "x"})), False, "gate is closed")
case("a plugin API the pin does not provide", manifest(lambda m: m["engines"].update(pluginApi="^1.99")), False, "not satisfied")
case("a DeckPilot range without the pin", manifest(lambda m: m["engines"].update(deckpilot=">=0.11.0 <1.0.0")), False, "does not accept")
case("another publisher", manifest(lambda m: m["publisher"].update(id="acme")), False, "publisher.id")
case("a fixture with a bundled id", manifest(lambda m: m.update(id="deckpilot.github")), False, r"plugins.json's|fixture")
case("a released plugin that DeckPilot does not bundle",
     policy(lambda d: d.update(plugins=d["fixtures"], fixtures=[])), False, "not a plugin DeckPilot bundles")
case("--release refuses a fixture", lambda r: None, False, "not a released official plugin", extra=["--release", "deckpilot.example-default"])
case("versions differ", replace(f"{FIXTURE}/Cargo.toml", 'version = "0.1.0"', 'version = "0.1.1"'), False, "differs from manifest.json")

# Every privileged usage of DeckPilot's privileged.rs `usages`, one by one,
# through the whole check (the gate is closed).
def contributes(key, value):
    return manifest(lambda m: m.setdefault("contributes", {}).__setitem__(key, value))


def first_action(fn):
    return manifest(lambda m: fn(m["contributes"]["actions"][0]))


# The shapes below are the manifests' own (DeckPilot's projects, git,
# workspace and workflows manifests at the pin): ParameterKind is tagged by
# `type`, a variable source is `host:project.*`, a view has id and kind.
for name, change, path in [
    ("openUrl.fromParams", manifest(lambda m: m["permissions"].append({"kind": "openUrl", "hosts": [], "fromParams": True, "justification": "x"})), r"permissions\[1\]\.fromParams"),
    ("node.changesContext", first_action(lambda a: a.setdefault("node", {}).__setitem__("changesContext", True)), r"node\.changesContext"),
    ("a resource parameter", first_action(lambda a: a["parameters"].append({"key": "repository", "label": "Repository", "kind": {"type": "resource", "kinds": ["directory"]}})), r"parameters\[\d+\]\.kind: a `resource` parameter"),
    ("a variable host source", contributes("variables", [{"id": "current.id", "name": "Project id", "valueType": {"type": "string"}, "scope": "project", "requires": ["project"], "source": "host:project.id"}]), r"variables\[0\]\.source: the host source `host:project\.id`"),
    ("a host view", contributes("views", [{"id": "workflows", "kind": "workflows"}]), r"views\[0\]\.kind: the `workflows` view"),
    ("statusItems, even empty", contributes("statusItems", []), r"contributes\.statusItems"),
    ("dashboardCards, even empty", contributes("dashboardCards", []), r"contributes\.dashboardCards"),
]:
    case(f"privileged: {name}", change, False, path + r".*gate is closed")
# A parameter kind that is not a resource is not one.
case("a text parameter is not privileged", first_action(lambda a: a["parameters"].append({"key": "x", "label": "X", "kind": {"type": "text"}})), True, r"member\(s\) checked")

# The mirror against the Rust validator itself, on DeckPilot's nine bundled
# manifests at the pin: the pinned deckpilot-registry's `package
# --first-party` (Manifest::validate_first_party) reports every privileged
# usage as `- <path>: ... is only for DeckPilot's own plugins shipped inside
# the app`; privileged_usages must report exactly the same paths. In CI
# (CI=true) the pinned checkout and CLI are required.
spec = __import__("importlib.util").util.spec_from_file_location("check_official", CHECK)
RULES = __import__("importlib.util").util.module_from_spec(spec)
spec.loader.exec_module(RULES)
deckpilot = ROOT / ".work/deckpilot/plugins/first-party"
cli = ROOT / ".work/bin/deckpilot-registry"
RUST_USAGE = re.compile(r"^- (\S+): .* is only for DeckPilot's own plugins shipped inside the app", re.M)
BUNDLED = ["docker", "git", "github", "gitlab", "jira", "notifications", "projects", "workflows", "workspace"]
if deckpilot.is_dir() and cli.is_file():
    for name in BUNDLED:
        directory = deckpilot / name
        with tempfile.TemporaryDirectory() as tmp:
            # The manifest is checked before the module is read: any file
            # serves as the module, and the run fails either way.
            out = subprocess.run([str(cli), "package", "--plugin-dir", str(directory), "--module",
                                  str(directory / "manifest.json"), "--out", str(Path(tmp) / "x.dpplugin"),
                                  "--first-party"], capture_output=True, text=True)
        rust = sorted(RUST_USAGE.findall(out.stdout + out.stderr))
        ours = sorted(path for path, _ in RULES.privileged_usages(json.loads((directory / "manifest.json").read_text())))
        expected_some = name in {"docker", "git", "projects", "workflows", "workspace"}
        if out.returncode != 0 and rust == ours and bool(rust) == expected_some:
            passed += 1
            print(f"ok   DeckPilot's {name}: the same {len(rust)} privileged usage(s) as the Rust validator")
        else:
            failed += 1
            print(f"FAIL DeckPilot's {name}: Rust {rust}, here {ours} (exit {out.returncode})")
elif os.environ.get("CI") == "true":
    failed += 1
    print("FAIL no pinned DeckPilot checkout and CLI (.work/deckpilot, .work/bin): run tools/build-cli.sh first")
else:
    print("note: no pinned DeckPilot checkout and CLI; the mirror is not compared with the Rust validator")

print(f"\n{passed} passed, {failed} failed")
sys.exit(1 if failed else 0)
