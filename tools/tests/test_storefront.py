#!/usr/bin/env python3
"""Tests for the storefront input schemas and tools/check-storefront.py (stdlib only).

    python3 -I tools/tests/test_storefront.py
    STOREFRONT_SCHEMA_CHECK=.work/bin/schema-check python3 -I tools/tests/test_storefront.py

* Schemas: every fixture under fixtures/storefront/<listing|editorial>/valid
  must pass and every one under .../invalid must fail, through the stdlib
  validator of tools/lib/storefront.py and, when STOREFRONT_SCHEMA_CHECK
  names the pinned schema-check (tools/build-cli.sh), through that too, so
  both implementations agree on the same fixtures.
* Media: the header walk on generated PNG, WebP and JPEG files: formats,
  dimensions, animation, metadata, CRCs, truncation, 16-bit, RIFF length,
  JPEG frame types, SVG.
* Text, URL and markdown rules, and check-storefront.py on scratch trees.
"""

import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
TOOLS = HERE.parent
REPO = TOOLS.parent
sys.path.insert(0, str(TOOLS / "lib"))
sys.path.insert(0, str(HERE))
import images  # noqa: E402
import storefront  # noqa: E402

FIXTURES = HERE / "fixtures" / "storefront"
SCHEMA_CHECK = os.environ.get("STOREFRONT_SCHEMA_CHECK")
SCRIPT = TOOLS / "check-storefront.py"
REFS = {"listing": storefront.LISTING_SCHEMA, "editorial": storefront.EDITORIAL_SCHEMA}


