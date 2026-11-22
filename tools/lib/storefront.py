"""Storefront input checks shared by the tools (stdlib only).

Used by tools/check-storefront.py, tools/check-submission.py and
tools/studio/studio.py. Import it with the directory of this file on
sys.path (the scripts run with `python3 -I`, which leaves their own
directory off it).

* SchemaSet: a small JSON Schema (2020-12) validator for the keywords the
  schemas in schemas/ use. The pinned Rust schema-check
  (tools/schema-check) stays the reference implementation of JSON Schema;
  tools/tests/test_storefront.py runs the shared fixtures through both.
* inspect_media / check_asset: a header and chunk walk of PNG, WebP and
  JPEG files, rule for rule DeckPilot's media.rs `inspect` (which is
  authoritative; ICC profiles allowed within its limits: JPEG APP2 64 KiB +
  1024, WebP ICCP 64 KiB, PNG iCCP 16 KiB compressed), plus the per-kind
  asset rules of docs/storefront.md. It never decompresses pixel data: a
  full decode is the pinned `deckpilot-registry storefront check`, which CI
  runs as soon as the pinned CLI has it.
* check_text: FORBIDDEN_CHARS (controls and invisible/format characters);
  the schemas' patterns hold its BMP part only.
* check_url / check_markdown: the URL and safe markdown rules.
* check_tree: every rule above plus the cross-file rules over
  registry/plugins/*/listing.json, their media/ and editorial/.
"""

import ipaddress
import json
import re
import socket
import struct
import unicodedata
import zlib
from pathlib import Path
from urllib.parse import urlsplit

LISTING_SCHEMA = "https://github.com/knittingcode/deckpilot-plugins/schemas/inputs/listing.schema.json"
EDITORIAL_SCHEMA = "https://github.com/knittingcode/deckpilot-plugins/schemas/inputs/editorial.schema.json"
LOCALES = ("en", "pl")
PLATFORMS = ("macos", "windows", "linux")
MEDIA_NAME = re.compile(r"^[a-z0-9-]{1,48}\.(png|webp|jpg)$")
MEDIA_FILE = re.compile(r"^media/([a-z0-9-]{1,48}\.(png|webp|jpg))$")
PLUGIN_ID = re.compile(r"^[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*\.[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*$")
# Every extension a contributor could use to smuggle an image or a vector
# file past review (check-submission.py, check-hygiene.py).
IMAGE_SUFFIXES = frozenset({".png", ".webp", ".jpg", ".jpeg", ".jpe", ".gif", ".svg", ".svgz", ".avif",
                            ".bmp", ".ico", ".tif", ".tiff", ".heic", ".heif", ".apng", ".jxl"})
MEDIA_SUFFIXES = frozenset({".png", ".webp", ".jpg"})

KIB, MIB = 1024, 1024 * 1024
MAX_PIXELS = 8_388_608
MAX_ICC = 64 * KIB
MAX_PLUGIN_MEDIA = 8 * MIB
MAX_EDITORIAL_MEDIA = 16 * MIB
# docs/storefront.md, "Asset rules".
ASSETS = {
    "icon": {"formats": ("png", "webp"), "ratios": ((1, 1),), "width": (256, 1024), "bytes": 256 * KIB,
             "label": "icon"},
    "screenshot": {"formats": ("png", "webp", "jpg"), "ratios": ((16, 10), (16, 9)), "width": (1280, 2560),
                   "bytes": MIB, "label": "screenshot"},
    "promo_art": {"formats": ("png", "webp", "jpg"), "ratios": ((2, 1),), "width": (1600, 3200),
                  "bytes": MIB, "label": "promo art"},
    "banner": {"formats": ("png", "webp", "jpg"), "ratios": ((3, 1),), "width": (1200, 3600),
               "bytes": MIB, "label": "banner art"},
}
RATIO_TOLERANCE = 0.01
FORMAT_NAMES = {"png": "PNG", "webp": "WebP", "jpg": "JPEG"}

# --------------------------------------------------------------------------
# JSON Schema (the subset of 2020-12 that schemas/ uses)
# --------------------------------------------------------------------------


def _pattern(text):
    # ECMA-262 `$` matches only at the very end; Python's also before a
    # final newline. The schemas anchor with a trailing `$` only.
    if text.endswith("$") and not text.endswith("\\$"):
        text = text[:-1] + r"\Z"
    return re.compile(text)


