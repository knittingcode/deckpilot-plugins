#!/usr/bin/env python3
"""The minisign key id of a signature or public key file (stdlib only; run
with python3 -I).

    python3 -I tools/lib/minisign_keyid.py FILE

prints the key id, 16 uppercase hex digits, exactly as minisign prints it
(the 8-byte key number as a little-endian integer, `%016X`). FILE is a
signature (`.minisig`) or a public key (`.pub`) as deckpilot-registry
writes them: base64 of the minisign text (also accepted unwrapped). The
key id names a release's signature files, `<id>-<version>.dpplugin.<KEYID>.minisig`
(official-release.yml); it is a label, never trust: release verify checks
every signature against the keys pinned in DeckPilot.
"""

import base64
import binascii
import sys

MAX_BYTES = 4096


class NotMinisign(ValueError):
    pass


def _text(data):
    if len(data) > MAX_BYTES:
        raise NotMinisign(f"{len(data)} bytes; a minisign file is at most {MAX_BYTES}")
    try:
        text = data.decode("ascii").strip()
    except UnicodeDecodeError:
        raise NotMinisign("not ASCII") from None
    if not text.startswith("untrusted comment: "):
        try:
            text = base64.b64decode(text, validate=True).decode("ascii")
        except (binascii.Error, UnicodeDecodeError):
            raise NotMinisign("neither minisign text nor base64 of it") from None
    lines = text.strip("\n").split("\n")
    if not lines[0].startswith("untrusted comment: ") or len(lines) < 2:
        raise NotMinisign("no minisign untrusted comment and key line")
    return lines


def key_id(data):
    """The key id of a minisign signature (4 lines, 74-byte signature line)
    or public key (2 lines, 42-byte key line)."""
    lines = _text(data)
    try:
        raw = base64.b64decode(lines[1].strip(), validate=True)
    except binascii.Error:
        raise NotMinisign("the key line is not base64") from None
    if len(lines) == 4 and lines[2].startswith("trusted comment: "):
        if len(raw) != 74 or raw[:2] not in (b"Ed", b"ED"):
            raise NotMinisign("not an Ed25519 minisign signature")
    elif len(lines) == 2:
        if len(raw) != 42 or raw[:2] != b"Ed":
            raise NotMinisign("not an Ed25519 minisign public key")
    else:
        raise NotMinisign("neither a minisign signature (4 lines) nor a public key (2 lines)")
    return f"{int.from_bytes(raw[2:10], 'little'):016X}"


def main(argv):
    if len(argv) != 2:
        print("usage: minisign_keyid.py FILE", file=sys.stderr)
        return 2
    try:
        with open(argv[1], "rb") as f:
            print(key_id(f.read(MAX_BYTES + 1)))
    except (OSError, NotMinisign) as e:
        print(f"error: {argv[1]}: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
