#!/usr/bin/env python3
"""Tests for tools/check-hygiene.py (stdlib only).

    python3 -I tools/tests/test_check_hygiene.py

Runs the real script on paths of a scratch directory, as the Repository
hygiene job runs it on the tracked files.
"""

import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent.parent / "check-hygiene.py"


class Hygiene(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def check(self, *paths):
        for path in paths:
            p = self.root / path
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(b"\x89PNG\r\n\x1a\n")
        r = subprocess.run([sys.executable, "-I", str(SCRIPT), *paths], cwd=self.root, capture_output=True, text=True,
                           env={k: v for k, v in os.environ.items() if k != "GITHUB_STEP_SUMMARY"})
        return r.returncode, r.stderr

    def test_media_in_the_media_dirs_passes(self):
        status, err = self.check("registry/plugins/acme.clock/media/icon.png",
                                 "registry/plugins/acme.clock/media/shot-1.webp",
                                 "registry/plugins/deckpilot.weather/media/promo.jpg",
                                 "editorial/media/launch-banner.webp")
        self.assertEqual(status, 0, err)

    def test_svg_is_refused_in_the_storefront(self):
        for path in ("registry/plugins/acme.clock/media/icon.svg", "editorial/media/banner.svg",
                     "registry/plugins/acme.clock/logo.svg", "editorial/art.SVG", "editorial/media/a.svgz"):
            with self.subTest(path=path):
                status, err = self.check(path)
                self.assertEqual(status, 1)
                self.assertIn(f"{path}: SVG is refused in the storefront", err)

    def test_images_outside_the_media_dirs_are_refused(self):
        for path in ("registry/plugins/acme.clock/icon.png", "registry/plugins/acme.clock/versions/x.webp",
                     "registry/media/icon.png", "editorial/banner.jpg", "registry/publishers/a.png"):
            with self.subTest(path=path):
                status, err = self.check(path)
                self.assertEqual(status, 1)
                self.assertIn(f"{path}: storefront images go only in registry/plugins/<id>/media/ or editorial/media/",
                              err)

    def test_media_dirs_hold_only_png_webp_jpg(self):
        for path in ("registry/plugins/acme.clock/media/icon.gif", "registry/plugins/acme.clock/media/shot.jpeg",
                     "registry/plugins/acme.clock/media/notes.txt", "registry/plugins/acme.clock/media/Icon.png",
                     "registry/plugins/acme.clock/media/sub/icon.png", "editorial/media/banner.avif",
                     "editorial/media/" + "a" * 49 + ".png"):
            with self.subTest(path=path):
                status, err = self.check(path)
                self.assertEqual(status, 1)
                self.assertIn(f"{path}: media/ holds only", err)

    def test_images_elsewhere_are_not_storefront_media(self):
        status, err = self.check("first-party/weather/assets/icon.png", "website/src/assets/brand/logo.svg",
                                 ".github/assets/banner.svg", "official/git/assets/icon.svg")
        self.assertEqual(status, 0, err)

    def test_key_files_are_still_refused(self):
        status, err = self.check("registry/plugins/acme.clock/media/index.key")
        self.assertEqual(status, 1)
        self.assertIn("key files are never committed", err)


if __name__ == "__main__":
    unittest.main(verbosity=2)
