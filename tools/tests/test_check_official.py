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
POLICY = json.loads((ROOT / "official/plugins.json").read_text())
# The held plugins of the committed plugins.json (source on main, never
# packaged, signed, released or listed).
HELD = [e for e in POLICY["plugins"] if e.get("held") is not None]
H = HELD[0]["path"] if HELD else None

passed = failed = 0


def run(root, *extra):
    args = [sys.executable, "-I", str(CHECK), "--root", str(root), "--commit", COMMIT,
            "--app-version", ENV["DECKPILOT_VERSION"], "--plugin-api", ENV["DECKPILOT_PLUGIN_API"], *extra]
    p = subprocess.run(args, capture_output=True, text=True)
    return p.returncode, p.stdout + p.stderr


def deckpilot_of(tmp):
    """A stand-in for the pinned DeckPilot checkout: its fixture, and every
    held plugin as DeckPilot has it (version 0.1.0, its own repository)."""
    dp = Path(tmp) / "deckpilot"
    shutil.copytree(ROOT / "official" / FIXTURE, dp / "plugins/first-party" / FIXTURE)
    # The reviewed privileged policy, as the pinned DeckPilot carries it.
    record = dp / "crates/deckpilot-plugin-registry/publisher/privileged-policy.json"
    record.parent.mkdir(parents=True)
    shutil.copy(ROOT / "official/privileged-policy.json", record)
    for e in HELD:
        shutil.copytree(ROOT / "official" / e["path"], dp / "plugins/first-party" / e["path"],
                        ignore=shutil.ignore_patterns("target"))
        def back(m, name=e["path"]):
            m["version"] = "0.1.0"
            m["repository"] = {"type": "git", "url": "https://github.com/knittingcode/deckpilot",
                               "directory": f"plugins/first-party/{name}"}
        edit_json(dp / "plugins/first-party" / e["path"] / "manifest.json", back)
    return dp


def case(name, change, want_ok, message="", extra=(), deckpilot=None):
    global passed, failed
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp) / "official"
        shutil.copytree(ROOT / "official", root, ignore=shutil.ignore_patterns("target"))
        # The fixture's schema test reads ../../../../schemas: not needed here.
        if deckpilot is not None:
            dp = deckpilot_of(tmp)
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
    return policy(lambda d: fn(next(e for e in d["plugins"] if e["path"] == H)))


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
case("a privileged kind", manifest(lambda m: m["permissions"].append({"kind": "hostViews", "justification": "x"})), False,
     "gate is closed|does not admit it")
case("a plugin API the pin does not provide", manifest(lambda m: m["engines"].update(pluginApi="^1.99")), False, "not satisfied")
case("a DeckPilot range without the pin", manifest(lambda m: m["engines"].update(deckpilot=">=0.11.0 <1.0.0")), False, "does not accept")
case("another publisher", manifest(lambda m: m["publisher"].update(id="acme")), False, "publisher.id")
case("a fixture with a bundled id", manifest(lambda m: m.update(id="deckpilot.github")), False, r"plugins.json's|fixture")
case("a released plugin that DeckPilot does not bundle",
     policy(lambda d: d.update(plugins=d["fixtures"], fixtures=[])), False, "not a plugin DeckPilot bundles")
