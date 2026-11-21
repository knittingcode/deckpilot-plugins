#!/usr/bin/env python3
"""The publisher keys pinned in DeckPilot, as this repository uses them
(stdlib only; run with python3 -I). validate.yml's Repository hygiene runs it
after `tools/official.sh fetch-deckpilot` (git only; the commit of
tools/deckpilot.env, which must be on DeckPilot's main).

* DeckPilot's crates/deckpilot-plugin-registry/publisher/first-party.json at
  the pinned commit pins at least two distinct keys (by Ed25519 key and by
  minisign key id), none of them revoked, and a threshold of at least 2 and
  at most the number of keys; publisher `deckpilot`, version 1.
* The verify jobs compare against exactly that file: in official-release.yml
  and official-test-sign.yml, job `verify` fetches the pinned DeckPilot
  (`tools/official.sh fetch-deckpilot`) and runs `release-verify` (or
  `test-verify`); neither workflow passes `--keys`; in tools/official.sh,
  release_verify and test_verify read the file through pinned_keys_file
  (that path under $DECKPILOT_DIR, threshold >= 2) and require every signer
  to be one of its keys (require_pinned_signers), and pass no `--keys`;
  tools/build-cli.sh builds the deckpilot-registry they run (whose pinned
  keys are this file, compiled in) from the same checkout.

Prints the file's sha256, its key ids and threshold.

    python3 -I tools/check-publisher-pin.py [--deckpilot DIR] [--root DIR]
"""

import argparse
import base64
import hashlib
import json
import re
import sys
from pathlib import Path

KEYS = "crates/deckpilot-plugin-registry/publisher/first-party.json"
PINNED_PATH = '"$DECKPILOT_DIR/crates/deckpilot-plugin-registry/publisher/first-party.json"'
VERIFY_JOBS = {
    ".github/workflows/official-release.yml": "release-verify",
    ".github/workflows/official-test-sign.yml": "test-verify",
}
FUNCTIONS = {"release_verify": "release-verify", "test_verify": "test-verify"}


def key_of(text, what):
    """(Ed25519 key bytes, key id) of a base64-wrapped minisign public key."""
    try:
        lines = base64.b64decode(text, validate=True).decode().splitlines()
        raw = base64.b64decode(lines[1], validate=True)
    except (ValueError, IndexError, UnicodeDecodeError):
        raise ValueError(f"{what} is not a base64-wrapped minisign public key")
    if len(raw) != 42 or raw[:2] != b"Ed":
        raise ValueError(f"{what} is not an Ed25519 minisign public key")
    return raw[10:], raw[2:10][::-1].hex().upper()


def check_keys(path):
    problems = []
    try:
        doc = json.loads(path.read_bytes())
    except (OSError, ValueError) as e:
        return [f"{path}: {e}"], None
    if not isinstance(doc, dict):
        return [f"{path}: not a JSON object"], None
    if doc.get("publisherKeysVersion") != 1:
        problems.append(f"{path}: publisherKeysVersion {doc.get('publisherKeysVersion')!r}, only 1")
    if doc.get("publisher") != "deckpilot":
        problems.append(f"{path}: publisher {doc.get('publisher')!r}, not deckpilot")
    keys, revoked = doc.get("keys"), doc.get("revokedKeys", [])
    threshold = doc.get("threshold")
    if not isinstance(keys, list) or not all(isinstance(k, str) for k in keys):
        return problems + [f"{path}: keys is not a list of keys"], None
    if not isinstance(revoked, list) or not all(isinstance(k, str) for k in revoked):
        return problems + [f"{path}: revokedKeys is not a list of keys"], None
    current, ids = set(), []
    for n, text in enumerate(keys):
        try:
            key, kid = key_of(text, f"keys[{n}]")
        except ValueError as e:
            problems.append(f"{path}: {e}")
            continue
        if key in current or kid in ids:
            problems.append(f"{path}: keys[{n}] ({kid}) is listed twice: the keys must be distinct")
        current.add(key)
        ids.append(kid)
    for n, text in enumerate(revoked):
        try:
            key, kid = key_of(text, f"revokedKeys[{n}]")
        except ValueError as e:
            problems.append(f"{path}: {e}")
            continue
        if key in current or kid in ids:
            problems.append(f"{path}: key {kid} is both pinned and revoked")
    if type(threshold) is not int or threshold < 2:
        problems.append(f"{path}: threshold {threshold!r}: an official release needs a threshold of at least 2")
    elif threshold > len(ids):
        problems.append(f"{path}: threshold {threshold} for {len(ids)} key(s)")
    if len(set(ids)) < 2:
        problems.append(f"{path}: {len(set(ids))} distinct key(s): at least 2 distinct publisher keys must be pinned")
    return problems, (sorted(ids), threshold)


