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
import zlib
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
    """One case per rule of DeckPilot's media.rs `inspect` (see the PR's rules table)."""

    def ok(self, data, fmt, w, h):
        self.assertEqual(storefront.inspect_media(data), {"format": fmt, "width": w, "height": h})

    def refused(self, data, text):
        with self.assertRaises(storefront.MediaError) as cm:
            storefront.inspect_media(data)
        self.assertIn(text, str(cm.exception))

    # -- all formats

    def test_formats_and_dimensions(self):
        self.ok(images.png(512, 512), "png", 512, 512)
        self.ok(images.png(16, 8, color=6), "png", 16, 8)
        self.ok(images.webp(1280, 800), "webp", 1280, 800)
        self.ok(images.webp(1280, 720, lossless=False), "webp", 1280, 720)
        self.ok(images.webp(1600, 800, extended=True), "webp", 1600, 800)
        self.ok(images.jpeg(1920, 1200), "jpg", 1920, 1200)
        self.ok(images.jpeg(1920, 1080, sof=0xC2, components=1), "jpg", 1920, 1080)

    def test_size_and_pixel_limits(self):
        self.refused(images.png(8, 8) + b"\0" * (1024 * 1024), "at most 1048576")
        self.refused(images.webp(4096, 4096), "pixels")
        self.refused(images.jpeg(64, 0), "DNL")
        self.refused(images.jpeg(0, 64), "no empty side")

    def test_svg_and_other_formats_are_refused(self):
        self.refused(b'<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg"/>', "SVG is refused")
        self.refused(b"\xef\xbb\xbf  <svg viewBox='0 0 1 1'></svg>", "SVG is refused")
        self.refused(b"GIF89a" + b"\0" * 20, "not a PNG, WebP or JPEG")
        self.refused(b"", "not a PNG, WebP or JPEG")

    # -- PNG

    def ihdr(self, w=8, h=8, depth=8, color=2, comp=0, filt=0, interlace=0):
        return struct.pack(">IIBBBBB", w, h, depth, color, comp, filt, interlace)

    def raw_png(self, *chunks):
        return b"\x89PNG\r\n\x1a\n" + b"".join(images.chunk(c, b) for c, b in chunks)

    def test_png_ihdr(self):
        idat = (b"IDAT", zlib.compress(b"\0" * 25))
        self.refused(images.png(8, 8, depth=16), "16-bit PNG is refused")
        self.refused(self.raw_png((b"IHDR", self.ihdr(color=2, depth=4)), idat, (b"IEND", b"")), "not valid for color type")
        self.refused(self.raw_png((b"IHDR", self.ihdr(color=5)), idat, (b"IEND", b"")), "color type 5")
        self.refused(self.raw_png((b"IHDR", self.ihdr(comp=1)), idat, (b"IEND", b"")), "undefined method")
        self.refused(self.raw_png((b"IHDR", self.ihdr(interlace=2)), idat, (b"IEND", b"")), "undefined method")
        self.refused(self.raw_png((b"IHDR", self.ihdr(w=0)), idat, (b"IEND", b"")), "no empty side")
        self.refused(self.raw_png((b"IHDR", self.ihdr() + b"\0"), idat, (b"IEND", b"")), "not 13 bytes")
        self.refused(self.raw_png((b"IHDR", self.ihdr()), (b"IHDR", self.ihdr()), idat, (b"IEND", b"")), "second IHDR")
        self.refused(self.raw_png((b"sRGB", b"\0"), (b"IHDR", self.ihdr()), idat, (b"IEND", b"")), "start with IHDR")
        self.ok(self.raw_png((b"IHDR", self.ihdr(depth=1, color=0)), idat, (b"IEND", b"")), "png", 8, 8)

    def test_png_chunks(self):
        self.refused(images.png(8, 8, extra=[(b"acTL", b"\0" * 8)]), "animated PNG")
        for meta in (b"tEXt", b"zTXt", b"iTXt", b"eXIf"):
            with self.subTest(chunk=meta):
                self.refused(images.png(8, 8, extra=[(meta, b"Author\0me")]), "metadata chunk")
        self.refused(images.png(8, 8, extra=[(b"tIME", b"\0" * 7)]), "is not allowed")
        self.refused(images.png(8, 8, extra=[(b"vpAg", b"\0" * 9)]), "is not allowed")
        self.ok(images.png(8, 8, extra=[(b"sRGB", b"\0"), (b"gAMA", b"\0" * 4), (b"cHRM", b"\0" * 32),
                                        (b"pHYs", b"\0" * 9)]), "png", 8, 8)

    def test_png_iccp(self):
        self.ok(images.png(8, 8, extra=[(b"iCCP", b"sRGB\0\0" + b"x" * 16000)]), "png", 8, 8)
        self.refused(images.png(8, 8, extra=[(b"iCCP", b"x" * (16 * 1024 + 1))]), "at most 16384 (compressed)")
        self.refused(images.png(8, 8, color=3, extra=[(b"PLTE", b"\0" * 3), (b"iCCP", b"x")]), "iCCP is misplaced")
        png = images.png(8, 8)
        self.refused(png[:-12] + images.chunk(b"iCCP", b"x") + png[-12:], "iCCP is misplaced")

    def test_png_order(self):
        png = images.png(8, 8)
        idat_end, iend = png[:-12], png[-12:]
        self.refused(idat_end + images.chunk(b"tRNS", b"\0" * 6) + iend, "tRNS follows the image data")
        self.refused(idat_end + images.chunk(b"pHYs", b"\0" * 9) + iend, "pHYs follows the image data")
        # IDAT must be consecutive: every chunk allowed between two IDATs is itself refused there first
        # (as in media.rs).
        self.refused(idat_end + images.chunk(b"sRGB", b"\0") + images.chunk(b"IDAT", zlib.compress(b"")) + iend,
                     "follows the image data")
        self.ok(idat_end + images.chunk(b"IDAT", b"") + iend, "png", 8, 8)
        self.ok(images.png(8, 8, extra=[(b"tRNS", b"\0" * 6)]), "png", 8, 8)

    def test_png_plte(self):
        self.ok(images.png(8, 8, color=3, extra=[(b"PLTE", b"\0" * 3)]), "png", 8, 8)
        self.refused(images.png(8, 8, color=3), "palette PNG without PLTE")
        self.refused(images.png(8, 8, color=0, extra=[(b"PLTE", b"\0" * 3)]), "PLTE in a grayscale image")
        self.refused(images.png(8, 8, color=4, extra=[(b"PLTE", b"\0" * 3)]), "PLTE in a grayscale image")
        for bad in (b"", b"\0" * 4, b"\0" * 771):
            with self.subTest(length=len(bad)):
                self.refused(images.png(8, 8, color=3, extra=[(b"PLTE", bad)]), "PLTE has a bad length")
        self.refused(images.png(8, 8, color=2, extra=[(b"PLTE", b"\0" * 3), (b"PLTE", b"\0" * 3)]), "PLTE is misplaced")
        png = images.png(8, 8, color=2)
        self.refused(png[:-12] + images.chunk(b"PLTE", b"\0" * 3) + png[-12:], "PLTE is misplaced")

    def test_png_container(self):
        self.refused(images.png(8, 8, after_iend=b"trailing"), "data follows IEND")
        self.refused(images.png(8, 8)[:-12] + images.chunk(b"IEND", b"x"), "IEND carries data")
        self.refused(images.png(8, 8)[:-12], "no IEND")
        self.refused(self.raw_png((b"IHDR", self.ihdr()), (b"IEND", b"")), "no image data")
        good = bytearray(images.png(8, 8))
        good[30] ^= 0xFF  # inside IHDR: its CRC no longer matches
        self.refused(bytes(good), "bad CRC")
        self.refused(images.png(8, 8)[:40], "truncated")

    # -- WebP

    def test_webp_simple(self):
        self.refused(images.webp(64, 64, size_delta=4), "RIFF size")
        self.refused(images.riff([(b"VP8L", images.vp8l(8, 8)), (b"ICCP", b"x")]), "'ICCP' is not allowed")
        self.refused(images.riff([(b"VP8L", images.vp8l(8, 8)), (b"EXIF", b"x")]), "metadata chunk EXIF")
        vp8l = bytearray(images.vp8l(8, 8))
        vp8l[4] |= 0x20  # version bits
        self.refused(images.riff([(b"VP8L", bytes(vp8l))]), "bad VP8L header")
        vp8 = bytearray(images.vp8(8, 8))
        vp8[0] |= 1  # not a key frame
        self.refused(images.riff([(b"VP8 ", bytes(vp8))]), "bad VP8 header")
        self.refused(images.riff([(b"ALPH", b"x")]), "'ALPH' is not allowed")
        self.refused(images.riff([]), "no chunk")
        self.refused(images.riff([(b"VP8L", images.vp8l(8, 8) + b"\0")])[:-1], "RIFF size")

    def test_webp_extended(self):
        self.refused(images.webp(64, 64, extended=True, flags=0x02), "animation flag")
        self.refused(images.webp(64, 64, extended=True, extra=[(b"ANIM", b"\0" * 6)]), "animated WebP")
        self.refused(images.webp(64, 64, extended=True, extra=[(b"EXIF", b"II*\0")]), "metadata chunk EXIF")
        self.refused(images.webp(64, 64, extended=True, extra=[(b"XMP ", b"<x/>")]), "metadata chunk XMP")
        self.refused(images.webp(64, 64, extended=True, flags=0x08), "EXIF metadata")
        self.refused(images.webp(64, 64, extended=True, flags=0x04), "XMP metadata")
        for flags in (0x01, 0x40, 0x80):
            self.refused(images.webp(64, 64, extended=True, flags=flags), "reserved bits")
        self.refused(images.riff([(b"VP8X", images.vp8x(8, 8) + b"\0"), (b"VP8L", images.vp8l(8, 8))]),
                     "not 10 bytes")
        self.refused(images.riff([(b"VP8X", images.vp8x(8, 8))]), "no image chunk")
        self.refused(images.riff([(b"VP8X", images.vp8x(8, 8)), (b"VP8L", images.vp8l(8, 8)),
                                  (b"VP8L", images.vp8l(8, 8))]), "more than one image chunk")
        self.refused(images.riff([(b"VP8X", images.vp8x(8, 8)), (b"VP8L", images.vp8l(9, 8))]), "differs")
        self.ok(images.riff([(b"VP8X", images.vp8x(8, 8, 0x10)), (b"ALPH", b"\0"), (b"VP8 ", images.vp8(8, 8))]),
                "webp", 8, 8)
        self.refused(images.riff([(b"VP8X", images.vp8x(8, 8)), (b"VP8 ", images.vp8(8, 8)), (b"ALPH", b"\0")]),
                     "ALPH is misplaced")

    def test_webp_icc(self):
        ok = images.riff([(b"VP8X", images.vp8x(8, 8, 0x20)), (b"ICCP", b"x" * 1000), (b"VP8L", images.vp8l(8, 8))])
        self.ok(ok, "webp", 8, 8)
        self.refused(images.riff([(b"VP8X", images.vp8x(8, 8)), (b"ICCP", b"x"), (b"VP8L", images.vp8l(8, 8))]),
                     "not flagged")
        self.refused(images.riff([(b"VP8X", images.vp8x(8, 8, 0x30)), (b"ALPH", b"\0"), (b"ICCP", b"x"),
                                  (b"VP8L", images.vp8l(8, 8))]), "misplaced")
        self.refused(images.riff([(b"VP8X", images.vp8x(8, 8, 0x20)), (b"ICCP", b"x" * (64 * 1024 + 1)),
                                  (b"VP8L", images.vp8l(8, 8))]), "at most 65536")

    # -- JPEG

    def test_jpeg_frame(self):
        self.refused(images.jpeg(64, 64, components=4), "4 components")
        self.refused(images.jpeg(64, 64, precision=12), "12-bit JPEG")
        for sof in (0xC1, 0xC3, 0xC9):
            self.refused(images.jpeg(64, 64, sof=sof), "only baseline or progressive")
        bad_len = images.jpeg(64, 64).replace(b"\xff\xc0\x00\x11", b"\xff\xc0\x00\x12", 1)
        bad_len = bad_len.replace(b"\x01\x11\x00\x02\x11\x00\x03\x11\x00", b"\x01\x11\x00\x02\x11\x00\x03\x11\x00\x00", 1)
        self.refused(bad_len, "frame header has a bad length")
        two = images.jpeg(64, 64)
        sof = two[two.index(b"\xff\xc0"):two.index(b"\xff\xda")]
        self.refused(two.replace(sof, sof + sof, 1), "second JPEG frame header")
        scan_first = two.replace(sof, b"", 1)
        self.refused(scan_first, "scan before its frame header")

    def test_jpeg_segments(self):
        self.ok(images.jpeg(64, 64, app=[(0xEE, b"Adobe\0\x64\0\0\0\0\x01")]), "jpg", 64, 64)
        self.ok(images.jpeg(64, 64, app=[(0xE2, b"ICC_PROFILE\0\x01\x01" + b"x" * 1000)]), "jpg", 64, 64)
        self.ok(images.jpeg(64, 64, app=[(0xE2, b"ICC_PROFILE\0\x01\x02" + b"x" * 60000),
                                         (0xE2, b"ICC_PROFILE\0\x02\x02" + b"x" * 6000)]), "jpg", 64, 64)
        for app, text in [
            ([(0xFE, b"hello")], "COM"),
            ([(0xE1, b"Exif\0\0II*\0")], "APP1"),
            ([(0xE1, b"http://ns.adobe.com/xap/1.0/\0<x/>")], "APP1"),
            ([(0xED, b"Photoshop 3.0\0")], "APP13"),
            ([(0xE2, b"MPF\0")], "APP2 other than ICC_PROFILE"),
            ([(0xE2, b"ICC_PROFILE\0\x01\x02" + b"x" * 60000), (0xE2, b"ICC_PROFILE\0\x02\x02" + b"x" * 6600)],
             "ICC profile of more than 65536"),
            ([(0xE0, b"JFXX\0\x10" + b"\0" * 8)], "APP0"),
            ([(0xE0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0")], "APP0"),  # a second APP0
            ([(0xEE, b"Adobe\0\x64\0\0\0\0\x01extra")], "APP14"),
            ([(0xEC, b"Ducky\0")], "APP12"),
            ([(0xE3, b"x")], "APP3"),
            ([(0xDE, b"x")], "FFDE"),
        ]:
            with self.subTest(text=text):
                self.refused(images.jpeg(64, 64, app=app), text)
        jfif_thumb = images.jpeg(64, 64).replace(
            b"\xff\xe0\x00\x10JFIF\0\x01\x01\0\0\x01\0\x01\0\0",
            b"\xff\xe0\x00\x16JFIF\0\x01\x01\0\0\x01\0\x01\x01\x02" + b"\0" * 6)
        self.refused(jfif_thumb, "APP0")
        self.ok(images.jpeg(64, 64, app=[(0xDD, b"\0\x10"), (0xDB, b"\0" * 65)]), "jpg", 64, 64)

    def test_jpeg_container(self):
        zip_tail = b"PK\x03\x04" + b"\0" * 26 + b"PK\x05\x06" + b"\0" * 18
        self.refused(images.jpeg(64, 64) + zip_tail, "after the JPEG EOI")
        self.refused(images.jpeg(64, 64) + b"\0", "after the JPEG EOI")
        self.refused(images.jpeg(64, 64)[:-2], "no EOI")
        self.refused(images.jpeg(64, 64)[:30], "truncated")
        self.refused(b"\xff\xd8\xff\xd8" + images.jpeg(64, 64)[2:], "second JPEG start of image")
        self.refused(b"\xff\xd8\xff\xd0" + images.jpeg(64, 64)[2:], "out of place")
        self.refused(b"\xff\xd8\xff\x00" + images.jpeg(64, 64)[2:], "stuffed JPEG byte")
        self.refused(b"\xff\xd8\xff\xd9", "no frame header")
        no_scan = images.jpeg(64, 64)
        self.refused(no_scan[:no_scan.index(b"\xff\xda")] + b"\xff\xd9", "no scan")

    def test_jpeg_scans(self):
        # Stuffed bytes, restart markers and a second (progressive) scan with tables between.
        body = images.jpeg(64, 64, sof=0xC2)[:-2]
        scan2 = (images.segment(0xC4, b"\x00" + b"\x01" * 16 + b"\x00")
                 + images.segment(0xDA, b"\x01\x01\x00\x00\x3f\x00"))
        self.ok(body + b"\xff\x00\x12\xff\xd0\x34" + scan2 + b"\x56\xff\x00" + b"\xff\xd9", "jpg", 64, 64)

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
        for bad in ("a\u17b4b", "a\u17b5b", "a\u206ab", "a\u206fb", "a\U0001d173b", "a\U0001bca0b", "a\u202eb", "a\u2066b", "a\u200bb", "a\x07b", "a\nb", "a\U000e0041b", "a\u00adb", "a\u2028b",
                    "a\u2029b", "a\u115fb", "a\u1160b", "a\u3164b", "a\uffa0b", "a\ufe00b", "a\ufe0fb", "a\U000e0100b",
                    "a\U000e01efb", "a\u200eb", "a\u200fb", "a\u061cb", "a\u180eb", "a\u034fb", "a\ufff9b", "a\ufffbb"):
            with self.subTest(text=repr(bad)):
                self.assertTrue(storefront.check_text(bad))
        self.assertEqual(storefront.check_text("a\nb", multiline=True), [])
        self.assertTrue(storefront.check_text("a\u2028b", multiline=True))
        self.assertEqual(storefront.check_text("Zażółć gęślą jaźń \U0001F642"), [])

    def test_checker_and_schema_refuse_the_same_bmp_characters(self):
        # The schema patterns are BMP-only (portable); the checker refuses the same BMP set plus astral ones.
        schemas = storefront.SchemaSet(REPO / "schemas")
        line = {"listing_version": 1, "id": "acme.clock"}
        for cp in range(0x0, 0x10000):
            if 0xD800 <= cp <= 0xDFFF or cp == 0x0A:
                continue
            text = f"a{chr(cp)}b"
            if not storefront.unicodedata.is_normalized("NFC", text):
                continue
            by_checker = bool(storefront.check_text(text))
            by_pattern = not storefront._pattern(schemas.docs[storefront.LISTING_SCHEMA]["$defs"]["line"]["pattern"]).search(text)
            if by_checker != by_pattern:
                self.fail(f"U+{cp:04X}: checker {'refuses' if by_checker else 'accepts'}, schema "
                          f"{'refuses' if by_pattern else 'accepts'}")

    def test_astral_refusals_are_the_checkers(self):
        schemas = storefront.SchemaSet(REPO / "schemas")
        for cp in (0x1BCA0, 0x1BCA3, 0x1D173, 0x1D17A, 0xE0000, 0xE0041, 0xE007F, 0xE0100, 0xE01EF):
            with self.subTest(cp=f"U+{cp:04X}"):
                text = f"a{chr(cp)}b"
                self.assertTrue(storefront.check_text(text))
                self.assertEqual(schemas.validate({"listing_version": 1, "id": "acme.clock", "promo": {"en": text}},
                                                  storefront.LISTING_SCHEMA), [])
        for cp in (0x1BCA4, 0x1D17B, 0xE0080, 0xE01F0, 0x1F642):
            self.assertEqual(storefront.check_text(f"a{chr(cp)}b"), [])
        for path in sorted((FIXTURES / "listing/checker-invalid").glob("*.json")):
            with self.subTest(fixture=path.name):
                doc = json.loads(path.read_text())
                self.assertEqual(schemas.validate(doc, storefront.LISTING_SCHEMA), [])
                self.assertTrue(any(storefront.check_text(t) for _, _, t in storefront.localized_texts(doc)))

    def test_schema_patterns_compile_as_ecmascript(self):
        node = shutil.which("node")
        if not node:
            self.skipTest("node is not installed")
        patterns = []
        for path in sorted((REPO / "schemas/inputs").glob("*.schema.json")):
            def walk(v):
                if isinstance(v, dict):
                    for k, x in v.items():
                        if k == "pattern" and isinstance(x, str):
                            patterns.append(x)
                        walk(x)
                elif isinstance(v, list):
                    for x in v:
                        walk(x)
            walk(json.loads(path.read_text()))
        script = (
            "const ps = JSON.parse(require('fs').readFileSync(0, 'utf8'));"
            "for (const p of ps) { new RegExp(p); new RegExp(p, 'u'); }"
            "const line = new RegExp(ps.find(p => p.includes('u00AD')));"
            "if (line.test('a\\u00ADb') || line.test('a\\u206Ab') || !line.test('Za\\u017C\\u00F3\\u0142\\u0107'))"
            " { throw new Error('line pattern behaves differently in ECMAScript'); }"
            "console.log(ps.length);")
        r = subprocess.run([node, "-e", script], input=json.dumps(patterns), capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(int(r.stdout.strip()), len(patterns))

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
