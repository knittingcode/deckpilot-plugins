#!/usr/bin/env python3
"""Tests of tools/compose-site.py (stdlib only; run with python3 -I)."""

import hashlib
import importlib.util
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("compose_site", Path(__file__).resolve().parent.parent / "compose-site.py")
compose_site = importlib.util.module_from_spec(spec)
sys.dont_write_bytecode = True
spec.loader.exec_module(compose_site)

REGISTRY = {
    "icon.svg": b"<svg/>",
    "v1/timestamp.json": b'{"seq": 1}\n',
    "v1/timestamp.json.minisig": b"signature\n",
    "v1/index/1.json": b'{"plugins": []}\n',
    "v1/plugins/acme.hello/abc.json": b"{}\n",
}
WEBSITE = {
    "index.html": b"<html>root</html>",
    "en/index.html": b"<html lang=en></html>",
    "pl/plugins/acme.hello/index.html": b"<html lang=pl></html>",
    "_astro/site.css": b"body{}",
    "404.html": b"<html>404</html>",
}


class ComposeTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def tree(self, name, files):
        base = self.root / name
        if base.exists():
            shutil.rmtree(base)
        base.mkdir()
        for path, data in files.items():
            (base / path).parent.mkdir(parents=True, exist_ok=True)
            (base / path).write_bytes(data)
        return base

    def run_compose(self, registry=None, website=None):
        return compose_site.compose(
            self.tree("registry", REGISTRY if registry is None else registry),
            self.tree("website", WEBSITE if website is None else website),
            self.root / "out",
        )

    def refused(self, message, **trees):
        with self.assertRaises(compose_site.ComposeError) as caught:
            self.run_compose(**trees)
        self.assertIn(message, str(caught.exception))

    def test_registry_is_copied_byte_for_byte_around_the_website(self):
        manifest, count = self.run_compose()
        self.assertEqual(count, len(WEBSITE))
        self.assertEqual(sorted(manifest), sorted(REGISTRY))
        for path, data in REGISTRY.items():
            self.assertEqual((self.root / "out" / path).read_bytes(), data)
            self.assertEqual(manifest[path], {"sha256": hashlib.sha256(data).hexdigest(), "size": len(data)})
        for path, data in WEBSITE.items():
            self.assertEqual((self.root / "out" / path).read_bytes(), data)
        self.assertEqual(len(compose_site.files_of(self.root / "out")), len(REGISTRY) + len(WEBSITE))

    def test_a_readme_only_registry_composes(self):
        manifest, _ = self.run_compose(registry={"icon.svg": b"<svg/>", "v1/README.txt": b"none yet\n"})
        self.assertEqual(sorted(manifest), ["icon.svg", "v1/README.txt"])

    def test_website_path_under_v1_is_refused_even_when_the_registry_has_no_such_file(self):
        self.refused("reserved registry prefix", website={**WEBSITE, "v1/extra.json": b"{}"})
        self.refused("reserved registry prefix", website={**WEBSITE, "V1/index.html": b"x"})

    def test_website_file_at_a_registry_path_is_refused(self):
        self.refused("collides with the registry path icon.svg", website={**WEBSITE, "icon.svg": b"<svg>other</svg>"})
        self.refused("collides with the registry path icon.svg", website={**WEBSITE, "ICON.SVG": b"x"})

    def test_website_cannot_turn_a_registry_file_or_directory_into_something_else(self):
        self.refused("under the registry file", website={**WEBSITE, "icon.svg/index.html": b"x"})
        registry = {**REGISTRY, "keys/1.json": b"{}"}
        self.refused("replace a registry directory", registry=registry, website={**WEBSITE, "keys": b"x"})

    def test_fixture_builds_are_refused(self):
        self.refused("test-fixture build", website={**WEBSITE, "FIXTURE_BUILD": b"x"})
        self.refused("test-fixture build", website={**WEBSITE, "en/index.html": b"<html data-fixture-build></html>"})

    def test_symlinks_are_refused(self):
        website = self.tree("website", WEBSITE)
        (website / "link.html").symlink_to(website / "index.html")
        with self.assertRaises(compose_site.ComposeError):
            compose_site.compose(self.tree("registry", REGISTRY), website, self.root / "out")

    def test_an_empty_or_v1_less_registry_and_a_rootless_website_are_refused(self):
        self.refused("registry tree is empty", registry={})
        self.refused("nothing under v1/", registry={"icon.svg": b"<svg/>"})
        self.refused("no index.html", website={"en/index.html": b"x"})

    def test_a_previous_output_is_replaced_not_merged(self):
        (self.root / "out" / "v1").mkdir(parents=True)
        (self.root / "out" / "v1" / "stale.json").write_bytes(b"old")
        self.run_compose()
        self.assertFalse((self.root / "out" / "v1" / "stale.json").exists())

    def test_a_registry_file_changed_during_the_copy_is_detected(self):
        original = compose_site.shutil.copyfile

        def tamper(source, target):
            original(source, target)
            if str(target).endswith("timestamp.json"):
                Path(target).write_bytes(b"tampered")

        compose_site.shutil.copyfile = tamper
        try:
            self.refused("registry paths changed")
        finally:
            compose_site.shutil.copyfile = original


if __name__ == "__main__":
    unittest.main()
