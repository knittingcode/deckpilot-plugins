#!/usr/bin/env python3
"""Tests of tools/check-wasm-imports.py on hand-made wasm modules (stdlib
only; run with python3 -I).

    python3 -I tools/tests/test_check_wasm_imports.py
"""

import subprocess
import sys
import tempfile
from pathlib import Path

CHECK = Path(__file__).resolve().parents[1] / "check-wasm-imports.py"
passed = failed = 0


def leb(n):
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        out.append(b | (0x80 if n else 0))
        if not n:
            return bytes(out)


def name(s):
    b = s.encode()
    return leb(len(b)) + b


def section(sid, body):
    return bytes([sid]) + leb(len(body)) + body


def module(*imports, extra=b""):
    """A module with one type `() -> ()` and the given imports
    ((module, field, kind_bytes))."""
    types = section(1, leb(1) + b"\x60\x00\x00")
    body = leb(len(imports)) + b"".join(name(m) + name(f) + k for m, f, k in imports)
    return b"\0asm\x01\0\0\0" + types + section(2, body) + extra


FUNC = b"\x00\x00"  # kind func, type index 0
HOST = ("deckpilot_v1", "host_call", FUNC)


def case(label, data, want_ok, message="", args=()):
    global passed, failed
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "m.wasm"
        path.write_bytes(data)
        p = subprocess.run([sys.executable, "-I", str(CHECK), str(path), *args], capture_output=True, text=True)
    out = p.stdout + p.stderr
    if (p.returncode == 0) == want_ok and message in out:
        passed += 1
        print(f"ok   {label}")
    else:
        failed += 1
        print(f"FAIL {label}: exit {p.returncode}: {out.strip()}")


case("only the host ABI", module(HOST), True, "1 import(s), all allowed: deckpilot_v1.host_call")
case("no imports at all", module(), True, "0 import(s)")
case("no import section", b"\0asm\x01\0\0\0", True, "0 import(s)")
case("a WASI import", module(HOST, ("wasi_snapshot_preview1", "fd_write", FUNC)), False,
     "wasi_snapshot_preview1.fd_write (func)")
case("the C shim's clock import", module(HOST, ("env", "__rquickjs_host_now_us", FUNC)), False, "env.__rquickjs_host_now_us")
case("an imported memory", module(HOST, ("env", "memory", b"\x02\x00\x01")), False, "env.memory (memory)")
case("the host ABI's name as a global", module(("deckpilot_v1", "host_call", b"\x03\x7f\x00")), False, "(global)")
case("--print lists every import", module(HOST, ("env", "x", FUNC)), False, "deckpilot_v1.host_call func", args=["--print"])
case("--allow widens the list", module(("env", "x", FUNC)), True, "all allowed: env.x", args=["--allow", "env.x"])
case("not wasm", b"\x7fELF....", False, "not a wasm module")
case("two import sections", module(HOST) + section(2, leb(0)), False, "two import sections")
case("a truncated section", module(HOST)[:-3], False, "error")
case("an import section with trailing bytes", b"\0asm\x01\0\0\0" + section(2, leb(0) + b"\x00"), False,
     "does not match its imports")

print(f"\n{passed} passed, {failed} failed")
sys.exit(1 if failed else 0)
