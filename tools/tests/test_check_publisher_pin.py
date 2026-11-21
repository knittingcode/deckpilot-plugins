#!/usr/bin/env python3
"""Tests of tools/check-publisher-pin.py (stdlib only; run with python3 -I):
DeckPilot pins at least two distinct keys with a threshold of at least 2,
and the verify jobs and tools/official.sh compare against exactly that
file. Copies of this repository's workflows and tools, edited per case; no
network.

    python3 -I tools/tests/test_check_publisher_pin.py
"""

import base64
import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CHECK = ROOT / "tools/check-publisher-pin.py"
FILES = [".github/workflows/official-release.yml", ".github/workflows/official-test-sign.yml",
         "tools/official.sh", "tools/build-cli.sh"]
KEYS = "crates/deckpilot-plugin-registry/publisher/first-party.json"

passed = failed = 0


def key(kid, seed=None):
    """A base64-wrapped minisign public key with key id `kid`."""
    body = bytes([seed if seed is not None else int(kid[:2], 16)]) * 32
    raw = b"Ed" + bytes.fromhex(kid)[::-1] + body
    text = f"untrusted comment: minisign public key: {kid}\n{base64.b64encode(raw).decode()}\n"
    return base64.b64encode(text.encode()).decode()


A, B, C = key("BDCDC80985EB60B5"), key("8C3C4E42700FA702"), key("1111111111111111")


def doc(keys=(A, B), threshold=2, revoked=(), **extra):
    d = {"publisherKeysVersion": 1, "publisher": "deckpilot", "keys": list(keys), "threshold": threshold,
         "revokedKeys": list(revoked)}
    d.update(extra)
    return json.dumps(d, indent=2) + "\n"


def case(name, ok, message="", keys=None, edit=None):
    """`keys`: the first-party.json text (default: two keys, threshold 2);
    `edit`: {relative path: function(text) -> text}."""
    global passed, failed
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp) / "repo"
        for rel in FILES:
            (root / rel).parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(ROOT / rel, root / rel)
        for rel, fn in (edit or {}).items():
            (root / rel).write_text(fn((root / rel).read_text()))
        dp = Path(tmp) / "deckpilot"
        if keys is not False:
            (dp / KEYS).parent.mkdir(parents=True)
            (dp / KEYS).write_text(doc() if keys is None else keys)
        p = subprocess.run([sys.executable, "-I", str(CHECK), "--root", str(root), "--deckpilot", str(dp)],
                           capture_output=True, text=True)
    out = p.stdout + p.stderr
    if (p.returncode == 0) == ok and re.search(message, out):
        passed += 1
        print(f"ok   {name}")
    else:
        failed += 1
        print(f"FAIL {name}: exit {p.returncode}: {out.strip()}")


def sub(pattern, repl, count=1):
    def fn(text):
        new, n = re.subn(pattern, repl, text, count=count, flags=re.M)
        assert n, pattern
        return new
    return fn


case("two distinct keys, threshold 2: passes, with the hash and key ids", True,
     r"sha256 [0-9a-f]{64}: keys 8C3C4E42700FA702, BDCDC80985EB60B5, threshold 2")
case("three keys, threshold 2", True, "threshold 2", keys=doc(keys=(A, B, C)))
case("no pinned DeckPilot checkout", False, "missing; run tools/official.sh fetch-deckpilot", keys=False)
case("no key pinned", False, "0 distinct key", keys=doc(keys=(), threshold=1))
case("one key, threshold 1", False, "threshold 1: an official release needs a threshold of at least 2",
     keys=doc(keys=(A,), threshold=1))
case("two keys, threshold 1", False, "threshold of at least 2", keys=doc(threshold=1))
case("threshold 2 of one key", False, "threshold 2 for 1 key", keys=doc(keys=(A,)))
case("threshold as a string", False, "threshold '2'", keys=doc(threshold="2"))
case("threshold true", False, "threshold True", keys=doc(threshold=True))
case("the same key twice", False, "listed twice", keys=doc(keys=(A, A)))
case("two key ids for one Ed25519 key", False, "listed twice",
     keys=doc(keys=(key("BDCDC80985EB60B5", 7), key("8C3C4E42700FA702", 7))))
case("one key id for two Ed25519 keys", False, "listed twice",
     keys=doc(keys=(key("BDCDC80985EB60B5", 1), key("BDCDC80985EB60B5", 2))))
