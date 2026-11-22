"""Builds small PNG, WebP and JPEG files for the storefront tests (stdlib only).

The PNGs are complete, decodable images. The WebP and JPEG files are
header-correct (what tools/lib/storefront.py inspects) but carry no real
compressed image data; the full decode is the Rust check's job.
"""

import struct
import zlib


def chunk(ctype, body):
    return struct.pack(">I", len(body)) + ctype + body + struct.pack(">I", zlib.crc32(ctype + body) & 0xFFFFFFFF)


def png(width, height, depth=8, color=2, extra=(), after_iend=b"", rgb=(0x40, 0x80, 0xC0)):
    """A PNG; `extra` is a list of (type, body) chunks inserted before IDAT."""
    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[color]
    row = b"\0" + bytes(rgb[i % 3] for i in range(width * channels * max(depth // 8, 1)))
    data = zlib.compress(row * height, 9)
    out = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, depth, color, 0, 0, 0))
    for ctype, body in extra:
        out += chunk(ctype, body)
    return out + chunk(b"IDAT", data) + chunk(b"IEND", b"") + after_iend


def riff(chunks, size_delta=0):
    body = b"WEBP"
    for fourcc, data in chunks:
        body += fourcc + struct.pack("<I", len(data)) + data + (b"\0" if len(data) & 1 else b"")
    return b"RIFF" + struct.pack("<I", len(body) + size_delta) + body


def vp8l(width, height):
    bits = (width - 1) | (height - 1) << 14
    return b"\x2f" + struct.pack("<I", bits) + b"\0" * 8


def vp8(width, height):
    return b"\0\0\0" + b"\x9d\x01\x2a" + struct.pack("<HH", width, height) + b"\0" * 8


def vp8x(width, height, flags=0):
    w, h = width - 1, height - 1
    return bytes([flags, 0, 0, 0, w & 0xFF, w >> 8 & 0xFF, w >> 16, h & 0xFF, h >> 8 & 0xFF, h >> 16])


def webp(width, height, lossless=True, extended=False, flags=0, extra=(), size_delta=0):
    chunks = []
    if extended:
        chunks.append((b"VP8X", vp8x(width, height, flags)))
    chunks.extend(extra)
    chunks.append((b"VP8L", vp8l(width, height)) if lossless else (b"VP8 ", vp8(width, height)))
    return riff(chunks, size_delta)


def segment(marker, body):
    return bytes([0xFF, marker]) + struct.pack(">H", len(body) + 2) + body


def jpeg(width, height, sof=0xC0, precision=8, components=3, app=()):
    """A JPEG; `app` is a list of (marker, body) segments after SOI."""
    out = b"\xff\xd8" + segment(0xE0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0")
    for marker, body in app:
        out += segment(marker, body)
    comps = b"".join(bytes([i + 1, 0x11, 0]) for i in range(components))
    out += segment(sof, struct.pack(">BHHB", precision, height, width, components) + comps)
    out += segment(0xDA, bytes([components]) + b"".join(bytes([i + 1, 0]) for i in range(components)) + b"\0\x3f\0")
    return out + b"\x12\x34\x56" + b"\xff\xd9"
