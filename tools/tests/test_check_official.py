#!/usr/bin/env python3
"""Tests of tools/check-official.py on copies of official/ with one change
each (stdlib only; run with python3 -I). No network, no cargo.

    python3 -I tools/tests/test_check_official.py
"""

import json
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

print(f"\n{passed} passed, {failed} failed")
sys.exit(1 if failed else 0)
