#!/usr/bin/env python3
"""Tests for tools/check-submission.py (stdlib only).

    python3 -I tools/tests/test_check_submission.py
    python3 -I tools/tests/test_check_submission.py --deckpilot DIR

Each case builds a scratch git repository with a base and a head commit and
runs the real script on it, as the Submission rules job does. With
--deckpilot (a DeckPilot checkout at the pinned commit), it also checks that
the script's RESERVED_PUBLISHERS and BUNDLED_PLUGIN_IDS (and those of
tools/check-first-party.py) equal DeckPilot's.
"""

import importlib.util
import json
import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent.parent / "check-submission.py"
FIRST_PARTY_SCRIPT = Path(__file__).resolve().parent.parent / "check-first-party.py"
AUTHOR, AUTHOR_ID = "octo", 4242
OWNER, OWNER_ID = "knittingcode", 109468044
DECKPILOT = None


def load_script(path=SCRIPT, name="check_submission"):
    sys.dont_write_bytecode = True
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


# The plugins DeckPilot bundles at the pinned commit (v0.10.0).
BUNDLED = ("deckpilot.docker", "deckpilot.git", "deckpilot.github", "deckpilot.gitlab", "deckpilot.jira",
           "deckpilot.notifications", "deckpilot.projects", "deckpilot.workflows", "deckpilot.workspace")
# The official plugins the registry may serve, and the privileged ones it never serves.
OFFICIAL_MARKETPLACE = ("deckpilot.github", "deckpilot.gitlab", "deckpilot.jira", "deckpilot.notifications")
BUNDLED_ONLY = ("deckpilot.docker", "deckpilot.git", "deckpilot.projects", "deckpilot.workflows", "deckpilot.workspace")


def deckpilot_list(source_rs, name):
    text = Path(source_rs).read_text(encoding="utf-8")
    block = re.search(rf"pub const {name}: &\[&str\] = &\[(.*?)\];", text, re.S)
    if not block:
        raise AssertionError(f"{name} not found in {source_rs}")
    body = "\n".join(line.split("//", 1)[0] for line in block.group(1).splitlines())
    return set(re.findall(r'"([^"]+)"', body))


class Repo:
    def __init__(self, root):
        self.root = Path(root)
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", "test")
        self.git("config", "user.email", "test@example.invalid")
        self.git("config", "commit.gpgsign", "false")
        self.write("README.md", "registry\n")
        self.base = self.commit("base")

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.root, check=True, capture_output=True, text=True).stdout

    def write(self, path, content):
        p = self.root / path
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(content if isinstance(content, str) else json.dumps(content, indent=2) + "\n")

    def commit(self, message):
        self.git("add", "-A")
        self.git("commit", "-q", "--allow-empty", "-m", message)
        return self.git("rev-parse", "HEAD").strip()

    def check(self, author=AUTHOR, author_id=AUTHOR_ID):
        head = self.commit("head")
        env = {k: v for k, v in os.environ.items() if k != "GITHUB_STEP_SUMMARY"}
        result = subprocess.run(
            [sys.executable, "-I", str(SCRIPT), "--base", self.base, "--head", head,
             "--author", author, "--author-id", str(author_id)],
            cwd=self.root, capture_output=True, text=True, env=env,
        )
        return result.returncode, result.stderr


def publisher(namespace, login=AUTHOR, github_id=AUTHOR_ID):
    return {"namespace": namespace, "github_id": github_id, "login": login, "verified": False}