case("--release refuses a fixture", lambda r: None, False, "not a released official plugin", extra=["--release", "deckpilot.example-default"])
# Held plugins: source only. Privileged usages are allowed in source for a
# held plugin and nothing else; --list marks it, --release refuses it.
if HELD:
    e = HELD[0]
    hm = json.loads((ROOT / "official" / H / "manifest.json").read_text())
    case("--list marks a held plugin", lambda r: None, True,
         rf"(?m)^{re.escape(H)} {re.escape(e['id'])} {re.escape(hm['version'])} \S+ held$", extra=["--list"])
    case("check prints every held plugin with its reason", lambda r: None, True,
         "(?s)" + "".join(rf"(?=.*held: {re.escape(x['id'])} \S+: {re.escape(x['held'])})" for x in HELD))
    for x in HELD:
        case(f"--release refuses held {x['id']}", lambda r: None, False, "not a released official plugin",
             extra=["--release", x["id"]])
        case(f"{x['id']} without held is refused (gate closed)",
             policy(lambda d, i=x["id"]: next(p for p in d["plugins"] if p["id"] == i).pop("held")), False, "gate is closed")
    case("held for nothing", lambda r: (held_entry(lambda e: e.update(permissions=[]))(r),
                                        manifest(lambda m: (m.update(permissions=[]), m.pop("contributes", None)), H)(r)),
         False, "nothing holds it")
    case("held without a reason", held_entry(lambda e: e.update(held=True)), False, "is a reason")
    case("held with an empty reason", held_entry(lambda e: e.update(held="  ")), False, "is a reason")
    case("held does not widen the permission policy",
         manifest(lambda m: m["permissions"].append({"kind": "storage", "justification": "x"}), H), False, "reviewed policy change")
    case("held does not widen process execution",
         lambda r: (held_entry(lambda e: e["permissions"].append("tools"))(r),
                    manifest(lambda m: m["permissions"].append({"kind": "tools", "tools": [{"executable": "sh"}], "justification": "x"}), H)(r)),
         False, "process execution")
    case("the contract equals DeckPilot's but for version and repository", lambda r: None, True, r"member\(s\) checked",
         deckpilot=lambda dp: None)
    case("a manifest field that differs from DeckPilot's", manifest(lambda m: m.update(keywords=["x"]), H), False,
         "`keywords` differs from DeckPilot's", deckpilot=lambda dp: None)
    case("a locale that differs from DeckPilot's",
         lambda r: (r / H / "locales/pl.json").write_text((r / H / "locales/pl.json").read_text() + " "), False,
         "locales/pl.json differs", deckpilot=lambda dp: None)
    case("a plugin DeckPilot no longer has is not compared", manifest(lambda m: m.update(keywords=["x"]), H), True,
         r"member\(s\) checked", deckpilot=lambda dp: shutil.rmtree(dp / "plugins/first-party" / H))
case("a held fixture", policy(lambda d: d["fixtures"][0].update(held="x")), False, "for a plugin only")
RELEASED = next(e["path"] for e in POLICY["plugins"] if e.get("held") is None)
case("a privileged kind on a plugin that is not held", lambda r: (
     policy(lambda d: next(e for e in d["plugins"] if e["path"] == RELEASED)["permissions"].append("hostViews"))(r),
     manifest(lambda m: m["permissions"].append({"kind": "hostViews", "justification": "x"}), RELEASED)(r)),
     False, "gate is closed|does not admit it")
case("versions differ", replace(f"{FIXTURE}/Cargo.toml", 'version = "0.1.0"', 'version = "0.1.1"'), False, "differs from manifest.json")

# The reviewed privileged policy (official/privileged-policy.json, a copy of
# DeckPilot's). The gate is closed in this repository; the cases with the
# gate open run a copy of check-official.py with PRIVILEGED_GATE_OPEN = True
# (the test's only), on a tree whose policy records an approved review.
GATE_LINE = "PRIVILEGED_GATE_OPEN = False\n"
REVIEW = {"reviewer": "throwaway test reviewer", "date": "2026-11-21", "outcome": "approved",
          "record": "tools/tests/test_check_official.py"}
PRIVILEGED = ["docker", "git", "projects", "workflows", "workspace"]


def run_open(root, *extra):
    script = Path(root).parent / "check-official-open.py"
    text = CHECK.read_text()
    assert text.count(GATE_LINE) == 1
    script.write_text(text.replace(GATE_LINE, "PRIVILEGED_GATE_OPEN = True\n"))
    args = [sys.executable, "-I", str(script), "--root", str(root), "--commit", COMMIT,
            "--app-version", ENV["DECKPILOT_VERSION"], "--plugin-api", ENV["DECKPILOT_PLUGIN_API"], *extra]
    p = subprocess.run(args, capture_output=True, text=True)
    return p.returncode, p.stdout + p.stderr