case("a pinned key also revoked", False, "both pinned and revoked", keys=doc(keys=(A, B), revoked=(B,)))
case("a key that is not a minisign public key", False, "not a base64-wrapped minisign public key",
     keys=doc(keys=(A, "bm90IGEga2V5")))
case("another publisher", False, "publisher 'acme'", keys=doc().replace('"deckpilot"', '"acme"'))
case("another version", False, "publisherKeysVersion 2", keys=doc().replace('"publisherKeysVersion": 1', '"publisherKeysVersion": 2'))
case("not JSON", False, "first-party.json", keys="{")

RELEASE, TEST = FILES[0], FILES[1]
case("official-release.yml passes --keys", False, "official-release.yml: `--keys`",
     edit={RELEASE: sub(r'(tools/official.sh release-verify "\$ID")', r'\1 --keys k.json')})
case("official-test-sign.yml passes --keys", False, "official-test-sign.yml: `--keys`",
     edit={TEST: sub(r'(tools/official.sh test-verify "\$RUN")', r'\1 --keys k.json')})
case("official-release.yml's verify does not fetch the pinned DeckPilot", False, "does not fetch the pinned DeckPilot",
     edit={RELEASE: sub(r"run: tools/official.sh fetch-deckpilot$", "run: echo skipped")})
case("official-test-sign.yml's verify does not fetch the pinned DeckPilot", False,
     "official-test-sign.yml: job verify does not fetch",
     edit={TEST: sub(r"run: tools/official.sh fetch-deckpilot$", "run: echo skipped")})
case("official-release.yml's verify does not run release-verify", False, "does not run tools/official.sh release-verify",
     edit={RELEASE: sub(r"run: tools/official.sh release-verify ", "run: tools/other.sh release-verify ")})
case("official-test-sign.yml's verify does not run test-verify", False, "does not run tools/official.sh test-verify",
     edit={TEST: sub(r"run: tools/official.sh test-verify ", "run: tools/other.sh test-verify ")})
case("official.sh reads another keys file", False, "pinned_keys_file does not read",
     edit={"tools/official.sh": sub(r"local pinned=\$DECKPILOT_DIR/crates", "local pinned=$REPO_ROOT/crates")})
case("official.sh accepts threshold 1", False, "threshold of at least 2 distinct keys",
     edit={"tools/official.sh": sub(r"\. >= 2\) and", ". >= 1) and")})
case("release_verify does not read the pinned file", False, "release_verify does not read DeckPilot's first-party.json",
     edit={"tools/official.sh": sub(r"^  pinned=\$\(pinned_keys_file\)\n  require_pinned_signers", "  pinned=x\n  require_pinned_signers")})
case("release_verify does not require pinned signers", False, "release_verify does not require both signers",
     edit={"tools/official.sh": sub(r'^  require_pinned_signers "\$pinned" "\$want_a" "\$want_b"\n  local claim=',
                                    "  local claim=")})
case("test_verify does not require pinned signers", False, "test_verify does not require both signers",
     edit={"tools/official.sh": sub(r'^  require_pinned_signers "\$pinned" "\$want_a" "\$want_b"\n  note "DeckPilot',
                                    '  note "DeckPilot')})
case("test_verify passes --keys", False, "test_verify passes --keys",
     edit={"tools/official.sh": sub(r'release test-verify --run "\$run" --signatures-dir "\$dir"$',
                                    'release test-verify --run "$run" --signatures-dir "$dir" --keys "$k"')})
case("fetch-deckpilot fetches elsewhere", False, "fetch-deckpilot does not fetch",
     edit={"tools/official.sh": sub(r'fetch-deckpilot\) fetch_deckpilot "\$DECKPILOT_DIR"', 'fetch-deckpilot) fetch_deckpilot "$WORK/other"')})
case("build-cli.sh builds another checkout", False, "tools/build-cli.sh: does not build",
     edit={"tools/build-cli.sh": sub(r'^fetch_deckpilot "\$DECKPILOT_DIR"$', 'fetch_deckpilot "$WORK/other"')})
case("a commented-out fetch does not count", False, "does not fetch the pinned DeckPilot",
     edit={RELEASE: sub(r"run: tools/official.sh fetch-deckpilot$", "run: echo x # tools/official.sh fetch-deckpilot")})

print(f"\n{passed} passed, {failed} failed")
sys.exit(1 if failed else 0)