class ReservedPublishers(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = Repo(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_ordinary_namespace_passes(self):
        self.repo.write(f"registry/publishers/{AUTHOR}.json", publisher("octo-tools"))
        self.repo.write("registry/plugins/octo-tools.clock/plugin.json", {"id": "octo-tools.clock"})
        self.repo.write("plugins/octo-tools.clock/build.json", {"id": "octo-tools.clock"})
        status, err = self.repo.check()
        self.assertEqual(status, 0, err)

    def test_deckpilot_namespace_is_blocked(self):
        self.repo.write(f"registry/publishers/{AUTHOR}.json", publisher("deckpilot"))
        status, err = self.repo.check()
        self.assertEqual(status, 1)
        self.assertIn("reserved for DeckPilot's first-party plugins", err)

    def test_deckpilot_plugin_paths_are_blocked(self):
        self.repo.write(f"registry/publishers/{AUTHOR}.json", publisher("deckpilot"))
        self.repo.write("registry/plugins/deckpilot.github/plugin.json", {"id": "deckpilot.github"})
        self.repo.write("plugins/deckpilot.github/build.json", {"id": "deckpilot.github"})
        status, err = self.repo.check()
        self.assertEqual(status, 1)
        self.assertIn("registry/plugins/deckpilot.github/plugin.json: the publisher 'deckpilot' is reserved", err)
        self.assertIn("plugins/deckpilot.github/build.json: the publisher 'deckpilot' is reserved", err)

    def test_deckpilot_paths_blocked_even_with_an_existing_record(self):
        # A deckpilot record that somehow reached main still grants nothing.
        self.repo.write(f"registry/publishers/{AUTHOR}.json", publisher("deckpilot"))
        self.repo.base = self.repo.commit("record on main")
        self.repo.write("registry/plugins/deckpilot.clock/plugin.json", {"id": "deckpilot.clock"})
        status, err = self.repo.check()
        self.assertEqual(status, 1)
        self.assertIn("registry/plugins/deckpilot.clock/plugin.json: the publisher 'deckpilot' is reserved", err)

    def test_manifest_publisher_id_deckpilot_is_blocked(self):
        self.repo.write(f"registry/publishers/{AUTHOR}.json", publisher("octo-tools"))
        self.repo.write("plugins/octo-tools.clock/manifest.json",
                        {"id": "octo-tools.clock", "publisher": {"id": "deckpilot", "name": "DeckPilot"}})
        status, err = self.repo.check()
        self.assertEqual(status, 1)
        self.assertIn("plugins/octo-tools.clock/manifest.json: the publisher 'deckpilot' is reserved", err)

    def test_other_reserved_publishers_are_blocked(self):
        for namespace in ("knittingcode", "github", "work-session"):
            with self.subTest(namespace=namespace):
                self.repo.write(f"registry/publishers/{AUTHOR}.json", publisher(namespace))
                status, err = self.repo.check()
                self.assertEqual(status, 1)
                self.assertIn(f"the publisher {namespace!r} is reserved by DeckPilot", err)

    def test_community_cannot_add_first_party_plugins_to_the_owner_record(self):
        # The owner's deckpilot record on main grants a contributor nothing.
        self.repo.write(f"registry/publishers/{OWNER}.json", owner_record())
        self.repo.base = self.repo.commit("record on main")
        self.repo.write("registry/plugins/deckpilot.weather/plugin.json", {"id": "deckpilot.weather"})
        status, err = self.repo.check()
        self.assertEqual(status, 1)
        self.assertIn("registry/plugins/deckpilot.weather/plugin.json: the publisher 'deckpilot' is reserved", err)


def owner_record(**changes):
    record = publisher("deckpilot", login=OWNER, github_id=OWNER_ID)
    record["verified"] = True
    record.update(changes)
    return record


class Maintainer(unittest.TestCase):
    """The repository owner's path for DeckPilot's own plugins (B6)."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = Repo(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def add_first_party(self, record=None):
        self.repo.write(f"registry/publishers/{OWNER}.json", record or owner_record())
        self.repo.write("registry/plugins/deckpilot.weather/plugin.json", {"id": "deckpilot.weather"})
        self.repo.write("plugins/deckpilot.weather/build.json", {"id": "deckpilot.weather"})

    def test_owner_may_add_first_party_records(self):
        self.add_first_party()
        status, err = self.repo.check(author=OWNER, author_id=OWNER_ID)
        self.assertEqual(status, 0, err)

    def test_owner_login_with_another_id_is_a_contributor(self):
        self.add_first_party()
        status, err = self.repo.check(author=OWNER, author_id=1)
        self.assertEqual(status, 1)
        self.assertIn("only a maintainer may change this", err)
        self.assertIn("reserved for DeckPilot's first-party plugins", err)

    def test_first_party_record_must_be_the_owners_and_verified(self):
        for record in (owner_record(verified=False), owner_record(github_id=1),
                       owner_record(login="someone-else")):
            with self.subTest(record=record):
                path = f"registry/publishers/{record['login']}.json"
                self.repo.write(path, record)
                status, err = self.repo.check(author=OWNER, author_id=OWNER_ID)
                self.assertEqual(status, 1)
                self.assertIn(f"{path}: the publisher 'deckpilot' must be {OWNER} ({OWNER_ID}), verified", err)
                (self.repo.root / path).unlink()

    def test_bundled_ids_are_refused_even_for_the_owner(self):
        # Never a recipe; in the registry, the privileged (bundled-only)
        # plugins never, and an official one only with its candidate entry.
        self.add_first_party()
        for pid in BUNDLED:
            self.repo.write(f"registry/plugins/{pid}/plugin.json", {"id": pid})
            self.repo.write(f"registry/plugins/{pid}/versions/0.9.0.json", {"version": "0.9.0"})
            self.repo.write(f"plugins/{pid}/build.json", {"id": pid})
        status, err = self.repo.check(author=OWNER, author_id=OWNER_ID)
        self.assertEqual(status, 1)
        for pid in BUNDLED:
            self.assertIn(f"plugins/{pid}/build.json: {pid} is bundled with DeckPilot; it has no build recipe", err)
        for pid in BUNDLED_ONLY:
            self.assertIn(f"registry/plugins/{pid}/plugin.json: {pid} is bundled with DeckPilot only", err)
        for pid in OFFICIAL_MARKETPLACE:
            self.assertIn(f"registry/plugins/{pid}/versions/0.9.0.json: no candidate entry", err)

    def test_owner_may_promote_an_official_candidate(self):
        self.add_first_party()
        for pid in OFFICIAL_MARKETPLACE:
            self.repo.write(f"official/marketplace/{pid}/versions/0.9.0.json", {"version": "0.9.0"})
            self.repo.write(f"registry/plugins/{pid}/plugin.json", {"id": pid})
            self.repo.write(f"registry/plugins/{pid}/versions/0.9.0.json", {"version": "0.9.0"})
        status, err = self.repo.check(author=OWNER, author_id=OWNER_ID)
        self.assertEqual(status, 0, err)

    def test_privileged_official_plugins_are_never_promoted(self):
        self.add_first_party()
        for pid in BUNDLED_ONLY:
            self.repo.write(f"official/marketplace/{pid}/versions/0.9.0.json", {"version": "0.9.0"})
            self.repo.write(f"registry/plugins/{pid}/versions/0.9.0.json", {"version": "0.9.0"})
        status, err = self.repo.check(author=OWNER, author_id=OWNER_ID)
        self.assertEqual(status, 1)
        for pid in BUNDLED_ONLY:
            self.assertIn(f"{pid} is bundled with DeckPilot only", err)

    def test_bundled_ids_are_refused_for_contributors(self):
        self.repo.write(f"registry/publishers/{AUTHOR}.json", publisher("octo-tools"))
        self.repo.write("registry/plugins/deckpilot.jira/plugin.json", {"id": "deckpilot.jira"})
        self.repo.write("registry/plugins/deckpilot.git/plugin.json", {"id": "deckpilot.git"})
        status, err = self.repo.check()
        self.assertEqual(status, 1)
        self.assertIn("deckpilot.jira is an official plugin; only a maintainer promotes it", err)
        self.assertIn("deckpilot.git is bundled with DeckPilot only", err)

    def test_other_reserved_publishers_are_refused_for_the_owner(self):
        self.repo.write(f"registry/publishers/{OWNER}.json", publisher("github", login=OWNER, github_id=OWNER_ID))
        self.repo.write("registry/plugins/github.clock/plugin.json", {"id": "github.clock"})
        status, err = self.repo.check(author=OWNER, author_id=OWNER_ID)
        self.assertEqual(status, 1)
        self.assertIn(f"registry/publishers/{OWNER}.json: the publisher 'github' is reserved by DeckPilot", err)
        self.assertIn("registry/plugins/github.clock/plugin.json: the publisher 'github' is reserved by DeckPilot", err)

    def test_owner_may_remove_a_first_party_listing(self):
        self.add_first_party()
        self.repo.base = self.repo.commit("records on main")
        (self.repo.root / "plugins/deckpilot.weather/build.json").unlink()
        (self.repo.root / "registry/plugins/deckpilot.weather/plugin.json").unlink()
        status, err = self.repo.check(author=OWNER, author_id=OWNER_ID)
        self.assertEqual(status, 0, err)


class MirrorsDeckPilot(unittest.TestCase):
    def test_first_party_publisher_is_reserved(self):
        module = load_script()
        self.assertIn(module.FIRST_PARTY_PUBLISHER, module.RESERVED_PUBLISHERS)

    def test_reserved_publishers_equal_deckpilot(self):
        if not DECKPILOT:
            self.skipTest("no DeckPilot checkout given (--deckpilot DIR)")
        root = Path(DECKPILOT)
        identity = root / "crates/deckpilot-plugin-manifest/src/identity.rs"
        module = load_script()
        self.assertEqual(set(module.RESERVED_PUBLISHERS), deckpilot_list(identity, "RESERVED_PUBLISHERS"))
        text = identity.read_text(encoding="utf-8")
        self.assertIn(f'pub const FIRST_PARTY_PUBLISHER: &str = "{module.FIRST_PARTY_PUBLISHER}";', text)

    def test_bundled_ids_equal_deckpilot(self):
        if not DECKPILOT:
            self.skipTest("no DeckPilot checkout given (--deckpilot DIR)")
        bundled = deckpilot_list(Path(DECKPILOT) / "crates/deckpilot-plugin-bundled/src/lib.rs",
                                 "BUNDLED_PLUGIN_IDS")
        self.assertEqual(set(load_script().BUNDLED_PLUGIN_IDS), bundled)
        first_party = load_script(FIRST_PARTY_SCRIPT, "check_first_party")
        self.assertEqual(set(first_party.BUNDLED_PLUGIN_IDS), bundled)

    def test_bundled_only_ids_agree_with_official_entry(self):
        entry = load_script(SCRIPT.parent / "official-entry.py", "official_entry")
        module = load_script()
        self.assertEqual(module.BUNDLED_ONLY_IDS, entry.BUNDLED_ONLY_IDS)
        self.assertEqual(set(module.BUNDLED_ONLY_IDS), set(BUNDLED_ONLY))
        self.assertEqual(set(module.OFFICIAL_MARKETPLACE_IDS), set(OFFICIAL_MARKETPLACE))

    def test_bundled_ids_agree_between_scripts(self):
        first_party = load_script(FIRST_PARTY_SCRIPT, "check_first_party")
        self.assertEqual(load_script().BUNDLED_PLUGIN_IDS, first_party.BUNDLED_PLUGIN_IDS)


if __name__ == "__main__":
    if len(sys.argv) >= 3 and sys.argv[1] == "--deckpilot":
        DECKPILOT = sys.argv[2]
        del sys.argv[1:3]
    unittest.main(verbosity=2)