class Schemas(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.schemas = storefront.SchemaSet(REPO / "schemas")

    def fixtures(self, kind, verdict):
        files = sorted((FIXTURES / kind / verdict).glob("*.json"))
        self.assertTrue(files, f"no fixtures in {kind}/{verdict}")
        return files

    def test_valid_fixtures_pass(self):
        for kind in REFS:
            for path in self.fixtures(kind, "valid"):
                with self.subTest(fixture=f"{kind}/valid/{path.name}"):
                    errors = self.schemas.validate(json.loads(path.read_text()), REFS[kind])
                    self.assertEqual(errors, [])

    def test_invalid_fixtures_fail(self):
        for kind in REFS:
            for path in self.fixtures(kind, "invalid"):
                with self.subTest(fixture=f"{kind}/invalid/{path.name}"):
                    self.assertNotEqual(self.schemas.validate(json.loads(path.read_text()), REFS[kind]), [])

    def test_no_placement_field_in_listings(self):
        listing = json.loads((REPO / "schemas/inputs/listing.schema.json").read_text())
        self.assertIs(listing["additionalProperties"], False)
        for word in ("featured", "order", "collection", "collections", "badge", "badges", "official",
                     "verified", "section", "sections", "rank", "priority", "pinned"):
            self.assertNotIn(word, listing["properties"])
        for path in ("highlights/items", "icon", "screenshots/items", "promo_art", "links"):
            node = listing["properties"]
            for part in path.split("/"):
                node = node[part] if part != "items" else node["items"]
            self.assertIs(node["additionalProperties"], False, path)

    def test_closed_locale_set(self):
        listing = json.loads((REPO / "schemas/inputs/listing.schema.json").read_text())
        self.assertEqual(listing["$defs"]["locale"]["enum"], list(storefront.LOCALES))

    def test_editorial_has_no_external_url(self):
        text = (REPO / "schemas/inputs/editorial.schema.json").read_text()
        doc = json.loads(text)
        self.assertEqual(doc["$defs"]["target"]["properties"]["type"]["enum"], ["plugin", "collection", "category"])
        self.assertNotIn('"url"', text)
        self.assertNotIn('"href"', text)

    def test_listing_example_of_the_docs_is_valid(self):
        doc = json.loads((FIXTURES / "listing/valid/full.json").read_text())
        self.assertEqual(self.schemas.validate(doc, storefront.LISTING_SCHEMA), [])

    def test_schema_check_agrees(self):
        if not SCHEMA_CHECK:
            self.skipTest("STOREFRONT_SCHEMA_CHECK is not set (the pinned schema-check)")
        for kind, ref in REFS.items():
            for verdict in ("valid", "invalid"):
                for path in self.fixtures(kind, verdict):
                    with self.subTest(fixture=f"{kind}/{verdict}/{path.name}"):
                        result = subprocess.run([SCHEMA_CHECK, "--schemas", str(REPO / "schemas"), ref, str(path)],
                                                capture_output=True, text=True)
                        self.assertEqual(result.returncode == 0, verdict == "valid", result.stdout + result.stderr)


class Media(unittest.TestCase):
    def ok(self, data, fmt, w, h):
        self.assertEqual(storefront.inspect_media(data), {"format": fmt, "width": w, "height": h})

    def refused(self, data, text):
        with self.assertRaises(storefront.MediaError) as cm:
            storefront.inspect_media(data)
        self.assertIn(text, str(cm.exception))

    def test_formats_and_dimensions(self):
        self.ok(images.png(512, 512), "png", 512, 512)
        self.ok(images.png(16, 8, color=6), "png", 16, 8)
        self.ok(images.webp(1280, 800), "webp", 1280, 800)
        self.ok(images.webp(1280, 720, lossless=False), "webp", 1280, 720)
        self.ok(images.webp(1600, 800, extended=True), "webp", 1600, 800)
        self.ok(images.jpeg(1920, 1200), "jpg", 1920, 1200)
        self.ok(images.jpeg(1920, 1080, sof=0xC2, components=1), "jpg", 1920, 1080)

    def test_svg_is_refused(self):
        self.refused(b'<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg"/>', "SVG is refused")
        self.refused(b"  <svg viewBox='0 0 1 1'></svg>", "SVG is refused")

    def test_other_formats_are_refused(self):
        self.refused(b"GIF89a" + b"\0" * 20, "not a PNG, WebP or JPEG")
        self.refused(b"", "not a PNG, WebP or JPEG")

    def test_png_rules(self):
        self.refused(images.png(8, 8, depth=16), "16-bit PNG is refused")
        self.refused(images.png(8, 8, extra=[(b"acTL", b"\0" * 8)]), "animated PNG")
        for meta in (b"tEXt", b"zTXt", b"iTXt", b"eXIf"):
            with self.subTest(chunk=meta):
                self.refused(images.png(8, 8, extra=[(meta, b"Author\0me")]), "metadata chunk")
        self.refused(images.png(8, 8, extra=[(b"vpAg", b"\0" * 9)]), "is not allowed")
        self.refused(images.png(8, 8, after_iend=b"trailing"), "data follows IEND")
        self.refused(images.png(8, 8, extra=[(b"iCCP", b"x" * (64 * 1024 + 1))]), "ICC profile")
        self.ok(images.png(8, 8, extra=[(b"sRGB", b"\0"), (b"pHYs", b"\0" * 9)]), "png", 8, 8)
        good = bytearray(images.png(8, 8))
        good[30] ^= 0xFF  # inside IHDR: its CRC no longer matches
        self.refused(bytes(good), "bad CRC")
        self.refused(images.png(8, 8)[:40], "truncated")

    def test_webp_rules(self):
        self.refused(images.webp(64, 64, size_delta=4), "RIFF size")
        self.refused(images.webp(64, 64, extended=True, flags=0x02), "animated WebP")
        self.refused(images.webp(64, 64, extended=True, extra=[(b"ANIM", b"\0" * 6)]), "animated WebP")
        self.refused(images.webp(64, 64, extended=True, extra=[(b"EXIF", b"II*\0")]), "metadata chunk EXIF")
        self.refused(images.webp(64, 64, extended=True, extra=[(b"XMP ", b"<x/>")]), "metadata chunk XMP")
        self.refused(images.webp(64, 64, extended=True, flags=0x08), "EXIF or XMP")
        self.refused(images.riff([(b"VP8X", images.vp8x(8, 8))]), "exactly one image")

    def test_jpeg_rules(self):
        self.refused(images.jpeg(64, 64, app=[(0xE1, b"Exif\0\0II*\0")]), "APP1")
        self.refused(images.jpeg(64, 64, app=[(0xE1, b"http://ns.adobe.com/xap/1.0/\0<x/>")]), "APP1")
        self.refused(images.jpeg(64, 64, components=4), "4 components")
        self.refused(images.jpeg(64, 64, precision=12), "12-bit JPEG")
        self.refused(images.jpeg(64, 64, sof=0xC3), "baseline or progressive")
        self.refused(images.jpeg(64, 64)[:-2], "EOI")
        self.refused(images.jpeg(64, 64)[:30], "truncated")

    def test_jpeg_segments_allow_list(self):
        self.ok(images.jpeg(64, 64, app=[(0xEE, b"Adobe\0\x64\0\0\0\0\x01")]), "jpg", 64, 64)
        for app, text in [
            ([(0xFE, b"hello")], "COM"),
            ([(0xE2, b"ICC_PROFILE\0\x01\x01" + b"x" * 64)], "APP2"),
            ([(0xE0, b"JFXX\0\x10" + b"\0" * 8)], "APP0"),
            ([(0xEE, b"Adobe\0\x64\0\0\0\0\x01extra")], "APP14"),
            ([(0xEC, b"Ducky\0")], "APP12"),
        ]:
            with self.subTest(text=text):
                self.refused(images.jpeg(64, 64, app=app), text)
        # A JFIF APP0 with a thumbnail.
        jfif_thumb = images.jpeg(64, 64).replace(
            b"\xff\xe0\x00\x10JFIF\0\x01\x01\0\0\x01\0\x01\0\0",
            b"\xff\xe0\x00\x16JFIF\0\x01\x01\0\0\x01\0\x01\x01\x02" + b"\0" * 6)
        self.refused(jfif_thumb, "APP0")

    def test_nothing_after_eoi(self):
        zip_tail = b"PK\x03\x04" + b"\0" * 26 + b"PK\x05\x06" + b"\0" * 18
        self.refused(images.jpeg(64, 64) + zip_tail, "after the JPEG EOI")
        self.refused(images.jpeg(64, 64) + b"\0", "after the JPEG EOI")

    def test_jpeg_scans(self):
        # Stuffed bytes, restart markers and a second (progressive) scan with tables between.
        body = images.jpeg(64, 64, sof=0xC2)[:-2]
        scan2 = (images.segment(0xC4, b"\x00" + b"\x01" * 16 + b"\x00")
                 + images.segment(0xDA, b"\x01\x01\x00\x00\x3f\x00"))
        self.ok(body + b"\xff\x00\x12\xff\xd0\x34" + scan2 + b"\x56\xff\x00" + b"\xff\xd9", "jpg", 64, 64)

    def test_pixel_budget(self):
        self.refused(images.webp(4096, 4096), "pixels")

    def test_asset_rules(self):
        cases = [
            ("icon", images.png(512, 512), "png", []),
            ("icon", images.png(512, 400), "png", ["must be square"]),
            ("icon", images.png(128, 128), "png", ["outside 256-1024"]),
            ("icon", images.jpeg(512, 512), "jpg", ["icon must be PNG or WebP"]),
            ("icon", images.png(512, 512), "webp", ["the bytes are PNG"]),
            ("screenshot", images.webp(2560, 1600), "webp", []),
            ("screenshot", images.webp(1920, 1080), "webp", []),
            ("screenshot", images.webp(1280, 1024), "webp", ["is not 16:10 or 16:9"]),
            ("screenshot", images.webp(1024, 640), "webp", ["outside 1280-2560"]),
            ("promo_art", images.jpeg(1600, 800), "jpg", []),
            ("promo_art", images.jpeg(1600, 900), "jpg", ["is not 2:1"]),
            ("banner", images.webp(1800, 600), "webp", []),
            ("banner", images.webp(1800, 900), "webp", ["is not 3:1"]),
        ]
        for kind, data, ext, expected in cases:
            with self.subTest(kind=kind, expected=expected):
                _, problems = storefront.check_asset(kind, data, ext)
                self.assertEqual(len(problems), len(expected), problems)
                for text, problem in zip(expected, problems):
                    self.assertIn(text, problem)

    def test_byte_limits(self):
        big = images.png(512, 512, extra=[(b"pHYs", b"\0" * 9)] * 1)
        big = big[:-12] + b"".join(images.chunk(b"IDAT", b"\0" * 60000) for _ in range(5)) + big[-12:]
        _, problems = storefront.check_asset("icon", big, "png")
        self.assertTrue(any("at most 262144" in p for p in problems), problems)


class TextRules(unittest.TestCase):
    def test_text(self):
        self.assertEqual(storefront.check_text("Pogoda na Twoim decku"), [])
        self.assertEqual(storefront.check_text("Café"), ["is not Unicode NFC"])
        for bad in ("a\u202eb", "a\u2066b", "a\u200bb", "a\x07b", "a\nb", "a\U000e0041b", "a\u00adb", "a\u2028b",
                    "a\u2029b", "a\u115fb", "a\u1160b", "a\u3164b", "a\uffa0b", "a\ufe00b", "a\ufe0fb", "a\U000e0100b",
                    "a\U000e01efb", "a\u200eb", "a\u200fb", "a\u061cb", "a\u180eb", "a\u034fb", "a\ufff9b", "a\ufffbb"):
            with self.subTest(text=repr(bad)):
                self.assertTrue(storefront.check_text(bad))
        self.assertEqual(storefront.check_text("a\nb", multiline=True), [])
        self.assertTrue(storefront.check_text("a\u2028b", multiline=True))
        self.assertEqual(storefront.check_text("Zażółć gęślą jaźń \U0001F642"), [])

    def test_checker_and_schema_refuse_the_same_characters(self):
        schemas = storefront.SchemaSet(REPO / "schemas")
        line = {"listing_version": 1, "id": "acme.clock"}
        points = (list(range(0x0, 0x250)) + list(range(0x2000, 0x2070)) + list(range(0xFE00, 0xFE10))
                  + list(range(0xE0000, 0xE0080)) + [0x034F, 0x061C, 0x115F, 0x1160, 0x180E, 0x3164, 0xFFA0, 0xFEFF,
                                                      0xFFF9, 0xFFFA, 0xFFFB, 0xFFFC, 0xE0100, 0xE01EF, 0xE01F0, 0x1F642])
        for cp in points:
            text = f"a{chr(cp)}b"
            if cp == 0x0A or not storefront.unicodedata.is_normalized("NFC", text):
                continue
            with self.subTest(cp=f"U+{cp:04X}"):
                by_checker = bool(storefront.check_text(text))
                by_schema = bool(schemas.validate(dict(line, promo={"en": text}), storefront.LISTING_SCHEMA))
                self.assertEqual(by_checker, by_schema)

    def test_urls(self):
        ok = "https://github.com/knittingcode/deckpilot-plugins/issues"
        self.assertEqual(storefront.check_url(ok, "deckpilot.weather"), [])
        self.assertEqual(storefront.check_url("https://knittingcode.github.io/deckpilot-plugins/", "deckpilot.x"), [])
        self.assertEqual(storefront.check_url("https://example.com/docs", "acme.clock"), [])
        for url, text in [
            ("http://example.com/", "https"),
            ("https://user:pw@example.com/", "user information"),
            ("https://example.com:8443/", "port"),
            ("https://192.168.0.1/", "IP address"),
            ("https://[::1]/", "IP address"),
            ("https://Example.com/", "lowercase"),
            ("https://xn--80ak6aa92e.com/", "punycode"),
            ("https://example.com/" + "a" * 520, "512"),
            ("https://example.com/a b", "spaces"),
            ("https://0x7f.0.0.1/", "IP address"),
            ("https://0177.0.0.1/", "IP address"),
            ("https://2130706433.example.1/", "IP address"),
            ("https://127.1/", "IP address"),
            ("https://example.0x10/", "IP address"),
            ("https://example.com/a/./b", "dot segments"),
            ("https://example.com/a/../b", "dot segments"),
            ("https://example.com/a/%2e%2e/b", "dot segments"),
            ("https://example.com/a/%2E/b", "dot segments"),
        ]:
            with self.subTest(url=url):
                self.assertTrue(any(text in p for p in storefront.check_url(url, "acme.clock")),
                                storefront.check_url(url, "acme.clock"))
        self.assertTrue(storefront.check_url("https://example.com/", "deckpilot.weather"))
        self.assertTrue(storefront.check_url("https://github.com/someone/else", "deckpilot.weather"))

    def test_markdown(self):
        self.assertEqual(storefront.check_markdown("A **7-day** _forecast_ with `code`.\n\n- one\n- two\n"
                                                   "[docs](https://example.com/docs)"), [])
        for md, text in [
            ("<script>alert(1)</script>", "raw HTML"),
            ("<img src=x onerror=alert(1)>", "raw HTML"),
            ("<!-- c -->", "raw HTML"),
            ("</p>", "raw HTML"),
            ("# Title", "heading"),
            ("![x](https://example.com/x.png)", "image"),
            ("| a | b |", "table"),
            ("- a\n  - b", "nested list"),
            ("[x](javascript:alert(1))", "https"),
            ("[x](http://example.com)", "https"),
            ('[x](https://example.com "title")', "not exactly"),
            ("[x](https://example.com/a(b))", "not exactly"),
            ("[x]( https://example.com)", "not exactly"),
            ("[x](<https://example.com>)", "not exactly"),
            ("[x](https://example.com/a b)", "not exactly"),
            ("[x](//example.com)", "not exactly"),
            ("](javascript:alert(1))", "not exactly"),
            ("[x]: https://example.com", "reference definition"),
            ("[x][ref]", "reference link"),
            ("<https://example.com>", "autolink"),
            ("[x](https://127.1/)", "IP address"),
        ]:
            with self.subTest(md=md):
                self.assertTrue(any(text in p for p in storefront.check_markdown(md)), storefront.check_markdown(md))
        self.assertEqual(storefront.check_markdown("1 < 2 and 3 <= 4"), [])


def write(root, rel, content):
    p = Path(root) / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    if isinstance(content, bytes):
        p.write_bytes(content)
    else:
        p.write_text(content if isinstance(content, str) else json.dumps(content, indent=2) + "\n")
    return p


class Tree(unittest.TestCase):
    """check-storefront.py on a scratch repository root."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        shutil.copytree(REPO / "schemas", self.root / "schemas")
        for pid, versions in (("deckpilot.weather", ["0.2.1"]), ("deckpilot.pomodoro", ["0.2.1"]),
                              ("deckpilot.break-reminder", ["0.1.0"]), ("deckpilot.memory", ["0.1.0"]),
                              ("acme.clock", ["1.0.0"])):
            write(self.root, f"registry/plugins/{pid}/plugin.json", {"id": pid, "categories": ["monitoring"]})
            for v in versions:
                write(self.root, f"registry/plugins/{pid}/versions/{v}.json", {"version": v})

    def tearDown(self):
        self.tmp.cleanup()

    def run_check(self):
        env = {k: v for k, v in os.environ.items() if k != "GITHUB_STEP_SUMMARY"}
        r = subprocess.run([sys.executable, "-I", str(SCRIPT), "--root", str(self.root)],
                           capture_output=True, text=True, env=env)
        return r.returncode, r.stderr

    def full_listing(self):
        doc = json.loads((FIXTURES / "listing/valid/full.json").read_text())
        d = "registry/plugins/deckpilot.weather"
        write(self.root, f"{d}/listing.json", doc)
        write(self.root, f"{d}/media/icon.png", images.png(512, 512))
        write(self.root, f"{d}/media/forecast.png", images.png(1280, 800))
        write(self.root, f"{d}/media/promo.webp", images.webp(1600, 800))
        return doc

    def full_editorial(self):
        doc = json.loads((FIXTURES / "editorial/valid/full.json").read_text())
        write(self.root, "editorial/editorial.json", doc)
        write(self.root, "editorial/media/weather-banner.webp", images.webp(1800, 600))
        return doc

    def test_empty_tree_passes(self):
        self.assertEqual(self.run_check(), (0, ""))

    def test_full_listing_and_editorial_pass(self):
        self.full_listing()
        self.full_editorial()
        status, err = self.run_check()
        self.assertEqual(status, 0, err)

    def test_listing_rules(self):
        doc = self.full_listing()
        doc["id"] = "deckpilot.pomodoro"
        doc["release_notes"] = {"9.9.9": {"en": "Never released."}}
        doc["links"]["support"] = "https://example.com/support"
        doc["promo"]["en"] = "Café"
        doc["description_md"]["en"] = "<b>bold</b>"
        write(self.root, "registry/plugins/deckpilot.weather/listing.json", doc)
        write(self.root, "registry/plugins/deckpilot.weather/media/unused.png", images.png(512, 512))
        status, err = self.run_check()
        self.assertEqual(status, 1)
        for text in ("is not the directory name deckpilot.weather", "9.9.9 is not a recorded version",
                     "links/support: of a deckpilot.* plugin", "promo/en: is not Unicode NFC",
                     "description_md/en: holds raw HTML", "media/unused.png: not referenced"):
            self.assertIn(text, err)

    def test_media_rules_in_the_tree(self):
        self.full_listing()
        d = self.root / "registry/plugins/deckpilot.weather/media"
        (d / "icon.png").write_bytes(images.webp(512, 512))           # bytes do not match the name
        (d / "forecast.png").write_bytes(images.png(1280, 1024))      # wrong aspect
        (d / "promo.webp").unlink()                                   # missing
        write(self.root, "registry/plugins/deckpilot.weather/media/logo.svg", "<svg/>")
        status, err = self.run_check()
        self.assertEqual(status, 1)
        for text in ("the bytes are WebP, the file name says .png", "is not 16:10 or 16:9",
                     "media/promo.webp does not exist", "logo.svg: SVG is refused"):
            self.assertIn(text, err)

    def test_symlinks_are_refused(self):
        self.full_listing()
        d = self.root / "registry/plugins/deckpilot.weather/media"
        (d / "forecast.png").unlink()
        (d / "forecast.png").symlink_to(self.root / "schemas/inputs/listing.schema.json")
        status, err = self.run_check()
        self.assertEqual(status, 1)
        self.assertIn("forecast.png: symlinks are refused", err)

    def test_media_without_listing(self):
        write(self.root, "registry/plugins/acme.clock/media/icon.png", images.png(512, 512))
        status, err = self.run_check()
        self.assertEqual(status, 1)
        self.assertIn("media/ without a listing.json", err)

    def test_placement_field_in_listing_is_a_schema_error(self):
        doc = self.full_listing()
        doc["featured"] = True
        write(self.root, "registry/plugins/deckpilot.weather/listing.json", doc)
        status, err = self.run_check()
        self.assertEqual(status, 1)
        self.assertIn("unknown field 'featured'", err)

    def test_promotional_text_is_flagged_not_failed(self):
        write(self.root, "registry/plugins/acme.clock/listing.json",
              {"listing_version": 1, "id": "acme.clock", "promo": {"en": "The official clock, verified"}})
        status, err = self.run_check()
        self.assertEqual(status, 0, err)
        self.assertIn("warning: registry/plugins/acme.clock/listing.json/promo/en: mentions 'official'", err)

    def test_editorial_rules(self):
        doc = self.full_editorial()
        doc["sections"][1]["plugins"].append("acme.missing")
        doc["sections"][2]["version"] = "0.9.0"
        doc["sections"][0]["window"] = {"starts_at": "2027-01-15T00:00:00Z", "ends_at": "2026-12-01T00:00:00Z"}
        doc["sections"][5]["id"] = "focus"
        doc["sections"][0]["items"][0]["target"] = {"type": "collection", "id": "nope"}
        write(self.root, "editorial/editorial.json", doc)
        write(self.root, "editorial/notes.txt", "x")
        status, err = self.run_check()
        self.assertEqual(status, 1)
        for text in ("acme.missing is not in registry/plugins/", "has no recorded version 0.9.0",
                     "ends_at must be after starts_at", "duplicate section id 'focus'", "no collection 'nope'",
                     "editorial/notes.txt: editorial/ holds editorial.json"):
            self.assertIn(text, err)

    def test_banner_art_rules(self):
        self.full_editorial()
        (self.root / "editorial/media/weather-banner.webp").write_bytes(images.webp(1800, 1200))
        status, err = self.run_check()
        self.assertEqual(status, 1)
        self.assertIn("is not 3:1", err)


if __name__ == "__main__":
    unittest.main(verbosity=2)