class SchemaSet:
    """Every *.schema.json below a directory, served by its $id."""

    def __init__(self, directory=None):
        self.docs = {}
        self._patterns = {}
        for path in sorted(Path(directory).rglob("*.schema.json")) if directory is not None else []:
            self._add(json.loads(path.read_text(encoding="utf-8")), path)

    @classmethod
    def from_docs(cls, docs):
        out = cls()
        for doc in docs:
            out._add(doc, "a schema")
        return out

    def _add(self, doc, where):
        sid = doc.get("$id") if isinstance(doc, dict) else None
        if not isinstance(sid, str):
            raise ValueError(f"{where} has no $id")
        if sid in self.docs:
            raise ValueError(f"two schemas have the $id {sid}")
        self.docs[sid] = doc

    def _resolve(self, ref, base):
        uri, _, fragment = ref.partition("#")
        uri = uri or base
        if uri not in self.docs:
            raise ValueError(f"unknown schema reference {ref}")
        node = self.docs[uri]
        for part in [p for p in fragment.split("/") if p]:
            part = part.replace("~1", "/").replace("~0", "~")
            node = node[int(part)] if isinstance(node, list) else node[part]
        return node, uri

    def validate(self, instance, ref):
        """A list of "<json pointer>: <message>" errors (empty: valid)."""
        errors = []
        schema, base = self._resolve(ref, None)
        self._check(instance, schema, base, "", errors)
        return errors

    def _re(self, text):
        if text not in self._patterns:
            self._patterns[text] = _pattern(text)
        return self._patterns[text]

    @staticmethod
    def _is_type(value, name):
        if name == "object":
            return isinstance(value, dict)
        if name == "array":
            return isinstance(value, list)
        if name == "string":
            return isinstance(value, str)
        if name == "boolean":
            return isinstance(value, bool)
        if name == "null":
            return value is None
        if name == "integer":
            return (isinstance(value, int) and not isinstance(value, bool)) or (
                isinstance(value, float) and value.is_integer())
        if name == "number":
            return isinstance(value, (int, float)) and not isinstance(value, bool)
        return False

    @staticmethod
    def _equal(a, b):
        if isinstance(a, bool) or isinstance(b, bool):
            return type(a) is type(b) and a == b
        if isinstance(a, (int, float)) and isinstance(b, (int, float)):
            return a == b
        return json.dumps(a, sort_keys=True) == json.dumps(b, sort_keys=True)

    def _ok(self, instance, schema, base):
        errors = []
        self._check(instance, schema, base, "", errors)
        return not errors

    def _check(self, inst, schema, base, path, errors):
        where = path or "/"
        if schema is True:
            return
        if schema is False:
            errors.append(f"{where}: not allowed")
            return
        if "$ref" in schema:
            target, tbase = self._resolve(schema["$ref"], base)
            self._check(inst, target, tbase, path, errors)
        if "type" in schema:
            types = schema["type"] if isinstance(schema["type"], list) else [schema["type"]]
            if not any(self._is_type(inst, t) for t in types):
                errors.append(f"{where}: expected {' or '.join(types)}")
                return
        if "const" in schema and not self._equal(inst, schema["const"]):
            errors.append(f"{where}: must be {json.dumps(schema['const'])}")
        if "enum" in schema and not any(self._equal(inst, e) for e in schema["enum"]):
            errors.append(f"{where}: must be one of {', '.join(json.dumps(e) for e in schema['enum'])}")
        if isinstance(inst, str):
            if "minLength" in schema and len(inst) < schema["minLength"]:
                errors.append(f"{where}: shorter than {schema['minLength']} character(s)")
            if "maxLength" in schema and len(inst) > schema["maxLength"]:
                errors.append(f"{where}: longer than {schema['maxLength']} characters ({len(inst)})")
            if "pattern" in schema and not self._re(schema["pattern"]).search(inst):
                errors.append(f"{where}: {json.dumps(inst[:60])} does not match {schema['pattern']}")
        if self._is_type(inst, "number"):
            if "minimum" in schema and inst < schema["minimum"]:
                errors.append(f"{where}: less than {schema['minimum']}")
            if "maximum" in schema and inst > schema["maximum"]:
                errors.append(f"{where}: greater than {schema['maximum']}")
        if isinstance(inst, dict):
            for name in schema.get("required", []):
                if name not in inst:
                    errors.append(f"{where}: {name!r} is required")
            props = schema.get("properties", {})
            for name, value in inst.items():
                sub = f"{path}/{name}"
                if name in props:
                    self._check(value, props[name], base, sub, errors)
                elif "additionalProperties" in schema:
                    extra = schema["additionalProperties"]
                    if extra is False:
                        errors.append(f"{sub}: unknown field {name!r}")
                    else:
                        self._check(value, extra, base, sub, errors)
                if "propertyNames" in schema:
                    name_errors = []
                    self._check(name, schema["propertyNames"], base, sub, name_errors)
                    if name_errors:
                        errors.append(f"{sub}: the key {name!r} is not allowed here")
            if "minProperties" in schema and len(inst) < schema["minProperties"]:
                errors.append(f"{where}: needs at least {schema['minProperties']} field(s)")
            if "maxProperties" in schema and len(inst) > schema["maxProperties"]:
                errors.append(f"{where}: at most {schema['maxProperties']} field(s)")
        if isinstance(inst, list):
            if "minItems" in schema and len(inst) < schema["minItems"]:
                errors.append(f"{where}: needs at least {schema['minItems']} item(s)")
            if "maxItems" in schema and len(inst) > schema["maxItems"]:
                errors.append(f"{where}: at most {schema['maxItems']} item(s) ({len(inst)})")
            if schema.get("uniqueItems"):
                seen = set()
                for item in inst:
                    key = json.dumps(item, sort_keys=True)
                    if key in seen:
                        errors.append(f"{where}: duplicate item {key}")
                    seen.add(key)
            if "items" in schema:
                for i, item in enumerate(inst):
                    self._check(item, schema["items"], base, f"{path}/{i}", errors)
        for sub in schema.get("allOf", []):
            self._check(inst, sub, base, path, errors)
        if "anyOf" in schema and not any(self._ok(inst, s, base) for s in schema["anyOf"]):
            errors.append(f"{where}: matches none of the allowed shapes")
        if "oneOf" in schema and sum(self._ok(inst, s, base) for s in schema["oneOf"]) != 1:
            errors.append(f"{where}: must match exactly one of the allowed shapes")
        if "not" in schema and self._ok(inst, schema["not"], base):
            errors.append(f"{where}: not allowed here")
        if "if" in schema:
            branch = "then" if self._ok(inst, schema["if"], base) else "else"
            if branch in schema:
                self._check(inst, schema[branch], base, path, errors)


# --------------------------------------------------------------------------
# Media: header and chunk walk (no pixel decode)
# --------------------------------------------------------------------------


class MediaError(ValueError):
    pass


# Rule for rule the walk of DeckPilot's crates/deckpilot-plugin-registry/
# src/media.rs (`inspect`, knittingcode/deckpilot#399 at 0a267f4e), which
# is authoritative; tools/tests/test_storefront.py has a case for every
# rule. Neither decodes pixels: the full decode is the Rust publishing
# tool's, in CI.
MAX_MEDIA_BYTES = MIB
MAX_PNG_ICCP = 16 * KIB          # a PNG iCCP chunk (zlib data, never inflated here)
MAX_JPEG_ICC = MAX_ICC + 1024    # the APP2 ICC_PROFILE segments together (with their headers)
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
PNG_ALLOWED = {b"IHDR", b"PLTE", b"tRNS", b"IDAT", b"IEND", b"sRGB", b"gAMA", b"cHRM", b"iCCP", b"pHYs"}
PNG_METADATA = {b"tEXt", b"zTXt", b"iTXt", b"eXIf"}
PNG_ANIMATION = {b"acTL", b"fcTL", b"fdAT"}
PNG_ANCILLARY_BEFORE_IDAT = {b"sRGB", b"gAMA", b"cHRM", b"pHYs"}


