#!/usr/bin/env python3
"""Checks the storefront inputs of a repository root (stdlib only).

    check-storefront.py [--root DIR] [--schemas DIR]

* registry/plugins/<id>/listing.json against schemas/inputs/listing.schema.json,
  and editorial/editorial.json against schemas/inputs/editorial.schema.json;
* the rules the schemas cannot express (docs/storefront.md): the id is the
  directory, every referenced media file exists in media/ beside it and
  every media file is referenced, release note versions are recorded,
  editorial ids are unique, referenced plugins, versions and collections
  exist, windows end after they start, URL, text (NFC, controls, bidi) and
  safe markdown rules, the totals per plugin and for editorial;
* every media file: format sniffed from its bytes (it must match the
  extension), dimensions, aspect, size and pixel count from its headers,
  no animation, no metadata chunks, PNG chunk allow-list and CRCs, WebP
  RIFF length, JPEG baseline or progressive, 8-bit, 1 or 3 components.
  SVG is refused.

This is a header-level check: it never decodes pixels. The authoritative
check, including a full decode of every asset, is the pinned
`deckpilot-registry storefront check` (DeckPilot WP1); tools/validate.sh
storefront runs it as soon as the pinned CLI has it.

Warnings (listing text of a non-deckpilot plugin that mentions "official",
"verified" or "DeckPilot team"; a placement of a plugin with no recorded
version yet) go to the job summary for review and do not fail the check.

Run with `python3 -I`. Exit status 1 on any error.
"""

import argparse
import os
import sys
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent / "lib"))
import storefront  # noqa: E402


def summary(*lines):
    path = os.environ.get("GITHUB_STEP_SUMMARY")
    if path:
        with open(path, "a", encoding="utf-8") as f:
            f.write("\n".join(lines) + "\n")


def main():
    here = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=str(here))
    parser.add_argument("--schemas", default=None, help="default: <root>/schemas")
    args = parser.parse_args()
    root = Path(args.root)
    schemas = storefront.SchemaSet(args.schemas or root / "schemas")
    report = storefront.check_tree(root, schemas)
    listings = sorted(p.parent.name for p in (root / "registry" / "plugins").glob("*/listing.json"))
    editorial = (root / "editorial" / "editorial.json").is_file()
    for w in report.warnings:
        print(f"warning: {w}", file=sys.stderr)
    for e in report.errors:
        print(f"error: {e}", file=sys.stderr)
    lines = ["### Storefront inputs (header-level check)", "",
             f"- listings: {', '.join(f'`{p}`' for p in listings) or 'none'}",
             f"- editorial: {'editorial/editorial.json' if editorial else 'none'}"]
    if report.warnings:
        lines += ["", "**For review:**", ""] + [f"- {w}" for w in report.warnings]
    if report.errors:
        lines += ["", f"**{len(report.errors)} error(s)**; see the log."]
    summary(*lines, "")
    print(f"storefront: {len(listings)} listing(s), editorial {'yes' if editorial else 'no'}, "
          f"{len(report.errors)} error(s), {len(report.warnings)} warning(s)")
    return 1 if report.errors else 0


if __name__ == "__main__":
    sys.exit(main())
