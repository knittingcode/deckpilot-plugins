#!/usr/bin/env python3
"""Composes the Pages artifact from the registry tree and the website
(stdlib only; run with python3 -I).

    compose-site.py --registry DIR --website DIR --out DIR [--manifest FILE]

GitHub Pages serves one tree per repository, so the registry DeckPilot
clients read and the human-facing website share it. This is the only place
where they meet, and the registry always wins:

* REGISTRY is the tree the existing pipeline built and verified: /v1/**
  (the signed registry, or README.txt while none is published) plus the
  root files it has always served (icon.svg). Every file of it is copied
  byte for byte.
* WEBSITE is the output of the website build (website/). It may not contain
  anything under a reserved prefix (v1/), any path the registry tree has,
  or a path that differs from one only by letter case; it may not be a
  test-fixture build; it may not contain symbolic links.
* After copying, every registry file is read back from OUT and its sha256
  compared with the one taken from REGISTRY before anything was copied, and
  OUT is checked to hold exactly the union of both trees. Any difference
  fails, so a website build can never change, shadow or drop a registry
  path.

The manifest (sha256 and size of every registry path) is printed, written
to --manifest, and used after deployment to compare the live bytes
(tools/verify-live-registry.sh).

This script signs nothing, verifies no signature and needs no secret. The
registry tree was verified before it gets here (tools/publish-site.sh,
`deckpilot-registry verify-site` in _site.yml).
"""

import argparse
import hashlib
import json
import os
import shutil
import sys
from pathlib import Path

# Path prefixes that belong to the registry alone, whether or not the
# current registry tree has files there.
RESERVED_PREFIXES = ("v1",)
FIXTURE_MARKER = "FIXTURE_BUILD"
FIXTURE_ATTRIBUTE = b"data-fixture-build"


class ComposeError(Exception):
    pass


def files_of(root):
    """Relative POSIX paths of every file under root; refuses links."""
    out = []
    for directory, names, filenames in os.walk(root, followlinks=False):
        for name in names + filenames:
            path = Path(directory) / name
            if path.is_symlink():
                raise ComposeError(f"{path} is a symbolic link")
        for name in filenames:
            path = Path(directory) / name
            if not path.is_file():
                raise ComposeError(f"{path} is not a regular file")
            out.append(path.relative_to(root).as_posix())
    return sorted(out)


def digest(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def manifest_of(root, paths):
    return {p: {"sha256": digest(root / p), "size": (root / p).stat().st_size} for p in paths}


def collisions(registry_paths, website_paths):
    problems = []
    registry_fold = {p.casefold(): p for p in registry_paths}
    # Every directory the registry tree uses, and every file, case-folded.
    registry_dirs = set()
    for p in registry_paths:
        parts = p.casefold().split("/")
        for i in range(1, len(parts)):
            registry_dirs.add("/".join(parts[:i]))
    for p in website_paths:
        fold = p.casefold()
        first = fold.split("/", 1)[0]
        if first in RESERVED_PREFIXES:
            problems.append(f"website path {p} is under the reserved registry prefix {first}/")
        elif fold in registry_fold:
            problems.append(f"website path {p} collides with the registry path {registry_fold[fold]}")
        elif fold in registry_dirs:
            problems.append(f"website file {p} would replace a registry directory")
        else:
            parts = fold.split("/")
            for i in range(1, len(parts)):
                if "/".join(parts[:i]) in registry_fold:
                    problems.append(f"website path {p} is under the registry file {registry_fold['/'.join(parts[:i])]}")
                    break
    return problems


def compose(registry, website, out):
    registry, website, out = Path(registry), Path(website), Path(out)
    for name, directory in (("registry", registry), ("website", website)):
        if not directory.is_dir():
            raise ComposeError(f"the {name} tree {directory} is not a directory")
    registry_paths = files_of(registry)
    website_paths = files_of(website)
    if not registry_paths:
        raise ComposeError("the registry tree is empty; refusing to deploy a site without /v1/")
    if not any(p.split("/", 1)[0] in RESERVED_PREFIXES for p in registry_paths):
        raise ComposeError("the registry tree has nothing under v1/")
    if "index.html" not in website_paths:
        raise ComposeError("the website has no index.html")

    if FIXTURE_MARKER in website_paths:
        raise ComposeError("the website is a test-fixture build (FIXTURE_BUILD); it is never deployed")
    for p in website_paths:
        if p.endswith(".html") and FIXTURE_ATTRIBUTE in (website / p).read_bytes():
            raise ComposeError(f"website page {p} is from a test-fixture build; it is never deployed")

    problems = collisions(registry_paths, website_paths)
    if problems:
        raise ComposeError("\n".join(problems))

    # The reference: taken from the verified tree before anything is copied.
    before = manifest_of(registry, registry_paths)

    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    for source, paths in ((website, website_paths), (registry, registry_paths)):
        for p in paths:
            target = out / p
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source / p, target)

    # Read everything back from the artifact itself.
    out_paths = files_of(out)
    expected = sorted(set(registry_paths) | set(website_paths))
    if out_paths != expected:
        extra = sorted(set(out_paths) - set(expected))
        missing = sorted(set(expected) - set(out_paths))
        raise ComposeError(f"the artifact is not the union of both trees (extra: {extra}, missing: {missing})")
    after = manifest_of(out, registry_paths)
    changed = [p for p in registry_paths if after[p] != before[p]]
    if changed:
        raise ComposeError(f"registry paths changed while composing: {changed}")
    reserved = [p for p in out_paths if p.split("/", 1)[0] in RESERVED_PREFIXES]
    if reserved != [p for p in registry_paths if p.split("/", 1)[0] in RESERVED_PREFIXES]:
        raise ComposeError("the artifact has files under a reserved prefix that the registry tree does not have")
    return before, len(website_paths)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--registry", required=True)
    parser.add_argument("--website", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--manifest")
    args = parser.parse_args()
    try:
        manifest, website_count = compose(args.registry, args.website, args.out)
    except ComposeError as error:
        for line in str(error).splitlines():
            print(f"error: {line}", file=sys.stderr)
        return 1
    text = json.dumps(manifest, indent=2, sort_keys=True) + "\n"
    if args.manifest:
        Path(args.manifest).write_text(text, encoding="utf-8")
    lines = [
        f"Composed the Pages artifact: {len(manifest)} registry file(s) byte for byte, {website_count} website file(s).",
        "",
        "| Registry path | sha256 | bytes |",
        "| --- | --- | --- |",
        *[f"| `/{p}` | `{m['sha256']}` | {m['size']} |" for p, m in sorted(manifest.items())],
    ]
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as f:
            f.write("\n".join(lines) + "\n")
    print("\n".join(lines))
    return 0


if __name__ == "__main__":
    sys.exit(main())
