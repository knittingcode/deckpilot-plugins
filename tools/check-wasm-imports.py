#!/usr/bin/env python3
"""The imports of a wasm module, and a check that it has only the host ABI's
(stdlib only; run with python3 -I).

    check-wasm-imports.py MODULE [--allow MODULE.NAME ...] [--print]

Parses the module's import section (WebAssembly core 1.0/2.0 binary
format) and fails unless every import is one of --allow (default: the
guest ABI 1's only import, `deckpilot_v1.host_call`, DeckPilot
plugins/runtime/crates/abi/src/guest.rs). A malformed module, a module
with two import sections, or any other import (WASI's
`wasi_snapshot_preview1.*`, the `env.*` of a C shim) fails. --print lists
every import as `module.name kind`, one per line, before checking.

The agent's host refuses such a module at load time anyway
(engine.rs); this runs where that host is not, on the build output of
official/, so a dependency that drags in an import (rquickjs-sys's
`env.__rquickjs_host_now_us`, a libc member with a WASI call) fails the
build here, not on a user's computer.
"""

import sys

ALLOWED = ("deckpilot_v1.host_call",)
KINDS = {0: "func", 1: "table", 2: "memory", 3: "global", 4: "tag"}


class Malformed(Exception):
    pass


def leb_u32(data, pos):
    result = shift = 0
    for _ in range(5):
        if pos >= len(data):
            raise Malformed("truncated LEB128")
        byte = data[pos]
        pos += 1
        result |= (byte & 0x7F) << shift
        if not byte & 0x80:
            return result, pos
        shift += 7
    raise Malformed("LEB128 longer than 5 bytes")


def name(data, pos):
    length, pos = leb_u32(data, pos)
    if pos + length > len(data):
        raise Malformed("truncated name")
    try:
        return data[pos:pos + length].decode("utf-8"), pos + length
    except UnicodeDecodeError:
        raise Malformed("a name is not UTF-8") from None


def skip_limits(data, pos):
    if pos >= len(data):
        raise Malformed("truncated limits")
    flags = data[pos]
    pos += 1
    _, pos = leb_u32(data, pos)
    if flags & 1:
        _, pos = leb_u32(data, pos)
    return pos


def imports(data):
    """[(module, name, kind)] of a wasm binary."""
    if data[:4] != b"\0asm" or data[4:8] != b"\x01\0\0\0":
        raise Malformed("not a wasm module (magic or version)")
    pos, found, sections = 8, [], 0
    while pos < len(data):
        section = data[pos]
        size, pos = leb_u32(data, pos + 1)
        end = pos + size
        if end > len(data):
            raise Malformed("a section runs past the end")
        if section == 2:
            sections += 1
            if sections > 1:
                raise Malformed("two import sections")
            count, p = leb_u32(data, pos)
            for _ in range(count):
                module, p = name(data, p)
                field, p = name(data, p)
                if p >= end:
                    raise Malformed("truncated import")
                kind = data[p]
                p += 1
                if kind == 0:
                    _, p = leb_u32(data, p)
                elif kind == 1:
                    p = skip_limits(data, p + 1)
                elif kind == 2:
                    p = skip_limits(data, p)
                elif kind == 3:
                    p += 2
                elif kind == 4:
                    _, p = leb_u32(data, p + 1)
                else:
                    raise Malformed(f"unknown import kind {kind}")
                found.append((module, field, KINDS[kind]))
            if p != end:
                raise Malformed("the import section's size does not match its imports")
        pos = end
    return found


def main(argv):
    if not argv or argv[0].startswith("-"):
        print(__doc__.strip().splitlines()[2].strip(), file=sys.stderr)
        return 2
    path, rest = argv[0], argv[1:]
    allowed, show = [], False
    while rest:
        flag = rest.pop(0)
        if flag == "--print":
            show = True
        elif flag == "--allow" and rest:
            allowed.append(rest.pop(0))
        else:
            print(f"unknown argument {flag}", file=sys.stderr)
            return 2
    allowed = tuple(allowed) or ALLOWED
    try:
        with open(path, "rb") as f:
            found = imports(f.read())
    except (OSError, Malformed) as e:
        print(f"error: {path}: {e}", file=sys.stderr)
        return 1
    if show:
        for module, field, kind in found:
            print(f"{module}.{field} {kind}")
    bad = [f"{m}.{n} ({k})" for m, n, k in found if f"{m}.{n}" not in allowed or k != "func"]
    if bad:
        print(f"error: {path} imports {', '.join(bad)}; only {', '.join(allowed)} (a function) is allowed",
              file=sys.stderr)
        return 1
    print(f"{path}: {len(found)} import(s), all allowed: {', '.join(f'{m}.{n}' for m, n, _ in found) or 'none'}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