def case_open(name, change, want_ok, message="", extra=(), release=PRIVILEGED, deckpilot=None):
    """With the gate open: `release` are the plugins no longer held."""
    global passed, failed
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp) / "official"
        shutil.copytree(ROOT / "official", root, ignore=shutil.ignore_patterns("target"))
        edit_json(root / "privileged-policy.json", lambda d: d.update(review=dict(REVIEW)))
        edit_json(root / "plugins.json",
                  lambda d: [e.pop("held", None) for e in d["plugins"] if e["path"] in release])
        if deckpilot is not None:
            dp = Path(tmp) / "deckpilot"
            deckpilot(root, dp)
            extra = (*extra, "--deckpilot", str(dp))
        change(root)
        rc, out = run_open(root, *extra)
    ok = (rc == 0) == want_ok and re.search(message, out)
    if ok:
        passed += 1
        print(f"ok   {name}")
    else:
        failed += 1
        print(f"FAIL {name}: exit {rc}: {out.strip()}")


def privileged_policy(fn):
    return lambda root: edit_json(root / "privileged-policy.json", fn)


def reviewed_plugin(pid, fn):
    return privileged_policy(lambda d: fn(next(p for p in d["plugins"] if p["id"] == pid)))


case("the gate is closed", lambda r: None, True, r"member")
gate = subprocess.run([sys.executable, "-I", str(CHECK), "--gate"], capture_output=True, text=True)
if gate.stdout.strip() == "closed" and gate.returncode == 0:
    passed += 1
    print("ok   --gate prints closed")
else:
    failed += 1
    print(f"FAIL --gate: {gate.stdout!r} {gate.returncode}")
case("the committed policy records no review",
     lambda r: None if json.loads((r / "privileged-policy.json").read_text())["review"] is None else 1 / 0, True, "member")
case("an invalid policy: threshold 1", privileged_policy(lambda d: d.update(minThreshold=1)), False, "minThreshold")
case("an invalid policy: another plugin", reviewed_plugin("deckpilot.workflows", lambda p: p.update(id="deckpilot.github")),
     False, "not one of DeckPilot's privileged plugins")
case("an invalid policy: process execution for workflows",
     reviewed_plugin("deckpilot.workflows", lambda p: p["permissions"].append({"kind": "tools", "tools": [{"executable": "git"}]})),
     False, "may not run")
case("an invalid policy: docker for git",
     reviewed_plugin("deckpilot.git", lambda p: p["permissions"][0]["tools"][0].update(executable="docker")), False, "may not run")
case("an invalid policy: a pending review without a reviewer",
     privileged_policy(lambda d: d.update(review={**REVIEW, "reviewer": " ", "outcome": "pending"})), False, "reviewer")
case("an invalid policy: a construct without its value",
     reviewed_plugin("deckpilot.projects", lambda p: next(c for c in p["constructs"] if "value" in c).pop("value")),
     False, "value is missing")


def deckpilot_with_policy(edit=None):
    def make(root, dp):
        target = dp / "crates/deckpilot-plugin-registry/publisher/privileged-policy.json"
        target.parent.mkdir(parents=True)
        shutil.copy(root / "privileged-policy.json", target)
        if edit:
            edit(target)
        for name in [FIXTURE]:
            shutil.copytree(ROOT / "official" / name, dp / "plugins/first-party" / name)
    return make


def other_bytes(path):
    path.write_text(path.read_text() + "\n")


# The accepted case: all five released with the gate open, exactly as reviewed.
case_open("open: the five reviewed plugins pass, exactly as reviewed", lambda r: None, True, r"member\(s\) checked")
case_open("open: held plugins stay valid", lambda r: None, True, r"held: deckpilot.git", release=["workflows"])
case_open("open: --release accepts a reviewed, no longer held plugin", lambda r: None, True, r"member",
          extra=["--release", "deckpilot.git"])