def sniff(data):
    """png, webp, jpg, or None. SVG and every other format is None."""
    if data.startswith(PNG_SIGNATURE):
        return "png"
    if data.startswith(b"\xff\xd8\xff"):
        return "jpg"
    if len(data) >= 12 and data[:4] == b"RIFF" and data[8:12] == b"WEBP":
        return "webp"
    return None


def _looks_like_svg(data):
    head = data[:1024].decode("utf-8", "replace").lower().lstrip("﻿").lstrip()
    return head.startswith("<?xml") or head.startswith("<svg") or "<svg" in head


def _png(data):
    pos, header, seen_plte, seen_idat, idat_done, ended = 8, None, False, False, False, False
    n = len(data)
    while pos < n:
        if pos + 8 > n:
            raise MediaError("PNG chunk header is truncated")
        (length,) = struct.unpack(">I", data[pos:pos + 4])
        if length > 0x7FFFFFFF:
            raise MediaError("PNG chunk length is out of range")
        ctype = data[pos + 4:pos + 8]
        end = pos + 8 + length
        if end + 4 > n:
            raise MediaError(f"truncated PNG chunk {ctype.decode('latin-1')}")
        body = data[pos + 8:end]
        (crc,) = struct.unpack(">I", data[end:end + 4])
        name = ctype.decode("latin-1")
        if zlib.crc32(ctype + body) & 0xFFFFFFFF != crc:
            raise MediaError(f"bad CRC in PNG chunk {name}")
        pos = end + 4
        if header is None and ctype != b"IHDR":
            raise MediaError("PNG does not start with IHDR")
        if ctype in PNG_ANIMATION:
            raise MediaError(f"animated PNG (APNG {name}) is refused")
        if ctype in PNG_METADATA:
            raise MediaError(f"PNG metadata chunk {name} is refused (privacy); strip it before committing")
        if ctype not in PNG_ALLOWED:
            raise MediaError(f"PNG chunk {name} is not allowed")
        if ctype != b"IDAT" and seen_idat:
            idat_done = True
        if ctype == b"IHDR":
            if header is not None:
                raise MediaError("second IHDR")
            if length != 13:
                raise MediaError("IHDR is not 13 bytes")
            width, height, depth, color, compression, filt, interlace = struct.unpack(">IIBBBBB", body)
            if color in (0, 3):
                depth_ok = depth in (1, 2, 4, 8)
            elif color in (2, 4, 6):
                depth_ok = depth == 8
            else:
                raise MediaError(f"PNG color type {color} is not defined")
            if depth == 16:
                raise MediaError("16-bit PNG is refused (8-bit channels only)")
            if not depth_ok:
                raise MediaError(f"PNG bit depth {depth} is not valid for color type {color}")
            if compression != 0 or filt != 0 or interlace > 1:
                raise MediaError("PNG IHDR names an undefined method")
            if width == 0 or height == 0 or width > 0x7FFFFFFF or height > 0x7FFFFFFF:
                raise MediaError(f"{width}x{height}: no empty side and at most {MAX_PIXELS} pixels")
            header = (width, height, color)
        elif ctype == b"PLTE":
            if header[2] in (0, 4):
                raise MediaError("PNG PLTE in a grayscale image")
            if seen_plte or seen_idat:
                raise MediaError("PNG PLTE is misplaced")
            if length == 0 or length % 3 or length > 768:
                raise MediaError("PNG PLTE has a bad length")
            seen_plte = True
        elif ctype == b"iCCP":
            if seen_idat or seen_plte:
                raise MediaError("PNG iCCP is misplaced (it precedes PLTE and IDAT)")
            if length > MAX_PNG_ICCP:
                raise MediaError(f"PNG iCCP chunk of {length} bytes; at most {MAX_PNG_ICCP} (compressed)")
        elif ctype == b"tRNS":
            if seen_idat:
                raise MediaError("PNG tRNS follows the image data")
        elif ctype == b"IDAT":
            if idat_done:
                raise MediaError("PNG IDAT chunks are not consecutive")
            seen_idat = True
        elif ctype == b"IEND":
            if length:
                raise MediaError("PNG IEND carries data")
            ended = True
            break
        elif seen_idat and ctype in PNG_ANCILLARY_BEFORE_IDAT:
            raise MediaError(f"PNG {name} follows the image data")
    if not ended:
        raise MediaError("PNG has no IEND")
    if pos != n:
        raise MediaError("data follows IEND")
    if not seen_idat:
        raise MediaError("PNG has no image data")
    if header[2] == 3 and not seen_plte:
        raise MediaError("palette PNG without PLTE")
    return header[0], header[1]


def _vp8_size(body):
    if len(body) < 10 or body[0] & 1 or body[3:6] != b"\x9d\x01\x2a":
        return None
    w, h = struct.unpack("<HH", body[6:10])
    return w & 0x3FFF, h & 0x3FFF


