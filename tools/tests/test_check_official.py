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
HELD = "workflows"

passed = failed = 0


def run(root, *extra):
    args = [sys.executable, "-I", str(CHECK), "--root", str(root), "--commit", COMMIT,
            "--app-version", ENV["DECKPILOT_VERSION"], "--plugin-api", ENV["DECKPILOT_PLUGIN_API"], *extra]
    p = subprocess.run(args, capture_output=True, text=True)
    return p.returncode, p.stdout + p.stderr


def deckpilot_of(root, tmp):
    """A stand-in for the pinned DeckPilot checkout: its fixture, and the held
    plugin as DeckPilot had it before the move (version 0.1.0, its own
    repository)."""
    dp = Path(tmp) / "deckpilot"
    shutil.copytree(ROOT / "official" / FIXTURE, dp / "plugins/first-party" / FIXTURE)
    shutil.copytree(ROOT / "official" / HELD, dp / "plugins/first-party" / HELD)
    def back(m):
        m["version"] = "0.1.0"
        m["repository"] = {"type": "git", "url": "https://github.com/knittingcode/deckpilot",
                           "directory": "plugins/first-party/workflows"}
    edit_json(dp / "plugins/first-party" / HELD / "manifest.json", back)
    return dp


def case(name, change, want_ok, message="", extra=(), deckpilot=None):
    global passed, failed
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp) / "official"
        shutil.copytree(ROOT / "official", root, ignore=shutil.ignore_patterns("target"))
        # The fixture's schema test reads ../../../../schemas: not needed here.
        if deckpilot is not None:
            dp = deckpilot_of(root, tmp)
            deckpilot(dp)
            extra = (*extra, "--deckpilot", str(dp))
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


def manifest(fn, member=FIXTURE):
    return lambda root: edit_json(root / member / "manifest.json", fn)


def held_entry(fn):
    def change(data):
        fn(next(e for e in data["plugins"] if e["path"] == HELD))
    return policy(change)


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
case("--list marks a held plugin", lambda r: None, True, r"workflows deckpilot.workflows 0\.1\.1 deckpilot-workflows held", extra=["--list"])
case("--release refuses a held plugin", lambda r: None, False, "not a released official plugin", extra=["--release", "deckpilot.workflows"])
case("a privileged kind without held", held_entry(lambda e: e.pop("held")), False, "gate is closed")
case("held for nothing", lambda r: (held_entry(lambda e: e.update(permissions=[]))(r),
                                    manifest(lambda m: m.update(permissions=[]), HELD)(r)), False, "nothing holds it")
case("held without a reason", held_entry(lambda e: e.update(held=True)), False, "is a reason")
case("a held fixture", policy(lambda d: d["fixtures"][0].update(held="x")), False, "for a plugin only")
case("the contract equals DeckPilot's but for version and repository", lambda r: None, True, r"\d+ member", deckpilot=lambda dp: None)
case("a view that differs from DeckPilot's", manifest(lambda m: m["contributes"]["views"][0].update(context="global"), HELD), False,
     "`contributes` differs from DeckPilot's", deckpilot=lambda dp: None)
case("a text that differs from DeckPilot's", replace(f"{HELD}/locales/pl.json", "Przepływy", "Przeplywy"), False,
     "locales/pl.json differs", deckpilot=lambda dp: None)
case("a plugin DeckPilot no longer has is not compared", manifest(lambda m: m.update(keywords=["x"]), HELD), True, r"\d+ member",
     deckpilot=lambda dp: shutil.rmtree(dp / "plugins/first-party" / HELD))
case("versions differ", replace(f"{FIXTURE}/Cargo.toml", 'version = "0.1.0"', 'version = "0.1.1"'), False, "differs from manifest.json")

print(f"\n{passed} passed, {failed} failed")
sys.exit(1 if failed else 0)