case_open("open: the copy equals DeckPilot's", lambda r: None, True, "member", deckpilot=deckpilot_with_policy())
case_open("open: a copy that differs from DeckPilot's", lambda r: None, False, "not byte-identical",
          deckpilot=deckpilot_with_policy(other_bytes))
case_open("open: a pinned DeckPilot without the policy", lambda r: None, False, "the gate cannot be open",
          deckpilot=lambda root, dp: shutil.copytree(ROOT / "official" / FIXTURE, dp / "plugins/first-party" / FIXTURE))
# Every refusal.
case_open("open: no review recorded", privileged_policy(lambda d: d.update(review=None)), False, "no approved security review")
case_open("open: a pending review", privileged_policy(lambda d: d["review"].update(outcome="pending")), False,
          "no approved security review")
case_open("open: a new construct", manifest(lambda m: m["contributes"].update(statusItems=["activeProject"]), "workflows"),
          False, r"contributes.statusItems \(`statusItems`\) is not reviewed")
case_open("open: a broader process permission",
          manifest(lambda m: m["permissions"][0]["tools"][0]["subcommands"].append({"name": "fetch"}), "git"),
          False, "permissions differ")
case_open("open: an extra executable",
          manifest(lambda m: m["permissions"][0]["tools"].append({"executable": "docker", "subcommands": [{"name": "ps"}]}), "git"),
          False, r"process execution|permissions differ")
case_open("open: a broader resources permission", manifest(lambda m: m["permissions"][1].update(open=True), "projects"),
          False, "permissions differ")
case_open("open: a broader resource parameter",
          manifest(lambda m: m["contributes"]["actions"][0]["parameters"][0]["kind"].update(kinds=["directory", "file"]), "git"),
          False, "differs from the reviewed value")
case_open("open: another version",
          lambda r: (manifest(lambda m: m.update(version="0.1.2"), "workspace")(r),
                     replace("workspace/Cargo.toml", 'version = "0.1.1"', 'version = "0.1.2"')(r)),
          False, "covers deckpilot.workspace 0.1.1 only")
case_open("open: a plugin the review does not cover",
          privileged_policy(lambda d: d.update(plugins=[p for p in d["plugins"] if p["id"] != "deckpilot.docker"])),
          False, "does not cover deckpilot.docker")
case_open("open: a reviewed construct no longer declared",
          reviewed_plugin("deckpilot.workflows", lambda p: p["constructs"].append(
              {"path": "contributes.statusItems", "what": "`statusItems`", "value": []})),
          False, "reviewed but not declared")

# N1: the record is compared type-strictly (JSON true is not 1), and a JSON
# object naming a key twice is refused.
def projects_changes_context_value(value):
    def fn(p):
        c = next(c for c in p["constructs"] if c["path"].endswith("node.changesContext"))
        c["value"] = value
    return reviewed_plugin("deckpilot.projects", fn)


case_open("open: a reviewed value of another JSON type (1 for true)", projects_changes_context_value(1), False,
          "differs from the reviewed value")
case_open("open: a reviewed value of another JSON type (1 for true in a reviewed permission)",
          reviewed_plugin("deckpilot.git", lambda p: p["permissions"][0]["tools"][0].update(network=1)), False,
          "permissions differ")
case_open("open: a manifest permission of another JSON type",
          manifest(lambda m: m["permissions"][1].update(paths=1), "git"), False, "permissions differ")


def duplicate_key(rel, key):
    def change(root):
        p = root / rel
        text = p.read_text()
        assert text.count("{") > 0
        p.write_text(text.replace("{", "{" + json.dumps(key) + ": 2, ", 1))
    return change


case("a duplicate key in the privileged policy", duplicate_key("privileged-policy.json", "privilegedPolicyVersion"),
     False, "duplicate key")
