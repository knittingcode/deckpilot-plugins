#!/usr/bin/env python3
"""Refuses what must never be committed (stdlib only; run with python3 -I).

* Private keys of any kind: minisign secret keys (plain or scrypt-encrypted,
  also base64-wrapped as the Tauri tooling writes them), PEM/OpenSSH private
  keys, and any *.key file.
* Generated output: site/ (the Pages tree is built by CI, never committed),
  releases/ and *.dpplugin packages (release assets live in GitHub Releases),
  Python bytecode (__pycache__/, *.pyc).
* keys/ may hold only public trust material: roots.json, <n>.json,
  <n>.json.minisig and README.md.

Checks every file tracked by git (or the paths given as arguments).
"""

import base64
import re
import subprocess
import sys
from pathlib import Path

# Split so this file does not match itself.
MINISIGN_SECRET = re.compile(rb"\bRW(?:RTY0|QAAE)Iy[A-Za-z0-9+/]{20,}")
SECRET_COMMENT = re.compile(rb"untrusted comment:[^\n]*" + rb"secret" + rb" key", re.I)
PEM = re.compile(rb"-----BEGIN [A-Z ]*" + rb"PRIVATE KEY-----")
B64_TOKEN = re.compile(rb"[A-Za-z0-9+/]{80,}={0,2}")
KEYS_ALLOWED = re.compile(r"^keys/(roots\.json|[1-9][0-9]*\.json(\.minisig)?|README\.md)$")


def secret_in(data):
    if MINISIGN_SECRET.search(data) or SECRET_COMMENT.search(data) or PEM.search(data):
        return True
    for token in B64_TOKEN.findall(data):
        try:
            decoded = base64.b64decode(token + b"=" * (-len(token) % 4), validate=False)
        except ValueError:
            continue
        if MINISIGN_SECRET.search(decoded) or SECRET_COMMENT.search(decoded) or PEM.search(decoded):
            return True
    return False


def main():
    paths = sys.argv[1:] or subprocess.run(
        ["git", "ls-files", "-z"], check=True, capture_output=True
    ).stdout.decode().split("\0")
    problems = []
    for path in filter(None, paths):
        p = Path(path)
        parts = p.parts
        if p.suffix == ".key":
            problems.append(f"{path}: key files are never committed")
        if parts and parts[0] in {"site", "releases"}:
            problems.append(f"{path}: generated output ({parts[0]}/) is never committed; CI builds it")
        if p.suffix == ".pyc" or "__pycache__" in parts:
            problems.append(f"{path}: Python bytecode is generated, never committed")
        if p.suffix == ".dpplugin":
            problems.append(f"{path}: packages are release assets, never committed")
        if parts and parts[0] == "keys" and not KEYS_ALLOWED.match(path):
            problems.append(f"{path}: keys/ holds only roots.json, <n>.json(.minisig) and README.md")
        if p.is_file():
            if secret_in(p.read_bytes()):
                problems.append(f"{path}: looks like a private key")
    for problem in problems:
        print(f"error: {problem}", file=sys.stderr)
    if problems:
        return 1
    print(f"hygiene: {len([p for p in paths if p])} tracked file(s) checked, nothing forbidden")
    return 0


if __name__ == "__main__":
    sys.exit(main())