def _vp8l_size(body):
    if len(body) < 5 or body[0] != 0x2F:
        return None
    (bits,) = struct.unpack("<I", body[1:5])
    if bits >> 29:
        return None  # version must be 0
    return (bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1


def _webp_other(fourcc):
    if fourcc in (b"ANIM", b"ANMF"):
        raise MediaError(f"animated WebP ({fourcc.decode('latin-1')}) is refused")
    if fourcc in (b"EXIF", b"XMP "):
        raise MediaError(f"WebP metadata chunk {fourcc.decode('latin-1').strip()} is refused (privacy)")
    raise MediaError(f"WebP chunk {fourcc.decode('latin-1')!r} is not allowed")


def _webp(data):
    (riff_size,) = struct.unpack("<I", data[4:8])
    if riff_size + 8 != len(data):
        raise MediaError(f"WebP RIFF size {riff_size + 8} is not the file length {len(data)}")
    chunks, pos = [], 12
    while pos < len(data):
        if pos + 8 > len(data):
            raise MediaError("truncated WebP chunk header")
        fourcc = data[pos:pos + 4]
        (size,) = struct.unpack("<I", data[pos + 4:pos + 8])
        body = data[pos + 8:pos + 8 + size]
        if len(body) != size:
            raise MediaError(f"truncated WebP chunk {fourcc!r}")
        pos += 8 + size + (size & 1)
        if pos > len(data):
            raise MediaError("WebP chunk padding is missing")
        chunks.append((fourcc, body))
    if not chunks:
        raise MediaError("WebP has no chunk")
    first, body = chunks[0]
    if first in (b"VP8 ", b"VP8L"):
        if len(chunks) > 1:
            _webp_other(chunks[1][0])
        size = (_vp8_size if first == b"VP8 " else _vp8l_size)(body)
        if size is None:
            raise MediaError(f"bad {first.decode('latin-1').strip()} header")
        return size
    if first != b"VP8X":
        _webp_other(first)
    if len(body) != 10:
        raise MediaError("WebP VP8X is not 10 bytes")
    flags = body[0]
    if flags & 0x02:
        raise MediaError("animated WebP (VP8X animation flag) is refused")
    if flags & 0x08:
        raise MediaError("WebP declares EXIF metadata (privacy)")
    if flags & 0x04:
        raise MediaError("WebP declares XMP metadata (privacy)")
    if flags & 0xC1 or body[1:4] != b"\0\0\0":
        raise MediaError("WebP VP8X sets reserved bits")
    width = (body[4] | body[5] << 8 | body[6] << 16) + 1
    height = (body[7] | body[8] << 8 | body[9] << 16) + 1
    image, alpha = None, False
    for index, (fourcc, chunk) in enumerate(chunks[1:], 1):
        if fourcc == b"ICCP":
            if not flags & 0x20 or index != 1:
                raise MediaError("WebP ICCP is misplaced or not flagged")
            if len(chunk) > MAX_ICC:
                raise MediaError(f"WebP ICC profile of {len(chunk)} bytes; at most {MAX_ICC}")
        elif fourcc == b"ALPH":
            if image is not None or alpha:
                raise MediaError("WebP ALPH is misplaced")
            alpha = True
        elif fourcc in (b"VP8 ", b"VP8L"):
            if image is not None:
                raise MediaError("WebP has more than one image chunk")
            image = (_vp8_size if fourcc == b"VP8 " else _vp8l_size)(chunk)
            if image is None:
                raise MediaError("bad WebP image chunk header")
        else:
            _webp_other(fourcc)
    if image is None:
        raise MediaError("WebP has no image chunk")
    if image != (width, height):
        raise MediaError("WebP image size differs from the VP8X canvas")
    return width, height


# JPEG: besides the frame (SOF0 baseline or SOF2 progressive, exactly one,
# before any scan, 8-bit, 1 or 3 components, length 6 + 3 x components,
# height not in a DNL), the tables (DHT, DAC, DQT, DRI, DNL) and the scans,
# only: one plain JFIF APP0 (14 bytes, no thumbnail), the 12-byte Adobe
# APP14, and APP2 ICC_PROFILE segments of at most 64 KiB + 1024 bytes
# together. COM, APP1 (Exif, XMP), APP13 (IPTC), every other APPn and
# marker are refused, and nothing may follow EOI (an EOI+ZIP polyglot fails).
JPEG_TABLES = {0xC4, 0xCC, 0xDB, 0xDD, 0xDC}  # DHT, DAC, DQT, DRI, DNL


def _jpeg(data):
    pos, size, scans, icc, app0 = 2, None, 0, 0, False
    n = len(data)
    while True:
        if pos >= n or data[pos] != 0xFF:
            raise MediaError("JPEG marker expected")
        while pos < n and data[pos] == 0xFF:
            pos += 1
        if pos >= n:
            raise MediaError("the JPEG ends inside a marker")
        marker = data[pos]
        pos += 1
        if marker == 0xD9:
            break
        if marker == 0xD8:
            raise MediaError("second JPEG start of image")
        if marker == 0x01 or 0xD0 <= marker <= 0xD7:
            raise MediaError(f"stand-alone JPEG marker FF{marker:02X} out of place")
        if marker == 0x00:
            raise MediaError("stuffed JPEG byte outside the entropy data")
        if pos + 2 > n:
            raise MediaError("truncated JPEG segment")
        (length,) = struct.unpack(">H", data[pos:pos + 2])
        if length < 2:
            raise MediaError("JPEG segment length below 2")
        body = data[pos + 2:pos + length]
        if len(body) != length - 2:
            raise MediaError("truncated JPEG segment")
        pos += length
        if marker in (0xC0, 0xC2):
            if size is not None:
                raise MediaError("second JPEG frame header")
            if scans:
                raise MediaError("JPEG frame header follows a scan")
            if not body:
                raise MediaError("truncated JPEG frame header")
            if body[0] != 8:
                raise MediaError(f"{body[0]}-bit JPEG is refused (8-bit only)")
            if len(body) < 6:
                raise MediaError("truncated JPEG frame header")
            height, width, components = struct.unpack(">HHB", body[1:6])
            if components not in (1, 3):
                raise MediaError(f"JPEG with {components} components is refused (grayscale or YCbCr only)")
            if len(body) != 6 + 3 * components:
                raise MediaError("JPEG frame header has a bad length")
            if height == 0:
                raise MediaError("JPEG with its height in a DNL marker is refused")
            size = (width, height)
        elif 0xC0 <= marker <= 0xCF and marker not in JPEG_TABLES:
            raise MediaError(f"JPEG coding process SOF{marker - 0xC0}: only baseline or progressive")
        elif marker == 0xE1:
            raise MediaError("JPEG APP1 (Exif or XMP metadata) is refused (privacy)")
        elif marker == 0xED:
            raise MediaError("JPEG APP13 (IPTC metadata) is refused (privacy)")
        elif marker == 0xFE:
            raise MediaError("JPEG COM (comment) segments are refused")
        elif marker == 0xE2:
            if not body.startswith(b"ICC_PROFILE\0"):
                raise MediaError("JPEG APP2 other than ICC_PROFILE is refused")
            icc += len(body)
            if icc > MAX_JPEG_ICC:
                raise MediaError(f"JPEG ICC profile of more than {MAX_ICC} bytes")
        elif marker == 0xE0:
            if app0 or not body.startswith(b"JFIF\0") or len(body) != 14 or body[12] or body[13]:
                raise MediaError("JPEG APP0 other than one plain JFIF header (no thumbnail) is refused")
            app0 = True
        elif marker == 0xEE:
            if not body.startswith(b"Adobe") or len(body) != 12:
                raise MediaError("JPEG APP14 other than the 12-byte Adobe segment is refused")
        elif 0xE3 <= marker <= 0xEF:
            raise MediaError(f"JPEG APP{marker - 0xE0} is refused")
        elif marker in JPEG_TABLES:
            pass
        elif marker == 0xDA:
            if size is None:
                raise MediaError("JPEG scan before its frame header")
            scans += 1
            while True:  # entropy-coded data, up to the next real marker
                nxt = data.find(b"\xff", pos)
                if nxt < 0 or nxt + 1 >= n:
                    raise MediaError("the JPEG ends inside a scan (no EOI)")
                follow = data[nxt + 1]
                if follow == 0x00 or 0xD0 <= follow <= 0xD7:
                    pos = nxt + 2
                elif follow == 0xFF:
                    pos = nxt + 1
                else:
                    pos = nxt
                    break
        else:
            raise MediaError(f"JPEG marker FF{marker:02X} is not allowed")
    if pos != n:
        raise MediaError(f"{n - pos} byte(s) after the JPEG EOI are refused")
    if size is None:
        raise MediaError("JPEG has no frame header")
    if not scans:
        raise MediaError("JPEG has no scan")
    return size


def inspect_media(data):
    """{"format", "width", "height"} from the headers, or MediaError."""
    if len(data) > MAX_MEDIA_BYTES:
        raise MediaError(f"{len(data)} bytes; at most {MAX_MEDIA_BYTES}")
    fmt = sniff(data)
    if fmt is None:
        if _looks_like_svg(data):
            raise MediaError("SVG is refused; export PNG or WebP")
        raise MediaError("not a PNG, WebP or JPEG file")
    try:
        width, height = {"png": _png, "webp": _webp, "jpg": _jpeg}[fmt](data)
    except (struct.error, IndexError, TypeError) as e:
        raise MediaError(f"truncated or malformed {FORMAT_NAMES[fmt]} ({e})") from None
    if not width or not height or width * height > MAX_PIXELS:
        raise MediaError(f"{width}x{height}: no empty side and at most {MAX_PIXELS} pixels")
    return {"format": fmt, "width": width, "height": height}


def check_asset(kind, data, extension):
    """(info or None, [problems]) for one asset of the given kind."""
    rules = ASSETS[kind]
    problems = []
    if len(data) > rules["bytes"]:
        problems.append(f"{rules['label']} is {len(data)} bytes; at most {rules['bytes']}")
    try:
        info = inspect_media(data)
    except MediaError as e:
        return None, problems + [str(e)]
    if info["format"] != extension:
        problems.append(f"the bytes are {FORMAT_NAMES[info['format']]}, the file name says .{extension}")
    if info["format"] not in rules["formats"]:
        problems.append(f"{rules['label']} must be {' or '.join(FORMAT_NAMES[f] for f in rules['formats'])}")
    w, h = info["width"], info["height"]
    lo, hi = rules["width"]
    if not lo <= w <= hi:
        problems.append(f"{rules['label']} width {w} is outside {lo}-{hi}")
    if kind == "icon":
        if w != h:
            problems.append(f"icon must be square ({w}x{h})")
    elif not any(abs(w / h - a / b) <= RATIO_TOLERANCE * (a / b) for a, b in rules["ratios"]):
        wanted = " or ".join(f"{a}:{b}" for a, b in rules["ratios"])
        problems.append(f"{rules['label']} {w}x{h} is not {wanted} (within 1%)")
    return info, problems


# --------------------------------------------------------------------------
# Text, URL, markdown
# --------------------------------------------------------------------------

# Refused in every text: controls (line break only in multiline text) and
# invisible or format characters: soft hyphen, CGJ, Arabic letter mark,
# Hangul fillers, Khmer inherent vowels, Mongolian vowel separator,
# zero-width and directional marks, line/paragraph separators, bidi
# embeddings, overrides and isolates, word joiner, invisible operators,
# deprecated format characters, variation selectors, BOM, interlinear
# annotations; outside the BMP shorthand format controls, musical symbol
# format characters, tag characters and supplementary variation
# selectors. The schemas' `line` and `multiline` patterns hold exactly the
# BMP part (portable regex without the `u` flag); the astral part is
# refused here only (and by DeckPilot's check).
INVISIBLE_BMP = ("\u00ad\u034f\u061c\u115f\u1160\u17b4\u17b5\u180e\u200b-\u200f\u2028\u2029\u202a-\u202e"
                 "\u2060-\u2064\u2066-\u206f\u3164\ufe00-\ufe0f\ufeff\uffa0\ufff9-\ufffb")
INVISIBLE_ASTRAL = "\U0001bca0-\U0001bca3\U0001d173-\U0001d17a\U000e0000-\U000e007f\U000e0100-\U000e01ef"
CONTROLS = "\u0000-\u0009\u000b-\u001f\u007f-\u009f"
FORBIDDEN_CHARS = re.compile("[" + CONTROLS + INVISIBLE_BMP + INVISIBLE_ASTRAL + "]")
MD_HTML = re.compile(r"<[A-Za-z/!?]")
MD_HEADING = re.compile(r"^ {0,3}#{1,6}(\s|$)", re.M)
MD_IMAGE = re.compile(r"!\[")
MD_TABLE = re.compile(r"^\s*\|", re.M)
MD_NESTED_LIST = re.compile(r"^(?: {2,}|\t)\s*[-*+]\s", re.M)
# The only link form: [text](https://…) with no title, no parentheses or
# spaces in the URL. Every other `](`, reference definitions, reference
# links and autolinks are refused.
MD_LINK_OPEN = re.compile(r"\]\(")
MD_LINK_EXACT = re.compile(r"\]\((https://[^\s()<>\"'\\]*)\)")
MD_REFERENCE = re.compile(r"^ {0,3}\[[^\]\n]+\]:", re.M)
MD_REF_LINK = re.compile(r"\]\[")
MD_AUTOLINK = re.compile(r"<[a-zA-Z][a-zA-Z0-9+.-]*:")
PROMOTIONAL = re.compile(r"\bofficial\b|\bverified\b|\bdeckpilot\s+team\b", re.I)


def check_text(text, multiline=False):
    problems = []
    if not unicodedata.is_normalized("NFC", text):
        problems.append("is not Unicode NFC")
    bad = FORBIDDEN_CHARS.search(text) or (not multiline and "\n" in text)
    if bad:
        problems.append("holds a control, bidi-override or zero-width character")
    return problems


def _ip_like(host):
    """An IP address in any notation a resolver might accept: dotted quad,
    integer, octal or hex parts (inet_aton), or a numeric or hex-like last label."""
    try:
        ipaddress.ip_address(host)
        return True
    except ValueError:
        pass
    try:
        socket.inet_aton(host)
        return True
    except OSError:
        pass
    last = host.rsplit(".", 1)[-1]
    return last.isdigit() or bool(re.fullmatch(r"0x[0-9a-f]*", last))


def check_url(url, plugin_id=None):
    problems = []
    if len(url) > 512:
        problems.append("is longer than 512 characters")
    try:
        parts = urlsplit(url)
    except ValueError:
        return ["is not a URL"]
    if parts.scheme != "https":
        problems.append("must be https")
    netloc = parts.netloc
    if "@" in netloc:
        problems.append("must not hold user information")
    host = netloc
    if host.startswith("["):
        return problems + ["must not be an IP address"]
    if ":" in host:
        problems.append("must not name a port")
        host = host.split(":", 1)[0]
    if not re.fullmatch(r"[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+", host or ""):
        problems.append("needs a lowercase ASCII host name")
    else:
        if _ip_like(host):
            problems.append("must not be an IP address (in any notation)")
        if any(label.startswith("xn--") for label in host.split(".")):
            problems.append("must not be punycode (xn--)")
    segments = parts.path.split("/")
    if any(s in (".", "..") or re.fullmatch(r"(\.|%2e){1,2}", s, re.I) for s in segments) or re.search(
            r"%2e", parts.path, re.I):
        problems.append("must not hold dot segments (/./, /../, %2e)")
    if not re.fullmatch(r"[!#-~]*", url):
        problems.append("holds spaces or non-ASCII characters (percent-encode them)")
    if plugin_id and plugin_id.startswith("deckpilot."):
        if not ((host == "github.com" and parts.path.startswith("/knittingcode/")) or host == "knittingcode.github.io"):
            problems.append("of a deckpilot.* plugin must be on github.com/knittingcode/ or knittingcode.github.io")
    return problems


def check_markdown(text, plugin_id=None):
    problems = check_text(text, multiline=True)
    if MD_HTML.search(text):
        problems.append("holds raw HTML (`<` followed by a letter, /, ! or ?)")
    if MD_HEADING.search(text):
        problems.append("holds a heading (not in the safe markdown subset)")
    if MD_IMAGE.search(text):
        problems.append("holds an image (not in the safe markdown subset)")
    if MD_TABLE.search(text):
        problems.append("holds a table (not in the safe markdown subset)")
    if MD_NESTED_LIST.search(text):
        problems.append("holds a nested list (one level only)")
    for m in MD_LINK_OPEN.finditer(text):
        exact = MD_LINK_EXACT.match(text, m.start())
        if not exact:
            problems.append(f"link at character {m.start()} is not exactly [text](https://…) (no title, spaces "
                            "or parentheses)")
            continue
        for p in check_url(exact.group(1), plugin_id):
            problems.append(f"link {exact.group(1)[:80]!r} {p}")
    if MD_REFERENCE.search(text):
        problems.append("holds a link reference definition ([x]: …); only inline [text](https://…) links")
    if MD_REF_LINK.search(text):
        problems.append("holds a reference link ([text][ref]); only inline [text](https://…) links")
    if MD_AUTOLINK.search(text):
        problems.append("holds an autolink (<scheme:…>); only inline [text](https://…) links")
    return problems


# --------------------------------------------------------------------------
# The whole tree
# --------------------------------------------------------------------------


class Report:
    def __init__(self):
        self.errors = []
        self.warnings = []

    def error(self, where, message):
        self.errors.append(f"{where}: {message}")

    def warn(self, where, message):
        self.warnings.append(f"{where}: {message}")


def load_json(path, report, where):
    try:
        return json.loads(Path(path).read_bytes().decode("utf-8"))
    except (OSError, ValueError) as e:
        report.error(where, f"not valid JSON ({e})")
        return None


def localized_texts(doc, path=""):
    """(json pointer, field name, text) for every localized string."""
    if isinstance(doc, dict):
        if doc and "en" in doc and all(k in LOCALES for k in doc) and all(isinstance(v, str) for v in doc.values()):
            for k, v in doc.items():
                yield f"{path}/{k}", path.rsplit("/", 1)[-1], v
            return
        for k, v in doc.items():
            if k == "release_notes" and isinstance(v, dict):
                for ver, texts in v.items():
                    if isinstance(texts, dict):
                        for loc, t in texts.items():
                            if isinstance(t, str):
                                yield f"{path}/release_notes/{ver}/{loc}", "release_notes", t
                continue
            yield from localized_texts(v, f"{path}/{k}")
    elif isinstance(doc, list):
        for i, v in enumerate(doc):
            yield from localized_texts(v, f"{path}/{i}")


def _text_rules(doc, where, report, plugin_id=None):
    for pointer, field, text in localized_texts(doc):
        if field in ("description_md", "release_notes"):
            problems = check_markdown(text, plugin_id)
        else:
            problems = check_text(text)
        for p in problems:
            report.error(f"{where}{pointer}", p)


def _media_dir(directory, where, report):
    """{name: Path} of the regular files in a media/ directory."""
    files = {}
    if not directory.exists() and not directory.is_symlink():
        return files
    if directory.is_symlink() or not directory.is_dir():
        report.error(where, "must be a directory (not a symlink)")
        return files
    for entry in sorted(directory.iterdir()):
        rel = f"{where}/{entry.name}"
        if entry.is_symlink():
            report.error(rel, "symlinks are refused")
        elif not entry.is_file():
            report.error(rel, "media/ holds files only")
        elif entry.suffix.lower() in (".svg", ".svgz"):
            report.error(rel, "SVG is refused; export PNG or WebP")
        elif not MEDIA_NAME.match(entry.name):
            report.error(rel, "media file names are [a-z0-9-]{1,48}.(png|webp|jpg)")
        else:
            files[entry.name] = entry
    return files


def recorded_versions(plugin_dir):
    out = {}
    vdir = plugin_dir / "versions"
    for v in sorted(vdir.glob("*.json")) if vdir.is_dir() else []:
        try:
            doc = json.loads(v.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            continue
        out[v.stem] = doc if isinstance(doc, dict) else {}
    return out


def _assets(doc, report, where, media, kinds, owner=None):
    """Checks every referenced asset; returns (referenced names, total bytes)."""
    used, total = set(), 0
    for pointer, kind, ref in kinds(doc):
        if not isinstance(ref, str):
            continue
        m = MEDIA_FILE.match(ref)
        if not m:
            continue  # the schema reports it
        name = m.group(1)
        used.add(name)
        if name not in media:
            report.error(f"{where}{pointer}", f"{ref} does not exist")
            continue
        data = media[name].read_bytes()
        _, problems = check_asset(kind, data, m.group(2))
        for p in problems:
            report.error(f"{where}{pointer} ({ref})", p)
    for name, path in media.items():
        total += path.stat().st_size
    return used, total


def listing_assets(doc):
    if not isinstance(doc, dict):
        return
    if isinstance(doc.get("icon"), dict):
        yield "/icon/file", "icon", doc["icon"].get("file")
    for i, s in enumerate(doc.get("screenshots") or []):
        if isinstance(s, dict):
            yield f"/screenshots/{i}/file", "screenshot", s.get("file")
    if isinstance(doc.get("promo_art"), dict):
        yield "/promo_art/file", "promo_art", doc["promo_art"].get("file")


def editorial_assets(doc):
    if not isinstance(doc, dict):
        return
    for i, s in enumerate(doc.get("sections") or []):
        if not isinstance(s, dict):
            continue
        for j, item in enumerate(s.get("items") or []):
            if isinstance(item, dict) and isinstance(item.get("artwork"), dict):
                yield f"/sections/{i}/items/{j}/artwork/file", "banner", item["artwork"].get("file")


def check_listing(root, plugin_dir, report, schemas=None):
    """One registry/plugins/<id>/ directory's listing.json and media/."""
    pid = plugin_dir.name
    where = f"registry/plugins/{pid}"
    listing = plugin_dir / "listing.json"
    media = _media_dir(plugin_dir / "media", f"{where}/media", report)
    if listing.is_symlink():
        report.error(f"{where}/listing.json", "symlinks are refused")
        return None
    if not listing.is_file():
        if media:
            report.error(f"{where}/media", "media/ without a listing.json")
        return None
    doc = load_json(listing, report, f"{where}/listing.json")
    if doc is None:
        return None
    lw = f"{where}/listing.json"
    if schemas is not None:
        for e in schemas.validate(doc, LISTING_SCHEMA):
            report.error(lw, e)
    if not isinstance(doc, dict):
        return None
    if doc.get("id") != pid:
        report.error(lw, f"id {doc.get('id')!r} is not the directory name {pid}")
    if not (plugin_dir / "plugin.json").is_file():
        report.error(lw, "no plugin.json beside it")
    versions = recorded_versions(plugin_dir)
    for ver in (doc.get("release_notes") or {}) if isinstance(doc.get("release_notes"), dict) else {}:
        if ver not in versions:
            report.error(f"{lw}/release_notes/{ver}", f"{ver} is not a recorded version (versions/{ver}.json)")
    for name, url in (doc.get("links") or {}).items() if isinstance(doc.get("links"), dict) else []:
        if isinstance(url, str):
            for p in check_url(url, pid):
                report.error(f"{lw}/links/{name}", p)
    _text_rules(doc, lw, report, pid)
    if not pid.startswith("deckpilot."):
        for pointer, _, text in localized_texts(doc):
            if PROMOTIONAL.search(text):
                report.warn(f"{lw}{pointer}", "mentions 'official', 'verified' or 'DeckPilot team'; review it "
                            "(badges come from the client, never from listing text)")
    used, total = _assets(doc, report, lw, media, listing_assets)
    for name in sorted(set(media) - used):
        report.error(f"{where}/media/{name}", "not referenced by listing.json")
    if total > MAX_PLUGIN_MEDIA:
        report.error(f"{where}/media", f"{total} bytes of media; at most {MAX_PLUGIN_MEDIA} per plugin")
    return doc


def known_plugins(root):
    """{id: {"versions": {...}, "info": plugin.json or {}}} from registry/plugins/."""
    out = {}
    pdir = Path(root) / "registry" / "plugins"
    for d in sorted(pdir.iterdir()) if pdir.is_dir() else []:
        if not d.is_dir() or d.is_symlink():
            continue
        info = {}
        try:
            info = json.loads((d / "plugin.json").read_text(encoding="utf-8"))
        except (OSError, ValueError):
            pass
        out[d.name] = {"versions": recorded_versions(d), "info": info if isinstance(info, dict) else {}}
    return out


def check_editorial(root, report, schemas=None, plugins=None):
    root = Path(root)
    edir = root / "editorial"
    if edir.is_symlink():
        report.error("editorial", "must be a directory (not a symlink)")
        return None
    if not edir.exists():
        return None
    for entry in sorted(edir.iterdir()):
        if entry.name not in ("editorial.json", "media", "README.md"):
            report.error(f"editorial/{entry.name}", "editorial/ holds editorial.json, media/ and README.md only")
        elif entry.is_symlink():
            report.error(f"editorial/{entry.name}", "symlinks are refused")
    media = _media_dir(edir / "media", "editorial/media", report)
    path = edir / "editorial.json"
    if not path.is_file() or path.is_symlink():
        if media:
            report.error("editorial/media", "media/ without editorial/editorial.json")
        return None
    where = "editorial/editorial.json"
    doc = load_json(path, report, where)
    if doc is None:
        return None
    if schemas is not None:
        for e in schemas.validate(doc, EDITORIAL_SCHEMA):
            report.error(where, e)
    if not isinstance(doc, dict):
        return None
    plugins = known_plugins(root) if plugins is None else plugins
    categories = {c for p in plugins.values() for c in (p["info"].get("categories") or []) if isinstance(c, str)}
    sections = [s for s in doc.get("sections") or [] if isinstance(s, dict)]
    collections = [c for c in doc.get("collections") or [] if isinstance(c, dict)]
    collection_ids = {c.get("id"): c for c in collections}

    def unique(items, label, base):
        seen = set()
        for i, item in enumerate(items):
            iid = item.get("id")
            if iid in seen:
                report.error(f"{where}{base}/{i}/id", f"duplicate {label} id {iid!r}")
            seen.add(iid)

    def plugin_ref(pointer, pid, version=None):
        if not isinstance(pid, str):
            return
        if pid not in plugins:
            report.error(f"{where}{pointer}", f"{pid} is not in registry/plugins/")
        elif version is not None:
            v = plugins[pid]["versions"].get(version)
            if v is None:
                report.error(f"{where}{pointer}", f"{pid} has no recorded version {version}")
            elif v.get("yanked"):
                report.error(f"{where}{pointer}", f"{pid} {version} is yanked")
        if pid in plugins and not plugins[pid]["versions"]:
            report.warn(f"{where}{pointer}", f"{pid} has no recorded version yet; clients drop it until it is published")

    def window(pointer, w):
        if isinstance(w, dict) and isinstance(w.get("starts_at"), str) and isinstance(w.get("ends_at"), str):
            if w["ends_at"] <= w["starts_at"]:
                report.error(f"{where}{pointer}/window", "ends_at must be after starts_at")

    unique(sections, "section", "/sections")
    unique(collections, "collection", "/collections")
    for i, s in enumerate(sections):
        base = f"/sections/{i}"
        window(base, s.get("window"))
        items = [x for x in s.get("items") or [] if isinstance(x, dict)]
        unique(items, "item", f"{base}/items")
        for j, item in enumerate(items):
            t = item.get("target") if isinstance(item.get("target"), dict) else {}
            tp = f"{base}/items/{j}/target/id"
            if t.get("type") == "plugin":
                plugin_ref(tp, t.get("id"))
            elif t.get("type") == "collection":
                c = collection_ids.get(t.get("id"))
                if c is None:
                    report.error(f"{where}{tp}", f"no collection {t.get('id')!r}")
                elif s.get("status") == "published" and c.get("status") != "published":
                    report.warn(f"{where}{tp}", f"collection {t.get('id')!r} is not published; clients drop the item")
            elif t.get("type") == "category" and t.get("id") not in categories:
                report.warn(f"{where}{tp}", f"no plugin.json lists the category {t.get('id')!r}")
        for k, pid in enumerate(s.get("plugins") or []):
            plugin_ref(f"{base}/plugins/{k}", pid)
        if "plugin" in s:
            plugin_ref(f"{base}/plugin", s.get("plugin"), s.get("version"))
        for k, c in enumerate(s.get("categories") or []):
            if isinstance(c, dict) and c.get("id") not in categories:
                report.warn(f"{where}{base}/categories/{k}/id", f"no plugin.json lists the category {c.get('id')!r}")
    for i, c in enumerate(collections):
        window(f"/collections/{i}", c.get("window"))
        for k, pid in enumerate(c.get("plugins") or []):
            plugin_ref(f"/collections/{i}/plugins/{k}", pid)
    _text_rules(doc, where, report)
    used, total = _assets(doc, report, where, media, editorial_assets)
    for name in sorted(set(media) - used):
        report.error(f"editorial/media/{name}", "not referenced by editorial.json")
    if total > MAX_EDITORIAL_MEDIA:
        report.error("editorial/media", f"{total} bytes of media; at most {MAX_EDITORIAL_MEDIA}")
    return doc


def check_tree(root, schemas=None):
    """A Report over every listing and the editorial of a repository root."""
    root = Path(root)
    report = Report()
    pdir = root / "registry" / "plugins"
    for d in sorted(pdir.iterdir()) if pdir.is_dir() else []:
        if d.is_dir() and not d.is_symlink():
            check_listing(root, d, report, schemas)
    check_editorial(root, report, schemas)
    return report