case("a duplicate key in plugins.json", duplicate_key("plugins.json", "officialVersion"), False, "duplicate key")
case("a duplicate key in a manifest", duplicate_key(f"{FIXTURE}/manifest.json", "id"), False, "duplicate key")


# N2: `official.sh check` with the gate open and no pinned DeckPilot
# checkout fails; with it closed it notes the missing checkout and passes.
def official_check(open_gate):
    with tempfile.TemporaryDirectory() as tmp:
        repo = Path(tmp) / "repo"
        shutil.copytree(ROOT / "tools", repo / "tools", ignore=shutil.ignore_patterns("__pycache__"))
        shutil.copytree(ROOT / "official", repo / "official", ignore=shutil.ignore_patterns("target"))
        if open_gate:
            script = repo / "tools/check-official.py"
            text = script.read_text()
            script.write_text(text.replace("PRIVILEGED_GATE_OPEN = False\n", "PRIVILEGED_GATE_OPEN = True\n")
                              .replace("PRIVILEGED_GATE_OPEN = True\n", "PRIVILEGED_GATE_OPEN = True\n"))
        env = {**os.environ, "DECKPILOT_PLUGINS_WORK": str(Path(tmp) / "work")}
        p = subprocess.run(["bash", str(repo / "tools/official.sh"), "check"], capture_output=True, text=True, env=env)
        return p.returncode, p.stdout + p.stderr


for open_gate, want_ok, message in [(True, False, "gate is open, but there is no pinned DeckPilot checkout"),
                                    (False, True, "no pinned DeckPilot checkout")]:
    rc, out = official_check(open_gate)
    if (rc == 0) == want_ok and message in out:
        passed += 1
        print(f"ok   official.sh check without a DeckPilot checkout, gate {'open' if open_gate else 'closed'}")
    elif not open_gate and "PRIVILEGED_GATE_OPEN = True" in CHECK.read_text():
        passed += 1
        print("ok   official.sh check, gate closed: not applicable (this tree's gate is open)")
    else:
        failed += 1
        print(f"FAIL official.sh check without a DeckPilot checkout, gate {'open' if open_gate else 'closed'}: "
              f"exit {rc}: {out.strip()}")

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
    case(f"privileged: {name}", change, False, path + r".*(gate is closed|does not admit it)")
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
    # DeckPilot's nine manifests, then every held plugin's manifest here:
    # what the package step must see refused, counted exactly as Rust does.
    targets = [(f"DeckPilot's {name}", deckpilot / name, name in {"docker", "git", "projects", "workflows", "workspace"})
               for name in BUNDLED]
    targets += [(f"held official/{e['path']}", ROOT / "official" / e["path"], True) for e in HELD]
    for label, directory, expected_some in targets:
        with tempfile.TemporaryDirectory() as tmp:
            # The manifest is checked before the module is read: any file
            # serves as the module, and the run fails either way.
            out = subprocess.run([str(cli), "package", "--plugin-dir", str(directory), "--module",
                                  str(directory / "manifest.json"), "--out", str(Path(tmp) / "x.dpplugin"),
                                  "--first-party"], capture_output=True, text=True)
        rust = sorted(RUST_USAGE.findall(out.stdout + out.stderr))
        ours = sorted(path for path, _ in RULES.privileged_usages(json.loads((directory / "manifest.json").read_text())))
        if out.returncode != 0 and rust == ours and bool(rust) == expected_some:
            passed += 1
            print(f"ok   {label}: the same {len(rust)} privileged usage(s) as the Rust validator")
        else:
            failed += 1
            print(f"FAIL {label}: Rust {rust}, here {ours} (exit {out.returncode})")
elif os.environ.get("CI") == "true":
    failed += 1
    print("FAIL no pinned DeckPilot checkout and CLI (.work/deckpilot, .work/bin): run tools/build-cli.sh first")
else:
    print("note: no pinned DeckPilot checkout and CLI; the mirror is not compared with the Rust validator")

print(f"\n{passed} passed, {failed} failed")
sys.exit(1 if failed else 0)