def jobs_of(text):
    jobs = text.split("\njobs:\n", 1)[1] if "\njobs:\n" in text else ""
    starts = list(re.finditer(r"^  ([A-Za-z0-9_-]+):\s*$", jobs, re.M))
    return {m.group(1): jobs[m.end(): starts[k + 1].start() if k + 1 < len(starts) else len(jobs)]
            for k, m in enumerate(starts)}


def code(text):
    """Text without comment lines (YAML and shell)."""
    return "\n".join(line for line in text.splitlines() if not line.lstrip().startswith("#"))


def function_body(script, name):
    m = re.search(rf"^{name}\(\) \{{\n(.*?)^\}}\n", script, re.M | re.S)
    return m.group(1) if m else None


def check_tools(root):
    problems = []
    for rel, command in VERIFY_JOBS.items():
        path = root / rel
        if not path.is_file():
            problems.append(f"{rel}: missing")
            continue
        text = code(path.read_text(encoding="utf-8"))
        if re.search(r"--keys\b", text):
            problems.append(f"{rel}: `--keys`: the verify job uses only the keys pinned in DeckPilot")
        verify = jobs_of(text).get("verify")
        if verify is None:
            problems.append(f"{rel}: no job verify")
            continue
        if not re.search(r"run:\s*tools/official\.sh fetch-deckpilot\s*$", verify, re.M):
            problems.append(f"{rel}: job verify does not fetch the pinned DeckPilot (tools/official.sh fetch-deckpilot)")
        if not re.search(rf"run:\s*tools/official\.sh {command}\s", verify):
            problems.append(f"{rel}: job verify does not run tools/official.sh {command}")
    official = root / "tools/official.sh"
    script = code(official.read_text(encoding="utf-8")) if official.is_file() else ""
    pinned = function_body(script, "pinned_keys_file")
    if pinned is None:
        problems.append("tools/official.sh: no pinned_keys_file")
    else:
        if f"local pinned={PINNED_PATH[1:-1]}" not in pinned:
            problems.append(f"tools/official.sh: pinned_keys_file does not read {PINNED_PATH[1:-1]}")
        if ".threshold | type == \"number\" and . >= 2" not in pinned or "(.keys | unique | length) >= .threshold" not in pinned:
            problems.append("tools/official.sh: pinned_keys_file does not require a threshold of at least 2 distinct keys")
    for name, command in FUNCTIONS.items():
        body = function_body(script, name)
        if body is None:
            problems.append(f"tools/official.sh: no function {name} ({command})")
            continue
        if "pinned=$(pinned_keys_file)" not in body:
            problems.append(f"tools/official.sh: {name} does not read DeckPilot's first-party.json (pinned_keys_file)")
        if not re.search(r'require_pinned_signers "\$pinned" "\$want_a" "\$want_b"', body):
            problems.append(f"tools/official.sh: {name} does not require both signers to be pinned keys")
        if re.search(r"--keys\b", body):
            problems.append(f"tools/official.sh: {name} passes --keys: only the keys pinned in DeckPilot")
    if not re.search(r'^\s*fetch-deckpilot\) fetch_deckpilot "\$DECKPILOT_DIR" ;;', script, re.M):
        problems.append("tools/official.sh: fetch-deckpilot does not fetch the pinned DeckPilot into $DECKPILOT_DIR")
    build = root / "tools/build-cli.sh"
    if not build.is_file() or not re.search(r'^fetch_deckpilot "\$DECKPILOT_DIR"$', code(build.read_text(encoding="utf-8")), re.M):
        problems.append("tools/build-cli.sh: does not build deckpilot-registry from the pinned checkout $DECKPILOT_DIR")
    return problems


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", default=str(Path(__file__).resolve().parents[1]))
    parser.add_argument("--deckpilot", help="the pinned DeckPilot checkout (default: <root>/.work/deckpilot)")
    args = parser.parse_args()
    root = Path(args.root)
    deckpilot = Path(args.deckpilot) if args.deckpilot else root / ".work/deckpilot"
    keys_file = deckpilot / KEYS
    problems = []
    summary = None
    if not keys_file.is_file():
        problems.append(f"{keys_file}: missing; run tools/official.sh fetch-deckpilot")
    else:
        found, summary = check_keys(keys_file)
        problems += found
    problems += check_tools(root)
    for problem in problems:
        print(f"error: {problem}", file=sys.stderr)
    if problems:
        return 1
    ids, threshold = summary
    digest = hashlib.sha256(keys_file.read_bytes()).hexdigest()
    print(f"publisher pin: DeckPilot's {KEYS} sha256 {digest}: keys {', '.join(ids)}, threshold {threshold}; "
          "official-release.yml and official-test-sign.yml verify against exactly it")
    return 0


if __name__ == "__main__":
    sys.exit(main())
