#!/usr/bin/env python3
"""Tests for tools/check-submission.py (stdlib only).

    python3 -I tools/tests/test_check_submission.py
    python3 -I tools/tests/test_check_submission.py --deckpilot DIR

Each case builds a scratch git repository with a base and a head commit and
runs the real script on it, as the Submission rules job does. With
--deckpilot (a DeckPilot checkout at the pinned commit), it also checks that
the script's RESERVED_PUBLISHERS equals DeckPilot's.
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
AUTHOR, AUTHOR_ID = "octo", 4242
DECKPILOT = None


def load_script():
    sys.dont_write_bytecode = True
    spec = importlib.util.spec_from_file_location("check_submission", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def deckpilot_reserved(identity_rs):
    text = Path(identity_rs).read_text(encoding="utf-8")
    block = re.search(r"pub const RESERVED_PUBLISHERS: &\[&str\] = &\[(.*?)\];", text, re.S)
    if not block:
        raise AssertionError(f"RESERVED_PUBLISHERS not found in {identity_rs}")
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

    def test_maintainer_is_not_blocked(self):
        # Reserving `deckpilot` in the registry itself is open (B6): the
        # rule binds community pull requests only.
        self.repo.write("registry/publishers/knittingcode.json",
                        publisher("deckpilot", login="knittingcode", github_id=1))
        self.repo.write("registry/plugins/deckpilot.github/plugin.json", {"id": "deckpilot.github"})
        status, err = self.repo.check(author="knittingcode", author_id=1)
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
        self.assertEqual(set(module.RESERVED_PUBLISHERS), deckpilot_reserved(identity))
        text = identity.read_text(encoding="utf-8")
        self.assertIn(f'pub const FIRST_PARTY_PUBLISHER: &str = "{module.FIRST_PARTY_PUBLISHER}";', text)


if __name__ == "__main__":
    if len(sys.argv) >= 3 and sys.argv[1] == "--deckpilot":
        DECKPILOT = sys.argv[2]
        del sys.argv[1:3]
    unittest.main(verbosity=2)
